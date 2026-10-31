//! Sample ordered source transitions and compare the lowered execution with scratch frames.
use super::*;
use std::time::{Duration, Instant};

#[derive(Default)]
struct SourceSamplingMetrics {
    enabled: bool,
    source_selection: Duration,
    loaded_arrangement: Duration,
    quiet_basis: Duration,
    scratch_setup: Duration,
    source_flips: Duration,
    frame_capture_comparison: Duration,
    ordered_lowering: Duration,
    candidate_sources: usize,
    racing_sources: usize,
    completed_sources: usize,
    static_links: usize,
    conditional_links: usize,
    program_starts: usize,
    quiet_component_walks: usize,
    setup_component_walks: usize,
    entry_state_walks: usize,
    captured_frames: usize,
    frame_comparison_walks: usize,
    observed_callbacks: usize,
}

impl SourceSamplingMetrics {
    fn add(&mut self, other: &Self) {
        self.scratch_setup += other.scratch_setup;
        self.source_flips += other.source_flips;
        self.frame_capture_comparison += other.frame_capture_comparison;
        self.ordered_lowering += other.ordered_lowering;
        self.completed_sources += other.completed_sources;
        self.setup_component_walks += other.setup_component_walks;
        self.entry_state_walks += other.entry_state_walks;
        self.captured_frames += other.captured_frames;
        self.frame_comparison_walks += other.frame_comparison_walks;
        self.observed_callbacks += other.observed_callbacks;
    }
}

impl Drop for SourceSamplingMetrics {
    fn drop(&mut self) {
        if self.enabled {
            eprintln!(
                "Redpiler source sampling: select={} ms loaded-arrangement={} ms quiet-basis={} ms scratch-restore/setup={} ms native-flips/callbacks={} ms frame-capture/comparison={} ms ordered-lowering={} ms; candidates={} racing={} completed={} static_links={} conditional_links={} program.start_calls={} full_component_walks={} (quiet={}, setup={}, frame_capture={}) entry_state_checks={} frame_comparison_walks={} frames={} observed_callbacks={}",
                self.source_selection.as_millis(),
                self.loaded_arrangement.as_millis(),
                self.quiet_basis.as_millis(),
                self.scratch_setup.as_millis(),
                self.source_flips.as_millis(),
                self.frame_capture_comparison.as_millis(),
                self.ordered_lowering.as_millis(),
                self.candidate_sources,
                self.racing_sources,
                self.completed_sources,
                self.static_links,
                self.conditional_links,
                self.program_starts,
                self.quiet_component_walks + self.setup_component_walks + self.captured_frames,
                self.quiet_component_walks,
                self.setup_component_walks,
                self.captured_frames,
                self.entry_state_walks,
                self.frame_comparison_walks,
                self.captured_frames,
                self.observed_callbacks
            );
        }
    }
}

#[derive(Clone, PartialEq, Eq)]
struct Frame {
    blocks: Vec<Block>,
    pending: Vec<mchprs_world::TickEntry>,
    deliveries: Vec<crate::world::RedstoneUpdateObservation>,
}
pub(in super::super) fn run(
    context: &Context<'_, impl World>,
    program: &mut Program,
    probe_indexes: &ProbeIndexes<'_>,
    phases: &mut CompilePhases,
) -> Result<(), ScratchError> {
    let world = context.world;
    let bounds = context.bounds;
    let entry = context.entry;
    let monitor = context.monitor;
    let source_count = program.sources.len();
    phases.next("source transition sampling");
    check_cancelled(monitor)?;
    let mut ordering_probe = ScratchWorld::from_world_monitored(world, bounds, entry.clone(), monitor)?;
    let mut sampling = SourceSamplingMetrics {
        enabled: phases.enabled,
        static_links: program.links.len() + program.side_links.len(),
        conditional_links: program.arrangement_links.len(),
        ..SourceSamplingMetrics::default()
    };
    phases.detail("source selection/index preparation", 0);
    let selection_started = Instant::now();
    let source_index = SourceIndex::new(&program, monitor)?;
    let output_sources: Vec<_> = program
        .sources
        .iter()
        .copied()
        .chain(program.repeaters.iter().map(|record| record.component))
        .chain(program.initial.iter().enumerate().filter_map(|(index, block)| {
            matches!(
                block,
                Block::RedstoneComparator { .. } | Block::Observer { .. } | Block::RedstoneTorch { .. } | Block::RedstoneWallTorch { .. }
            )
            .then_some(index)
        }))
        .collect();
    let racing_sources: std::collections::HashSet<_> = output_sources
        .iter()
        .copied()
        .filter(|&source| matches!(program.initial[source], Block::StoneButton { .. }) || source_index.feeds_two_racers(&program, source))
        .collect();
    sampling.source_selection = selection_started.elapsed();
    sampling.candidate_sources = output_sources.len();
    sampling.racing_sources = racing_sources.len();
    let mut comparisons = Vec::new();
    if !racing_sources.is_empty() {
        let source = 0;
        let mut groups_by_target = vec![Vec::new(); program.positions.len()];
        for (index, group) in program.input_arrangements.iter().enumerate() {
            if index % 1024 == 0 {
                check_cancelled(monitor)?;
            }
            groups_by_target[group.target].push(index);
        }
        let mut links_by_target = vec![Vec::new(); program.positions.len()];
        for (index, link) in program.arrangement_links.iter().enumerate() {
            if index % 1024 == 0 {
                check_cancelled(monitor)?;
            }
            links_by_target[link.target].push(index);
        }
        phases.detail("loaded-arrangement preparation", source);
        let arrangement_started = Instant::now();
        let loaded = program.start();
        sampling.loaded_arrangement += arrangement_started.elapsed();
        sampling.program_starts += 1;
        phases.detail("quiet-basis preparation", source);
        let quiet_started = Instant::now();
        for (id, &at) in program.positions.iter().enumerate() {
            if id % 1024 == 0 {
                check_cancelled(monitor)?;
            }
            let block = ordering_probe.world().get_block(at);
            let constant_power = program.incoming[id]
                .iter()
                .filter_map(|&link| {
                    let link = &program.links[link];
                    let conditional = groups_by_target[id].iter().any(|&group| {
                        let group = &program.input_arrangements[group];
                        group.target == id && !group.side && group.conditional_sources.contains(&ArrangementSource::Component(link.source))
                    });
                    ((matches!(program.initial[link.source], Block::RedstoneBlock)
                        || program.initial[link.source].pressure_plate_powered() == Some(true))
                        && !conditional)
                        .then_some(15u8.saturating_sub(link.attenuation))
                })
                .chain(links_by_target[id].iter().filter_map(|&index| {
                    let link = &program.arrangement_links[index];
                    let constant = match link.source {
                        ArrangementSource::Token(_) => true,
                        ArrangementSource::Component(node) => {
                            matches!(program.initial[node], Block::RedstoneBlock) || program.initial[node].pressure_plate_powered() == Some(true)
                        }
                    };
                    (link.target == id && !link.side && constant && link.active[loaded.group_arrangements[link.group]])
                        .then_some(15u8.saturating_sub(link.attenuation))
                }))
                .max()
                .unwrap_or(0);
            #[cfg(test)]
            if program.positions.len() < 1000 {
                let original = program.incoming[id]
                    .iter()
                    .filter_map(|&index| {
                        let link = &program.links[index];
                        let conditional = program.input_arrangements.iter().any(|group| {
                            group.target == id && !group.side && group.conditional_sources.contains(&ArrangementSource::Component(link.source))
                        });
                        ((matches!(program.initial[link.source], Block::RedstoneBlock)
                            || program.initial[link.source].pressure_plate_powered() == Some(true))
                            && !conditional)
                            .then_some(15u8.saturating_sub(link.attenuation))
                    })
                    .chain(program.arrangement_links.iter().filter_map(|link| {
                        let constant = match link.source {
                            ArrangementSource::Token(_) => true,
                            ArrangementSource::Component(node) => {
                                matches!(program.initial[node], Block::RedstoneBlock) || program.initial[node].pressure_plate_powered() == Some(true)
                            }
                        };
                        (link.target == id && !link.side && constant && link.active[loaded.group_arrangements[link.group]])
                            .then_some(15u8.saturating_sub(link.attenuation))
                    }))
                    .max()
                    .unwrap_or(0);
                assert_eq!(constant_power, original);
            }
            let quiet = match block {
                Block::Lever { mut lever } => {
                    lever.powered = false;
                    Block::Lever { lever }
                }
                Block::StoneButton { mut button } => {
                    button.powered = false;
                    Block::StoneButton { button }
                }
                Block::RedstoneRepeater { mut repeater } => {
                    repeater.powered = false;
                    repeater.locked = false;
                    Block::RedstoneRepeater { repeater }
                }
                Block::RedstoneComparator { mut comparator } => {
                    comparator.powered = false;
                    Block::RedstoneComparator { comparator }
                }
                Block::Observer { mut observer } => {
                    observer.powered = false;
                    Block::Observer { observer }
                }
                Block::RedstoneWire { mut wire } => {
                    wire.power = constant_power;
                    Block::RedstoneWire { wire }
                }
                Block::RedstoneLamp { .. } => Block::RedstoneLamp { lit: constant_power > 0 },
                Block::Hopper { facing, .. } => Block::Hopper {
                    facing,
                    enabled: constant_power == 0,
                },
                Block::IronTrapdoor { facing, half, .. } => Block::IronTrapdoor {
                    facing,
                    half,
                    powered: constant_power > 0,
                },
                Block::NoteBlock { instrument, note, .. } => Block::NoteBlock {
                    instrument,
                    note,
                    powered: constant_power > 0,
                },
                block if block.is_copper_bulb() => {
                    let (lit, _) = block.copper_bulb_state().unwrap();
                    block.with_copper_bulb_state(lit, constant_power > 0).unwrap()
                }
                Block::RedstoneTorch { .. } => Block::RedstoneTorch { lit: false },
                Block::RedstoneWallTorch { facing, .. } => Block::RedstoneWallTorch { lit: false, facing },
                _ => block,
            };
            if matches!(block, Block::RedstoneComparator { .. }) {
                ordering_probe
                    .world_mut()
                    .set_block_entity(at, BlockEntity::Comparator { output_strength: 0 });
            }
            ordering_probe.world_mut().set_block(at, quiet);
        }
        ordering_probe.checkpoint()?;
        sampling.quiet_basis += quiet_started.elapsed();
        sampling.quiet_component_walks += 1;
    }
    let mut racing_jobs = Vec::new();
    for (output_index, &source) in output_sources.iter().enumerate() {
        phases.detail("source transition", source);
        check_cancelled(monitor)?;
        if !racing_sources.contains(&source) {
            phases.detail("ordered-program lowering", source);
            let lowering_started = Instant::now();
            let operations = source_index.operations(source);
            program.set_output_program(source, false, operations.clone());
            program.set_output_program(source, true, operations);
            sampling.ordered_lowering += lowering_started.elapsed();
            sampling.completed_sources += 1;
            continue;
        }
        racing_jobs.push((output_index, source));
    }
    phases.detail("parallel source probes", 0);
    let batches = ordering_probe.probe_jobs(&racing_jobs, monitor, |scratch, &(output_index, source)| {
        let mut sampling = SourceSamplingMetrics::default();
        let mut outputs = [Vec::new(), Vec::new()];
        let scratch_started = Instant::now();
        scratch.revert();
        sampling.scratch_setup += scratch_started.elapsed();
        let pos = program.positions[source];
        let initial_power = false;
        let mut first_edge = None;
        let mut samples: [Option<Vec<Frame>>; 2] = [None, None];
        let edge_count = if matches!(program.initial[source], Block::StoneButton { .. }) {
            2
        } else {
            3
        };
        for powered in [!initial_power, initial_power, !initial_power].into_iter().take(edge_count) {
            check_cancelled(monitor)?;
            let w = scratch.world_mut();
            let mut frames = vec![];
            if let Block::RedstoneRepeater { mut repeater } = w.get_block(pos) {
                if repeater.powered == powered {
                    repeater.powered = !powered;
                    w.set_block(pos, Block::RedstoneRepeater { repeater });
                }
            }
            let flip_started = Instant::now();
            w.observe_redstone_updates(true);
            match w.get_block(pos) {
                Block::Lever { lever } if lever.powered != powered => {
                    crate::interaction::use_redstone(w.get_block(pos), w, pos, None);
                }
                Block::StoneButton { button } if button.powered != powered => {
                    if powered {
                        crate::redstone::button::press(button, w, pos, None);
                    } else {
                        crate::redstone::button::release(button, w, pos);
                    }
                }
                Block::RedstoneRepeater { mut repeater } if repeater.powered != powered => {
                    repeater.powered = powered;
                    w.set_block(pos, Block::RedstoneRepeater { repeater });
                    crate::redstone::on_state_change(repeater.facing.block_facing(), w, pos);
                }
                Block::RedstoneComparator { mut comparator } => {
                    comparator.powered = powered;
                    w.set_block_entity(
                        pos,
                        BlockEntity::Comparator {
                            output_strength: if powered { 15 } else { 0 },
                        },
                    );
                    w.set_block(pos, Block::RedstoneComparator { comparator });
                    crate::redstone::on_state_change(comparator.facing.block_facing(), w, pos);
                }
                Block::Observer { mut observer } if observer.powered != powered => {
                    observer.powered = powered;
                    w.set_block(pos, Block::Observer { observer });
                    crate::redstone::on_observer_state_change(observer.facing, w, pos);
                }
                Block::RedstoneTorch { .. } => {
                    w.set_block(pos, Block::RedstoneTorch { lit: powered });
                    crate::redstone::on_torch_state_change(w, pos);
                }
                Block::RedstoneWallTorch { facing, .. } => {
                    w.set_block(pos, Block::RedstoneWallTorch { lit: powered, facing });
                    crate::redstone::on_torch_state_change(w, pos);
                }
                _ => continue,
            }
            sampling.source_flips += flip_started.elapsed();
            // SourceOrdering records the immediate flip; scheduled feedback need not settle.
            let frame_started = Instant::now();
            frames.push(Frame {
                blocks: program.positions.iter().map(|&p| w.get_block(p)).collect(),
                pending: w.scheduler().iter_entries().collect(),
                deliveries: w.take_redstone_update_observations(),
            });
            sampling.captured_frames += 1;
            sampling.observed_callbacks += frames[0].deliveries.len();
            if let Some(previous) = &first_edge {
                if powered != initial_power && *previous != frames {
                    if !matches!(program.initial[source], Block::StoneButton { .. }) {
                        eprintln!("sampled source probe at {pos:?}: repeated transition differs");
                    }
                }
            } else {
                first_edge = Some(frames.clone());
            }
            sampling.frame_capture_comparison += frame_started.elapsed();
            let lowering_started = Instant::now();
            let targets = &source_index.targets[source];
            let mut ordered = Vec::new();
            for delivery in &frames[0].deliveries {
                let Some(&target) = probe_indexes.components.get(&delivery.pos) else {
                    continue;
                };
                let Some(record) = targets.iter().find(|record| record.target == target) else {
                    continue;
                };
                if ordered.contains(&target) || matches!(delivery.block, Block::RedstoneWire { .. }) {
                    continue;
                }
                if matches!(delivery.block, Block::Observer { .. })
                    || delivery
                        .input_on
                        .is_some_and(|(main, side)| (if record.rear { main } else { side }) == powered)
                {
                    ordered.push(target);
                }
            }
            for record in targets {
                if !ordered.contains(&record.target) {
                    ordered.push(record.target);
                }
            }
            let mut operations = Vec::new();
            for target in ordered {
                operations.extend(targets.iter().find(|record| record.target == target).unwrap().operations.iter().cloned());
            }
            outputs[usize::from(powered)] = operations;
            samples[usize::from(powered)] = Some(frames);
            sampling.ordered_lowering += lowering_started.elapsed();
            scratch.finish_probe()?;
        }
        sampling.completed_sources += 1;
        let entry_check_started = Instant::now();
        sampling.entry_state_walks += 1;
        let comparison = if output_index >= source_count
            || program.initial.iter().any(|block| match block {
                Block::Lever { lever } => lever.powered,
                Block::StoneButton { button } => button.powered,
                Block::RedstoneRepeater { repeater } => repeater.powered || repeater.locked,
                Block::RedstoneWire { wire } => wire.power != 0,
                Block::RedstoneLamp { lit } => *lit,
                _ => false,
            }) {
            None
        } else {
            Some((output_index, pos, samples))
        };
        sampling.frame_capture_comparison += entry_check_started.elapsed();
        Ok((source, outputs, comparison, sampling))
    })?;
    for (source, outputs, comparison, work) in batches {
        sampling.add(&work);
        for (powered, operations) in [false, true].into_iter().zip(outputs) {
            program.set_output_program(source, powered, operations);
        }
        if let Some(comparison) = comparison {
            comparisons.push(comparison);
        }
    }
    for (output_index, pos, samples) in comparisons {
        phases.detail("captured-frame runtime comparison", output_sources[output_index]);
        check_cancelled(monitor)?;
        let source = output_sources[output_index];
        sampling.program_starts += 1;
        let mut runtime = program.start();
        for powered in [true, false] {
            runtime.set_input(output_index, powered)?;
            for (step, frame) in samples[usize::from(powered)].as_ref().unwrap().iter().enumerate() {
                check_cancelled(monitor)?;
                if step != 0 {
                    runtime.tick()?;
                }
                let compare_started = Instant::now();
                let pending_matches = if matches!(program.initial[source], Block::StoneButton { .. }) {
                    let pos = program.positions[source];
                    runtime.pending_entries().into_iter().filter(|entry| entry.pos == pos).collect::<Vec<_>>()
                        == frame.pending.iter().filter(|entry| entry.pos == pos).cloned().collect::<Vec<_>>()
                } else {
                    runtime.pending_entries().into_iter().collect::<Vec<_>>() == frame.pending
                };
                sampling.frame_comparison_walks += 1;
                let blocks_match = runtime.blocks == frame.blocks;
                sampling.frame_capture_comparison += compare_started.elapsed();
                if !blocks_match || !pending_matches {
                    eprintln!(
                        "sampled runtime at {pos:?}, edge={powered}, step={step}: expected blocks={:?}, pending={:?}; observed blocks={:?}, pending={:?}",
                        frame.blocks,
                        frame.pending,
                        runtime.blocks,
                        runtime.pending_entries().into_iter().collect::<Vec<_>>()
                    );
                }
            }
        }
    }
    Ok(())
}

struct SourceTarget {
    target: usize,
    rear: bool,
    operations: Vec<Operation>,
}

struct SourceIndex {
    targets: Vec<Vec<SourceTarget>>,
}

impl SourceIndex {
    fn target(&mut self, source: usize, target: usize) -> &mut SourceTarget {
        let targets = &mut self.targets[source];
        let index = targets.iter().position(|record| record.target == target).unwrap_or_else(|| {
            targets.push(SourceTarget {
                target,
                rear: false,
                operations: Vec::new(),
            });
            targets.len() - 1
        });
        &mut targets[index]
    }

    fn new(program: &Program, monitor: Option<&TaskMonitor>) -> Result<Self, ScratchError> {
        let mut index = Self {
            targets: (0..program.initial.len()).map(|_| Vec::new()).collect(),
        };
        for (id, link) in program.links.iter().enumerate() {
            if id % 1024 == 0 {
                check_cancelled(monitor)?;
            }
            let target = index.target(link.source, link.target);
            target.rear = true;
            target.operations.push(Operation::Contribution(id));
        }
        for (id, link) in program.arrangement_links.iter().enumerate() {
            if id % 1024 == 0 {
                check_cancelled(monitor)?;
            }
            if let ArrangementSource::Component(source) = link.source {
                let target = index.target(source, link.target);
                target.rear |= !link.side;
                target.operations.push(Operation::ArrangementContribution(id));
            }
        }
        for link in &program.side_links {
            index.target(link.source, link.target);
        }
        for record in &program.repeaters {
            for &source in record.side_sources.iter().flatten() {
                index.target(source, record.component);
            }
        }
        let mut watched = vec![None; program.initial.len()];
        for (id, wire) in program.watched_wires.iter().enumerate() {
            watched[wire.wire].get_or_insert(id);
        }
        for (source, targets) in index.targets.iter_mut().enumerate() {
            if source % 1024 == 0 {
                check_cancelled(monitor)?;
            }
            for record in targets {
                let target = record.target;
                match program.initial[target] {
                    Block::RedstoneWire { .. } => {
                        record.operations.push(Operation::RefreshWire(target));
                        if let Some(wire) = watched[target] {
                            record.operations.push(Operation::CheckWatchedWire(wire));
                        }
                    }
                    Block::RedstoneLamp { .. } => record.operations.push(Operation::UpdateLamp(target)),
                    Block::Hopper { .. } => record.operations.push(Operation::UpdateHopper(target)),
                    Block::IronTrapdoor { .. } => record.operations.push(Operation::UpdateTrapdoor(target)),
                    block if block.is_command_block() => record.operations.push(Operation::UpdateCommandBlock(target)),
                    Block::NoteBlock { .. } => record.operations.push(Operation::UpdateNoteBlock(target)),
                    block if block.is_copper_bulb() => record.operations.push(Operation::UpdateCopperBulb(target)),
                    Block::RedstoneRepeater { .. } => record.operations.push(Operation::UpdateRepeater(target)),
                    Block::RedstoneComparator { .. } => record.operations.push(Operation::UpdateComparator(target)),
                    Block::RedstoneTorch { .. } | Block::RedstoneWallTorch { .. } => record.operations.push(Operation::UpdateTorch(target)),
                    _ => {}
                }
            }
        }
        #[cfg(test)]
        if program.initial.len() < 1000 {
            for source in 0..program.initial.len() {
                assert_eq!(index.operations(source), source_operations(program, source));
                assert_eq!(index.feeds_two_racers(program, source), feeds_two_racers(program, source));
            }
        }
        Ok(index)
    }

    fn operations(&self, source: usize) -> Vec<Operation> {
        self.targets[source].iter().flat_map(|record| record.operations.iter().cloned()).collect()
    }

    fn feeds_two_racers(&self, program: &Program, source: usize) -> bool {
        self.targets[source]
            .iter()
            .filter(|record| {
                matches!(
                    program.initial[record.target],
                    Block::RedstoneRepeater { .. }
                        | Block::RedstoneComparator { .. }
                        | Block::RedstoneTorch { .. }
                        | Block::RedstoneWallTorch { .. }
                        | Block::Observer { .. }
                        | Block::Piston { .. }
                        | Block::RedstoneWire { .. }
                        | Block::RedstoneBlock
                )
            })
            .take(2)
            .count()
            == 2
    }
}

#[cfg(test)]
fn feeds_two_racers(program: &Program, source: usize) -> bool {
    let mut targets = Vec::new();
    for link in program.links.iter().filter(|link| link.source == source) {
        if !targets.contains(&link.target) {
            targets.push(link.target);
        }
    }
    for link in program
        .arrangement_links
        .iter()
        .filter(|link| link.source == ArrangementSource::Component(source))
    {
        if !targets.contains(&link.target) {
            targets.push(link.target);
        }
    }
    for link in program.side_links.iter().filter(|link| link.source == source) {
        if !targets.contains(&link.target) {
            targets.push(link.target);
        }
    }
    for record in &program.repeaters {
        if record.side_sources.as_ref().is_some_and(|side| side.contains(&source)) && !targets.contains(&record.component) {
            targets.push(record.component);
        }
    }
    targets
        .into_iter()
        .filter(|&target| {
            matches!(
                program.initial[target],
                Block::RedstoneRepeater { .. }
                    | Block::RedstoneComparator { .. }
                    | Block::RedstoneTorch { .. }
                    | Block::RedstoneWallTorch { .. }
                    | Block::Observer { .. }
                    | Block::Piston { .. }
                    | Block::RedstoneWire { .. }
                    | Block::RedstoneBlock
            )
        })
        .take(2)
        .count()
        == 2
}

#[cfg(test)]
fn source_operations(program: &Program, source: usize) -> Vec<Operation> {
    let mut targets = Vec::new();
    for link in program.links.iter().filter(|link| link.source == source) {
        if !targets.contains(&link.target) {
            targets.push(link.target);
        }
    }
    for link in program
        .arrangement_links
        .iter()
        .filter(|link| link.source == ArrangementSource::Component(source))
    {
        if !targets.contains(&link.target) {
            targets.push(link.target);
        }
    }
    for link in program.side_links.iter().filter(|link| link.source == source) {
        if !targets.contains(&link.target) {
            targets.push(link.target);
        }
    }
    for record in &program.repeaters {
        if record.side_sources.as_ref().is_some_and(|side| side.contains(&source)) && !targets.contains(&record.component) {
            targets.push(record.component);
        }
    }
    let mut operations = Vec::new();
    for target in targets {
        operations.extend(
            program
                .links
                .iter()
                .enumerate()
                .filter_map(|(index, link)| (link.source == source && link.target == target).then_some(Operation::Contribution(index))),
        );
        operations.extend(program.arrangement_links.iter().enumerate().filter_map(|(index, link)| {
            (link.source == ArrangementSource::Component(source) && link.target == target).then_some(Operation::ArrangementContribution(index))
        }));
        match program.initial[target] {
            Block::RedstoneWire { .. } => {
                operations.push(Operation::RefreshWire(target));
                if let Some(record) = program.watched_wires.iter().position(|record| record.wire == target) {
                    operations.push(Operation::CheckWatchedWire(record));
                }
            }
            Block::RedstoneLamp { .. } => operations.push(Operation::UpdateLamp(target)),
            Block::Hopper { .. } => operations.push(Operation::UpdateHopper(target)),
            Block::IronTrapdoor { .. } => operations.push(Operation::UpdateTrapdoor(target)),
            block if block.is_command_block() => operations.push(Operation::UpdateCommandBlock(target)),
            Block::NoteBlock { .. } => operations.push(Operation::UpdateNoteBlock(target)),
            block if block.is_copper_bulb() => operations.push(Operation::UpdateCopperBulb(target)),
            Block::RedstoneRepeater { .. } => operations.push(Operation::UpdateRepeater(target)),
            Block::RedstoneComparator { .. } => operations.push(Operation::UpdateComparator(target)),
            Block::Piston { .. } => {}
            Block::RedstoneTorch { .. } | Block::RedstoneWallTorch { .. } => operations.push(Operation::UpdateTorch(target)),
            _ => {}
        }
    }
    operations
}
