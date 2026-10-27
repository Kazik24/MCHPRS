use super::*;
use crate::waveform::{Probe, Recorder, signal_value};
use mchprs_network::packets::clientbound::{CDeclareCommandsNode as Node, CDeclareCommandsNodeParser as Parser, CTabComplete, CTabCompleteMatch};

pub(crate) const HELP: &str = "/waveform add <name> [x y z]: watch a block.\n/waveform addbulk <name> <bits>: left-click low to high; cancel with /waveform cancel.\n/waveform on [ticks] | off | status | list | remove <name> | web\n/waveform clear: clear history, keep watches and recording. /waveform reset confirm: remove all watches and history, stop recording.\non/off discard history; off keeps probes. Recorded command output is public.";
const ACTIONS: [&str; 12] = [
    "on", "off", "status", "add", "addbulk", "cancel", "remove", "list", "clear", "reset", "web", "help",
];

fn permission_action(action: &str) -> &str {
    match action {
        "on" => "enable",
        "off" => "disable",
        other => other,
    }
}

fn check_syntax(args: &[&str]) -> Result<(), String> {
    match args {
        []
        | ["on"]
        | ["on", _]
        | ["add", _]
        | ["add", _, _, _, _]
        | ["addbulk", _, _]
        | ["remove", _]
        | ["reset", "confirm"]
        | ["off" | "status" | "list" | "clear" | "reset" | "web" | "cancel" | "help"] => Ok(()),
        ["addbulk", name] if Probe::valid_name(name) => Err(format!("Missing bit count; use /waveform addbulk {name} <bits>.")),
        [action, ..] if ACTIONS.contains(action) => {
            let arguments = match *action {
                "add" => " <name> [x y z]",
                "addbulk" => " <name> <bits>",
                "remove" => " <name>",
                "on" => " [ticks]",
                "reset" => " confirm",
                _ => "",
            };
            Err(format!("Usage: /waveform {action}{arguments}."))
        }
        _ => Err("Unknown waveform action; use /waveform help.".into()),
    }
}

fn tick_limit_error(limit: usize) -> String {
    if limit == 0 {
        "No recording allowance; ask staff.".into()
    } else {
        format!("Tick limit is {limit}; use /waveform on {limit}.")
    }
}

fn probe_limit_error(limit: usize) -> String {
    if limit == 0 {
        "No probe allowance; ask staff.".into()
    } else {
        format!("Probe limit reached ({limit}); remove a probe first.")
    }
}

pub(super) fn command_node(nodes: &mut Vec<Node<'static>>) -> i32 {
    fn child(nodes: &mut Vec<Node<'static>>, parent: usize, node: Node<'static>) -> usize {
        let id = nodes.len();
        let mut children = nodes[parent].children.to_vec();
        children.push(id as i32);
        nodes[parent].children = Box::leak(children.into_boxed_slice());
        nodes.push(node);
        id
    }
    let root = nodes.len();
    nodes.push(Node::literal("waveform", &[]).executable());
    for action in ACTIONS {
        let literal = Node::literal(action, &[]);
        let branch = child(
            nodes,
            root,
            if matches!(action, "add" | "addbulk" | "remove") {
                literal
            } else {
                literal.executable()
            },
        );
        match action {
            "reset" => {
                child(nodes, branch, Node::literal("confirm", &[]).executable());
            }
            "on" => {
                child(
                    nodes,
                    branch,
                    Node::argument("ticks", Parser::Integer(1, 10000), &[])
                        .executable()
                        .suggestions("minecraft:ask_server"),
                );
            }
            "add" | "addbulk" | "remove" => {
                let name = Node::argument("name", Parser::String(0), &[]);
                let name = match action {
                    "add" => name.executable(),
                    "remove" => name.executable().suggestions("minecraft:ask_server"),
                    _ => name,
                };
                let name = child(nodes, branch, name);
                if action == "addbulk" {
                    child(
                        nodes,
                        name,
                        Node::argument("bits", Parser::Integer(1, 1024), &[])
                            .executable()
                            .suggestions("minecraft:ask_server"),
                    );
                } else if action == "add" {
                    let x = child(
                        nodes,
                        name,
                        Node::argument("x", Parser::Integer(i32::MIN, i32::MAX), &[]).suggestions("minecraft:ask_server"),
                    );
                    let y = child(
                        nodes,
                        x,
                        Node::argument("y", Parser::Integer(0, PLOT_BLOCK_HEIGHT - 1), &[]).suggestions("minecraft:ask_server"),
                    );
                    child(
                        nodes,
                        y,
                        Node::argument("z", Parser::Integer(i32::MIN, i32::MAX), &[])
                            .executable()
                            .suggestions("minecraft:ask_server"),
                    );
                }
            }
            _ => {}
        }
    }
    root as i32
}

pub(crate) struct Selection {
    plot: (i32, i32),
    name: String,
    bits: usize,
    positions: Vec<BlockPos>,
}

impl PlotWorld {
    #[cfg(test)]
    pub(crate) fn enable_waveform_for_test(&mut self, probes: Vec<Probe>, ticks: u64) -> Result<(), String> {
        self.waveform = self.prepare_waveform(probes, Some((ticks, 64)))?;
        Ok(())
    }
    #[cfg(test)]
    pub(crate) fn waveform_page_for_test(&self) -> serde_json::Value {
        self.waveform
            .page(
                (self.x, self.z),
                1,
                &crate::waveform::api::Query {
                    cursor: None,
                    limit: 4096,
                    metadata: false,
                },
                false,
            )
            .unwrap()
    }
    fn prepare_waveform(&self, probes: Vec<Probe>, active: Option<(u64, usize)>) -> Result<Recorder, String> {
        if active.is_some() && self.piston_state.phase != AdvancePhase::BetweenTicks {
            return Err("Finish the current tick with /adv 1, then retry.".into());
        }
        let mut next = Recorder::default();
        next.replace(probes);
        if let Some((ticks, limit)) = active {
            let values = next.probes.iter().map(|p| signal_value(self, p.pos)).collect();
            next.on(ticks, self.piston_state.logical_tick, values, limit)?;
        }
        Ok(next)
    }
    pub(super) fn reset_waveform(&mut self) {
        if self.waveform.capture.is_none() {
            return;
        }
        let values = self.waveform.probes.iter().map(|p| signal_value(self, p.pos)).collect();
        self.waveform.discontinuity(
            self.piston_state.logical_tick,
            self.piston_state.phase != AdvancePhase::BetweenTicks,
            values,
        );
    }
    pub(super) fn record_waveform_change(&mut self, pos: BlockPos) {
        for watched in self.waveform.affected(pos) {
            let value = signal_value(self, watched);
            self.waveform.changed(watched, value, self.piston_state.logical_tick);
        }
    }
}
impl Plot {
    fn check_waveform_access(&self, player: usize, action: &str) -> Result<(), String> {
        let actor = &self.players[player];
        if !actor.can_use_commands() || !actor.has_permission("commands.waveform") {
            return Err("No waveform access; ask staff.".into());
        }
        if !actor.has_permission(&format!("commands.waveform.{}", permission_action(action))) {
            return Err(format!("No permission for /waveform {action}; ask staff."));
        }
        if !actor.can_edit_plot(self.owner, (self.world.x, self.world.z)) {
            return Err("You need edit access to this plot.".into());
        }
        Ok(())
    }

    fn check_waveform_block(&self, pos: BlockPos) -> Result<(), String> {
        if !Self::in_plot_bounds(self.world.x, self.world.z, pos.x, pos.z) {
            return Err("Choose a block inside this plot.".into());
        }
        if !(0..PLOT_BLOCK_HEIGHT).contains(&pos.y) {
            return Err(format!("Y must be between 0 and {}.", PLOT_BLOCK_HEIGHT - 1));
        }
        if matches!(self.world.get_block(pos), Block::Air {}) {
            return Err("That position is air; choose a block.".into());
        }
        Ok(())
    }

    pub(super) fn complete_waveform(&self, player: usize, id: i32, text: &str) -> Option<CTabComplete> {
        if text.split_once(char::is_whitespace)?.0 != "/waveform" {
            return None;
        }
        let split = text.rfind(char::is_whitespace)?;
        let start = split + text[split..].chars().next()?.len_utf8();
        let words: Vec<_> = text[..start].split_whitespace().collect();
        let prefix = &text[start..];
        let mut choices: Vec<String> = if words.len() == 1 {
            ACTIONS
                .iter()
                .copied()
                .filter(|action| matches!(*action, "help" | "cancel") || self.check_waveform_access(player, action).is_ok())
                .map(str::to_owned)
                .collect()
        } else if self.check_waveform_access(player, words[1]).is_err() {
            Vec::new()
        } else {
            match words.as_slice() {
                [_, "remove"] => self.world.waveform.probes.iter().map(|p| p.name.clone()).collect(),
                [_, "on"] => {
                    let (_, limit) = self.players[player].waveform_limits().ok().unwrap_or((0, 0));
                    [1, 20, 100, 1000, 10000, limit]
                        .into_iter()
                        .filter(|ticks| *ticks > 0 && *ticks <= limit)
                        .map(|ticks| ticks.to_string())
                        .collect()
                }
                [_, "addbulk", _] => {
                    let (signals, _) = self.players[player].waveform_limits().ok().unwrap_or((0, 0));
                    let left = signals.saturating_sub(self.world.waveform.probes.len());
                    [1, 2, 4, 8, 16, 32, 64, 128, 256, 512, 1024, left]
                        .into_iter()
                        .filter(|bits| *bits > 0 && *bits <= left)
                        .map(|bits| bits.to_string())
                        .collect()
                }
                [_, "add", _, ..] if (3..=5).contains(&words.len()) => {
                    let actor = &self.players[player];
                    let pos = worldedit::ray_trace_block(&self.world, actor.eye_position(), actor.pitch as f64, actor.yaw as f64, 10.0)
                        .unwrap_or_else(|| actor.pos.block_pos());
                    vec![[pos.x, pos.y.clamp(0, PLOT_BLOCK_HEIGHT - 1), pos.z][words.len() - 3].to_string()]
                }
                _ => Vec::new(),
            }
        };
        choices.retain(|choice| choice.starts_with(prefix));
        choices.sort();
        choices.dedup();
        choices.truncate(100);
        Some(CTabComplete {
            id,
            start: text[..start].encode_utf16().count() as i32,
            length: prefix.encode_utf16().count() as i32,
            matches: choices.into_iter().map(|match_| CTabCompleteMatch { match_, tooltip: None }).collect(),
        })
    }

    fn save_waveform_definitions(&self, probes: &[Probe]) -> Result<(), String> {
        #[cfg(any(test, feature = "plot-bench"))]
        if self.transient_test_fixture {
            return Ok(());
        }
        database::set_waveform_probes(self.world.x, self.world.z, probes).map_err(|error| {
            tracing::error!(%error,"Cannot save waveform definitions");
            "Could not save probes; try again.".into()
        })
    }
    fn set_waveform(&mut self, probes: Vec<Probe>, active: Option<(u64, usize)>) -> Result<(), String> {
        let next = self.world.prepare_waveform(probes, active)?;
        if next.probes != self.world.waveform.probes {
            self.save_waveform_definitions(&next.probes)?;
        }
        self.world.waveform = next;
        Ok(())
    }
    fn clear_waveform_history(&mut self) -> Result<String, String> {
        if self.world.waveform.capture.is_some() && self.world.piston_state.phase != AdvancePhase::BetweenTicks {
            return Err("Finish the current tick with /adv 1, then retry.".into());
        }
        self.world.reset_waveform();
        if let Some(capture) = &mut self.world.waveform.capture {
            capture.reason = Some("history_cleared");
        }
        Ok("Waveform history cleared; watches and recording state kept".into())
    }
    fn reset_waveform_settings(&mut self, confirmed: bool) -> Result<String, String> {
        if !confirmed {
            return Ok("This removes all watches and history; use /waveform reset confirm.".into());
        }
        self.set_waveform(Vec::new(), None)?;
        self.players.iter_mut().for_each(|player| player.redstone_tools.waveform = None);
        Ok("Waveform reset; watches and history removed, recording off".into())
    }
    pub(super) fn handle_waveform(&mut self, player: usize, args: &[&str]) -> Result<String, String> {
        check_syntax(args)?;
        if args == ["help"] {
            return Ok(HELP.into());
        }
        if args == ["cancel"] {
            self.players[player].redstone_tools.waveform = None;
            return Ok("Waveform selection cancelled; existing probes unchanged".into());
        }
        let action = args.first().copied().unwrap_or("status");
        self.check_waveform_access(player, action)?;
        let actor = &self.players[player];
        if !CONFIG.waveform_enabled {
            return Err("Waveform is disabled; ask staff to enable it.".into());
        }
        let enabled = self.world.waveform.enabled();
        let ticks = self.world.waveform.capture.as_ref().map_or(0, |c| c.ticks);
        match args {
            [] | ["status"] => Ok(format!(
                "Waveform: {}; {} probes; {} ticks; {} records; {}; {} bytes reserved; your allowance: {}",
                self.world.waveform.capture.as_ref().map_or("off", |c| c.state),
                self.world.waveform.probes.len(),
                ticks,
                self.world.waveform.capture.as_ref().map_or(0, |c| c.records.len()),
                self.world.waveform.capture.as_ref().and_then(|c| c.reason).unwrap_or("no stop reason"),
                self.world.waveform.capture.as_ref().map_or(0, |c| c.reserved),
                actor
                    .waveform_limits()
                    .map(|(signals, ticks)| format!("{signals} signals, {ticks} ticks"))
                    .unwrap_or_else(|_| "unavailable".into())
            )),
            ["list"] => Ok(self
                .world
                .waveform
                .probes
                .iter()
                .map(|p| {
                    format!(
                        "{}: ({}, {}, {}) = {}",
                        p.name,
                        p.pos.x,
                        p.pos.y,
                        p.pos.z,
                        match signal_value(&self.world, p.pos) {
                            Some(true) => "On",
                            Some(false) => "Off",
                            None => "unavailable",
                        }
                    )
                })
                .collect::<Vec<_>>()
                .join("\n")),
            ["web"] => {
                let mut url = reqwest::Url::parse(&CONFIG.waveform_public_url).map_err(|_| "Viewer URL is invalid; ask staff.")?;
                url.query_pairs_mut()
                    .append_pair("x", &self.world.x.to_string())
                    .append_pair("y", &self.world.z.to_string());
                Ok(url.to_string())
            }
            ["off"] => {
                self.players[player].redstone_tools.waveform = None;
                self.world.waveform.off();
                Ok("Waveform disabled; captured history/events discarded, watches kept".into())
            }
            ["reset"] => self.reset_waveform_settings(false),
            ["reset", "confirm"] => self.reset_waveform_settings(true),
            ["on"] | ["on", _] => {
                if self.redpiler.is_active() || self.compiling_redpiler() {
                    return Err("Use /rp reset before starting waveform recording.".into());
                }
                let (signals, limit) = actor.waveform_limits()?;
                if limit == 0 {
                    return Err(tick_limit_error(limit));
                }
                let ticks = match args.get(1) {
                    Some(text) => text.parse::<u64>().map_err(|_| "Ticks must be a positive whole number.")?,
                    None => limit.min(1000) as u64,
                };
                if ticks == 0 {
                    return Err("Tick count must be at least 1.".into());
                }
                if ticks > limit as u64 {
                    return Err(tick_limit_error(limit));
                }
                if self.world.waveform.probes.len() > signals {
                    return Err(format!(
                        "Probe limit is {signals}; remove {} probes first.",
                        self.world.waveform.probes.len() - signals
                    ));
                }
                self.set_waveform(self.world.waveform.probes.clone(), Some((ticks, signals)))?;
                Ok(format!(
                    "Waveform enabled for up to {ticks} ticks; old capture discarded. Command-block output is public."
                ))
            }
            ["clear"] => self.clear_waveform_history(),
            ["add", name, ..] => {
                let (signals, limit) = actor.waveform_limits()?;
                if self.world.waveform.probes.len() >= signals {
                    return Err(probe_limit_error(signals));
                }
                if enabled && ticks > limit as u64 {
                    return Err(tick_limit_error(limit));
                }
                if !Probe::valid_name(name) {
                    return Err("Use letters, digits or _; start with a letter or _ (max 32).".into());
                }
                let pos = match args {
                    ["add", _] => worldedit::ray_trace_block(&self.world, actor.eye_position(), actor.pitch as f64, actor.yaw as f64, 10.0)
                        .ok_or("Look at a block within 10 blocks.")?,
                    ["add", _, x, y, z] => BlockPos::new(
                        x.parse().map_err(|_| "X must be a whole block coordinate.")?,
                        y.parse().map_err(|_| "Y must be a whole block coordinate.")?,
                        z.parse().map_err(|_| "Z must be a whole block coordinate.")?,
                    ),
                    _ => unreachable!("validated waveform arguments"),
                };
                self.check_waveform_block(pos)?;
                if self.world.waveform.probes.iter().any(|p| p.name == *name) {
                    return Err(format!("Probe '{name}' exists; choose another name."));
                }
                if let Some(probe) = self.world.waveform.probes.iter().find(|p| p.pos == pos) {
                    return Err(format!("Already watched as '{}'; choose another block.", probe.name));
                }
                let mut probes = self.world.waveform.probes.clone();
                probes.push(Probe { name: (*name).into(), pos });
                self.set_waveform(probes, enabled.then_some((ticks, signals)))?;
                Ok(format!(
                    "Added {name}; {}",
                    if enabled {
                        "new capture started; old history discarded"
                    } else {
                        "capture is off; use /waveform on"
                    }
                ))
            }
            ["addbulk", name, bits] => {
                let bits = bits.parse::<usize>().map_err(|_| "Bit count must be a positive whole number.")?;
                self.begin_waveform_selection(player, name, bits)
            }
            ["remove", name] => {
                let probes: Vec<_> = self.world.waveform.probes.iter().filter(|p| p.name != *name).cloned().collect();
                if probes.len() == self.world.waveform.probes.len() {
                    return Err("No such probe; use /waveform list.".into());
                }
                if enabled {
                    if self.world.piston_state.phase != AdvancePhase::BetweenTicks {
                        return Err("Finish the current tick with /adv 1, then retry.".into());
                    }
                    self.save_waveform_definitions(&probes)?;
                    self.world.waveform.replace(probes);
                    self.world.reset_waveform();
                } else {
                    self.set_waveform(probes, None)?;
                }
                Ok("Probe removed; active history discarded if recording".into())
            }
            _ => unreachable!("validated waveform arguments"),
        }
    }

    fn begin_waveform_selection(&mut self, player: usize, name: &str, bits: usize) -> Result<String, String> {
        self.validate_waveform_batch(player, name, bits)?;
        self.players[player].redstone_tools.waveform = Some(Selection {
            plot: (self.world.x, self.world.z),
            name: name.into(),
            bits,
            positions: Vec::with_capacity(bits),
        });
        Ok(format!(
            "Left-click {bits} blocks, low bit first: {name}_0 through {name}_{}. Use /waveform cancel to exit. Existing probes stay unchanged until the last click.",
            bits - 1
        ))
    }

    fn validate_waveform_batch(&self, player: usize, name: &str, bits: usize) -> Result<(usize, u64), String> {
        let (signals, limit) = self.players[player].waveform_limits()?;
        let ticks = self
            .world
            .waveform
            .capture
            .as_ref()
            .filter(|_| self.world.waveform.enabled())
            .map_or(0, |c| c.ticks);
        if bits == 0 {
            return Err("Bit count must be at least 1.".into());
        }
        let remaining = signals.saturating_sub(self.world.waveform.probes.len());
        if remaining == 0 {
            return Err(probe_limit_error(signals));
        }
        if bits > remaining {
            return Err(format!("Only {remaining} probe slots left; choose fewer bits."));
        }
        if ticks > limit as u64 {
            return Err(tick_limit_error(limit));
        }
        if !Probe::valid_name(name) {
            return Err("Use letters, digits or _; start with a letter or _ (max 32).".into());
        }
        if !Probe::valid_name(&format!("{name}_{}", bits - 1)) {
            return Err("Name too long; leave room for _<bit> (32 characters total).".into());
        }
        let names: HashSet<_> = self.world.waveform.probes.iter().map(|p| p.name.as_str()).collect();
        if (0..bits).any(|bit| names.contains(format!("{name}_{bit}").as_str())) {
            return Err("A bulk probe name exists; choose another base name.".into());
        }
        Ok((signals, ticks))
    }

    pub(super) fn select_waveform_bit(&mut self, player: usize, pos: BlockPos) -> Result<String, String> {
        let actor = &self.players[player];
        let Some(selection) = &actor.redstone_tools.waveform else {
            return Err("Start selection with /waveform addbulk <name> <bits>.".into());
        };
        if selection.plot != (self.world.x, self.world.z) {
            self.players[player].redstone_tools.waveform = None;
            return Err("Selection cancelled after changing plots; start again.".into());
        }
        if let Err(error) = self.check_waveform_access(player, "addbulk") {
            self.players[player].redstone_tools.waveform = None;
            return Err(format!("Selection cancelled: {error}"));
        }
        if self.git_checkout_locked() {
            return Err("Wait for the plot checkout to finish, then retry.".into());
        }
        let (signals, ticks) = self.validate_waveform_batch(player, &selection.name, selection.bits)?;
        self.check_waveform_block(pos)?;
        if !self.container_in_reach(player, pos) {
            return Err("Move closer to the block, then click again.".into());
        }
        if selection.positions.contains(&pos) {
            return Err("That block is selected; click another.".into());
        }
        if let Some(probe) = self.world.waveform.probes.iter().find(|p| p.pos == pos) {
            return Err(format!("Already watched as '{}'; click another block.", probe.name));
        }
        if let Some(probe) = self.world.waveform.probes.iter().find(|p| selection.positions.contains(&p.pos)) {
            return Err(format!("Selected block is now watched as '{}'; cancel and retry.", probe.name));
        }
        let mut selection = self.players[player].redstone_tools.waveform.take().unwrap();
        selection.positions.push(pos);
        if selection.positions.len() < selection.bits {
            let message = format!(
                "Selected {}_{} at ({}, {}, {}); next: {}_{} ({}/{})",
                selection.name,
                selection.positions.len() - 1,
                pos.x,
                pos.y,
                pos.z,
                selection.name,
                selection.positions.len(),
                selection.positions.len(),
                selection.bits
            );
            self.players[player].redstone_tools.waveform = Some(selection);
            return Ok(message);
        }
        let mut probes = self.world.waveform.probes.clone();
        probes.extend(selection.positions.iter().enumerate().map(|(bit, &pos)| Probe {
            name: format!("{}_{bit}", selection.name),
            pos,
        }));
        if let Err(error) = self.set_waveform(probes, self.world.waveform.enabled().then_some((ticks, signals))) {
            selection.positions.pop();
            self.players[player].redstone_tools.waveform = Some(selection);
            return Err(error);
        }
        Ok(format!(
            "Added {} probes: {}_0 through {}_{}. Selection finished; {}",
            selection.bits,
            selection.name,
            selection.name,
            selection.bits - 1,
            if self.world.waveform.enabled() {
                "new capture started; old history discarded"
            } else {
                "capture is off; use /waveform on"
            }
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mchprs_network::packets::serverbound::{SPlayerBlockPlacemnt, SPlayerDigging, ServerBoundPacketHandler};

    fn fixture() -> (Plot, std::net::TcpStream, [BlockPos; 3]) {
        let (mut plot, peer) = super::super::client_sync_tests::fixture(false);
        plot.owner = Some(plot.players[0].uuid);
        plot.players[0].set_test_permissions(&["mchprs.access.commands", "commands.waveform", "commands.waveform.addbulk"]);
        let positions = [BlockPos::new(32, 21, 34), BlockPos::new(33, 21, 34), BlockPos::new(34, 21, 34)];
        for pos in positions {
            plot.world.set_block(pos, Block::from_name("lever").unwrap());
        }
        (plot, peer, positions)
    }

    #[test]
    fn waveform_incomplete_commands_have_specific_single_line_errors() {
        let (mut plot, _peer, _) = fixture();
        for (args, expected) in [
            (vec!["addbulk", "test"], "Missing bit count; use /waveform addbulk test <bits>."),
            (vec!["addbulk"], "Usage: /waveform addbulk <name> <bits>."),
            (vec!["add"], "Usage: /waveform add <name> [x y z]."),
            (vec!["add", "test", "1"], "Usage: /waveform add <name> [x y z]."),
            (vec!["remove"], "Usage: /waveform remove <name>."),
            (vec!["on", "1", "2"], "Usage: /waveform on [ticks]."),
            (vec!["oops"], "Unknown waveform action; use /waveform help."),
            (vec!["addbulk", "bad\nname"], "Usage: /waveform addbulk <name> <bits>."),
        ] {
            let error = plot.handle_waveform(0, &args).unwrap_err();
            assert_eq!(error, expected);
            assert_eq!(error.lines().count(), 1);
        }
        assert_eq!(plot.handle_waveform(0, &["help"]).unwrap(), HELP);
    }

    #[test]
    fn waveform_command_hints_require_complete_arguments() {
        let mut nodes = Vec::new();
        let root = command_node(&mut nodes) as usize;
        let find = |parent: usize, name: &str| -> usize {
            *nodes[parent].children.iter().find(|&&id| nodes[id as usize].name == Some(name)).unwrap() as usize
        };
        let bulk = find(root, "addbulk");
        let name = find(bulk, "name");
        let bits = find(name, "bits");
        assert_eq!(nodes[bulk].flags & 4, 0);
        assert_eq!(nodes[name].flags & 4, 0);
        assert_ne!(nodes[bits].flags & 4, 0);
        assert!(matches!(nodes[bits].parser, Some(Parser::Integer(1, 1024))));
        assert_eq!(nodes[bits].suggestions_type, Some("minecraft:ask_server"));
        let name = find(find(root, "add"), "name");
        assert_ne!(nodes[name].flags & 4, 0);
        let x = find(name, "x");
        let y = find(x, "y");
        let z = find(y, "z");
        assert_eq!(nodes[x].flags & 4, 0);
        assert_eq!(nodes[y].flags & 4, 0);
        assert_ne!(nodes[z].flags & 4, 0);
        let reset = find(root, "reset");
        assert_ne!(nodes[reset].flags & 4, 0);
        assert_ne!(nodes[find(reset, "confirm")].flags & 4, 0);
        assert!(nodes.iter().flat_map(|node| node.children).all(|&id| (id as usize) < nodes.len()));
    }

    #[test]
    fn waveform_completion_filters_permissions_limits_and_utf16_ranges() {
        let (mut plot, _peer, positions) = fixture();
        plot.players[0].set_test_permissions(&["mchprs.access.commands", "commands.waveform", "commands.waveform.*"]);
        plot.world.waveform.replace(vec![Probe {
            name: "data_0".into(),
            pos: positions[0],
        }]);
        let result = plot.complete_waveform(0, 42, "/waveform remove da").unwrap();
        assert_eq!((result.id, result.start, result.length), (42, 17, 2));
        assert_eq!(result.matches[0].match_, "data_0");
        let result = plot.complete_waveform(0, 43, "/waveform remove 😀").unwrap();
        assert_eq!(result.length, 2);
        assert!(result.matches.is_empty());
        let (signals, ticks) = plot.players[0].waveform_limits().unwrap();
        plot.world.waveform.replace(
            (0..signals - 2)
                .map(|bit| Probe {
                    name: format!("wire_{bit}"),
                    pos: positions[0],
                })
                .collect(),
        );
        let result = plot.complete_waveform(0, 44, "/waveform addbulk test ").unwrap();
        assert_eq!(result.matches.iter().map(|m| m.match_.as_str()).collect::<Vec<_>>(), ["1", "2"]);
        assert!(
            plot.complete_waveform(0, 45, "/waveform on ")
                .unwrap()
                .matches
                .iter()
                .all(|m| m.match_.parse::<usize>().unwrap() <= ticks)
        );
        plot.players[0].deny_test_permissions();
        assert!(plot.complete_waveform(0, 46, "/waveform remove ").unwrap().matches.is_empty());
        assert!(plot.complete_waveform(0, 47, "/waveform ad").unwrap().matches.is_empty());
        assert!(plot.complete_waveform(0, 48, " /waveform ").is_none());
    }

    #[test]
    fn waveform_completion_reaches_the_chat_client() {
        use mchprs_network::packets::PacketDecoderExt;
        use mchprs_network::packets::serverbound::STabComplete;
        let (mut plot, mut peer, positions) = fixture();
        plot.players[0].set_test_permissions(&["mchprs.access.commands", "commands.waveform", "commands.waveform.remove"]);
        plot.world.waveform.replace(vec![Probe {
            name: "ready".into(),
            pos: positions[0],
        }]);
        plot.handle_tab_complete(
            STabComplete {
                transaction_id: 42,
                text: "/waveform remove rea".into(),
            },
            0,
        );
        let (id, mut frame) = mchprs_network::test_support::read_frame(&mut peer, false).unwrap();
        assert_eq!(id, 0x0f);
        for expected in [42, 17, 3, 1] {
            assert_eq!(frame.read_varint().unwrap(), expected);
        }
        assert_eq!(frame.read_string().unwrap(), "ready");
        assert!(!frame.read_bool().unwrap());
        assert_eq!(frame.position() as usize, frame.get_ref().len());
    }

    #[test]
    fn waveform_clear_preserves_watches_limits_memory_and_recording_but_reset_requires_confirmation() {
        let (mut plot, _peer, positions) = fixture();
        plot.world
            .enable_waveform_for_test(
                vec![Probe {
                    name: "clock".into(),
                    pos: positions[0],
                }],
                10,
            )
            .unwrap();
        plot.world.waveform.text(positions[0], r#"{"text":"old output"}"#, 0);
        let old = plot.world.waveform.capture.as_ref().unwrap();
        assert_eq!(old.records.len(), 1);
        let (old_id, reserved) = (old.id, old.reserved);
        plot.begin_waveform_selection(0, "data", 2).unwrap();
        assert!(plot.reset_waveform_settings(false).unwrap().contains("/waveform reset confirm"));
        assert_eq!(plot.world.waveform.capture.as_ref().unwrap().id, old_id);
        assert_eq!(plot.world.waveform.capture.as_ref().unwrap().records.len(), 1);
        assert_eq!(plot.world.waveform.probes.len(), 1);
        assert!(plot.players[0].redstone_tools.waveform.is_some());
        assert!(check_syntax(&["reset", "yes"]).is_err());

        plot.clear_waveform_history().unwrap();
        let fresh = plot.world.waveform.capture.as_ref().unwrap();
        assert!(fresh.records.is_empty());
        assert_ne!(fresh.id, old_id);
        assert_eq!((fresh.ticks, fresh.reserved), (10, reserved));
        assert!(plot.world.waveform.enabled());
        assert_eq!(plot.world.waveform.probes[0].name, "clock");
        assert!(plot.players[0].redstone_tools.waveform.is_some());
        plot.world.waveform.text(positions[0], r#"{"text":"new output"}"#, 0);
        assert_eq!(plot.world.waveform.capture.as_ref().unwrap().records.len(), 1);

        plot.players[0].set_test_permissions(&["mchprs.access.commands", "commands.waveform", "commands.waveform.clear"]);
        assert_eq!(
            plot.handle_waveform(0, &["reset", "confirm"]).unwrap_err(),
            "No permission for /waveform reset; ask staff."
        );
        assert_eq!(plot.world.waveform.probes.len(), 1);
        plot.reset_waveform_settings(true).unwrap();
        assert!(plot.world.waveform.probes.is_empty());
        assert!(plot.world.waveform.capture.is_none());
        assert!(plot.players[0].redstone_tools.waveform.is_none());
    }

    #[test]
    fn waveform_bulk_clicks_are_non_destructive_ordered_and_commit_together() {
        let (mut plot, _peer, positions) = fixture();
        plot.begin_waveform_selection(0, "data", 3).unwrap();
        plot.handle_player_block_placement(
            SPlayerBlockPlacemnt {
                hand: 0,
                pos: positions[0].packed(),
                face: 1,
                cursor_x: 0.5,
                cursor_y: 0.5,
                cursor_z: 0.5,
                inside_block: false,
                sequence: 1,
            },
            0,
        );
        assert!(plot.players[0].redstone_tools.waveform.as_ref().unwrap().positions.is_empty());
        for (bit, pos) in positions.into_iter().enumerate() {
            plot.handle_player_digging(
                SPlayerDigging {
                    status: 0,
                    pos: pos.packed(),
                    face: 1,
                    sequence: bit as i32 + 2,
                },
                0,
            );
            assert_eq!(plot.world.get_block(pos), Block::from_name("lever").unwrap());
            if bit < 2 {
                assert!(plot.world.waveform.probes.is_empty());
            }
        }
        assert!(plot.players[0].redstone_tools.waveform.is_none());
        assert_eq!(
            plot.world.waveform.probes.iter().map(|p| (p.name.as_str(), p.pos)).collect::<Vec<_>>(),
            vec![("data_0", positions[0]), ("data_1", positions[1]), ("data_2", positions[2])]
        );
        assert!(plot.world.waveform.capture.is_none());
    }

    #[test]
    fn waveform_bulk_rejects_invalid_targets_duplicates_and_permission_loss() {
        let (mut plot, _peer, positions) = fixture();
        assert!(plot.begin_waveform_selection(0, "data", 0).is_err());
        assert!(plot.begin_waveform_selection(0, "data", usize::MAX).is_err());
        assert!(plot.begin_waveform_selection(0, &"a".repeat(32), 2).is_err());
        plot.begin_waveform_selection(0, "data", 2).unwrap();
        assert!(plot.select_waveform_bit(0, BlockPos::new(100, 21, 100)).is_err());
        plot.select_waveform_bit(0, positions[0]).unwrap();
        assert!(plot.select_waveform_bit(0, positions[0]).is_err());
        assert_eq!(plot.players[0].redstone_tools.waveform.as_ref().unwrap().positions.len(), 1);
        plot.players[0].deny_test_permissions();
        assert!(plot.select_waveform_bit(0, positions[1]).is_err());
        assert!(plot.players[0].redstone_tools.waveform.is_none());
        assert!(plot.world.waveform.probes.is_empty());
    }

    #[test]
    fn waveform_bulk_rechecks_conflicts_before_commit_and_cancel_keeps_existing_probes() {
        let (mut plot, _peer, positions) = fixture();
        plot.begin_waveform_selection(0, "data", 2).unwrap();
        plot.select_waveform_bit(0, positions[0]).unwrap();
        plot.world.waveform.replace(vec![Probe {
            name: "data_1".into(),
            pos: positions[2],
        }]);
        assert!(plot.select_waveform_bit(0, positions[1]).is_err());
        assert_eq!(plot.world.waveform.probes.len(), 1);
        plot.world.waveform.replace(vec![Probe {
            name: "other".into(),
            pos: positions[0],
        }]);
        assert!(
            plot.select_waveform_bit(0, positions[1])
                .unwrap_err()
                .contains("Selected block is now watched")
        );
        plot.world.waveform.replace(
            (0..plot.players[0].waveform_limits().unwrap().0)
                .map(|bit| Probe {
                    name: format!("existing_{bit}"),
                    pos: BlockPos::new(40 + (bit % 16) as i32, 21, 40 + (bit / 16) as i32),
                })
                .collect(),
        );
        assert!(plot.select_waveform_bit(0, positions[1]).unwrap_err().contains("Probe limit reached"));
        plot.handle_waveform(0, &["cancel"]).unwrap();
        assert!(plot.players[0].redstone_tools.waveform.is_none());
        assert_eq!(plot.world.waveform.probes[0].name, "existing_0");
    }

    #[test]
    fn waveform_bulk_recording_restarts_once_after_all_bits_are_selected() {
        let (mut plot, _peer, positions) = fixture();
        plot.world
            .enable_waveform_for_test(
                vec![Probe {
                    name: "old".into(),
                    pos: positions[2],
                }],
                10,
            )
            .unwrap();
        let id = plot.world.waveform.capture.as_ref().unwrap().id;
        plot.begin_waveform_selection(0, "data", 2).unwrap();
        plot.select_waveform_bit(0, positions[0]).unwrap();
        assert_eq!(plot.world.waveform.capture.as_ref().unwrap().id, id);
        plot.select_waveform_bit(0, positions[1]).unwrap();
        assert_ne!(plot.world.waveform.capture.as_ref().unwrap().id, id);
        assert_eq!(plot.world.waveform.probes.len(), 3);
        assert!(plot.world.waveform.enabled());
    }
}
