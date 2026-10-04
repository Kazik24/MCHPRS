use super::{Plot, ResultAction, SelectionBounds};
use crate::player::{PacketSender, Player};
use crate::world::World;
use anyhow::{bail, Context, Result};
use mchprs_blocks::block_entities::BlockEntity;
use mchprs_blocks::blocks::Block;
use mchprs_blocks::BlockPos;
use regex::RegexBuilder;
use serde_json::{json, Value};
use std::collections::BTreeSet;
use std::ops::Range;

const PAGE_SIZE: usize = 7;
const MAX_RESULTS: usize = 4096;
const MAX_SCAN_BLOCKS: u64 = 16_777_216;
const MAX_QUERY_BYTES: usize = 1024;
const MAX_SIGN_LINE_BYTES: usize = 8192;
const MAX_RESULT_TEXT_BYTES: usize = 2 * 1024 * 1024;

#[derive(Clone, Copy, Debug)]
pub(super) enum SearchKind {
    Blocks,
    Signs,
}

impl SearchKind {
    pub(super) fn command(self) -> &'static str {
        match self {
            Self::Blocks => "//find",
            Self::Signs => "//signsearch",
        }
    }

    fn cache(self, player: &Player) -> Option<&SearchCache> {
        match self {
            Self::Blocks => player.redstone_tools.block_search.as_ref(),
            Self::Signs => player.redstone_tools.sign_search.as_ref(),
        }
    }

    fn store(self, player: &mut Player, cache: SearchCache) {
        match self {
            Self::Blocks => player.redstone_tools.block_search = Some(cache),
            Self::Signs => player.redstone_tools.sign_search = Some(cache),
        }
    }
}

#[derive(Clone, Copy, Debug)]
enum SignSide {
    Front,
    Back,
}

impl SignSide {
    fn label(self) -> &'static str {
        match self {
            Self::Front => "front",
            Self::Back => "back",
        }
    }
}

#[derive(Debug)]
struct SignMatch {
    side: SignSide,
    line: usize,
    text: String,
    highlight: Range<usize>,
}

#[derive(Debug)]
struct SearchHit {
    position: BlockPos,
    lines: Vec<SignMatch>,
}

#[derive(Debug)]
pub(crate) struct SearchCache {
    plot: (i32, i32),
    hits: Vec<SearchHit>,
    truncated: bool,
}

impl SearchCache {
    pub(super) fn page_count(&self) -> usize {
        self.hits.len().div_ceil(PAGE_SIZE)
    }

    fn page(&self, number: usize) -> Result<&[SearchHit]> {
        if number == 0 || number > self.page_count() {
            bail!("Page must be between 1 and {}", self.page_count());
        }
        let start = (number - 1) * PAGE_SIZE;
        let end = (start + PAGE_SIZE).min(self.hits.len());
        Ok(&self.hits[start..end])
    }

    fn display(&self, player: &Player, kind: SearchKind, page: usize) -> Result<()> {
        if self.hits.is_empty() {
            if page != 1 {
                bail!("There are no cached results to paginate");
            }
            player.send_raw_system_message(
                json!({
                    "text": "No matches found, nya~",
                    "color": "light_purple"
                })
                .to_string(),
            );
            return Ok(());
        }
        let hits = self.page(page)?;
        let suffix = match self.truncated {
            true => " (result limit reached)",
            false => "",
        };
        player.send_raw_system_message(json!({
            "text": format!("{} matches{suffix}; page {page}/{}, nya~", self.hits.len(), self.page_count()),
            "color": "light_purple"
        }).to_string());

        for hit in hits {
            let pos = hit.position;
            let label = format!("({}, {}, {})", pos.x, pos.y, pos.z);
            let mut parts = vec![ResultAction::Teleport(pos).component(&label)];
            for line in &hit.lines {
                parts.push(
                    json!({"text": format!("\n  {} {}: ", line.side.label(), line.line + 1)}),
                );
                parts.extend(highlight_parts(&line.text, line.highlight.clone()));
            }
            player.send_raw_system_message(json!({"text": "", "extra": parts}).to_string());
        }

        let mut pages = Vec::new();
        if page > 1 {
            pages.push(
                ResultAction::Page {
                    kind,
                    page: page - 1,
                }
                .component("[Previous] "),
            );
        }
        if page < self.page_count() {
            pages.push(
                ResultAction::Page {
                    kind,
                    page: page + 1,
                }
                .component("[Next]"),
            );
        }
        if !pages.is_empty() {
            player.send_raw_system_message(json!({"text": "", "extra": pages}).to_string());
        }
        Ok(())
    }
}

/// Supported masks: block names, partial state properties, numeric state IDs,
/// comma-separated unions, and `*`. This deliberately does not parse WorldEdit patterns.
#[derive(Debug)]
struct BlockMask {
    states: BTreeSet<u32>,
}

impl BlockMask {
    fn parse(token: &str) -> Result<Self> {
        validate_query(token)?;
        let mut depth = 0;
        let mut states = BTreeSet::new();
        let terms = token.split(|character| {
            match character {
                '[' => depth += 1,
                ']' => depth -= 1,
                _ => {}
            }
            character == ',' && depth == 0
        });
        for term in terms {
            Self::add_term(term, &mut states)?;
        }
        if depth != 0 {
            bail!("Unbalanced mask properties");
        }
        Ok(Self { states })
    }

    fn add_term(term: &str, states: &mut BTreeSet<u32>) -> Result<()> {
        if term == "*" {
            states.extend(0..mchprs_blocks::generated::STATE_PROPERTIES.len() as u32);
            return Ok(());
        }
        if let Ok(id) = term.parse::<u32>() {
            if id as usize >= mchprs_blocks::generated::STATE_PROPERTIES.len() {
                bail!("Unknown block state ID: {id}");
            }
            states.insert(id);
            return Ok(());
        }
        let (name, properties) = match term.split_once('[') {
            Some((name, tail)) => {
                let properties = tail
                    .strip_suffix(']')
                    .context("Mask properties must end with ]")?;
                (name, properties)
            }
            None => (term, ""),
        };
        let name = name.strip_prefix("minecraft:").unwrap_or(name);
        let definition = mchprs_blocks::generated::BLOCKS
            .iter()
            .find(|definition| definition.0 == name)
            .with_context(|| format!("Unknown block: {name}"))?;
        let mut constraints = Vec::new();
        if !properties.is_empty() {
            for property in properties.split(',') {
                let (key, value) = property
                    .split_once('=')
                    .context("Use property=value in masks")?;
                if constraints.iter().any(|&(existing, _)| existing == key) {
                    bail!("Duplicate mask property: {key}");
                }
                if !(definition.2..=definition.3)
                    .any(|id| Block::from_id(id).property(key) == Some(value))
                {
                    bail!("Unknown property or value: {key}={value}");
                }
                constraints.push((key, value));
            }
        }
        for id in definition.2..=definition.3 {
            let block = Block::from_id(id);
            if constraints
                .iter()
                .all(|&(key, value)| block.property(key) == Some(value))
            {
                states.insert(id);
            }
        }
        Ok(())
    }

    fn matches(&self, block: Block) -> bool {
        self.states.contains(&block.get_id())
    }
}

impl Plot {
    pub(super) fn search_blocks(&mut self, player: usize, args: &[&str]) -> Result<()> {
        if self.show_cached_search(player, SearchKind::Blocks, args)? {
            return Ok(());
        }
        let [query] = args else {
            bail!("Usage: //find <mask> or //find -p <page>");
        };
        let mask = BlockMask::parse(query)?;
        let bounds = self.search_bounds(player)?;
        let cache = self.scan_selection(bounds, |plot, position| {
            let block = plot.world.get_block(position);
            mask.matches(block).then_some(Vec::new())
        });
        SearchKind::Blocks.store(&mut self.players[player], cache);
        self.display_search(player, SearchKind::Blocks, 1)
    }

    pub(super) fn search_signs(&mut self, player: usize, args: &[&str]) -> Result<()> {
        if self.show_cached_search(player, SearchKind::Signs, args)? {
            return Ok(());
        }
        if args.is_empty() {
            bail!("Usage: //signsearch <regex> or //signsearch -p <page>");
        }
        let query = args.join(" ");
        validate_query(&query)?;
        let regex = RegexBuilder::new(&query)
            .size_limit(1024 * 1024)
            .dfa_size_limit(1024 * 1024)
            .build()
            .context("Invalid regular expression")?;
        let bounds = self.search_bounds(player)?;
        let cache = self.scan_selection(bounds, |plot, position| {
            let BlockEntity::Sign(sign) = plot.world.get_block_entity(position)? else {
                return None;
            };
            let mut lines = Vec::new();
            for (side, rows) in [
                (SignSide::Front, &sign.rows),
                (SignSide::Back, &sign.back_rows),
            ] {
                for (line, row) in rows.iter().enumerate() {
                    let text = flatten_sign_text(row);
                    let Some(found) = regex.find(&text) else {
                        continue;
                    };
                    let highlight = found.range();
                    lines.push(SignMatch {
                        side,
                        line,
                        text,
                        highlight,
                    });
                }
            }
            match lines.is_empty() {
                true => None,
                false => Some(lines),
            }
        });
        SearchKind::Signs.store(&mut self.players[player], cache);
        self.display_search(player, SearchKind::Signs, 1)
    }

    fn show_cached_search(&self, player: usize, kind: SearchKind, args: &[&str]) -> Result<bool> {
        if args.first() != Some(&"-p") {
            return Ok(false);
        }
        let [_, page] = args else {
            bail!("Usage: {} -p <page>", kind.command());
        };
        let page = page
            .parse::<usize>()
            .context("Page must be a positive integer")?;
        self.display_search(player, kind, page)?;
        Ok(true)
    }

    fn display_search(&self, player: usize, kind: SearchKind, page: usize) -> Result<()> {
        let player = &self.players[player];
        let cache = kind.cache(player).context("Run a search first")?;
        if cache.plot != (self.world.x, self.world.z) {
            bail!("These search results belong to another plot; run a new search");
        }
        if page == 0 {
            bail!("Page numbers start at 1");
        }
        cache.display(player, kind, page)
    }

    fn search_bounds(&self, player: usize) -> Result<SelectionBounds> {
        let bounds = SelectionBounds::from_player(&self.players[player], &self.world)?;
        if bounds.volume() > MAX_SCAN_BLOCKS {
            bail!("Search selections may contain at most {MAX_SCAN_BLOCKS} blocks");
        }
        Ok(bounds)
    }

    fn scan_selection(
        &self,
        bounds: SelectionBounds,
        find: impl Fn(&Self, BlockPos) -> Option<Vec<SignMatch>>,
    ) -> SearchCache {
        let mut hits = Vec::new();
        let mut text_bytes = 0;
        let mut truncated = false;
        'scan: for x in bounds.start.x..=bounds.end.x {
            for y in bounds.start.y..=bounds.end.y {
                for z in bounds.start.z..=bounds.end.z {
                    let position = BlockPos::new(x, y, z);
                    if let Some(lines) = find(self, position) {
                        let line_bytes: usize = lines.iter().map(|line| line.text.len()).sum();
                        if hits.len() == MAX_RESULTS
                            || text_bytes + line_bytes > MAX_RESULT_TEXT_BYTES
                        {
                            truncated = true;
                            break 'scan;
                        }
                        text_bytes += line_bytes;
                        hits.push(SearchHit { position, lines });
                    }
                }
            }
        }
        hits.sort_by_key(|hit| (hit.position.x, hit.position.y, hit.position.z));
        SearchCache {
            plot: (self.world.x, self.world.z),
            hits,
            truncated,
        }
    }
}

fn validate_query(query: &str) -> Result<()> {
    if query.len() > MAX_QUERY_BYTES {
        bail!("Queries may contain at most {MAX_QUERY_BYTES} bytes");
    }
    Ok(())
}

fn flatten_sign_text(row: &str) -> String {
    match serde_json::from_str::<Value>(row) {
        Ok(component) => {
            let mut text = String::new();
            append_literal_text(&component, &mut text, 0);
            text
        }
        Err(_) => bounded_text(row),
    }
}

fn bounded_text(text: &str) -> String {
    let mut end = text.len().min(MAX_SIGN_LINE_BYTES);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    text[..end].to_owned()
}

fn append_literal_text(value: &Value, output: &mut String, depth: usize) {
    if depth > 64 || output.len() >= MAX_SIGN_LINE_BYTES {
        return;
    }
    match value {
        Value::String(text) => output.push_str(text),
        Value::Array(parts) => {
            for part in parts {
                append_literal_text(part, output, depth + 1);
            }
        }
        Value::Object(component) => {
            if let Some(text) = component.get("text").and_then(Value::as_str) {
                output.push_str(text);
            }
            if let Some(extra) = component.get("extra") {
                append_literal_text(extra, output, depth + 1);
            }
        }
        _ => {}
    }
    if output.len() > MAX_SIGN_LINE_BYTES {
        *output = bounded_text(output);
    }
}

fn highlight_parts(text: &str, range: Range<usize>) -> [Value; 3] {
    [
        json!({"text": &text[..range.start], "color": "white"}),
        json!({"text": &text[range.clone()], "color": "yellow", "bold": true}),
        json!({"text": &text[range.end..], "color": "white"}),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use regex::Regex;

    #[test]
    fn masks_match_partial_properties_and_reject_unknowns() {
        let mask = BlockMask::parse("repeater[facing=north],stone").unwrap();
        let repeater = Block::from_name("repeater").unwrap();
        for id in 0..mchprs_blocks::generated::STATE_PROPERTIES.len() as u32 {
            let block = Block::from_id(id);
            let expected = block.get_name() == "stone"
                || (block.get_name() == repeater.get_name()
                    && block.property("facing") == Some("north"));
            assert_eq!(mask.matches(block), expected);
        }
        for invalid in [
            "",
            "stone[foo=bar]",
            "repeater[delay=5]",
            "stone[",
            "missing",
            "stone,",
            "repeater[delay=1,delay=2]",
        ] {
            assert!(BlockMask::parse(invalid).is_err(), "{invalid}");
        }
    }

    #[test]
    fn sign_text_keeps_parent_and_children_and_unicode_ranges() {
        let text = flatten_sign_text(r#"{"text":"é","extra":[{"text":"nya"},"~"]}"#);
        assert_eq!(text, "énya~");
        let range = Regex::new("nya").unwrap().find(&text).unwrap().range();
        let parts = highlight_parts(&text, range);
        assert_eq!(parts[0]["text"], "é");
        assert_eq!(parts[1]["text"], "nya");
        assert_eq!(parts[2]["text"], "~");
        let empty = Regex::new("").unwrap().find(&text).unwrap().range();
        assert_eq!(highlight_parts(&text, empty)[1]["text"], "");
    }

    #[test]
    fn pages_validate_before_slicing() {
        let hits = (0..15)
            .map(|x| SearchHit {
                position: BlockPos::new(x, 1, 0),
                lines: Vec::new(),
            })
            .collect();
        let cache = SearchCache {
            plot: (0, 0),
            hits,
            truncated: false,
        };
        assert_eq!(cache.page_count(), 3);
        assert_eq!(cache.page(2).unwrap()[0].position.x, 7);
        assert_eq!(cache.page(3).unwrap().len(), 1);
        assert!(cache.page(0).is_err());
        assert!(cache.page(usize::MAX).is_err());
    }
}
