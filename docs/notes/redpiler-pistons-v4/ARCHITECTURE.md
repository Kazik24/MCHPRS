# Graph-native piston architecture

Status: proposal for review. This incorporates the user's explicit correction: **the graph does not simulate physical movement and has no Motion/progress model**. Read [requirements](FINDINGS.md) and [agent rules](AGENT_GUIDE.md) first.

## 1. The model in one circuit

A piston is a stateful switch with normal power, QC power, and update inputs. Its state selects electrical connections and ordered update outputs. A delivered update samples power; a queued action later validates it. A noninstant action can disable connections and schedule their restoration. Shared blocks are shared graph resources whose contact selection changes.

```text
normal power ---+
QC power -------+--> Piston node --> selected connections
update input -------^       |
                            +------> ordered update outputs
                            +------> scheduled connection changes
```

Power availability, sampling, requesting, acceptance, and publication remain separate. A change of QC data is not an update event. An update event need not change power. This distinction represents both BUD behavior and instant chains without a special BUD or instant node initially.

There are no moving-block nodes, per-block trajectories, interpolation values, carried block entities, or spatial movement loop. A little logical state is necessary: temporarily unavailable contacts, retained shared resources, and cancelled delayed work can affect subsequent queries. These are graph state and events, not physical movement simulation.

Two final connection sets alone cannot reproduce all requested noninstant histories. Preserve transient *connection availability* when observable. Do not add an electrical state merely because the interpreter has an internal field.

## 2. Layers and proposed files

```text
World snapshot + ordinary pending ticks
        |
        v
Inventory -> piston/contact discovery -> power links -> ordered update routes
        |
        v
Prepared circuit -> validation -> reviewed rewrites -> dense lowering
        |
        v
Graph backend: node state + shared resources + graph event queues
        |
        +---- input/output host boundary
        +---- projection/continuation adapter at flush or reset
```

The parser owns geometry. Execution follows compiled IDs, selects contacts, samples inputs, and delivers events. The host owns external commands, presentation, and interpreter handoff.

```text
redpiler/
  compile_graph.rs       typed construction graph and validation
  parser/
    mod.rs               preparation stages and diagnostics
    geometry.rs          pistons, contacts, shared resources, closure
    power.rs             guarded normal/side/QC/conductor links
    updates.rs           ordered update/shape routes
  backend/
    graph/
      mod.rs             lowering, ownership, host boundary
      schedule.rs        logical phases and pending graph events
      components.rs      fixed component rules and callbacks
      wire.rs            ordered generalized dust evaluation
      piston.rs          sample, accept, select contacts, publish updates
```

Create modules when their assigned step needs them. Reuse existing helpers and queues. No general instruction VM, physical World adapter, worker pool, or new dependency is needed.

Keep the current fast ordinary direct backend working. A rich piston graph must never fall back to native simulation.

## 3. Ownership and memory

| Owner | Data | Lifetime |
| --- | --- | --- |
| Program | Node descriptions, typed links, contact alternatives, local guards, update routes, wire plans, piston/resource descriptions | Immutable after lowering |
| Circuit state | Fixed component properties, piston flags, each shared resource's contact/availability, pending ticks/actions/events, logical clock | One serial owner |
| Wire walk | Lazy property snapshots, visitation/heading/layer data, ordered work queues | One synchronous walk; independent frame for nesting |
| Projection adapter | Position/templates needed to display or reconstruct graph-authored state | Immutable provenance; no simulation authority |

Use owned Vec/Box slices and checked indexing. No Arc, atomics, locks, packed IDs, or unsafe access initially. Lower StableGraph indices once. Construction staging is discarded after lowering.

Each mutable fact has one owner. Wire strength lives in its RedstoneWire state. Resource location lives in ResourceState. A piston does not own a second private copy of a shared resource. Contact facts are computed from those states and immutable descriptions, not mirrored into a mutable cell array.

Runtime nodes represent required circuit behavior: fixed components, wires, pistons, and connection contacts. Compile-time site references needed for ordered dust can include distinct constant air/solid references without turning every spatial cell into a mutable graph node.

## 4. Typed graph vocabulary

The following are proposed Rust shapes. Adapt names to repository conventions, preserving responsibilities. Introduce only the fields required by a reviewed step. Existing BlockFace, BlockPos, NodeType, NodeState, RedstoneWire, PistonAction, TickPriority, and CommandBlockEntity remain useful.

```rust
struct NodeId(u32);     // The value indexes program.nodes and state.components for one circuit node.
struct PistonId(u32);   // The value indexes program.pistons and state.pistons for one actuator.
struct ResourceId(u32); // The value indexes program.resources and state.resources for one shared block.
struct ContactId(u32);  // The value indexes program.contacts for one selectable connection location.
struct SiteId(u32);     // The value indexes program.sites for one immutable query or dust traversal location.
struct ProfileId(u32);  // The value indexes program.profiles for one immutable set of block facts.
struct GuardId(u32);    // The value indexes program.guards for one connection or delivery condition.
struct RouteId(u32);    // The value indexes program.routes for one ordered callback procedure.

enum InputPort { // This selects the receiving electrical channel.
    Main,           // This receives the component's primary or normal power input.
    Side,           // This receives side input used by comparator or repeater rules.
    Quasi,          // This receives piston QC power without causing sampling by itself.
    StrongIncident, // This receives primitive strong power used to calculate conduction.
}

enum PowerPort { // This selects the source electrical channel.
    Output,            // This reads the component's ordinary output strength.
    Weak(BlockFace),   // This reads weak power using the stored query face.
    Strong(BlockFace), // This reads primitive strong power using the stored query face.
    Conducted,         // This reads the maximum adjacent primitive strong input through a conductor.
}

enum LinkKind { // This distinguishes electrical dependencies from callback relations.
    Power { // This describes a guarded electrical channel between two nodes.
        from_port: PowerPort, // This selects the source node's power channel.
        to_port: InputPort,   // This selects the destination node's input channel.
        attenuation: u8,      // This specifies how much strength is lost along the connection.
        guard: GuardId,       // This references the condition that enables the connection.
    },
    Update { // This describes a guarded neighbor update independently of power changes.
        direction: Option<BlockFace>, // This preserves the supplied callback direction, including None.
        guard: GuardId,               // This references the condition that permits the update.
    },
    Shape { // This describes a guarded shape or support callback.
        direction: BlockFace, // This preserves the face supplied to the shape handler.
        guard: GuardId,       // This references the condition that permits the shape callback.
    },
}

struct LinkSpec { // This binds one typed relation to its graph endpoints.
    from: NodeId,   // This identifies the source circuit node.
    to: NodeId,     // This identifies the receiving circuit node.
    kind: LinkKind, // This describes the relation's channel, condition, and callback metadata.
}
```

Update and Shape relations are first-class; their execution order comes from route tables, not graph edge iteration. Existing Default/Side constructors can remain ordinary power conveniences. Clamping/deduplication/cycle passes must consume the appropriate link family. Packed lowering and old binary export cannot silently truncate richer links.

Conducted is the maximum adjacent primitive strong input. It does not relay charge through arbitrary chains of cubes. A stationary redstone resource supplies weak 15, strong 0. Disabled contacts supply no carried material's emission/conduction.

## 5. Program and runtime shapes

```rust
struct CircuitProgram { // This owns the immutable descriptions used by graph execution.
    nodes: Box<[NodeSpec]>,         // This stores circuit node descriptions indexed by NodeId.
    links: Box<[LinkSpec]>,         // This stores typed relations grouped by destination during lowering.
    profiles: Box<[Profile]>,       // This stores immutable block facts indexed by ProfileId.
    guards: Box<[Guard]>,           // This stores local connection and callback conditions indexed by GuardId.
    contacts: Box<[ContactSpec]>,   // This stores selectable connection descriptions indexed by ContactId.
    routes: Box<[Route]>,           // This stores ordered callback procedures indexed by RouteId.
    wires: Box<[WirePlan]>,         // This stores compiled dust neighborhoods referenced by wire nodes.
    sites: Box<[SiteSpec]>,         // This stores immutable query and traversal bindings indexed by SiteId.
    pistons: Box<[PistonSpec]>,      // This stores actuator descriptions indexed by PistonId.
    resources: Box<[ResourceSpec]>, // This stores shared block descriptions indexed by ResourceId.
}

struct NodeSpec { // This describes one node's behavior and compiled dependencies.
    position: BlockPos,                  // This records the source position for diagnostics and projection.
    kind: RuntimeNodeKind,               // This selects the node's behavior and specialized description.
    incoming: std::ops::Range<usize>,     // This selects the node's incoming entries in program.links.
    output_route: Option<RouteId>,       // This references an optional output notification procedure.
}

enum RuntimeNodeKind { // This selects the runtime behavior of a circuit node.
    Electrical(NodeType), // This stores an existing fixed-component behavior description.
    Observer { // This binds a stationary observer to its watched site and orientation.
        watched: SiteId,    // This identifies the compiled site whose qualifying updates are observed.
        facing: BlockFace, // This stores the fixed orientation used by callback and output rules.
    },
    Wire(u32),          // This stores the index of the node's WirePlan in program.wires.
    Piston(PistonId),   // This references the actuator's immutable description and dedicated state.
    Contact(ContactId), // This references the selectable connection whose facts this node exposes.
}

enum ComponentState { // This owns mutable properties that are not stored in specialized tables.
    Electrical(NodeState), // This stores ordinary powered, locked, and output-strength properties.
    Wire(RedstoneWire),    // This is the sole owner of a wire's stored strength and sides.
    CopperBulb { // This keeps a bulb's input state distinct from its retained light state.
        powered: bool, // This records whether the bulb currently considers itself powered.
        lit: bool,     // This records the bulb's retained visible light state.
    },
    CommandBlock(CommandBlockEntity), // This owns command activation, result, and execution metadata.
    Specialized, // This stores no duplicate properties for pistons or derived contacts.
}

struct CircuitState { // This owns all persistent mutable simulation and pending work.
    components: Box<[ComponentState]>, // This stores ordinary component properties indexed by NodeId.
    pistons: Box<[PistonState]>,        // This stores actuator state indexed by PistonId.
    resources: Box<[ResourceState]>,   // This stores authoritative shared contact selection indexed by ResourceId.
    scheduler: TickScheduler<ScheduledNodeTick>, // This owns fixed-component deadlines, priorities, and pending membership.
    piston_actions: std::collections::VecDeque<PistonRequest>, // This preserves queued actuator requests in FIFO order.
    deferred: Vec<DeferredEvent>,       // This stores pending timed connection changes with their sequence identities.
    deferred_work: Vec<u64>,            // This freezes eligible deferred event sequence IDs for the current phase.
    deferred_cursor: usize,             // This identifies the next entry to consume in deferred_work.
    clock: Clock,                       // This owns logical time and the current execution phase.
    next_sequence: u64,                  // This allocates distinct creation-order identities for deferred events.
    dirty_nodes: FxHashSet<NodeId>,      // This records nodes awaiting presentation without changing their electrical state.
    effects: Vec<Effect>,               // This buffers external presentation effects in production order.
}

struct GraphBackend { // This combines one immutable program with its mutable execution state.
    program: CircuitProgram, // This owns the validated compiled circuit descriptions.
    state: CircuitState,     // This owns the current state and pending work for that program.
}

struct PreparedCircuit { // This stages a read-only compilation result before validation and activation.
    graph: CompileGraph,               // This owns the construction graph before dense runtime lowering.
    descriptions: CircuitDescriptions, // This holds supporting descriptions using construction references.
    initial: InitialCircuitState,      // This holds imported logical state and pending ticks before lowering.
}
```

Input links are grouped by target during lowering, making incoming ranges meaningful. Validate ranges, IDs, strengths, guard acyclicity, and kind/state correspondence before publication.

CircuitDescriptions holds the not-yet-lowered contact/guard/route/wire/piston/resource descriptions using construction indices. InitialCircuitState holds imported fixed component properties, initial piston/resource state, ordinary pending ticks, and logical clock. These staging records lower once into the program/state above; they are not extra execution authorities.

Electrical excludes specialized kinds. Observers reuse electrical powered state. Command-block entity fields already include powered state and success count. Never mirror wire strength or resource contact in generic output fields. Existing pending_tick flags are derived scheduler-membership caches, not separate deadlines.

## 6. Contacts and shared resources

A contact is a compiled place where a resource or actuator state changes the circuit's connections. It is not a simulated moving block. It can join/remove several channels, change dust-query facts, and select callback recipients.

```rust
struct Profile { // This records immutable block facts extracted during compilation.
    raw_state: u32,                // This stores the registry state used for projection and diagnostics.
    registry_type: u32,            // This stores the block type used to validate type-bound scheduled work.
    solid: bool,                   // This records the solidity fact used by conduction and dust queries.
    transparent: bool,             // This records the transparency fact used by compiled query rules.
    cube: bool,                    // This records the cube fact used by relevant connection and support rules.
    electrical: ProfileElectrical, // This selects how the profile obtains primitive electrical emission.
}

enum ProfileElectrical { // This classifies primitive emission without decoding blocks at runtime.
    None,           // This supplies no primitive emission while retaining the profile's conduction facts.
    RedstoneSource, // This supplies stationary redstone-block weak power 15 and strong power 0.
    Component,      // This obtains emission from the referenced component's ports and state.
}

struct ContactSpec { // This describes one connection location and its possible logical views.
    node: NodeId,                    // This identifies the circuit node exposing the contact's connections.
    position: BlockPos,               // This records the physical source location for diagnostics and projection.
    variants: Box<[ContactVariant]>,  // This lists guarded alternatives from which exactly one live view is selected.
}

struct ContactVariant { // This describes the facts and receiver selected under one contact condition.
    when: GuardId,                       // This references the condition that selects this alternative.
    profile: ProfileId,                   // This references the electrical and geometry facts exposed by this view.
    receiver: Option<CallbackReceiver>,   // This selects an eligible callback handler or no handler.
}

enum CallbackReceiver { // This selects the handler reached through a resolved site or contact.
    Component(NodeId),    // This identifies the ordinary component that receives the callback.
    PistonBase(PistonId), // This identifies the actuator whose base receives the callback.
    PistonHead(PistonId), // This identifies the actuator to which a head callback is forwarded.
}

struct ResourceSpec { // This describes one conserved shared block independently of actor ownership.
    material: ProfileId,           // This references the supported material's stationary facts and projection template.
    contacts: Box<[ContactId]>,    // This lists the certified contacts that this resource may select.
}

struct ResourceState { // This is the sole authority for a shared resource's current connection selection.
    at: ContactId, // This identifies the contact currently selected by the resource.
    enabled: bool, // This records whether the selected contact exposes the resource's stationary channels.
    revision: u64, // This invalidates delayed resource events issued before the latest replacement or recapture.
}
```

Profiles are immutable registry facts extracted at compile time. Inert conduction follows solid/strong-input rules; RedstoneSource emits weak 15/strong 0. The evaluator does not decode raw blocks or call World-dependent power helpers. Contact variants cover empty, supported stationary material, actuator head/base, and electrically unavailable cases as required. At a live query exactly one alternative must apply. A captured query retains the alternative observed by its wire frame.

ResourceState is the authoritative contact selection. Disabled means the resource is retained but currently supplies no stationary material channels. Release/drop/recapture changes that selection or availability through typed graph operations; it does not create or lose a private actor-owned copy.

revision invalidates obsolete delayed resource events. Piston and resource revisions are separate: reversing one actor must not accidentally cancel an independent shared resource's valid future event. Check that no two resources claim an exclusive contact.

The initial movable subset remains redstone blocks and explicitly supported entity-free inert cubes such as required wool/concrete/quartz states. Reject moved observers, electrical components, containers, pistons, and unknown material. A stationary observer or foreign base at Far remains fixed context.

Contact graphs can split a net into as many components as the parsed connections require. There is no four-piece cap and no enumeration of all global piston pose combinations.

## 7. Compact piston state and events

```rust
struct PistonSpec { // This describes one actuator's fixed geometry bindings and callback procedures.
    node: NodeId,          // This identifies the circuit node that samples and accepts actuator actions.
    facing: BlockFace,     // This stores the immutable actuator direction used by parsed lane rules.
    sticky: bool,          // This determines whether supported retraction may pull a shared resource.
    base: ContactId,       // This identifies the contact exposing base facts and callback eligibility.
    near: ContactId,       // This identifies the immediately forward head or resource contact.
    lane: PistonLane,      // This selects the admitted resource capacity and optional Far contact.
    routes: PistonRoutes,  // This references ordered notifications for changes and restorations.
}

enum PistonLane { // This describes the supported connection footprint without assigning private ownership.
    Empty, // This admits an actuator without a transported resource or owned Far contact.
    SingleResource { // This admits at most one shared resource along the actor's parsed lane.
        far: ContactId, // This identifies the second forward contact used by extension and pull rules.
    },
}

struct PistonState { // This stores the compact logical state needed for actuator behavior.
    extended: bool,                    // This records the actuator's pose bit at the current publication boundary.
    availability: PistonAvailability,  // This selects derived base and head contact eligibility.
    revision: u64,                     // This invalidates obsolete delayed base or head events.
}

enum PistonAvailability { // This describes contact publication state without physical progress.
    Ready,                 // This enables base sampling and derives head forwarding from the pose.
    HeadPending,           // This permits base sampling while head forwarding awaits publication.
    PublishingHeadRemoval, // This exposes a removed head while nested callbacks still see a sampleable base.
    BasePending,           // This disables base sampling and head forwarding until base restoration.
}

struct PistonRoutes { // This binds actuator operations to their ordered callback procedures.
    base_changed: RouteId,              // This references notifications published when the base contact changes.
    near_changed: RouteId,              // This references notifications published when the Near contact changes.
    far_changed: Option<RouteId>,       // This references optional notifications for changes at the resource's Far contact.
    base_restored: RouteId,             // This references notifications and rechecks for base restoration.
    head_restored: RouteId,             // This references head notifications without a native self-recheck.
    resource_restored_near: Option<RouteId>, // This references a resource self-recheck and notifications at Near.
    resource_restored_far: Option<RouteId>, // This references a resource self-recheck and notifications at Far.
}

struct PistonRequest { // This records a sampled action awaiting live acceptance.
    piston: PistonId,    // This identifies the actor whose live power and contacts must be rechecked.
    action: PistonAction, // This stores the requested extension, retraction, or retraction without pull.
}

enum DeferredAction { // This selects a delayed logical publication and its validity check.
    EnableBase { // This restores a still-valid actor's base contact eligibility.
        piston: PistonId, // This identifies the actor whose base is restored.
        revision: u64,    // This must match the actor's current revision before publication.
    },
    EnableHead { // This restores a still-valid actor's head contact eligibility.
        piston: PistonId, // This identifies the actor whose head is restored.
        revision: u64,    // This must match the actor's current revision before publication.
    },
    EnableResource { // This restores a still-valid shared resource's selected stationary channels.
        resource: ResourceId, // This identifies the shared resource to enable.
        contact: ContactId,   // This identifies the expected destination contact for this event.
        revision: u64,        // This must match the resource's current revision before publication.
        actor: PistonId,      // This retains the initiating actor used to derive transfer direction and kind.
    },
}

struct DeferredEvent { // This schedules one logical connection publication.
    due_tick: u64,          // This records the logical tick at which the event becomes eligible.
    sequence: u64,          // This provides a stable event identity and creation order within the deferred phase.
    action: DeferredAction, // This stores the publication target and revision check.
}
```

The pose bit and availability describe graph eligibility. Ready enables base sampling and lets head forwarding follow the pose. HeadPending keeps base sampling while head forwarding is unavailable. BasePending disables base sampling and removes head forwarding. Contact facts are derived from this state; no independent head/base flags are needed initially.

PublishingHeadRemoval is a transient publication state required by a concrete reference sequence: retraction interrupts an unavailable head and notifies its removal while the base is still extended and sampleable, before making the base unavailable. Nested callbacks must see that view. It cannot be a stationary entry state and does not survive the completed synchronous action. Similarly, extension notifications preceding the late pose write see the old pose. These distinctions are ordered graph publication, not physical movement stages.

An empty ordinary actor has no transported Far resource. A single-resource lane has capacity for at most one supported resource; it does not permanently bind that actor to a payload. Resolve the shared contact live at sample/acceptance.

Named functions are sample_piston, enqueue_action, accept_action, extend_connections, retract_connections, release_resource, and apply_deferred_event. They change flags/contact selection and publish source-specific routes in the reference order.

A noninstant operation can temporarily disable resource emission or base/head callbacks and enqueue their re-enabling events. An instant sequence can interrupt/cancel those events and immediately select another connection state. No floating-point progress or tick-by-tick advancement of physical entities is necessary.

EnableResource retains the initiating actor and destination contact. In the initial SingleResource lane, that actor's Far destination means extension and Near means retraction; direction comes from its immutable facing. Preserve this provenance after sharing. Test both cases, because an unavailable extending resource and an unavailable retracting resource can require different release/pull behavior. A future lane that makes this derivation ambiguous must add a typed transfer kind before admission.

Delayed-event deadlines and creation/FIFO order supply timing. Derive any required early-release predicate from the relevant pending event, phase, and current publication order; a not-yet-due event can still affect such a predicate. Pass the current publication sequence as callback context where necessary rather than advancing every resource. Do not recreate progress under another field name. If that derivation fails a concrete reference case, bring the smallest additional logical state to review.

Different base/head/resource completions are separate graph events when their update order is observable. Skipping intermediate physical interpolation is safe only when its effects on sampling are captured by those logical predicates. Do not blindly copy a three-tick delay: establish first deferred eligibility and tick/phase convention in Step 08. At a resource's deadline, another earlier completion can notify its actor before this resource publishes; preserve that case as well as notifications before the deadline.

Facing/stickiness remain immutable while compiled. Requests can read that fixed description; future mutable configuration would require captured metadata. Equal actor/action requests deduplicate; different actions remain ordered.

## 8. Local guards and parser proof

```rust
enum OperandView { // This selects which observation a query uses.
    Live,     // This reads the current graph state at the query boundary.
    Captured, // This reads the site's lazy observation from the active wire walk.
}

enum Guard { // This describes a bounded local condition for a connection or callback.
    Always,           // This permits the relation without a conditional test.
    State(StateTest), // This evaluates live logical state used to select contacts.
    Site { // This evaluates a compiled site property using an explicit observation view.
        site: SiteId,       // This identifies the compiled site whose property is tested.
        view: OperandView,  // This chooses current state or the active walk's captured observation.
        test: SiteTest,     // This selects the property predicate to evaluate.
    },
    Not(GuardId),          // This negates the referenced guard's result.
    All(Box<[GuardId]>),   // This requires every referenced guard to hold.
    Any(Box<[GuardId]>),   // This requires at least one referenced guard to hold.
}

enum StateTest { // This selects a predicate over authoritative live piston or resource state.
    PistonExtended(PistonId), // This tests the identified actuator's current pose bit.
    PistonAvailability { // This tests the identified actor's contact publication state.
        piston: PistonId,       // This identifies the actor whose availability is inspected.
        is: PistonAvailability, // This specifies the availability value required by the predicate.
    },
    ResourceAt { // This tests whether a shared resource selects a particular contact.
        resource: ResourceId, // This identifies the resource whose authoritative selection is inspected.
        contact: ContactId,   // This identifies the required selected contact.
    },
    ResourceEnabled(ResourceId), // This tests whether the identified resource's stationary channels are enabled.
}

enum SiteTest { // This selects a predicate over a resolved site's immutable facts.
    Solid,              // This tests the selected profile's solidity fact.
    Transparent,        // This tests the selected profile's transparency fact.
    Cube,               // This tests the selected profile's cube fact.
    IsWire,             // This tests whether the resolved site represents a wire.
    Profile(ProfileId), // This requires the resolved profile to match the referenced profile.
}
```

Use bounded, acyclic, local expressions. State guards resolve a live contact alternative; capture_site then stores that resolved profile/receiver/properties. Later captured queries read the stored observation, not the resource's current selection. Do not invent a captured global resource table. Captured site guards require the active walk frame; guard evaluation must not recursively resolve its own contact selector. Add wire-property predicates only when a real channel requires them. No SAT/BDD framework, global Cartesian pose table, or extracted CPU truth table.

The dedicated piston parser inventories saved geometry, resolves shared resources, closes permitted contacts under transfer/drop/recapture, then compiles power and callback dependencies. It must certify that the reduced logical state is enough for all admitted operations.

Closure obligations include possible successful longer pushes, outside-footprint transfers, support loss, observer-visible intermediate changes, and timing-sensitive recapture. A legal blocked extension remains legal runtime behavior. A possible successful operation outside the supported lane model is a compilation error with a concrete witness.

Do not certify only the saved pose. Use conservative local reachable alternatives and explicit preparation limits; incomplete analysis fails. Larger requested fixtures remain targets to prove, not assumed admissible from inventory.

The reference interpreter's physical internal state is test evidence. Compare it through a justified graph-observable projection; never require the graph to recreate a mutable world just to compare two records. Conversely, do not hide a physical distinction that changes a future callback or output.

## 9. Ordered routes and callbacks

```rust
struct Route { // This owns one callback procedure's ordered recipient sequence.
    deliveries: Box<[RouteDelivery]>, // This preserves recipient order and duplicate deliveries.
}

struct RouteDelivery { // This describes one guarded delivery within an ordered route.
    target: SiteId,     // This identifies the compiled site whose receiver is resolved at delivery.
    kind: DeliveryKind, // This selects the callback family and literal direction.
    guard: GuardId,    // This references the condition that permits this delivery.
}

enum DeliveryKind { // This selects the receiver operation while retaining its supplied direction.
    Update { // This delivers an ordinary neighbor update.
        direction: Option<BlockFace>, // This preserves the update's supplied face or directionless None.
    },
    Shape { // This delivers a shape or support change.
        direction: BlockFace, // This preserves the face supplied to the shape handler.
    },
}

enum CallbackState { // This selects the recipient state supplied to callback dispatch.
    Live,                   // This resolves the recipient from current graph state at dispatch.
    Captured(ObservedSite), // This supplies the observation retained by the originating wire walk.
}

struct Callback { // This describes one callback ready for synchronous dispatch.
    target: SiteId,       // This identifies the compiled recipient site.
    kind: DeliveryKind,   // This selects the receiver operation and supplied direction.
    state: CallbackState, // This selects live resolution or a supplied captured observation.
}
```

Routes and typed callback edges are two views of the same construction relations. Lowering validates their correspondence. Retain procedure/sequence provenance; execution follows routes, never incidental edge order.

deliver_route resolves each receiver at delivery and finishes its nested callback before the next delivery. Preserve literal direction/None, duplicates, shape-versus-power exclusions, and head-to-base forwarding. A generic state write is not an observer trigger.

An observer watches a compiled site/contact and receives qualifying routes even when the watched contact changes only logical availability. Preserve powered/None recovery, pending suppression, and directional output power.

## 10. Generalized dust with compiled context

```rust
struct SiteSpec { // This binds one query or traversal identity to a compiled source.
    position: BlockPos, // This records the location for compile-time ordering, diagnostics, and projection.
    source: SiteSource, // This selects where the site's facts and receiver are obtained.
}

enum SiteSource { // This selects a site's immutable binding without runtime spatial discovery.
    Component(NodeId),   // This obtains properties and callbacks from the identified circuit component.
    Contact(ContactId),  // This obtains the currently selected facts and receiver of the identified contact.
    Constant(ProfileId), // This supplies immutable air or fixed-context facts from the identified profile.
}

struct WirePlan { // This stores the compiled neighborhood for one generalized wire.
    node: NodeId,                       // This identifies the wire's circuit node and authoritative component state.
    canonical_neighbors: [SiteId; 24],  // This identifies the ordered context sites used by Turbo traversal.
}

struct ObservedSite { // This retains one lazily resolved site view for a wire walk.
    profile: ProfileId,                  // This identifies the profile selected when the site was captured.
    receiver: Option<CallbackReceiver>,  // This retains the callback handler eligible in that captured view.
    properties: ObservedProperties,      // This retains the captured block-state properties used by relevant handlers.
}

enum ObservedProperties { // This stores captured block properties without copying live entity outputs.
    Passive, // This records that the captured site has no dynamic block-state properties.
    Electrical { // This retains fixed-component block properties needed by supplied-state handlers.
        powered: bool, // This records the component's powered property at capture.
        locked: bool,  // This records the repeater lock property at capture where applicable.
    },
    Wire(RedstoneWire), // This retains captured wire sides and strength as updated by this walk's rules.
    Piston { // This retains the actuator property supplied by a captured base or head view.
        extended: bool, // This records the pose bit visible when the site was captured.
    },
    CopperBulb { // This retains the bulb properties visible to the originating walk.
        powered: bool, // This records the bulb's powered property at capture.
        lit: bool,     // This records the bulb's retained light property at capture.
    },
}

struct WalkSite { // This owns one site's transient traversal state inside a single wire walk.
    observed: ObservedSite, // This stores the lazily captured facts, receiver, and properties.
    visited: bool,          // This records whether traversal has visited this site.
    x_bias: i32,            // This records the horizontal X bias used to choose traversal heading.
    z_bias: i32,            // This records the horizontal Z bias used to choose traversal heading.
    layer: u32,             // This records the latest traversal layer assigned to the site.
}

struct WireWalk { // This owns independent scratch state for one synchronous dust propagation walk.
    sites: FxHashMap<SiteId, WalkSite>, // This maps discovered site identities to their local observations and traversal state.
    queues: [std::collections::VecDeque<SiteId>; 3], // This preserves ordered work for the current and next two traversal layers.
    layer: u32, // This records the traversal layer currently being processed.
}
```

SiteId retains distinct traversal identities for air/solid context; SiteSource is immutable. There is no mutable block array or spatial neighbor lookup. The parser supplies the 24 references and electrical/query/shape context.

Named operations are evaluate_seed, capture_site, choose_heading, enqueue_neighbors, evaluate_turbo_wire, and dispatch_captured_callback. Seed and Turbo contributor rules differ. Reuse TURBO_ORDER, on_neighbor_changed_from, and regulate_sides where pure.

Capture block properties/contact alternatives lazily. Refresh captured wire power as the reference walk does, retaining captured query facts. Strong-input neighbors and comparator/entity outputs may remain live; make each operand choice explicit. Nested walks own independent frames.

Contacts can change solid/ascent/conduction facts without moving a block in the runtime. Necessary wire shape changes arrive through parsed routes. Selecting another contact variant does not automatically normalize dust or deliver all neighbors an update.

## 11. Scheduler and host effects

```rust
struct ScheduledNodeTick { // This binds scheduled fixed-component work to a node and expected block type.
    node: NodeId,    // This identifies the component that receives the scheduled transition.
    block_type: u32, // This records the registry type required for the work to remain valid.
}

enum GraphPhase { // This selects the graph's logical execution phase.
    BetweenTicks,        // This denotes a boundary with no tick phase currently executing.
    ScheduledTicks,      // This processes due fixed-component ticks around one scheduler advance.
    PistonActions,       // This validates queued actuator actions and drains newly appended actions.
    DeferredConnections, // This publishes the frozen batch of due connection changes.
}

struct Clock { // This records logical time and the scheduling boundary needed for ordered execution.
    logical_tick: u64,         // This records the current game tick used by deadlines and command execution.
    phase: GraphPhase,         // This identifies the execution phase currently in progress.
    scheduled_advanced: bool,  // This records whether the scheduler has advanced during this tick.
}

enum Effect { // This describes buffered host presentation without becoming simulation authority.
    BlockAction { // This requests a client-visible block action at a compiled node's position.
        node: NodeId,      // This identifies the node whose projection position anchors the action.
        action: BlockAction, // This stores the block action payload sent to the host.
    },
    Sound { // This requests a sound at a compiled node's projection position.
        node: NodeId, // This identifies the node whose position anchors the sound.
        id: i32,      // This identifies the sound in the host's sound registry.
        category: i32, // This selects the host sound category.
        volume: f32,  // This specifies the sound's playback volume.
        pitch: f32,   // This specifies the sound's playback pitch.
    },
}
```

Reuse TickScheduler priority/FIFO/ring semantics and position/type-bound membership. Preserve imported duplicate ticks; remove membership before dispatch. Piston actions and delayed graph connections are distinct from fixed-component ticks.

Drain the old current bucket, advance once, drain the new bucket, drain piston actions including nested newly appended actions, then process due deferred connection events in their defined creation order. Events produced during deferred publication wait for the next piston-action phase. Freeze sequence IDs in deferred_work before delivery; deferred_cursor marks consumed work. Newly created/replaced work must not inherit another event's turn. Validate this ordering against the reference, including ties.

The batch/cursor records a scheduling frontier, not physical motion. It is necessary for supported partial-phase inspection/continuation. If the initial host exposes only complete ticks, it can be local driver scratch instead of persistent CircuitState fields; tests and later fine stepping must use an explicit frontier when they expose those boundaries. Deadlines alone do not identify already consumed versus newly inserted work.

The deferred phase applies logical connection events; it does not iterate physical entities or update progress. Absolute deadlines can skip unobservable interpolation work. Per-operation tests still verify event boundaries.

Command activation, conditions, chain references, success counts, and feedback belong to graph component code. The existing host allowlist/output parser may be called synchronously for execution/result only. Never defer a command result until flush. Sound/block-action buffers are presentation, not simulation.

## 12. Activation, projection, and continuation

Preparation borrows the world and stages everything. Validate/lower/check cancellation before publishing. Transfer scheduler ownership once after success. Failed preparation preserves blocks, entities, queues, clock, and active flags.

Positions/state templates are projection metadata. A display flush cannot change graph state, sample power, finish a transition, or settle dust. Graph inputs read authoritative logical state; geometry/configuration edits reset first. Preserve untouched context entities.

Exact interpreter continuation is still required by the earlier agreement. It is a boundary adapter obligation, not a reason to put a physical Motion model in the executor. At reset, translate piston state, selected contacts, pending graph events, deadlines, and ordering into the interpreter's required blocks/entities/queues.

For a stopped noninstant transition, derive physical interpolation metadata from its graph event timeline only inside that adapter. The graph stores no progress fields and does not advance physical motion. Existing physical identity values are implementation details; reconstruct identities consistently with the pending future and compare normalized provenance, not unrelated raw IDs.

This derivation is a proof obligation. If two physical histories collapse to the same graph state yet have different continuation, the abstraction is insufficient. Retain the smallest distinguishing logical fact or reject that mechanic before activation. Never silently delay reset until settlement or claim exact continuation without a test.

Fine stepping need not copy unobservable interpreter micro-operations. Preserve supported semantic inspection boundaries; reset before unsupported modes. Do not approximate a partially executed observable callback/event boundary.

## 13. Optimization boundary

Always construct required generalized dust/contact/update detail independently of -O. A later pass removes only certified detail while preserving channels, updates/order, observers, history, arbitrary admitted controls, and continuation.

Start with static callback-insensitive dust cones and retain fast ordinary lowering. Dynamic contacts, stale saved dust, shape routes, and prospective heads invalidate naive final-strength-only elimination.

Later instant/reset and BUD rewrites are stateful graph transducers. Prove response, cancellation, restoration, rearm, shared-resource behavior, and boundary equivalence. Actor count/proximity alone is not a certificate.

Versioned binary export, visualization products, and multithreading need separate plans. Minimal textual graph/provenance/event traces are required now.

## 14. Review invariants

- No Motion type, progress fields, carried-block model, mutable world-cell array, or physical movement loop exists in graph execution.
- One owner for each mutable fact; resource selection is shared, wire strength is not mirrored.
- Runtime follows validated IDs and local guarded relations; geometry discovery is compile-time work.
- Normal/QC inputs, callbacks, requests, acceptance, and delayed publications stay distinguishable.
- Every contact selects an unambiguous variant; shared resources cannot duplicate or conflict at exclusive contacts.
- Supplied/live queries, route order, duplicates, FIFO, priorities, and deferred ties have explicit semantics.
- Parser closure and abstraction sufficiency are demonstrated, not inferred from stationary inventory.
- Flush is presentation; reset is future-equivalent projection without forced settlement.
- Old ordinary paths remain valid; rich graphs never enter incompatible rewrites/export or native fallback.
- Complex code is broken into named semantic steps, with useful first-divergence traces.

The main open questions are whether local shared-contact closure admits all requested fixtures, and whether compact logical flags/events suffice for exact noninstant continuation. Resolve them with minimal traces before adding fields or broadening the model.
