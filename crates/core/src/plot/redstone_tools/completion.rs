use super::{Plot, ToolCommand};
use mchprs_network::packets::clientbound::{CTabComplete, CTabCompleteMatch};

impl Plot {
    pub(in crate::plot) fn complete_redstone_tools(
        &mut self,
        player: usize,
        transaction: i32,
        text: &str,
    ) -> Option<CTabComplete> {
        let (command, tail) = text.split_once(' ')?;
        let tool = ToolCommand::parse(command)?;
        if !self.players[player].has_permission(tool.permission()) {
            return Some(CTabComplete {
                id: transaction,
                start: 0,
                length: 0,
                matches: Vec::new(),
            });
        }
        let start = text.rfind(' ')? + 1;
        let prefix = &text[start..];
        let args: Vec<_> = tail.split_whitespace().collect();
        let mut choices: Vec<String> = match tool {
            ToolCommand::Wire if !tail.trim_start().contains(char::is_whitespace) => {
                ["free", "plane", "off"].map(str::to_owned).to_vec()
            }
            ToolCommand::Container if args.len() < 2 && !tail.contains(' ') => {
                ["chest", "barrel", "hopper", "furnace"]
                    .map(str::to_owned)
                    .to_vec()
            }
            ToolCommand::Container => (0..=15)
                .map(|power| power.to_string())
                .chain(["a", "b", "c", "d", "e", "f"].map(str::to_owned))
                .collect(),
            ToolCommand::RStack => [
                "me", "north", "south", "east", "west", "up", "down", "ne", "nw", "se", "sw",
                "neu", "ned", "forward", "back", "left", "right", "-e", "-w", "-a",
            ]
            .map(str::to_owned)
            .to_vec(),
            ToolCommand::AutoStack => [
                "off", "me", "north", "south", "east", "west", "up", "down", "ne", "nw", "se",
                "sw", "neu", "ned", "forward", "back", "left", "right", "-e",
            ]
            .map(str::to_owned)
            .to_vec(),
            ToolCommand::Find | ToolCommand::SignSearch if args.first() == Some(&"-p") => {
                let cache = match tool {
                    ToolCommand::Find => self.players[player].redstone_tools.block_search.as_ref(),
                    _ => self.players[player].redstone_tools.sign_search.as_ref(),
                };
                let pages = cache.map_or(0, |cache| cache.page_count());
                (1..=pages).map(|page| page.to_string()).collect()
            }
            ToolCommand::Find => mchprs_blocks::generated::BLOCKS
                .iter()
                .map(|definition| definition.0.to_owned())
                .chain(std::iter::once("-p".into()))
                .collect(),
            ToolCommand::SignSearch => vec!["-p".into()],
            _ => Vec::new(),
        };
        choices.retain(|choice| choice.starts_with(prefix));
        choices.sort();
        choices.dedup();
        choices.truncate(100);
        Some(CTabComplete {
            id: transaction,
            start: text[..start].encode_utf16().count() as i32,
            length: prefix.encode_utf16().count() as i32,
            matches: choices
                .into_iter()
                .map(|text| CTabCompleteMatch {
                    match_: text,
                    tooltip: None,
                })
                .collect(),
        })
    }
}

#[cfg(test)]
mod tests {
    use crate::plot::client_sync_tests::fixture;

    #[test]
    fn wire_aiming_modes_complete_only_the_first_argument() {
        let (mut plot, _peer) = fixture(false);
        for (text, expected) in [
            ("/wire ", vec!["free", "off", "plane"]),
            ("/wire f", vec!["free"]),
            ("/wire p", vec!["plane"]),
            ("/wire o", vec!["off"]),
            ("//wire fr", vec!["free"]),
            ("/wire  p", vec!["plane"]),
            ("/wire free ", vec![]),
            ("//wire plane o", vec![]),
        ] {
            let response = plot.complete_redstone_tools(0, 7, text).unwrap();
            assert_eq!(response.id, 7);
            assert_eq!(response.start, (text.rfind(' ').unwrap() + 1) as i32);
            assert_eq!(
                response.length,
                (text.len() - response.start as usize) as i32
            );
            assert_eq!(
                response
                    .matches
                    .iter()
                    .map(|entry| entry.match_.as_str())
                    .collect::<Vec<_>>(),
                expected,
                "{text}"
            );
        }
    }
}
