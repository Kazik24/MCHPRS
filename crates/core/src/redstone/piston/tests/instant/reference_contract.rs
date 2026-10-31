//! Canonical interpreter observations for the first graph-native piston step.
use super::*;
use crate::redstone::piston::trace::{self, Operation as PistonTraceOperation};
use mchprs_blocks::BlockFacing;
use mchprs_blocks::blocks::{Block, RedstonePiston};
use mchprs_world::{PistonAction, PistonEvent, TickPriority};
use serde_json::Value;
use std::collections::BTreeMap;

/// This fixture-local key is the immutable provenance later graph IDs resolve through.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct FixturePosition(
    /// The imported selection-local coordinate used to resolve graph IDs through provenance.
    [i32; 3],
);

impl FixturePosition {
    fn from_world(pos: BlockPos) -> Self {
        Self([pos.x - ORIGIN.x, pos.y - ORIGIN.y, pos.z - ORIGIN.z])
    }
}

impl std::fmt::Display for FixturePosition {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{},{},{}", self.0[0], self.0[1], self.0[2])
    }
}

/// The position and action are stable semantics; runtime-local identities are omitted.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct PistonEventObservation {
    actor: FixturePosition, // Source position identifying the piston across independent runs.
    sticky: bool,           // Captured stickiness used by live event validation.
    facing: BlockFace,      // Captured event direction, preserved independently from live facing.
    action: PistonAction,   // Requested extension or retraction operation.
}

/// A moving native cell projects to unavailable; motion detail stays in NativeInspection.
#[derive(Clone, Debug, PartialEq, Eq)]
enum ProjectedCell {
    Air, // The boundary has no stationary block or enabled resource.
    Stationary {
        name: String,                  // Registry block name used by the native projection.
        properties: Value,             // Observable saved block properties at this boundary.
        power_available: Option<bool>, // Piston query result when this cell is an actuator.
    },
    Unavailable, // Native moving geometry supplies no stationary material connections.
}

/// This binds an observed block state to its imported fixture position.
#[derive(Clone, Debug, PartialEq, Eq)]
struct CellObservation {
    position: FixturePosition, // Source site whose projected contents changed.
    cell: ProjectedCell,       // Logical view compared with graph contact selection.
}

/// Named manifest ports provide the graph/native comparison boundary.
#[derive(Clone, Debug, PartialEq, Eq)]
struct ComponentObservation {
    port: String,              // Manifest path naming this external component boundary.
    position: FixturePosition, // Source site for the declared port.
    cell: ProjectedCell,       // Native state exposed at the port.
}

/// This names whether one declared shared payload is electrically exposed at a contact.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ResourceContactState {
    Absent,   // The declared material is not at this contact.
    Enabled,  // The stationary material exposes its power/conduction channels.
    Disabled, // The material is retained in a native moving cell and supplies no channels.
}

/// The origin position is the resource key; no actor owns a private payload copy.
#[derive(Clone, Debug, PartialEq, Eq)]
struct SharedResourceObservation {
    source: FixturePosition,                   // Imported material position used as its stable key.
    selected_contact: Option<FixturePosition>, // Current contact containing the one shared material.
    enabled: bool,                             // Whether the selected material is stationary and live.
}

/// This preserves scheduler bucket distance, priority, and yielded order for each source cell.
#[derive(Clone, Debug, PartialEq, Eq)]
struct PendingTick {
    position: FixturePosition,        // Position-bound callback target, not a carried block identity.
    ticks_left: u32,                  // Remaining scheduler buckets at this observed phase.
    priority: TickPriority,           // Priority within the pending bucket.
    expected_block_type: Option<u32>, // Registry type required for the callback to remain valid.
    order: usize,                     // Index in the existing scheduler iteration order.
}

/// This is the position-bound completion deadline derived from native progress and phase.
#[derive(Clone, Debug, PartialEq, Eq)]
struct PendingDeadline {
    position: FixturePosition, // Native target projected to its next uninterrupted publication.
    due_tick: u64,             // Absolute logical tick derived from phase and remaining visits.
    order: usize,              // Native completion order, normalized by position rather than identity.
}

/// Each variant records one ordered interpreter observation used by a later graph replay.
#[derive(Clone, Debug, PartialEq, Eq)]
enum OrderedObservation {
    Callback {
        // One delivered callback, in the native recorder's exact order.
        target: FixturePosition,         // Source position receiving this callback.
        block: String,                   // Live or supplied block handler name.
        direction: Option<BlockFace>,    // Literal callback direction, including directionless None.
        source: Option<FixturePosition>, // Notifying source position, when the interpreter provides one.
        parents: Vec<FixturePosition>,   // Nested callback ancestry in delivery order.
        power_available: Option<bool>,   // Current piston power query, separate from sampling.
    },
    PistonSample {
        // One update-time power sample, including same-value samples.
        actor: FixturePosition, // Source position of the sampled piston.
        extended: bool,         // Supplied piston pose at the sampling callback.
        powered: bool,          // Power availability observed by that sample.
    },
    RequestQueued(PistonEventObservation),     // A sampled action appended to the piston FIFO.
    AcceptanceAttempt(PistonEventObservation), // A popped request revalidated against live state.
    Accepted(PistonEventObservation),          // A request passed validation and changed logical publication.
    ScheduledTickDispatched {
        // One position-bound scheduled entry consumed by the interpreter.
        position: FixturePosition,        // Source position of the scheduled callback.
        expected_block_type: Option<u32>, // Type guard carried by the scheduled entry.
    },
    WireStrengthChanged {
        // One ordered dust power publication.
        position: FixturePosition, // Source position of the changed wire.
        old: u8,                   // Previous stored strength.
        new: u8,                   // Newly published strength.
    },
}

/// These tags keep strict import, settling, notified inputs, and continuation distinct.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ObservationStage {
    Imported,      // Strict loaded snapshot before any notified reconstruction.
    Settling,      // Explicit manifest wait used to reach a named quiescent boundary.
    NotifiedInput, // External operation followed by the existing update callbacks.
    RawInput,      // Direct storage edit with no notification replay.
    Continuation,  // Interpreter execution after the declared input protocol.
}

/// This record contains only graph-comparable observations and justified projections.
#[derive(Clone, Debug, PartialEq, Eq)]
struct ComparisonRecord {
    tick: u64,                                     // Interpreter logical tick for this observation boundary.
    phase: AdvancePhase,                           // Scheduler/piston phase at the same boundary.
    stage: ObservationStage,                       // Import, settling, input, or continuation protocol stage.
    action: usize,                                 // Original manifest action ordinal associated with this boundary.
    ordered: Vec<OrderedObservation>,              // Callbacks and piston lifecycle operations in exact order.
    components: Vec<ComponentObservation>,         // Named manifest boundary ports at this point.
    published_cells: Vec<CellObservation>,         // Justified logical projection of changed native cells.
    resources: Vec<SharedResourceObservation>,     // Authoritative shared-material contact observations.
    pending_ticks: Vec<PendingTick>,               // Ordered interpreter tick queue projection.
    pending_requests: Vec<PistonEventObservation>, // FIFO piston requests awaiting live acceptance.
    deadlines: Vec<PendingDeadline>,               // Projected next publication deadlines, without motion state.
}

/// This retains the manifest pin and protocol boundaries used to produce a reference.
#[derive(Clone, Debug, PartialEq, Eq)]
struct FixtureProtocol {
    fixture: String,               // Existing schematic path selected by the frozen manifest.
    sha256: String,                // Frozen fixture digest verified by the shared loader.
    case_id: String,               // Named action sequence from that same manifest revision.
    strict_import_was_quiet: bool, // Loader confirmed no initial callback or pending work.
    actions: Vec<ProtocolAction>,  // Ordered waits and external operations exactly as declared.
}

/// This preserves the original manifest action and separates waits from notified edits.
#[derive(Clone, Debug, PartialEq, Eq)]
struct ProtocolAction {
    order: usize,                    // One-based index in the source manifest's action list.
    operation: String,               // Existing test-loader operation name.
    delivery: Option<InputDelivery>, // None identifies an explicit settling wait.
    raw: Value,                      // Exact manifest parameters retained for protocol review.
    settled_ticks: Option<u64>,      // Actual wait duration recorded by the existing readiness helper.
}

/// Notified inputs and direct storage writes have different interpreter semantics.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum InputDelivery {
    Notified,        // The action invokes the interpreter's ordinary notification path.
    RawStorageWrite, // The action edits storage without replaying notifications.
}

/// Native physical records are inspection evidence, never comparison state for the graph.
#[derive(Clone, Debug, PartialEq, Eq)]
struct NativeInspection {
    operation: usize,                             // Native fine-step ordinal, excluded from graph comparisons.
    piston_state: Value,                          // Native-only serialized event/motion records, including raw identities.
    changed_cells: Vec<(FixturePosition, Value)>, // Native block/entity deltas for physical inspection.
}

/// The existing piston trace is normalized by source position before it enters comparisons.
#[derive(Clone, Debug, PartialEq, Eq)]
struct PistonTraceObservation {
    tick: u64,                                  // Logical tick reported by the existing piston trace collector.
    phase: AdvancePhase,                        // Phase reported by that collector.
    operation: PistonTraceOperationObservation, // Sample or accepted event normalized to provenance.
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum PistonTraceOperationObservation {
    Sample {
        // Collected piston power sample.
        actor: FixturePosition, // Source position, replacing any runtime-local ID.
        extended: bool,         // Supplied pose at sample time.
        powered: bool,          // Sampled power availability.
    },
    Accepted(PistonEventObservation), // Successfully applied event from the piston trace collector.
}

/// Raw physical details remain available for diagnosing the native oracle only.
#[derive(Clone, Debug, PartialEq, Eq)]
struct ReferenceEpisode {
    protocol: FixtureProtocol,                 // Pin and external protocol required to replay this oracle episode.
    records: Vec<ComparisonRecord>,            // Native observations later grouped at semantic boundaries.
    piston_trace: Vec<PistonTraceObservation>, // Cross-check from the dedicated piston trace collector.
    native: Vec<NativeInspection>,             // Native-only physical detail excluded from graph comparisons.
}

/// Fixture coordinates identify the sole source payload and its possible shared contacts.
#[derive(Clone, Copy)]
struct SharedResourceSpec {
    source: FixturePosition,              // Imported material position used as its immutable resource provenance.
    material: &'static str,               // Registry name used to recognize stationary and in-flight forms.
    contacts: &'static [FixturePosition], // Fixture positions where this test observes that shared resource.
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Difference {
    Protocol,              // Fixture hash, case, import, or external action protocol differed.
    SemanticBoundaryCount, // The compared continuations expose different semantic boundaries.
    Boundary,              // Tick, phase, stage, or action context first diverged.
    OrderedObservations,   // Callback direction/order or piston lifecycle order differed.
    Components,            // A named component boundary projection differed.
    PublishedCells,        // A changed logical cell projection differed.
    Resources,             // Shared-resource contact selection or availability differed.
    PendingTicks,          // Position, deadline bucket, priority, or order differed.
    PendingRequests,       // Queued piston request order or payload differed.
    Deadlines,             // A projected completion/publication deadline differed.
}

fn source_pos(value: &Value) -> FixturePosition {
    FixturePosition::from_world(serde_json::from_value(value.clone()).unwrap())
}

fn io_fixture(id: &str) -> Value {
    super::manifests_at(&super::root().join("test_data/instant-pistons-io"))
        .into_iter()
        .find(|manifest| manifest["id"] == id)
        .unwrap()
}

fn is_position(value: &Value) -> bool {
    value
        .as_array()
        .is_some_and(|parts| parts.len() == 3 && parts.iter().all(Value::is_number))
}

fn project_cell(value: &Value) -> ProjectedCell {
    match value["name"].as_str().unwrap() {
        "air" => ProjectedCell::Air,
        "moving_piston" => ProjectedCell::Unavailable,
        name => ProjectedCell::Stationary {
            name: name.to_owned(),
            properties: value["properties"].clone(),
            power_available: value["piston_power"].as_bool(),
        },
    }
}

fn event_observation(event: PistonEvent) -> PistonEventObservation {
    PistonEventObservation {
        actor: FixturePosition::from_world(event.pos),
        sticky: event.sticky,
        facing: event.facing,
        action: event.action,
    }
}

fn ordered_observation(entry: &Value) -> Option<OrderedObservation> {
    let data = &entry["data"];
    Some(match entry["kind"].as_str().unwrap() {
        "callback" => OrderedObservation::Callback {
            target: source_pos(&data["pos"]),
            block: data["block"].as_str().unwrap().to_owned(),
            direction: serde_json::from_value(data["dir"].clone()).unwrap(),
            source: data["source"].as_object().map(|_| source_pos(&data["source"])),
            parents: data["parents"].as_array().unwrap().iter().map(source_pos).collect(),
            power_available: data["piston_power"].as_bool(),
        },
        "piston_sample" => OrderedObservation::PistonSample {
            actor: source_pos(&data["pos"]),
            extended: data["extended"].as_bool().unwrap(),
            powered: data["powered"].as_bool().unwrap(),
        },
        "event_enqueue" => OrderedObservation::RequestQueued(event_observation(serde_json::from_value(data.clone()).unwrap())),
        "event_execute" => OrderedObservation::AcceptanceAttempt(event_observation(serde_json::from_value(data.clone()).unwrap())),
        "event_applied" => OrderedObservation::Accepted(event_observation(serde_json::from_value(data.clone()).unwrap())),
        "scheduled_tick" => OrderedObservation::ScheduledTickDispatched {
            position: source_pos(&data["pos"]),
            expected_block_type: data["block_type"].as_u64().map(|n| n as u32),
        },
        "wire_strength" => OrderedObservation::WireStrengthChanged {
            position: source_pos(&data["pos"]),
            old: data["old"].as_u64().unwrap() as u8,
            new: data["new"].as_u64().unwrap() as u8,
        },
        // Movement operations retain physical identities in NativeInspection only.
        "motion" => return None,
        kind => panic!("unhandled interpreter trace operation {kind}"),
    })
}

fn component_observations(
    port: &str,
    declared: &Value,
    actual: &Value,
    dimensions: (u32, u32, u32),
    rotation: u32,
    output: &mut Vec<ComponentObservation>,
) {
    if is_position(declared) {
        let local = triple(declared);
        let position = FixturePosition::from_world(transform(local, dimensions, rotation));
        output.push(ComponentObservation {
            port: port.to_owned(),
            position,
            cell: project_cell(actual),
        });
    } else if let Some(items) = declared.as_array() {
        let values = actual.as_array().unwrap();
        assert_eq!(items.len(), values.len(), "manifest observation shape: {port}");
        for (index, (declared, actual)) in items.iter().zip(values).enumerate() {
            component_observations(&format!("{port}[{index}]"), declared, actual, dimensions, rotation, output);
        }
    } else {
        let fields = declared.as_object().unwrap();
        for (name, declared) in fields {
            component_observations(&format!("{port}.{name}"), declared, &actual[name], dimensions, rotation, output);
        }
    }
}

fn resource_contact(value: &Value, material: &str) -> ResourceContactState {
    if value["name"] == material {
        return ResourceContactState::Enabled;
    }
    let carried_id = value["entity"]["MovingPiston"]["block_state"].as_u64().map(|id| id as u32);
    if value["name"] == "moving_piston" && carried_id.is_some_and(|id| Block::from_id(id).get_name() == material) {
        ResourceContactState::Disabled
    } else {
        ResourceContactState::Absent
    }
}

fn resource_observation(resource: &SharedResourceSpec, current: &BTreeMap<FixturePosition, Value>) -> SharedResourceObservation {
    let selected: Vec<_> = resource
        .contacts
        .iter()
        .copied()
        .filter(|contact| {
            current
                .get(contact)
                .is_some_and(|cell| resource_contact(cell, resource.material) != ResourceContactState::Absent)
        })
        .collect();
    assert!(selected.len() <= 1, "one shared payload has one live contact");
    let selected_contact = selected.first().copied();
    let enabled = selected_contact.is_some_and(|contact| resource_contact(&current[&contact], resource.material) == ResourceContactState::Enabled);
    SharedResourceObservation {
        source: resource.source,
        selected_contact,
        enabled,
    }
}

fn pending_ticks(snapshot: &Value) -> Vec<PendingTick> {
    snapshot["scheduled"]
        .as_array()
        .unwrap()
        .iter()
        .enumerate()
        .map(|(order, tick)| PendingTick {
            position: source_pos(&tick["pos"]),
            ticks_left: tick["ticks_left"].as_u64().unwrap() as u32,
            priority: serde_json::from_value(tick["tick_priority"].clone()).unwrap(),
            expected_block_type: tick["block_type"].as_u64().map(|id| id as u32),
            order,
        })
        .collect()
}

fn pending_requests(state: &Value) -> Vec<PistonEventObservation> {
    state["events"]
        .as_array()
        .unwrap()
        .iter()
        .map(|event| event_observation(serde_json::from_value(event.clone()).unwrap()))
        .collect()
}

fn pending_deadlines(state: &Value, tick: u64, phase: AdvancePhase) -> Vec<PendingDeadline> {
    let work = state["movement_work"].as_array().unwrap();
    let cursor = state["movement_cursor"].as_u64().unwrap() as usize;
    state["motions"]
        .as_array()
        .unwrap()
        .iter()
        .enumerate()
        .map(|(order, motion)| {
            let identity = motion["identity"].as_u64().unwrap();
            let has_current_visit = match phase {
                AdvancePhase::BetweenTicks => false,
                AdvancePhase::ScheduledTicks | AdvancePhase::PistonEvents => true,
                AdvancePhase::MovingEntities => work.iter().enumerate().skip(cursor).any(|(_, item)| item[1].as_u64() == Some(identity)),
            };
            let first_visit_offset = if has_current_visit { 0 } else { 1 };
            let progress = motion["progress"].as_f64().unwrap();
            let remaining_visits = if progress >= 1.0 {
                1
            } else if progress >= 0.5 {
                2
            } else {
                3
            };
            PendingDeadline {
                position: source_pos(&motion["pos"]),
                due_tick: tick + first_visit_offset + remaining_visits - 1,
                order,
            }
        })
        .collect()
}

fn protocol_action(action: &Value, order: usize, readiness: &[Value]) -> ProtocolAction {
    let operation = action["op"].as_str().unwrap().to_owned();
    let settled_ticks = match operation.as_str() {
        "wait" | "wait_ready" => Some(
            readiness.iter().find(|entry| entry["action"].as_u64() == Some(order as u64)).unwrap()["used_ticks"]
                .as_u64()
                .unwrap(),
        ),
        _ => None,
    };
    let delivery = match operation.as_str() {
        "wait" | "wait_ready" => None,
        "lever" | "destroy" | "notify" | "set_notified" | "construct_retracted" => Some(InputDelivery::Notified),
        "raw" => Some(InputDelivery::RawStorageWrite),
        kind => panic!("unclassified manifest operation {kind}"),
    };
    ProtocolAction {
        order,
        operation,
        delivery,
        raw: action.clone(),
        settled_ticks,
    }
}

fn observation_stage(snapshot: &Value, actions: &[ProtocolAction]) -> ObservationStage {
    match snapshot["label"].as_str().unwrap() {
        "initial" => ObservationStage::Imported,
        "setup-step" => ObservationStage::Settling,
        "step" => ObservationStage::Continuation,
        "after-action" => {
            let action = snapshot["action"].as_u64().unwrap() as usize;
            match actions[action - 1].delivery {
                Some(InputDelivery::Notified) => ObservationStage::NotifiedInput,
                Some(InputDelivery::RawStorageWrite) => ObservationStage::RawInput,
                None => ObservationStage::Settling,
            }
        }
        label => panic!("unclassified reference snapshot {label}"),
    }
}

fn capture_fixture_episode(manifest: &Value, case: &Value, resource_specs: &[SharedResourceSpec]) -> ReferenceEpisode {
    let mut raw = None;
    let piston_trace = trace::capture(|| {
        raw = Some(episode(manifest, case, 0, "pico", true));
    });
    let raw = raw.unwrap();
    assert_eq!(raw["fixture_sha256"], manifest["sha256"]);
    let initial = &raw["samples"][0];
    assert_eq!(initial["label"], "initial");
    assert!(initial["scheduled"].as_array().unwrap().is_empty());
    assert!(initial["piston_state"]["events"].as_array().unwrap().is_empty());
    assert!(initial["piston_state"]["motions"].as_array().unwrap().is_empty());

    let actions: Vec<_> = case["actions"]
        .as_array()
        .unwrap()
        .iter()
        .enumerate()
        .map(|(i, action)| protocol_action(action, i + 1, raw["readiness_checks"].as_array().unwrap()))
        .collect();
    let protocol = FixtureProtocol {
        fixture: manifest["fixture"].as_str().unwrap().to_owned(),
        sha256: manifest["sha256"].as_str().unwrap().to_owned(),
        case_id: case["id"].as_str().unwrap().to_owned(),
        strict_import_was_quiet: true,
        actions,
    };

    let watch: Vec<_> = raw["watch_absolute"].as_array().unwrap().iter().map(source_pos).collect();
    let mut current = BTreeMap::new();
    let mut projected_current = BTreeMap::new();
    let mut records = Vec::new();
    let mut native = Vec::new();
    for snapshot in raw["samples"].as_array().unwrap() {
        let changed_cells: Vec<_> = snapshot["changes"]
            .as_array()
            .unwrap()
            .iter()
            .map(|change| {
                let position = watch[change[0].as_u64().unwrap() as usize];
                let value = change[1].clone();
                current.insert(position, value.clone());
                (position, value)
            })
            .collect();
        let published_cells = changed_cells
            .iter()
            .filter_map(|(position, value)| {
                let cell = project_cell(value);
                (projected_current.insert(*position, cell.clone()).as_ref() != Some(&cell)).then_some(CellObservation { position: *position, cell })
            })
            .collect();
        let mut components = Vec::new();
        for (name, declared) in manifest["ports"]["observations"].as_object().unwrap() {
            component_observations(
                name,
                declared,
                &snapshot["observations"][name],
                (
                    manifest["dimensions"][0].as_u64().unwrap() as u32,
                    manifest["dimensions"][1].as_u64().unwrap() as u32,
                    manifest["dimensions"][2].as_u64().unwrap() as u32,
                ),
                0,
                &mut components,
            );
        }
        let tick = snapshot["tick"].as_u64().unwrap();
        let phase: AdvancePhase = serde_json::from_value(snapshot["phase"].clone()).unwrap();
        let ordered = snapshot["callbacks"].as_array().unwrap().iter().filter_map(ordered_observation).collect();
        let resources = resource_specs.iter().map(|resource| resource_observation(resource, &current)).collect();
        let state = &snapshot["piston_state"];
        records.push(ComparisonRecord {
            tick,
            phase,
            stage: observation_stage(snapshot, &protocol.actions),
            action: snapshot["action"].as_u64().unwrap() as usize,
            ordered,
            components,
            published_cells,
            resources,
            pending_ticks: pending_ticks(snapshot),
            pending_requests: pending_requests(state),
            deadlines: pending_deadlines(state, tick, phase),
        });
        native.push(NativeInspection {
            operation: snapshot["operation"].as_u64().unwrap() as usize,
            piston_state: state.clone(),
            changed_cells,
        });
    }

    let piston_trace = piston_trace
        .into_iter()
        .map(|entry| PistonTraceObservation {
            tick: entry.tick,
            phase: entry.phase,
            operation: match entry.operation {
                PistonTraceOperation::Sample { pos, extended, powered } => PistonTraceOperationObservation::Sample {
                    actor: FixturePosition::from_world(pos),
                    extended,
                    powered,
                },
                PistonTraceOperation::Applied(event) => PistonTraceOperationObservation::Accepted(event_observation(event)),
            },
        })
        .collect::<Vec<_>>();
    assert_trace_agrees(&records, &piston_trace);
    ReferenceEpisode {
        protocol,
        records,
        piston_trace,
        native,
    }
}

fn assert_trace_agrees(records: &[ComparisonRecord], trace: &[PistonTraceObservation]) {
    let ordered: Vec<_> = records
        .iter()
        .flat_map(|record| {
            record.ordered.iter().filter_map(move |observation| {
                let operation = match observation {
                    OrderedObservation::PistonSample { actor, extended, powered } => PistonTraceOperationObservation::Sample {
                        actor: *actor,
                        extended: *extended,
                        powered: *powered,
                    },
                    OrderedObservation::Accepted(event) => PistonTraceOperationObservation::Accepted(*event),
                    _ => return None,
                };
                Some(PistonTraceObservation {
                    tick: record.tick,
                    phase: record.phase,
                    operation,
                })
            })
        })
        .collect();
    assert_eq!(ordered, trace, "ordered recorder agrees with piston trace");
}

fn first_difference(expected: &ComparisonRecord, actual: &ComparisonRecord) -> Option<Difference> {
    if (expected.tick, expected.phase, expected.stage, expected.action) != (actual.tick, actual.phase, actual.stage, actual.action) {
        return Some(Difference::Boundary);
    }
    if expected.ordered != actual.ordered {
        return Some(Difference::OrderedObservations);
    }
    if expected.components != actual.components {
        return Some(Difference::Components);
    }
    if expected.published_cells != actual.published_cells {
        return Some(Difference::PublishedCells);
    }
    if expected.resources != actual.resources {
        return Some(Difference::Resources);
    }
    if expected.pending_ticks != actual.pending_ticks {
        return Some(Difference::PendingTicks);
    }
    if expected.pending_requests != actual.pending_requests {
        return Some(Difference::PendingRequests);
    }
    if expected.deadlines != actual.deadlines {
        return Some(Difference::Deadlines);
    }
    None
}

fn compare_episodes(expected: &ReferenceEpisode, actual: &ReferenceEpisode) -> Result<(), Difference> {
    if expected.protocol != actual.protocol {
        return Err(Difference::Protocol);
    }
    compare_records(&expected.records, &actual.records)
}

fn compare_records(expected: &[ComparisonRecord], actual: &[ComparisonRecord]) -> Result<(), Difference> {
    let expected = semantic_boundaries(expected);
    let actual = semantic_boundaries(actual);
    if expected.len() != actual.len() {
        return Err(Difference::SemanticBoundaryCount);
    }
    for (expected, actual) in expected.iter().zip(&actual) {
        if let Some(difference) = first_difference(expected, actual) {
            return Err(difference);
        }
    }
    Ok(())
}

fn same_semantic_state(left: &ComparisonRecord, right: &ComparisonRecord) -> bool {
    left.components == right.components
        && left.resources == right.resources
        && left.pending_ticks == right.pending_ticks
        && left.pending_requests == right.pending_requests
        && left.deadlines == right.deadlines
}

fn is_semantic_boundary(previous: Option<&ComparisonRecord>, record: &ComparisonRecord) -> bool {
    previous.is_none_or(|previous| previous.stage != record.stage || previous.action != record.action || !same_semantic_state(previous, record))
        || !record.ordered.is_empty()
        || !record.published_cells.is_empty()
}

fn semantic_boundaries(records: &[ComparisonRecord]) -> Vec<ComparisonRecord> {
    let mut boundaries: Vec<ComparisonRecord> = Vec::new();
    for record in records {
        if !is_semantic_boundary(boundaries.last(), record) {
            continue;
        }
        let same_boundary = boundaries
            .last()
            .is_some_and(|last| (last.tick, last.phase, last.stage, last.action) == (record.tick, record.phase, record.stage, record.action));
        if same_boundary && boundaries.last().is_some_and(|last| same_semantic_state(last, record)) {
            let boundary = boundaries.last_mut().unwrap();
            boundary.ordered.extend(record.ordered.iter().cloned());
            boundary.published_cells.extend(record.published_cells.iter().cloned());
            boundary.components.clone_from(&record.components);
            boundary.resources.clone_from(&record.resources);
            boundary.pending_ticks.clone_from(&record.pending_ticks);
            boundary.pending_requests.clone_from(&record.pending_requests);
            boundary.deadlines.clone_from(&record.deadlines);
        } else {
            boundaries.push(record.clone());
        }
    }
    boundaries
}

fn compact_trace(record: &ComparisonRecord) -> String {
    let operations: Vec<_> = record
        .ordered
        .iter()
        .map(|operation| match operation {
            OrderedObservation::Callback { target, direction, .. } => format!("U({target},{direction:?})"),
            OrderedObservation::PistonSample { actor, powered, .. } => {
                format!("sample({actor},{powered})")
            }
            OrderedObservation::RequestQueued(event) => {
                format!("queue({:?}@{})", event.action, event.actor)
            }
            OrderedObservation::AcceptanceAttempt(event) => {
                format!("check({:?}@{})", event.action, event.actor)
            }
            OrderedObservation::Accepted(event) => {
                format!("accept({:?}@{})", event.action, event.actor)
            }
            OrderedObservation::ScheduledTickDispatched { position, .. } => {
                format!("tick({position})")
            }
            OrderedObservation::WireStrengthChanged { position, old, new } => {
                format!("wire({position}:{old}->{new})")
            }
        })
        .take(8)
        .collect();
    let suffix = record.ordered.len().saturating_sub(operations.len());
    format!(
        "t{}/{} {}{}",
        record.tick,
        format_args!("{:?}", record.phase),
        operations.join(" > "),
        if suffix == 0 { String::new() } else { format!(" > … +{suffix}") }
    )
}

fn event(record: &ComparisonRecord, kind: impl Fn(&OrderedObservation) -> bool) -> bool {
    record.ordered.iter().any(kind)
}

fn record_with_contract_checks(episode: &ReferenceEpisode) -> &ComparisonRecord {
    episode
        .records
        .iter()
        .find(|record| {
            record.resources.len() == 1
                && !record.deadlines.is_empty()
                && record.ordered.len() > 1
                && record
                    .ordered
                    .iter()
                    .any(|operation| matches!(operation, OrderedObservation::Callback { direction: Some(_), .. }))
        })
        .expect("one OR boundary has callbacks, a pending deadline, and a shared resource")
}

fn first_callback_mut(record: &mut ComparisonRecord) -> &mut Option<BlockFace> {
    record
        .ordered
        .iter_mut()
        .find_map(|operation| match operation {
            OrderedObservation::Callback { direction, .. } if direction.is_some() => Some(direction),
            _ => None,
        })
        .expect("reference boundary contains a directional callback")
}

#[test]
fn empty_ordinary_actuator_has_a_native_sample_request_and_publication_trace() {
    let mut world = super::empty();
    let base = ORIGIN + BlockPos::new(0, 0, 0);
    let input = base.offset(BlockFace::West);
    let head = base.offset(BlockFace::South);
    world.set_block(
        base,
        Block::Piston {
            piston: RedstonePiston {
                facing: BlockFacing::South,
                sticky: false,
                extended: false,
            },
        },
    );
    let watched = [input, base, head];
    let callbacks = std::cell::RefCell::new(Vec::new());
    let piston_trace = trace::capture(|| {
        *callbacks.borrow_mut() = super::capture_at(&watched, || {
            world.set_block(input, Block::RedstoneBlock {});
            crate::redstone::update_surrounding_blocks(&mut world, input);
            for _ in 0..3 {
                world.tick_interpreted();
            }
        });
    });
    let trace = piston_trace
        .iter()
        .map(|entry| match entry.operation {
            PistonTraceOperation::Sample { pos, extended, powered } => format!("sample({},{extended},{powered})", FixturePosition::from_world(pos)),
            PistonTraceOperation::Applied(event) => format!("accept({:?}@{})", event.action, FixturePosition::from_world(event.pos)),
        })
        .collect::<Vec<_>>();
    let callbacks = callbacks.into_inner();
    assert!(trace.iter().any(|entry| entry.starts_with("sample(0,0,0,false,true)")));
    assert!(trace.iter().any(|entry| entry == "accept(Extend@0,0,0)"));
    assert!(
        callbacks
            .iter()
            .any(|entry| { entry["kind"] == "callback" && entry["data"]["pos"] == json!(base) && entry["data"]["dir"] == json!(BlockFace::West) })
    );
    assert!(matches!(
        world.get_block(base),
        Block::Piston { piston } if piston.extended && !piston.sticky
    ));
    assert!(matches!(world.get_block(head), Block::PistonHead { head } if !head.sticky));
    assert!(super::quiet(&world));
    let boundary = super::physical(&world, &watched);
    assert_eq!(boundary["cells"][1]["properties"]["extended"], "true");
    let ordered: Vec<_> = callbacks
        .iter()
        .filter_map(|entry| {
            let data = &entry["data"];
            let position = data.get("pos").map(source_pos).map(|position| position.to_string());
            Some(match entry["kind"].as_str().unwrap() {
                "callback" => format!(
                    "update({},{:?},power={:?})",
                    position.unwrap(),
                    serde_json::from_value::<Option<BlockFace>>(data["dir"].clone()).unwrap(),
                    data["piston_power"].as_bool()
                ),
                "piston_sample" => format!(
                    "sample({},{},{})",
                    position.unwrap(),
                    data["extended"].as_bool().unwrap(),
                    data["powered"].as_bool().unwrap()
                ),
                "event_enqueue" => format!("queue({}@{})", data["action"], position.unwrap()),
                "event_execute" => format!("check({}@{})", data["action"], position.unwrap()),
                "event_applied" => format!("accept({}@{})", data["action"], position.unwrap()),
                _ => return None,
            })
        })
        .collect();
    println!("empty actuator trace: {} > publish(0,0,0,extended=true)", ordered.join(" > "));
}

#[test]
fn fixtures_build_a_pinned_ordered_reference_contract() {
    let chain_manifest = super::fixture("instant_chain");
    let mut chain_case = super::named_case(&chain_manifest, "falling");
    chain_case["ticks"] = json!(3);
    let chain = capture_fixture_episode(&chain_manifest, &chain_case, &[]);
    assert_eq!(chain.protocol.sha256, chain_manifest["sha256"]);
    assert!(chain.protocol.strict_import_was_quiet);
    assert!(
        chain
            .protocol
            .actions
            .iter()
            .any(|action| { action.delivery == Some(InputDelivery::Notified) && action.operation == "destroy" })
    );
    assert!(chain.piston_trace.iter().any(|entry| matches!(
        entry.operation,
        PistonTraceOperationObservation::Accepted(PistonEventObservation { sticky: true, .. })
    )));
    assert!(
        chain.native.iter().flat_map(|entry| &entry.changed_cells).any(|(_, cell)| {
            cell["name"] == "moving_piston"
                && cell["entity"]["MovingPiston"]["block_state"]
                    .as_u64()
                    .is_some_and(|id| matches!(Block::from_id(id as u32).get_name(), "redstone_block" | "light_blue_wool"))
        }),
        "native-only cells retain the carried redstone/conductor evidence"
    );

    let bud_manifest = io_fixture("bud_noninstantinputs");
    let data_only = capture_fixture_episode(&bud_manifest, &super::named_case(&bud_manifest, "data-only"), &[]);
    let sampled = capture_fixture_episode(&bud_manifest, &super::named_case(&bud_manifest, "sample-1"), &[]);
    let memory = FixturePosition([0, 3, 3]);
    assert!(data_only.protocol.actions[0].delivery == Some(InputDelivery::Notified));
    assert!(
        !data_only.records.iter().any(|record| {
            record.ordered.iter().any(|operation| {
                matches!(
                    operation,
                    OrderedObservation::PistonSample { actor, .. } if *actor == memory
                )
            })
        }),
        "QC data alone does not sample the piston"
    );
    assert!(
        sampled.records.iter().any(|record| {
            record.ordered.iter().any(|operation| {
                matches!(
                    operation,
                    OrderedObservation::PistonSample { actor, .. } if *actor == memory
                )
            })
        }),
        "the distinct update input produces a power sample"
    );

    let or_manifest = io_fixture("or_1");
    let shared = SharedResourceSpec {
        source: FixturePosition([0, 1, 7]),
        material: "redstone_block",
        contacts: &[FixturePosition([0, 1, 7]), FixturePosition([1, 1, 7]), FixturePosition([0, 1, 8])],
    };
    let or_case = super::named_case(&or_manifest, "events-11-12");
    let or = capture_fixture_episode(&or_manifest, &or_case, &[shared]);
    assert!(
        or.records.iter().all(|record| { record.resources[0].selected_contact.is_some() }),
        "one physical payload remains selected at a shared contact"
    );
    let repeated = capture_fixture_episode(&or_manifest, &or_case, &[shared]);
    assert_eq!(compare_episodes(&or, &repeated), Ok(()));

    let adder_manifest = io_fixture("adder_1bit");
    let mut adder_case = super::named_case(&adder_manifest, "prepared-1-1-0");
    adder_case["ticks"] = json!(1);
    let adder = capture_fixture_episode(&adder_manifest, &adder_case, &[]);
    let target = FixturePosition([11, 2, 1]);
    assert!(adder.records.iter().any(|record| event(record, |operation| matches!(
        operation,
        OrderedObservation::RequestQueued(event) if event.actor == target
    ))));
    assert!(adder.records.iter().any(|record| event(record, |operation| matches!(
        operation,
        OrderedObservation::AcceptanceAttempt(event) if event.actor == target
    ))));
    assert!(
        !adder.records.iter().any(|record| event(record, |operation| matches!(
            operation,
            OrderedObservation::Accepted(event) if event.actor == target
        ))),
        "late inhibition cancels the queued target at acceptance"
    );

    println!("reference trace: {}", compact_trace(record_with_contract_checks(&or)));
}

#[test]
fn comparator_catches_direction_order_deadline_and_shared_resource_changes() {
    let manifest = io_fixture("or_1");
    let case = super::named_case(&manifest, "events-11-12");
    let resource = SharedResourceSpec {
        source: FixturePosition([0, 1, 7]),
        material: "redstone_block",
        contacts: &[FixturePosition([0, 1, 7]), FixturePosition([1, 1, 7]), FixturePosition([0, 1, 8])],
    };
    let reference = capture_fixture_episode(&manifest, &case, &[resource]);
    let expected = record_with_contract_checks(&reference).clone();

    let mut direction = expected.clone();
    let direction_value = first_callback_mut(&mut direction);
    *direction_value = direction_value.map(BlockFace::opposite);
    assert_eq!(first_difference(&expected, &direction), Some(Difference::OrderedObservations));

    let mut order = expected.clone();
    let pair = (0..order.ordered.len())
        .flat_map(|i| (i + 1..order.ordered.len()).map(move |j| (i, j)))
        .find(|&(i, j)| order.ordered[i] != order.ordered[j])
        .expect("trace has distinct ordered observations");
    order.ordered.swap(pair.0, pair.1);
    assert_eq!(first_difference(&expected, &order), Some(Difference::OrderedObservations));

    let mut deadline = expected.clone();
    deadline.deadlines[0].due_tick += 1;
    assert_eq!(first_difference(&expected, &deadline), Some(Difference::Deadlines));

    let mut shared_contact = expected.clone();
    shared_contact.resources[0].enabled = !shared_contact.resources[0].enabled;
    assert_eq!(first_difference(&expected, &shared_contact), Some(Difference::Resources));
    println!("comparison self-check: direction, order, deadline, and resource changes detected");
}

#[test]
fn comparator_ignores_unobservable_progress_visits() {
    let manifest = io_fixture("or_1");
    let case = super::named_case(&manifest, "events-11-12");
    let reference = capture_fixture_episode(&manifest, &case, &[]);
    let index = reference
        .records
        .iter()
        .position(|record| record.phase == AdvancePhase::MovingEntities && record.ordered.is_empty() && record.published_cells.is_empty())
        .expect("episode includes a native movement progress visit");
    let mut with_progress_visit = reference.records.clone();
    let mut progress = with_progress_visit[index].clone();
    progress.tick += 1;
    progress.ordered.clear();
    progress.published_cells.clear();
    with_progress_visit.insert(index + 1, progress);
    assert_eq!(compare_records(&reference.records, &with_progress_visit), Ok(()));
    with_progress_visit.remove(index + 1);
    assert_eq!(compare_records(&reference.records, &with_progress_visit), Ok(()));
    assert!(reference.native.windows(2).all(|steps| { steps[0].operation <= steps[1].operation }));
}

fn deep_audit_record() -> ComparisonRecord {
    ComparisonRecord {
        tick: 2,
        phase: AdvancePhase::PistonEvents,
        stage: ObservationStage::Continuation,
        action: 0,
        ordered: Vec::new(),
        components: Vec::new(),
        published_cells: Vec::new(),
        resources: vec![SharedResourceObservation {
            source: FixturePosition([0, 0, 0]),
            selected_contact: Some(FixturePosition([0, 0, 1])),
            enabled: true,
        }],
        pending_ticks: Vec::new(),
        pending_requests: Vec::new(),
        deadlines: Vec::new(),
    }
}

#[test]
fn deep_audit_comparison_preserves_resource_state_at_callback_boundary() {
    let start = deep_audit_record();
    let mut callback = start.clone();
    callback.ordered.push(OrderedObservation::PistonSample {
        actor: FixturePosition([5, 0, 0]),
        extended: false,
        powered: false,
    });
    let finish = start.clone();
    let expected = vec![start.clone(), callback.clone(), finish.clone()];
    callback.resources[0].enabled = false;
    let actual = vec![start, callback, finish];
    assert!(
        compare_records(&expected, &actual).is_err(),
        "later restoration must not erase a resource discrepancy at the same ordered sample"
    );
}

#[test]
fn deep_audit_comparison_preserves_deadline_at_callback_boundary() {
    let mut start = deep_audit_record();
    start.deadlines.push(PendingDeadline {
        position: FixturePosition([0, 0, 1]),
        due_tick: 4,
        order: 0,
    });
    let mut callback = start.clone();
    callback.ordered.push(OrderedObservation::PistonSample {
        actor: FixturePosition([5, 0, 0]),
        extended: false,
        powered: false,
    });
    let finish = start.clone();
    let expected = vec![start.clone(), callback.clone(), finish.clone()];
    callback.deadlines[0].due_tick += 1;
    let actual = vec![start, callback, finish];
    assert!(
        compare_records(&expected, &actual).is_err(),
        "later snapshots must not erase the wrong pending deadline at an observed boundary"
    );
}

#[test]
fn deep_audit_comparison_preserves_pending_tick_at_callback_boundary() {
    let mut start = deep_audit_record();
    start.pending_ticks.push(PendingTick {
        position: FixturePosition([0, 0, 1]),
        ticks_left: 2,
        priority: TickPriority::Normal,
        expected_block_type: Some(1),
        order: 0,
    });
    let mut callback = start.clone();
    callback.ordered.push(OrderedObservation::PistonSample {
        actor: FixturePosition([5, 0, 0]),
        extended: false,
        powered: false,
    });
    let finish = start.clone();
    let expected = vec![start.clone(), callback.clone(), finish.clone()];
    callback.pending_ticks[0].ticks_left += 1;
    let actual = vec![start, callback, finish];
    assert!(
        compare_records(&expected, &actual).is_err(),
        "an observed boundary must retain the exact pending scheduled-work state"
    );
}
