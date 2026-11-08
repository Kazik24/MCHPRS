//! Sample ordered source transitions and compare the lowered execution with scratch frames.
use super::*;
use crate::redpilator::sampled::program::{PISTON_HEAD_NOTIFICATION, notification_component};
use crate::redpilator::sampled::source_cache::{CachedEdge, SourceOrderCache, quiet_chunk_fingerprint};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

#[derive(Default)]
struct SourceSamplingMetrics {
    /// whether timing and counter diagnostics should be emitted on drop.
    enabled: bool,
    /// spent selecting candidate sources.
    source_selection: Duration,
    /// spent loading the active token arrangement.
    loaded_arrangement: Duration,
    /// spent preparing the quiet baseline state.
    quiet_basis: Duration,
    /// spent restoring and setting up scratch probes.
    scratch_setup: Duration,
    /// spent applying source transitions and native callbacks.
    source_flips: Duration,
    /// spent capturing and comparing scratch frames.
    frame_capture_comparison: Duration,
    /// spent lowering ordered callbacks into operation programs.
    ordered_lowering: Duration,
    /// total candidate source components considered.
    candidate_sources: usize,
    /// candidate sources with racing outputs requiring ordered sampling.
    racing_sources: usize,
    /// sources fully sampled before completion or cancellation.
    completed_sources: usize,
    /// ordinary electrical links counted for diagnostics.
    static_links: usize,
    /// token-dependent links counted for diagnostics.
    conditional_links: usize,
    /// number of runtime program starts during sampling.
    program_starts: usize,
    /// full component walks used to construct quiet baselines.
    quiet_component_walks: usize,
    /// full component walks used to set up probes.
    setup_component_walks: usize,
    /// component checks used to capture entry states.
    entry_state_walks: usize,
    /// scratch frames captured for comparison.
    captured_frames: usize,
    /// component visits used while comparing captured frames.
    frame_comparison_walks: usize,
    /// native callbacks observed during probes.
    observed_callbacks: usize,
    /// frames retaining component blocks and pending work for comparison/diagnostics.
    full_frames: usize,
    /// estimated bytes copied into retained full frames.
    full_frame_bytes: usize,
    /// racing sources whose cached callback traces were reused.
    cache_hits: usize,
    /// racing sources sampled because no valid cached trace was available.
    cache_misses: usize,
    /// probes whose read certificate or cache allocation was declined.
    cache_declined: usize,
    /// retained source-order cache size after enforcing its cap.
    cache_bytes: usize,
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
        self.full_frames += other.full_frames;
        self.full_frame_bytes += other.full_frame_bytes;
        self.cache_hits += other.cache_hits;
        self.cache_misses += other.cache_misses;
        self.cache_declined += other.cache_declined;
        self.cache_bytes = self.cache_bytes.max(other.cache_bytes);
    }
}

impl Drop for SourceSamplingMetrics {
    fn drop(&mut self) {
        if self.enabled {
            eprintln!(
                "Redpilator source sampling: select={} ms loaded-arrangement={} ms quiet-basis={} ms scratch-restore/setup={} ms native-flips/callbacks={} ms frame-capture/comparison={} ms ordered-lowering={} ms; candidates={} racing={} completed={} static_links={} conditional_links={} program.start_calls={} full_component_walks={} (quiet={}, setup={}, frame_capture={}) entry_state_checks={} frame_comparison_walks={} frames={} observed_callbacks={} full_frames={} full_frame_bytes={} cache_hits={} cache_misses={} cache_declined={} cache_bytes={}",
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
                self.quiet_component_walks + self.setup_component_walks + self.full_frames,
                self.quiet_component_walks,
                self.setup_component_walks,
                self.full_frames,
                self.entry_state_walks,
                self.frame_comparison_walks,
                self.captured_frames,
                self.observed_callbacks,
                self.full_frames,
                self.full_frame_bytes,
                self.cache_hits,
                self.cache_misses,
                self.cache_declined,
                self.cache_bytes
            );
        }
    }
}

#[derive(Clone, PartialEq, Eq)]
struct Frame {
    /// Component states are retained only for the existing selected runtime comparison.
    blocks: Option<Vec<Block>>,
    /// Pending native ticks are retained with the selected comparison frame.
    pending: Option<Vec<mchprs_world::TickEntry>>,
    /// ordered neighbor/update callbacks captured with the frame.
    deliveries: Vec<crate::world::RedstoneUpdateObservation>,
}

#[derive(Clone, Copy)]
struct ProbeJob {
    output_index: usize,
    source: usize,
    needs_comparison: bool,
    signature: Option<[u8; 32]>,
}

struct ProbeResult {
    output_index: usize,
    source: usize,
    outputs: [Vec<Operation>; 2],
    comparison: Option<(usize, BlockPos, [Option<Vec<Frame>>; 2])>,
    work: SourceSamplingMetrics,
    source_block: Block,
    signature: Option<[u8; 32]>,
    read_chunks: Option<Vec<usize>>,
    edges: Vec<CachedEdge>,
}

#[derive(Serialize)]
struct SourceProbeSignature {
    source: BlockPos,
    block: u32,
    racing: bool,
    targets: Vec<SourceTargetSignature>,
}

#[derive(Serialize)]
struct SourceTargetSignature {
    target: BlockPos,
    block: u32,
    rear: bool,
    operations: Vec<OperationSignature>,
}

#[derive(Serialize)]
enum OperationSignature {
    Contribution { source: BlockPos, target: BlockPos, attenuation: u8 },
    ArrangementContribution(ArrangementLinkSignature),
    RefreshWire { target: BlockPos, block: u32 },
    CheckWatchedWire { wire: BlockPos, watchers: Vec<(BlockPos, bool)> },
    Update { kind: u8, target: BlockPos, block: u32 },
}

#[derive(Serialize)]
struct ArrangementLinkSignature {
    target: BlockPos,
    source: ArrangementSourceSignature,
    side: bool,
    silent: bool,
    attenuation: u8,
    active: Vec<bool>,
    group: ArrangementGroupSignature,
}

#[derive(Serialize)]
struct ArrangementGroupSignature {
    target: BlockPos,
    side: bool,
    silent: bool,
    tokens: Vec<TokenSignature>,
    token_states: Vec<Vec<usize>>,
    conditional_sources: Vec<ArrangementSourceSignature>,
    inputs: Vec<Vec<(ArrangementSourceSignature, u8)>>,
}

#[derive(Serialize)]
enum ArrangementSourceSignature {
    Component(BlockPos),
    Token(TokenSignature),
}

#[derive(Serialize)]
struct TokenSignature {
    block: u32,
    position: Option<BlockPos>,
    cells: Vec<BlockPos>,
}

fn source_probe_signature(program: &Program, source: usize, targets: &[SourceTarget], racing: bool) -> Option<[u8; 32]> {
    let signature = SourceProbeSignature {
        source: *program.positions.get(source)?,
        block: program.initial.get(source)?.get_id(),
        racing,
        targets: targets
            .iter()
            .map(|record| {
                Some(SourceTargetSignature {
                    target: *program.positions.get(record.target)?,
                    block: program.initial.get(record.target)?.get_id(),
                    rear: record.rear,
                    operations: record
                        .operations
                        .iter()
                        .map(|&operation| operation_signature(program, operation))
                        .collect::<Option<_>>()?,
                })
            })
            .collect::<Option<_>>()?,
    };
    Some(Sha256::digest(bincode::serialize(&signature).ok()?).into())
}

fn operation_signature(program: &Program, operation: Operation) -> Option<OperationSignature> {
    let block_at = |target: usize| Some((*program.positions.get(target)?, program.initial.get(target)?.get_id()));
    Some(match operation {
        Operation::Contribution(index) => {
            let link = program.links.get(index)?;
            OperationSignature::Contribution {
                source: *program.positions.get(link.source)?,
                target: *program.positions.get(link.target)?,
                attenuation: link.attenuation,
            }
        }
        Operation::ArrangementContribution(index) => {
            let link = program.arrangement_links.get(index)?;
            OperationSignature::ArrangementContribution(ArrangementLinkSignature {
                target: *program.positions.get(link.target)?,
                source: arrangement_source_signature(program, link.source)?,
                side: link.side,
                silent: link.silent,
                attenuation: link.attenuation,
                active: link.active.clone(),
                group: arrangement_group_signature(program, link.group)?,
            })
        }
        Operation::RefreshWire(target) => {
            let (target, block) = block_at(target)?;
            OperationSignature::RefreshWire { target, block }
        }
        Operation::CheckWatchedWire(index) => {
            let wire = program.watched_wires.get(index)?;
            let mut watchers = Vec::with_capacity(wire.watchers.len());
            for &watcher in &wire.watchers {
                let component = notification_component(watcher);
                watchers.push((*program.positions.get(component)?, watcher & PISTON_HEAD_NOTIFICATION != 0));
            }
            OperationSignature::CheckWatchedWire {
                wire: *program.positions.get(wire.wire)?,
                watchers,
            }
        }
        Operation::UpdateLamp(target) => update_signature(0, target, block_at)?,
        Operation::UpdateHopper(target) => update_signature(1, target, block_at)?,
        Operation::UpdateTrapdoor(target) => update_signature(2, target, block_at)?,
        Operation::UpdateCommandBlock(target) => update_signature(3, target, block_at)?,
        Operation::UpdateNoteBlock(target) => update_signature(4, target, block_at)?,
        Operation::UpdateCopperBulb(target) => update_signature(5, target, block_at)?,
        Operation::UpdateRepeater(target) => update_signature(6, target, block_at)?,
        Operation::UpdatePiston(target) => update_signature(7, target, block_at)?,
        Operation::UpdateTorch(target) => update_signature(8, target, block_at)?,
        Operation::UpdateComparator(target) => update_signature(9, target, block_at)?,
    })
}

fn update_signature(kind: u8, target: usize, block_at: impl Fn(usize) -> Option<(BlockPos, u32)>) -> Option<OperationSignature> {
    let (target, block) = block_at(target)?;
    Some(OperationSignature::Update { kind, target, block })
}

fn arrangement_source_signature(program: &Program, source: ArrangementSource) -> Option<ArrangementSourceSignature> {
    Some(match source {
        ArrangementSource::Component(component) => ArrangementSourceSignature::Component(*program.positions.get(component)?),
        ArrangementSource::Token(token) => ArrangementSourceSignature::Token(token_signature(program, token)?),
    })
}

fn token_signature(program: &Program, token: usize) -> Option<TokenSignature> {
    let token = program.tokens.get(token)?;
    Some(TokenSignature {
        block: token.block.get_id(),
        position: token.position,
        cells: token.cells.clone(),
    })
}

fn arrangement_group_signature(program: &Program, group: usize) -> Option<ArrangementGroupSignature> {
    let group = program.input_arrangements.get(group)?;
    Some(ArrangementGroupSignature {
        target: *program.positions.get(group.target)?,
        side: group.side,
        silent: group.silent,
        tokens: group.tokens.iter().map(|&token| token_signature(program, token)).collect::<Option<_>>()?,
        token_states: group.token_states.clone(),
        conditional_sources: group
            .conditional_sources
            .iter()
            .copied()
            .map(|source| arrangement_source_signature(program, source))
            .collect::<Option<_>>()?,
        inputs: group
            .inputs
            .iter()
            .map(|inputs| {
                inputs
                    .iter()
                    .map(|&(source, attenuation)| Some((arrangement_source_signature(program, source)?, attenuation)))
                    .collect::<Option<_>>()
            })
            .collect::<Option<_>>()?,
    })
}

fn lower_edge(
    program: &Program,
    probe_indexes: &ProbeIndexes<'_>,
    targets: &[SourceTarget],
    powered: bool,
    deliveries: &[crate::world::RedstoneUpdateObservation],
) -> Vec<Operation> {
    let mut ordered = Vec::new();
    for delivery in deliveries {
        let Some(notification) = probe_indexes.notification(delivery) else {
            continue;
        };
        let target = notification_component(notification);
        let Some(record) = targets.iter().find(|record| record.target == target) else {
            continue;
        };
        if ordered.contains(&notification) || matches!(delivery.block, Block::RedstoneWire { .. }) {
            continue;
        }
        if matches!(delivery.block, Block::Observer { .. })
            // Notification-only torches have no source input contribution to match.
            || matches!(record.operations.as_slice(), [Operation::UpdateTorch(_)])
            || delivery
                .input_on
                .is_some_and(|(main, side)| (if record.rear { main } else { side }) == powered)
        {
            ordered.push(notification);
        }
    }
    for record in targets {
        if !ordered.iter().any(|&notification| notification_component(notification) == record.target) {
            ordered.push(record.target);
        }
    }
    let mut operations = Vec::new();
    // Dust/head callbacks must see the source's new piston input contributions.
    ordered.sort_by_key(|&target| !matches!(program.initial[notification_component(target)], Block::Piston { .. }));
    let mut staged_pistons = HashSet::new();
    for target in ordered {
        let target = notification_component(target);
        if matches!(program.initial[target], Block::Piston { .. }) && !staged_pistons.insert(target) {
            continue;
        }
        if let Some(record) = targets.iter().find(|record| record.target == target) {
            operations.extend(record.operations.iter().cloned());
        }
    }
    operations
}

fn current_quiet_chunk(world: &crate::plot::PlotWorld, index: usize, cache: &mut HashMap<usize, [u8; 32]>) -> Option<[u8; 32]> {
    if let Some(&fingerprint) = cache.get(&index) {
        return Some(fingerprint);
    }
    let fingerprint = quiet_chunk_fingerprint(world, index)?;
    cache.insert(index, fingerprint);
    Some(fingerprint)
}

pub(in super::super) fn run(
    context: &Context<'_, impl World>,
    program: &mut Program,
    probe_indexes: &ProbeIndexes<'_>,
    phases: &mut CompilePhases<'_>,
    prior_cache: Option<&SourceOrderCache>,
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
    let mut source_cache = SourceOrderCache::new(world, bounds, &entry.scheduled, &entry.piston, world.is_cursed());
    let cache_context_matches =
        prior_cache.is_some_and(|cache| cache.context_matches(world, bounds, &entry.scheduled, &entry.piston, world.is_cursed()));
    phases.detail("source selection/index preparation", 0);
    let selection_started = Instant::now();
    let source_index = SourceIndex::new(&program, monitor)?;
    let control_sources = program.sources.iter().copied();
    let repeater_outputs = program.repeaters.iter().map(|record| record.component);
    let other_outputs = program.initial.iter().enumerate().filter_map(|(index, block)| {
        matches!(
            block,
            Block::RedstoneComparator { .. } | Block::Observer { .. } | Block::RedstoneTorch { .. } | Block::RedstoneWallTorch { .. }
        )
        .then_some(index)
    });
    let output_sources: Vec<_> = control_sources.chain(repeater_outputs).chain(other_outputs).collect();
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
            let fixed_link_power = program.incoming[id].iter().filter_map(|&link| {
                let link = &program.links[link];
                let conditional = groups_by_target[id].iter().any(|&group| {
                    let group = &program.input_arrangements[group];
                    group.target == id && !group.side && group.conditional_sources.contains(&ArrangementSource::Component(link.source))
                });
                (matches!(program.initial[link.source], Block::RedstoneBlock) && !conditional).then_some(15u8.saturating_sub(link.attenuation))
            });
            let arranged_link_power = links_by_target[id].iter().filter_map(|&index| {
                let link = &program.arrangement_links[index];
                let constant = match link.source {
                    ArrangementSource::Token(_) => true,
                    ArrangementSource::Component(node) => {
                        matches!(program.initial[node], Block::RedstoneBlock)
                    }
                };
                (link.target == id && !link.side && constant && link.active[loaded.group_arrangements[link.group]])
                    .then_some(loaded.arrangement_contribution(index).2)
            });
            // Both iterators yield only constant, currently active input strengths.
            let constant_power = fixed_link_power.chain(arranged_link_power).max().unwrap_or(0);
            #[cfg(test)]
            if program.positions.len() < 1000 {
                let original_fixed_link_power = program.incoming[id].iter().filter_map(|&index| {
                    let link = &program.links[index];
                    let conditional = program.input_arrangements.iter().any(|group| {
                        group.target == id && !group.side && group.conditional_sources.contains(&ArrangementSource::Component(link.source))
                    });
                    (matches!(program.initial[link.source], Block::RedstoneBlock) && !conditional).then_some(15u8.saturating_sub(link.attenuation))
                });
                let original_arranged_link_power = program.arrangement_links.iter().enumerate().filter_map(|(index, link)| {
                    let constant = match link.source {
                        ArrangementSource::Token(_) => true,
                        ArrangementSource::Component(node) => {
                            matches!(program.initial[node], Block::RedstoneBlock)
                        }
                    };
                    (link.target == id && !link.side && constant && link.active[loaded.group_arrangements[link.group]])
                        .then_some(loaded.arrangement_contribution(index).2)
                });
                // The reference path combines fixed and arranged constant inputs, then takes the strongest.
                let original = original_fixed_link_power.chain(original_arranged_link_power).max().unwrap_or(0);
                assert_eq!(constant_power, original);
            }
            let quiet = match block {
                block if block.pressure_plate_powered().is_some() => block.with_pressure_plate_power(false).unwrap(),
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
    let initial_quiet = !racing_sources.is_empty()
        && !program.initial.iter().any(|block| match block {
            block if block.pressure_plate_powered().is_some() => block.pressure_plate_powered() == Some(true),
            Block::Lever { lever } => lever.powered,
            Block::StoneButton { button } => button.powered,
            Block::RedstoneRepeater { repeater } => repeater.powered || repeater.locked,
            Block::RedstoneWire { wire } => wire.power != 0,
            Block::RedstoneLamp { lit } => *lit,
            _ => false,
        });
    sampling.entry_state_walks = usize::from(!racing_sources.is_empty());
    let mut racing_jobs = Vec::new();
    let mut racing_output_indices = Vec::new();
    let mut completed = HashMap::<usize, ProbeResult>::new();
    let mut current_fingerprints = HashMap::new();
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

        racing_output_indices.push(output_index);
        let pos = program.positions[source];
        let source_block = program.initial[source];
        let needs_comparison = output_index < source_count && initial_quiet;
        let signature = source_probe_signature(&program, source, &source_index.targets[source], true);
        let can_reuse = !needs_comparison && cache_context_matches && signature.is_some();
        let cached = if can_reuse {
            prior_cache.and_then(|cache| cache.source(pos))
        } else {
            None
        };
        let reusable = cached.filter(|record| {
            prior_cache
                .unwrap()
                .source_matches(record, source_block, signature.as_ref().unwrap(), |index| {
                    current_quiet_chunk(ordering_probe.world(), index, &mut current_fingerprints)
                })
        });
        if let Some(record) = reusable {
            if let Some(cache) = &mut source_cache {
                cache.note_hit();
                let chunks = SourceOrderCache::chunks(record).to_vec();
                let edges = SourceOrderCache::edges(record).to_vec();
                if !cache.insert_source(pos, source_block, signature.unwrap(), chunks, edges, |index| {
                    current_quiet_chunk(ordering_probe.world(), index, &mut current_fingerprints)
                }) {
                    cache.note_declined();
                }
            }
            let mut outputs = [Vec::new(), Vec::new()];
            for edge in SourceOrderCache::edges(record) {
                outputs[usize::from(edge.powered)] =
                    lower_edge(&program, probe_indexes, &source_index.targets[source], edge.powered, &edge.deliveries);
            }
            if let Some(cache) = &mut source_cache {
                sampling.cache_hits = cache.reuse_counts().0;
                sampling.cache_declined = cache.declined_sources();
            }
            completed.insert(
                output_index,
                ProbeResult {
                    output_index,
                    source,
                    outputs,
                    comparison: None,
                    work: SourceSamplingMetrics::default(),
                    source_block,
                    signature,
                    read_chunks: None,
                    edges: Vec::new(),
                },
            );
        } else {
            if let Some(cache) = &mut source_cache {
                cache.note_miss();
                sampling.cache_misses = cache.reuse_counts().1;
            }
            racing_jobs.push(ProbeJob {
                output_index,
                source,
                needs_comparison,
                signature,
            });
        }
    }
    phases.detail("parallel source probes", 0);
    let batches = ordering_probe.probe_jobs(&racing_jobs, monitor, |scratch, job| {
        let ProbeJob {
            output_index,
            source,
            needs_comparison,
            signature,
        } = *job;
        let mut sampling = SourceSamplingMetrics::default();
        let mut outputs = [Vec::new(), Vec::new()];
        let scratch_started = Instant::now();
        scratch.revert();
        sampling.scratch_setup += scratch_started.elapsed();
        let pos = program.positions[source];
        let source_block = program.initial[source];
        let initial_power = false;
        let mut first_edge: Option<Vec<crate::world::RedstoneUpdateObservation>> = None;
        let mut samples: [Option<Vec<Frame>>; 2] = [None, None];
        let mut cached_edges = Vec::new();
        let mut read_chunks = HashSet::new();
        let mut can_cache = signature.is_some();
        let edge_count = if matches!(source_block, Block::StoneButton { .. }) { 2 } else { 3 };
        for powered in [!initial_power, initial_power, !initial_power].into_iter().take(edge_count) {
            check_cancelled(monitor)?;
            scratch.world_mut().begin_source_probe_reads();
            let (deliveries, flip_duration) = {
                let w = scratch.world_mut();
                if let Block::RedstoneRepeater { mut repeater } = w.get_block(pos) {
                    if repeater.powered == powered {
                        repeater.powered = !powered;
                        w.set_block(pos, Block::RedstoneRepeater { repeater });
                    }
                }
                let flip_started = Instant::now();
                w.observe_redstone_updates(true);
                match w.get_block(pos) {
                    block if block.pressure_plate_powered().is_some_and(|was_powered| was_powered != powered) => {
                        crate::redstone::set_pressure_plate(w, pos, block, powered);
                    }
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
                    _ => {
                        can_cache = false;
                        continue;
                    }
                }
                (w.take_redstone_update_observations(), flip_started.elapsed())
            };
            sampling.source_flips += flip_duration;
            scratch.finish_probe()?;
            match scratch.world_mut().take_source_probe_reads() {
                Some(chunks) => read_chunks.extend(chunks),
                None => can_cache = false,
            }
            cached_edges.push(CachedEdge {
                powered,
                deliveries: deliveries.clone(),
            });

            let frame_started = Instant::now();
            let w = scratch.world_mut();
            let blocks = needs_comparison.then(|| program.positions.iter().map(|&p| w.get_block(p)).collect::<Vec<_>>());
            let pending = needs_comparison.then(|| w.scheduler().iter_entries().collect::<Vec<_>>());
            if let Some(blocks) = &blocks {
                sampling.full_frames += 1;
                sampling.full_frame_bytes += blocks.capacity() * std::mem::size_of::<Block>()
                    + pending
                        .as_ref()
                        .map_or(0, |pending| pending.capacity() * std::mem::size_of::<mchprs_world::TickEntry>());
            }
            let frame = Frame { blocks, pending, deliveries };
            sampling.captured_frames += 1;
            sampling.observed_callbacks += frame.deliveries.len();
            if let Some(previous) = &first_edge {
                if powered != initial_power && *previous != frame.deliveries && phases.enabled {
                    eprintln!("sampled source probe at {pos:?}: repeated callback transition differs");
                }
            } else {
                first_edge = Some(frame.deliveries.clone());
            }
            sampling.frame_capture_comparison += frame_started.elapsed();
            let lowering_started = Instant::now();
            outputs[usize::from(powered)] = lower_edge(&program, probe_indexes, &source_index.targets[source], powered, &frame.deliveries);
            if needs_comparison {
                samples[usize::from(powered)] = Some(vec![frame]);
            }
            sampling.ordered_lowering += lowering_started.elapsed();
        }
        sampling.completed_sources += 1;
        let entry_check_started = Instant::now();
        let comparison = needs_comparison.then_some((output_index, pos, samples));
        sampling.frame_capture_comparison += entry_check_started.elapsed();
        let read_chunks = if can_cache {
            let mut chunks: Vec<_> = read_chunks.into_iter().collect();
            chunks.sort_unstable();
            Some(chunks)
        } else {
            None
        };
        Ok(ProbeResult {
            output_index,
            source,
            outputs,
            comparison,
            work: sampling,
            source_block,
            signature,
            read_chunks,
            edges: cached_edges,
        })
    })?;
    // Serial probe execution mutates the caller's scratch world. Restore the quiet basis
    // before hashing new dependencies; parallel workers leave it untouched.
    ordering_probe.revert();
    for mut result in batches {
        sampling.add(&result.work);
        if let (Some(cache), Some(signature), Some(chunks)) = (&mut source_cache, result.signature, result.read_chunks.take()) {
            if !cache.insert_source(
                program.positions[result.source],
                result.source_block,
                signature,
                chunks,
                result.edges.clone(),
                |index| current_quiet_chunk(ordering_probe.world(), index, &mut current_fingerprints),
            ) {
                cache.note_declined();
            }
        } else if let Some(cache) = &mut source_cache {
            cache.note_declined();
        }
        completed.insert(result.output_index, result);
    }
    for output_index in racing_output_indices {
        let result = completed.remove(&output_index).expect("every racing source has a result");
        for (powered, operations) in [false, true].into_iter().zip(result.outputs) {
            program.set_output_program(result.source, powered, operations);
        }
        if let Some(comparison) = result.comparison {
            comparisons.push(comparison);
        }
        if result.work.completed_sources == 0 {
            sampling.completed_sources += 1;
        }
    }
    if let Some(cache) = &mut source_cache {
        sampling.cache_hits = cache.reuse_counts().0;
        sampling.cache_misses = cache.reuse_counts().1;
        sampling.cache_declined = cache.declined_sources();
        sampling.cache_bytes = cache.estimated_bytes();
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
                        == frame
                            .pending
                            .as_ref()
                            .unwrap()
                            .iter()
                            .filter(|entry| entry.pos == pos)
                            .cloned()
                            .collect::<Vec<_>>()
                } else {
                    runtime.pending_entries().into_iter().collect::<Vec<_>>().as_slice() == frame.pending.as_ref().unwrap().as_slice()
                };
                sampling.frame_comparison_walks += 1;
                let blocks_match = runtime.blocks.as_slice() == frame.blocks.as_ref().unwrap().as_slice();
                sampling.frame_capture_comparison += compare_started.elapsed();
                if phases.enabled && (!blocks_match || !pending_matches) {
                    eprintln!(
                        "sampled runtime at {pos:?}, edge={powered}, step={step}: expected blocks={:?}, pending={:?}; observed blocks={:?}, pending={:?}",
                        frame.blocks.as_ref().unwrap(),
                        frame.pending.as_ref().unwrap(),
                        runtime.blocks,
                        runtime.pending_entries().into_iter().collect::<Vec<_>>()
                    );
                }
            }
        }
    }
    let retained = source_cache.and_then(SourceOrderCache::retain);
    if retained.is_none() {
        sampling.cache_bytes = 0;
    }
    program.source_order_cache = retained.map(std::sync::Arc::new);
    Ok(())
}

struct SourceTarget {
    /// target component index into `Program::positions`.
    target: usize,
    /// whether the source supplies the target's rear/primary input rather than only a side input.
    rear: bool,
    /// ordered runtime operations triggered when this source changes this target.
    operations: Vec<Operation>,
}

struct SourceIndex {
    /// per source component, the target components and operations reached by that source.
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
        for source in 0..program.initial.len() {
            for target in reactive_torch_neighbours(program, source) {
                index.target(source, target);
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
            targets.sort_by_key(|record| !matches!(program.initial[record.target], Block::Piston { .. }));
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

fn reactive_torch_neighbours(program: &Program, source: usize) -> impl Iterator<Item = usize> + '_ {
    program.reactive_neighbours[source]
        .iter()
        .copied()
        .filter(|&target| matches!(program.initial[target], Block::RedstoneTorch { .. } | Block::RedstoneWallTorch { .. }))
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
    for target in reactive_torch_neighbours(program, source) {
        if !targets.contains(&target) {
            targets.push(target);
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
    for target in reactive_torch_neighbours(program, source) {
        if !targets.contains(&target) {
            targets.push(target);
        }
    }
    let mut operations = Vec::new();
    targets.sort_by_key(|&target| !matches!(program.initial[target], Block::Piston { .. }));
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
