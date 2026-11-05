//! Binary movement tables. Every offset in a row is an index into its named table.
use super::{AUX_TABLES, Program, ScratchError, encode_notification, overflow};
use crate::redpiler::sampled::program::ArrangementSource;
use mchprs_blocks::blocks::Block;

fn id(value: usize) -> Result<u32, ScratchError> {
    u32::try_from(value).map_err(|_| overflow())
}
fn optional(value: Option<usize>) -> Result<u32, ScratchError> {
    value.map(id).transpose().map(|n| n.unwrap_or(u32::MAX))
}
pub(super) fn source(value: ArrangementSource) -> Result<u32, ScratchError> {
    let (value, tag) = match value {
        ArrangementSource::Component(n) => (n, 0),
        ArrangementSource::Token(n) => (n, 0x8000_0000),
    };
    if value >= 0x8000_0000 {
        return Err(overflow());
    }
    Ok(id(value)? | tag)
}
fn range(value: &std::ops::Range<usize>) -> Result<[u32; 2], ScratchError> {
    Ok([id(value.start)?, id(value.len())?])
}

pub(super) fn encode_movement(p: &Program, t: &mut [Vec<u32>; AUX_TABLES]) -> Result<(), ScratchError> {
    for (i, piston) in p.pistons.iter().enumerate() {
        let b = &p.piston_bindings[i];
        t[7].extend([
            id(b.component)?,
            id(b.home_cell)?,
            id(b.far_cell)?,
            optional(b.mate)?,
            u32::from(piston.sticky),
            piston.facing as u32,
            u32::from(piston.extended),
            u32::from(piston.head_present),
            u32::from(b.home_blocked),
            u32::from(b.far_blocked),
        ]);
        for (table, row) in [(8, &b.home_heads), (9, &b.far_heads)] {
            let start = id(t[10].len())?;
            t[table].extend([start, id(row.len())?]);
            for &other in row {
                t[10].push(id(other)?);
            }
        }
    }
    for (index, token) in p.tokens.iter().enumerate() {
        let initial = token.position.and_then(|pos| p.projection_indices.get(&pos).copied());
        t[11].extend([
            optional(initial)?,
            u32::from(matches!(token.block, Block::RedstoneBlock)) * 15,
            optional(initial)?,
        ]);
        let start = id(t[13].len())?;
        t[12].extend([start, id(p.token_cells[index].len())?]);
        for &cell in &p.token_cells[index] {
            t[13].push(id(cell)?);
        }
        let row = &p.token_piston_components[index];
        let start = id(t[15].len())?;
        t[14].extend([start, id(row.len())?]);
        for &node in row {
            t[15].push(id(node)?);
        }
        let start = id(t[19].len() / 6)?;
        t[18].extend([start, id(token.cells.len())?]);
        for cell in 0..token.cells.len() {
            t[19].extend(range(&p.movement_cells[index][cell])?);
            t[19].extend(range(&p.move_steps[index][cell][0])?);
            t[19].extend(range(&p.move_steps[index][cell][1])?);
        }
    }
    for cell in 0..p.projection_positions.len() {
        t[16].push(optional(p.cell_components[cell].map(|n| n.get() - 1))?);
        let start = id(t[17].len())?;
        t[16].extend([start, id(p.cell_tokens[cell].len())?]);
        for &token in &p.cell_tokens[cell] {
            t[17].push(id(token)?);
        }
        let start = id(t[17].len())?;
        t[16].extend([start, id(p.cell_pistons[cell].len())?]);
        for &piston in &p.cell_pistons[cell] {
            t[17].push(id(piston)?);
        }
        let observers: Vec<_> = p.cell_observers[cell].iter().copied().filter(|&node| {
            matches!(p.initial[node], Block::Observer { observer } if p.positions[node].offset(observer.facing.into()) == p.projection_positions[cell])
        }).collect();
        let start = id(t[17].len())?;
        t[16].extend([start, id(observers.len())?]);
        for node in observers {
            t[17].push(id(node)?);
        }
    }
    for r in &p.movement_transitions {
        t[20].extend([id(r.group)?, id(r.delta)?, id(r.depart)?, id(r.arrive)?, id(r.states)?]);
    }
    for r in &p.movement_ranges {
        t[21].extend(range(r)?);
    }
    for r in &p.movement_flips {
        t[22].extend([
            id(r.target)?,
            source(r.source)?,
            u32::from(r.attenuation),
            id(r.link)?,
            u32::from(r.entering),
            u32::from(r.side),
        ]);
    }
    for r in &p.movement_steps {
        let wires = range(&r.wires)?;
        t[23].extend([
            encode_notification(p, r.node)?,
            u32::from(r.watcher),
            u32::from(r.wire),
            wires[0],
            wires[1],
        ]);
    }
    for &wire in &p.movement_wires {
        t[24].push(id(wire)?);
    }
    for r in &p.arrangement_links {
        let start = id(t[26].len())?;
        t[25].extend([
            id(r.target)?,
            u32::from(r.side),
            u32::from(r.silent),
            source(r.source)?,
            u32::from(r.attenuation),
            id(r.group)?,
            start,
            id(r.active.len())?,
        ]);
        for &active in &r.active {
            t[26].push(u32::from(active));
        }
    }
    for group in &p.movement_groups {
        let mut state = 0usize;
        for axis in &group.axes {
            let token = &p.tokens[axis.token];
            let digit = token
                .position
                .and_then(|pos| token.cells.iter().position(|&cell| cell == pos))
                .map_or(0, |cell| axis.states[cell + 1]);
            state = state
                .checked_add(digit.checked_mul(axis.stride).ok_or_else(overflow)?)
                .ok_or_else(overflow)?;
        }
        t[27].push(id(state)?);
    }
    for row in &p.outgoing_arrangements {
        let start = id(t[29].len())?;
        t[28].extend([start, id(row.len())?]);
        for &link in row {
            t[29].push(id(link)?);
        }
    }
    for &moving in &p.movable_sources {
        t[30].push(u32::from(moving));
    }
    for index in 0..p.pistons.len() {
        let row = p.home_watchers.get(index).map_or(&[][..], |row| row.as_slice());
        let start = id(t[4].len())?;
        t[31].extend([start, id(row.len())?]);
        for &node in row {
            t[4].push(encode_notification(p, node)?);
        }
    }
    for &cell in &p.component_cells {
        t[32].push(id(cell)?);
    }
    Ok(())
}

#[cfg(any(windows, target_os = "linux"))]
impl super::LlvmRuntime {
    pub(in crate::redpiler) fn sync_projection(&self, runtime: &mut crate::redpiler::sampled::Runtime) -> Result<(), ScratchError> {
        self.sync(runtime, true)
    }

    pub(in crate::redpiler) fn sync_publication(&self, runtime: &mut crate::redpiler::sampled::Runtime) -> Result<(), ScratchError> {
        self.sync(runtime, false)
    }

    fn sync(&self, runtime: &mut crate::redpiler::sampled::Runtime, for_execution: bool) -> Result<(), ScratchError> {
        use crate::redpiler::sampled::executor::{Motion, PistonEvent};
        use std::num::NonZeroUsize;
        let invalid = || ScratchError {
            position: None,
            reason: "invalid LLVM graph snapshot",
        };
        let words = unsafe { (self.library.snapshot_words)(self.state.as_ptr().cast()) } as usize;
        let program = std::sync::Arc::clone(&runtime.program);
        let p = &program;
        let capacity = p.pistons.len() * 2 + p.tokens.len();
        let electrical = p.links.len() + p.arrangement_links.len() + p.movement_groups.len() + p.watched_wires.len() + p.initial.len() * 3;
        let limit = 8
            + p.initial.len()
            + p.tokens.len()
            + p.pistons.len()
            + capacity * 9
            + p.pistons.len() * 6
            + electrical
            + p.reactive_wires.len()
            + 1
            + p.initial.len() * 5;
        if words < 8 || words > limit {
            return Err(invalid());
        }
        let mut snapshot = vec![0u32; words];
        self.call_status(unsafe { (self.library.read_snapshot)(self.state.as_ptr().cast(), snapshot.as_mut_ptr(), words as u32) })?;
        let s = &snapshot;
        let n = s[0] as usize;
        let tokens = s[1] as usize;
        let pistons = s[2] as usize;
        let motions = s[3] as usize;
        let events = s[4] as usize;
        let clock = u64::from(s[5]) | (u64::from(s[6]) << 32);
        if n != p.initial.len()
            || tokens != p.tokens.len()
            || pistons != p.pistons.len()
            || runtime.reactive_wire_strengths.len() != p.reactive_wires.len()
            || runtime.note_queued.len() != n
            || motions > capacity
            || events > pistons * 3
            || s[7] != 6
            || 8 + n + tokens + pistons + motions * 9 + events * 2 + electrical + p.reactive_wires.len() + 1 + n * 2 > words
        {
            return Err(invalid());
        }
        let token_start = 8 + n;
        let piston_start = token_start + tokens;
        let motion_start = piston_start + pistons;
        let event_end = motion_start + motions * 9 + events * 2;
        for (node, &state) in s[8..token_start].iter().enumerate() {
            let kind = super::component_state(p.initial[node]).0;
            if state
                > match kind {
                    4 | 13 => 3,
                    5 => 15,
                    _ => 1,
                }
            {
                return Err(invalid());
            }
        }
        let mut token_digits = Vec::with_capacity(tokens);
        for (token, &cell) in s[token_start..piston_start].iter().enumerate() {
            if cell != u32::MAX && cell as usize >= p.projection_positions.len() {
                return Err(invalid());
            }
            token_digits.push(if cell == u32::MAX {
                0
            } else {
                p.token_cells[token]
                    .iter()
                    .position(|&candidate| candidate == cell as usize)
                    .ok_or_else(invalid)?
                    + 1
            });
        }
        if s[piston_start..motion_start].iter().any(|&flags| flags > 15) {
            return Err(invalid());
        }
        for row in s[motion_start..motion_start + motions * 9].chunks_exact(9) {
            if row[0] as usize >= pistons
                || (row[1] != u32::MAX && row[1] as usize >= tokens)
                || row[2] as usize >= p.projection_positions.len()
                || row[4] > 7
            {
                return Err(invalid());
            }
        }
        for row in s[motion_start + motions * 9..event_end].chunks_exact(2) {
            if row[0] as usize >= pistons || row[1] > 2 {
                return Err(invalid());
            }
        }
        // Snapshot 6 appends wire sides after the queued note metadata.
        let reactive_start = event_end + electrical;
        let note_count_offset = reactive_start + p.reactive_wires.len();
        let note_count = s[note_count_offset] as usize;
        let instruments_start = note_count_offset + 1;
        let note_events_start = instruments_start + n;
        let sides_start = words - n;
        if note_count > n || note_events_start + note_count * 3 != sides_start {
            return Err(invalid());
        }
        let wire_sides = &s[sides_start..];
        for (node, &sides) in wire_sides.iter().enumerate() {
            if sides > 255
                || (0..4).any(|side| (sides >> (side * 2)) & 3 == 3)
                || (sides != 0 && !matches!(p.initial[node], Block::RedstoneWire { .. }))
            {
                return Err(invalid());
            }
        }
        let instruments = &s[instruments_start..note_events_start];
        if instruments.iter().any(|&instrument| instrument > 15) || s[reactive_start..note_count_offset].iter().any(|&strength| strength > 15) {
            return Err(invalid());
        }
        let mut seen_notes = vec![false; n];
        for row in s[note_events_start..sides_start].chunks_exact(3) {
            let component = row[0] as usize;
            if component >= n || !matches!(p.initial[component], Block::NoteBlock { .. }) || seen_notes[component] || row[1] > 15 || row[2] > 24 {
                return Err(invalid());
            }
            seen_notes[component] = true;
        }
        let fixed_geometry = p.tokens.is_empty() && p.pistons.is_empty();
        let previous_cells = (!fixed_geometry).then(|| {
            (0..p.projection_positions.len())
                .map(|cell| runtime.project_cell(cell))
                .collect::<Vec<_>>()
        });
        let previous_comparators = (!fixed_geometry).then(|| runtime.comparator_outputs.clone());
        for (node, &state) in s[8..token_start].iter().enumerate() {
            let block = match p.initial[node] {
                Block::Lever { mut lever } => {
                    lever.powered = state != 0;
                    Block::Lever { lever }
                }
                Block::StoneButton { mut button } => {
                    button.powered = state != 0;
                    Block::StoneButton { button }
                }
                Block::RedstoneLamp { .. } => Block::RedstoneLamp { lit: state != 0 },
                Block::IronTrapdoor { facing, half, .. } => Block::IronTrapdoor {
                    facing,
                    half,
                    powered: state != 0,
                },
                block if block.is_copper_bulb() => block.with_copper_bulb_state(state & 1 != 0, state & 2 != 0).ok_or_else(invalid)?,
                Block::RedstoneComparator { mut comparator } => {
                    comparator.powered = state != 0;
                    Block::RedstoneComparator { comparator }
                }
                Block::Hopper { facing, .. } => Block::Hopper { facing, enabled: state != 0 },
                block if block.pressure_plate_powered().is_some() => block.with_pressure_plate_power(state != 0).ok_or_else(invalid)?,
                Block::NoteBlock { note, .. } => Block::NoteBlock {
                    instrument: mchprs_blocks::blocks::Instrument::from_id(instruments[node]),
                    note,
                    powered: state != 0,
                },
                Block::RedstoneRepeater { mut repeater } => {
                    repeater.powered = state & 1 != 0;
                    repeater.locked = state & 2 != 0;
                    Block::RedstoneRepeater { repeater }
                }
                Block::RedstoneWire { mut wire } => {
                    use mchprs_blocks::blocks::RedstoneWireSide;
                    let sides = wire_sides[node];
                    wire.north = RedstoneWireSide::from_id(sides & 3);
                    wire.south = RedstoneWireSide::from_id((sides >> 2) & 3);
                    wire.east = RedstoneWireSide::from_id((sides >> 4) & 3);
                    wire.west = RedstoneWireSide::from_id((sides >> 6) & 3);
                    wire.power = state as u8;
                    Block::RedstoneWire { wire }
                }
                Block::Observer { mut observer } => {
                    observer.powered = state != 0;
                    Block::Observer { observer }
                }
                Block::RedstoneTorch { .. } => Block::RedstoneTorch { lit: state != 0 },
                Block::RedstoneWallTorch { facing, .. } => Block::RedstoneWallTorch { facing, lit: state != 0 },
                Block::Piston { mut piston } => {
                    piston.extended = state != 0;
                    Block::Piston { piston }
                }
                block => block,
            };
            if fixed_geometry && runtime.blocks[node] != block {
                runtime.mark_dirty(p.component_cells[node]);
            }
            runtime.blocks[node] = block;
        }
        for (token, &cell) in s[token_start..piston_start].iter().enumerate() {
            runtime.token_positions[token] = (cell != u32::MAX).then(|| p.projection_positions[cell as usize]);
        }
        runtime.token_digits.copy_from_slice(&token_digits);
        for (piston, &flags) in s[piston_start..motion_start].iter().enumerate() {
            runtime.piston_extended[piston] = flags & 1 != 0;
            runtime.piston_head_present[piston] = flags & 2 != 0;
            runtime.piston_head_moving[piston] = flags & 4 != 0;
            runtime.piston_base_moving[piston] = flags & 8 != 0;
        }
        if for_execution {
            runtime.initialize_routing();
            if let Some(error) = runtime.failure.clone() {
                return Err(error);
            }
        }
        runtime.motion_time = clock;
        runtime.motions.clear();
        runtime.active_motions.clear();
        runtime.free_motion_slots.clear();
        runtime.piston_motion.fill(None);
        runtime.cell_motion.fill(None);
        for row in s[motion_start..motion_start + motions * 9].chunks_exact(9) {
            let index = runtime.motions.len();
            let motion = Motion {
                piston: row[0] as usize,
                token: (row[1] != u32::MAX).then_some(row[1] as usize),
                destination: row[2] as usize,
                order: row[3] as usize,
                extending: row[4] & 1 != 0,
                piston_done: row[4] & 2 != 0,
                token_done: row[4] & 4 != 0,
                start: u64::from(row[5]) | (u64::from(row[6]) << 32),
                landing: u64::from(row[7]) | (u64::from(row[8]) << 32),
            };
            if !motion.piston_done {
                runtime.piston_motion[motion.piston] = NonZeroUsize::new(index + 1);
            }
            if motion.token.is_some() && !motion.token_done {
                runtime.cell_motion[motion.destination] = NonZeroUsize::new(index + 1);
            }
            runtime.motions.push(motion);
            runtime.active_motions.push_back(index);
        }
        runtime.piston_events.clear();
        runtime.queued_piston_events.fill(0);
        for row in s[motion_start + motions * 9..event_end].chunks_exact(2) {
            let event = match row[1] {
                0 => PistonEvent::Extend,
                1 => PistonEvent::Retract,
                _ => PistonEvent::EarlyRetract,
            };
            runtime.piston_events.push_back((row[0] as usize, event));
            runtime.queued_piston_events[row[0] as usize] |= 1 << row[1];
        }
        let mut cursor = event_end;
        for values in [&mut runtime.contributions, &mut runtime.arrangement_contributions] {
            for value in values {
                *value = u8::try_from(s[cursor]).ok().filter(|&n| n <= 15).ok_or_else(invalid)?;
                cursor += 1;
            }
        }
        for value in &mut runtime.group_arrangements {
            *value = s[cursor] as usize;
            cursor += 1;
        }
        for value in &mut runtime.watched_strengths {
            *value = u8::try_from(s[cursor]).ok().filter(|&n| n <= 15).ok_or_else(invalid)?;
            cursor += 1;
        }
        for levels in &mut runtime.input_levels {
            levels[0].active = s[cursor];
            levels[1].active = s[cursor + 1];
            cursor += 2;
        }
        for node in 0..runtime.comparator_outputs.len() {
            let value = u8::try_from(s[cursor]).ok().filter(|&n| n <= 15).ok_or_else(invalid)?;
            if fixed_geometry && runtime.comparator_outputs[node] != value {
                runtime.mark_dirty(p.component_cells[node]);
            }
            runtime.comparator_outputs[node] = value;
            cursor += 1;
        }
        for value in &mut runtime.reactive_wire_strengths {
            *value = u8::try_from(s[cursor]).ok().filter(|&n| n <= 15).ok_or_else(invalid)?;
            cursor += 1;
        }
        if cursor != note_count_offset {
            return Err(invalid());
        }
        runtime.notes.clear();
        runtime.note_queued.fill(false);
        for row in s[note_events_start..sides_start].chunks_exact(3) {
            let component = row[0] as usize;
            runtime.note_queued[component] = true;
            runtime.notes.push((
                component,
                p.positions[component],
                mchprs_blocks::blocks::Instrument::from_id(row[1]),
                row[2],
            ));
        }
        if for_execution {
            let mut levels = runtime.input_levels.clone();
            for channels in &mut levels {
                channels[0].active = 0;
                channels[1].active = 0;
            }
            let mut histograms = vec![crate::redpiler::sampled::input::Histogram::default(); runtime.input_histograms.len()];
            for (index, link) in p.links.iter().enumerate() {
                if p.effective_links[index] {
                    levels[link.target][0].change(0, runtime.contributions[index], &mut histograms);
                }
            }
            for (index, link) in p.arrangement_links.iter().enumerate() {
                levels[link.target][usize::from(link.side)].change(0, runtime.arrangement_contributions[index], &mut histograms);
            }
            for (source, links) in p.side_outputs.iter().enumerate() {
                let strength = runtime.output(source);
                for &(target, attenuation) in links {
                    levels[target][1].change(0, strength.saturating_sub(attenuation), &mut histograms);
                }
            }
            if levels
                .iter()
                .zip(&runtime.input_levels)
                .any(|(actual, saved)| actual[0].active != saved[0].active || actual[1].active != saved[1].active)
            {
                return Err(invalid());
            }
            runtime.input_levels = levels;
            runtime.input_histograms = histograms;
        }
        if let (Some(previous_cells), Some(previous_comparators)) = (previous_cells, previous_comparators) {
            for (cell, previous) in previous_cells.into_iter().enumerate() {
                let comparator_changed = p.cell_components[cell].is_some_and(|index| {
                    let node = index.get() - 1;
                    previous_comparators[node] != runtime.comparator_outputs[node]
                });
                if previous != runtime.project_cell(cell) || comparator_changed {
                    runtime.mark_dirty(cell);
                }
            }
        }
        runtime.set_pending_with_capacity(self.pending_entries()?, self.pending_capacity as usize)?;
        self.sync_commands(runtime)?;
        Ok(())
    }
}
