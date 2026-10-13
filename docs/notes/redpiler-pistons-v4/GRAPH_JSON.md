# Graph JSON inspection contract

Status: proposed v1 format, 2026-10-12. Review alongside Side A1; implementation must map actual types faithfully.

## Principles

Pretty-printed UTF-8 JSON, explicit tagged records, stable ordering, and named fields. Export semantic data rather than Rust debug text, packed memory, pointer addresses, or enum discriminants. Positions are provenance; runtime connections refer to typed IDs. There are no motion/progress/carried-block fields.

IDs are readable typed strings (`node:0`, `site:0`, `guard:0`, `contact:0`, `resource:0`, `piston:0`, `route:0`, `profile:0`, `wire:0`). They remain stable within an artifact and its snapshots/trace, but are not promised stable after recompilation. Links have export-local IDs so inspections can refer to duplicate records. Array order follows source IDs; route delivery order is authoritative. A future rewrite needs an explicit provenance mapping to compare two artifacts.

Version 1 is inspection/replay, not a backend import or executable ABI. Reject unknown major versions; tolerate unknown additive fields. A future runnable format must define every pending-work item and execution frontier before advertising that capability.

## Top-level shape and field meanings

```typescript
type GraphExport = {
  format: "mchprs.redpiler.graph"; // Identifies this inspection format.
  version: { major: number; minor: number }; // Separates incompatible contracts from additive changes.
  stage: "electrical_draft" | "program"; // States whether callback assembly and program validation are complete.
  capabilities: string[]; // Lists available sections such as power, routes, snapshots, dust_path_certificates, or trace_replay.
  source: Source; // Describes the selection and source revision used to create this artifact.
  graph: GraphTables; // Holds immutable graph records and all typed references.
  snapshots: Snapshot[]; // Holds explicitly captured logical states without duplicating graph ownership.
  trace: TraceRecord[] | null; // Contains recorded ordered observations, or null when no capture exists.
  diagnostics: Diagnostic[]; // Explains unsupported features or preparation details without hiding missing semantics.
};
type Source = {
  revision: string | null; // Names the source revision when known.
  bounds: { min: Position; max: Position }; // Records the compiled selection rather than inferred ownership.
};
type Position = { x: number; y: number; z: number }; // Gives block coordinates only as provenance.
type GraphTables = {
  nodes: Node[]; // Records circuit behavior, source position, specialized plan references, and output route.
  profiles: Profile[]; // Records raw block state, readable name, registry identity, and query facts.
  sites: Site[]; // Records each immutable query location and its contact, component, or constant source.
  guards: Guard[]; // Records tagged state/site expressions and logical compositions.
  contacts: Contact[]; // Records guarded profile and receiver alternatives at each connection contact.
  pistons: Piston[]; // Records facing, sticky flag, base/near/lane references, and optional route bindings.
  resources: Resource[]; // Records one shared material identity and all permitted contacts.
  power_links: PowerLink[]; // Records source site/channel, target input or conductor channel, face, attenuation, and guard.
  wire_plans: WirePlan[]; // Records the exact 24 canonical site references for each explicit wire.
  dust_paths?: DustPathCertificate[]; // Records checked path bindings when the experimental dust pass accepts paths; older v1 exports may omit this additive table.
  wire_relations: WireRelation[]; // Records guarded step/ascent/descent dependencies and their directions.
  comparator_plans: ComparatorPlan[]; // Records rear site, literal/live overrides, far guard, and threshold precedence.
  routes: Route[] | null; // Records ordered callback procedures, or null before route assembly exists.
};
type Snapshot = {
  id: string; // Identifies a captured state referenced by the trace.
  components: ComponentSnapshot[]; // Stores component or wire state keyed by its authoritative node ID.
  pistons: PistonSnapshot[]; // Stores extension, head occupancy, availability, and revision keyed by piston ID.
  resources: ResourceSnapshot[]; // Stores selected contact, enabled flag, and revision keyed by resource ID.
  execution: JsonValue | null; // Stores reviewed scheduler/frontier data when available, otherwise no continuation claim.
};
type TraceRecord = {
  sequence: string; // Preserves exact order without JavaScript integer precision loss.
  tick: string; // Preserves the logical tick as a lossless integer string.
  phase: string; // Names the executor phase of the observation.
  kind: string; // Names the recorded callback, sample, request, acceptance, publication, or boundary event.
  details: JsonValue; // Contains event-specific JSON data without claiming that a trace is currently available.
  snapshot: string | null; // References the state captured at this boundary when one exists.
};
type Diagnostic = {
  code: string; // Provides a stable machine-readable diagnostic category.
  message: string; // Explains the issue in ordinary language.
  positions: Position[]; // Supplies the actors or locations involved in the diagnostic.
};
```

The concrete record and tagged-variant schemas below define the v1 payload; serializers must preserve these distinctions. Complete-program-only fields remain null in drafts, never invented IDs or empty arrays that imply completeness.

## Concrete v1 record schemas

Every field and variant below has a one-sentence meaning. Enum tags use the shown strings, and optional values serialize as JSON `null`.

```typescript
type Node = {
  id: string; // Identifies this circuit node with a node:N ID.
  kind: NodeKind; // Identifies the node's electrical or specialized behavior.
  position: Position | null; // Records the block position when the node has one.
  output_route: string | null; // References the node's ordered output route when route assembly is available.
};
type NodeKind =
  | { kind: "electrical"; component: ElectricalKind } // Stores a fixed redstone component's rule parameters.
  | { kind: "observer"; watched: string; facing: string } // Stores the observed site ID and observer direction.
  | { kind: "wire"; plan: string } // References the wire's neighborhood plan.
  | { kind: "piston"; piston: string } // References the node's piston description.
  | { kind: "contact"; contact: string }; // References the node's selectable contact.
type ElectricalKind =
  | { type: "repeater"; delay: number; facing_diode: boolean } // Stores repeater delay and its directional diode behavior.
  | { type: "torch" } // Identifies a redstone torch component.
  | { type: "observer"; watched: Position } // Stores the watched block position.
  | { type: "comparator"; mode: "compare" | "subtract"; far_input: number | null; facing_diode: boolean } // Stores comparator mode, optional far input, and diode behavior.
  | { type: "lamp" } // Identifies a redstone lamp component.
  | { type: "copper_bulb" } // Identifies a copper bulb component.
  | { type: "button" } // Identifies a button component.
  | { type: "lever" } // Identifies a lever component.
  | { type: "pressure_plate" } // Identifies a pressure plate component.
  | { type: "trapdoor" } // Identifies a trapdoor component.
  | { type: "wire" } // Identifies a redstone wire component.
  | { type: "constant" } // Identifies a fixed electrical source component.
  | { type: "command_block"; repeating: boolean; chain: boolean; automatic: boolean; initial_tick: boolean } // Stores command-block subtype and activation properties.
  | { type: "note_block"; instrument: string; note: number }; // Stores the note-block instrument and note value.

type Profile = {
  id: string; // Identifies one deduplicated block profile with a profile:N ID.
  raw_state: number; // Stores the packed registry state used to identify the block variant.
  name: string; // Gives the block's readable registry name.
  registry_id: number; // Stores the block registry type ID.
  solid: boolean; // Records whether the profile is solid for site queries.
  transparent: boolean; // Records whether the profile is transparent for site queries.
  cube: boolean; // Records whether the profile is a full cube for site queries.
  electrical: "none" | "redstone_source" | "component"; // Classifies the profile's electrical role.
};
type Site = {
  id: string; // Identifies one immutable query location with a site:N ID.
  position: Position; // Records the source block position for inspection.
  source: SiteSource; // Identifies the graph record that supplies this site's current view.
};
type SiteSource =
  | { kind: "component"; node: string } // Reads the view from a circuit node.
  | { kind: "contact"; contact: string } // Reads the currently selected view from a contact.
  | { kind: "constant"; profile: string }; // Reads immutable facts from a profile.

type Guard = { id: string } & GuardExpression; // Identifies one expression with a guard:N ID.
type GuardExpression =
  | { kind: "always" } // Enables its relation unconditionally.
  | { kind: "state"; test: StateTest } // Tests piston or shared-resource state.
  | { kind: "site"; site: string; view: "live" | "captured"; test: SiteTest } // Tests a site in the specified live or captured operand view.
  | { kind: "not"; guard: string } // Negates an earlier guard.
  | { kind: "all"; guards: string[] } // Requires every referenced earlier guard.
  | { kind: "any"; guards: string[] }; // Requires at least one referenced earlier guard.
type StateTest =
  | { kind: "piston_extended"; piston: string } // Tests the piston extension flag.
  | { kind: "piston_head_present"; piston: string } // Tests whether the head occupies its near contact.
  | { kind: "piston_availability"; piston: string; availability: PistonAvailability } // Tests a piston connection-availability phase.
  | { kind: "resource_at"; resource: string; contact: string } // Tests whether a shared resource is selected at a contact.
  | { kind: "resource_enabled"; resource: string }; // Tests whether the shared resource is electrically available.
type SiteTest =
  | { kind: "solid" } // Tests the site's solid property.
  | { kind: "transparent" } // Tests the site's transparent property.
  | { kind: "cube" } // Tests the site's full-cube property.
  | { kind: "is_wire" } // Tests whether the site's profile is redstone wire.
  | { kind: "profile"; profile: string }; // Tests whether the site uses the referenced profile.

type Contact = {
  id: string; // Identifies one selectable location with a contact:N ID.
  node: string; // References the node that owns the contact description.
  position: Position; // Records the contact's world position.
  variants: ContactVariant[]; // Preserves each guarded profile and receiver alternative in source order.
};
type ContactVariant = {
  when: string; // References the guard that selects this alternative.
  profile: string; // References the block profile visible in this alternative.
  receiver: Receiver | null; // Identifies the callback recipient, or null when this view has none.
};
type Receiver =
  | { kind: "component"; node: string } // Delivers the callback to a component node.
  | { kind: "piston_base"; piston: string } // Delivers the callback to a piston base handler.
  | { kind: "piston_head"; piston: string } // Delivers the callback to a piston head handler.
  | { kind: "contact"; contact: string }; // Delivers the callback through another contact.

type Piston = {
  id: string; // Identifies a piston description with a piston:N ID.
  node: string; // References the piston circuit node.
  position: Position; // Records the piston base position.
  facing: string; // Records the piston facing direction.
  sticky: boolean; // Records whether the piston is sticky.
  base: string; // References the piston-base contact.
  near: string; // References the near-side head contact.
  far: string; // References the far-side contact.
  lane: PistonLane; // Distinguishes an empty lane from one containing a shared resource.
  routes: PistonRoutes | null; // Stores callback route bindings when complete program assembly provides them.
};
type PistonLane =
  | { kind: "empty" } // States that no shared resource occupies the piston lane.
  | { kind: "single_resource"; far: string }; // References the far contact for the lane's single shared resource.
type PistonRoutes = {
  base_changed: string; // References the route delivered when the base changes.
  near_changed: string; // References the route delivered when the near contact changes.
  far_changed: string | null; // References the optional route delivered when the far contact changes.
  base_restored: string; // References the route delivered when the base is restored.
  head_restored: string; // References the route delivered when the head is restored.
  resource_restored: string | null; // References the optional route delivered when the resource is restored.
};
type Resource = {
  id: string; // Identifies one shared movable material with a resource:N ID.
  material: string; // References the resource's immutable block profile.
  contacts: string[]; // Lists every contact at which this resource may be selected.
};

type PowerLink = {
  id: string; // Identifies one power dependency, including duplicates, with a power_link:N ID.
  from: PowerSource; // Names the source site and exact query port.
  to: PowerTarget; // Names the receiving input port or conductor site.
  face: string; // Preserves the direction used by the original power query.
  attenuation: number; // Records signal strength lost along this dependency.
  guard: string; // References the guard that enables this dependency.
};
type PowerSource = { site: string; port: PowerPort }; // Binds a source site to its output/query channel.
type PowerPort =
  | { kind: "output" } // Reads the component's current output strength.
  | { kind: "stored" } // Reads the component's stored strength.
  | { kind: "weak"; face: string } // Reads weak power for the specified face.
  | { kind: "strong"; face: string } // Reads strong power for the specified face.
  | { kind: "conducted"; include_dust: boolean }; // Reads conducted power with the stated dust inclusion policy.
type PowerTarget =
  | { kind: "input"; node: string; port: "main" | "side" | "quasi" | "strong_incident" } // Supplies one named component input channel.
  | { kind: "conductor"; site: string; include_dust: boolean }; // Supplies a conductor site using the stated dust inclusion policy.
type WirePlan = {
  id: string; // Identifies a wire plan with a wire:N ID.
  node: string; // References the wire node whose neighborhood is described.
  canonical_neighbors: string[]; // Lists exactly 24 site IDs in the traversal's canonical neighbor order.
  shape_route: string | null; // References the optional wire shape callback route.
  turbo_route: string | null; // References the optional wire Turbo callback route.
};
type WireRelation = {
  id: string; // Identifies one wire dependency with a wire_relation:N ID.
  from: string; // References the originating wire node.
  to: string; // References the destination site.
  kind: "step" | "ascend" | "descend"; // Identifies the wire-neighbor geometry relation.
  direction: string; // Preserves the relation's direction.
  guard: string; // References the guard that enables this relation.
};
type DustPathCertificate = {
  id: string; // Identifies one checked path certificate with a dust_path:N ID.
  input_node: string; // References the electrical node that supplies the path's only certified input.
  input_site: string; // References the source site owned by input_node.
  input_face: string; // Preserves the horizontal face used by the direct ingress query.
  wires: string[]; // Lists the original wire node IDs in input-to-output order, with each wire owned by one certificate.
};

type ComparatorPlan = {
  id: string; // Identifies one comparator query plan with a comparator_plan:N ID.
  node: string; // References the comparator node.
  rear: string; // References the rear-input site.
  direct_override: ComparatorOverride | null; // Stores the direct container override when present.
  far_override: FarOverride | null; // Stores the guarded far-input override query when present.
  far_threshold: number; // Stores the comparator's far-input threshold.
};
type FarOverride = {
  site: string; // References the site queried for a far override.
  value: ComparatorOverride; // Stores the literal value or live component output selected there.
  guard: string; // References the guard for the far override.
};
type ComparatorOverride =
  | { kind: "literal"; value: number } // Stores a fixed comparator override strength.
  | { kind: "live_component_output"; node: string }; // Reads the referenced component's current output strength.

type Route = { id: string; deliveries: RouteDelivery[] }; // Identifies one ordered callback procedure.
type RouteDelivery = {
  ordinal: number; // Gives this callback's zero-based position in its procedure.
  target: string; // References the target site.
  kind: "update" | "shape"; // Selects the callback operation.
  direction: string | null; // Preserves the callback direction or its directionless value.
  guard: string; // References the callback's eligibility guard.
  view: "live" | "captured"; // Selects whether the recipient reads live or captured site state.
};

type Snapshot = {
  id: string; // Identifies this captured logical state.
  components: ComponentSnapshot[]; // Stores authoritative state for circuit component nodes.
  pistons: PistonSnapshot[]; // Stores extension, head occupancy, and connection availability state.
  resources: ResourceSnapshot[]; // Stores each shared resource's selected contact and availability.
  execution: JsonValue | null; // Stores a reviewed continuation frontier, or null when none is available.
};
type ComponentSnapshot = { node: string; state: ComponentState }; // Associates one state record with its node ID.
type ComponentState =
  | { kind: "electrical"; powered: boolean; locked: boolean; output_strength: number; pending_tick: boolean } // Stores electrical flags, output, and pending-tick status.
  | { kind: "wire"; power: number; north: WireSide; south: WireSide; east: WireSide; west: WireSide } // Stores wire power and four directional side shapes.
  | { kind: "specialized" }; // Marks a specialized node whose detailed state is represented elsewhere.
type WireSide = "up" | "side" | "none"; // Names the exported wire connection shape for one horizontal side.
type PistonAvailability = "ready" | "head_pending" | "publishing_head_removal" | "base_pending" | "base_pending_head_present"; // Names the piston connection-availability phase, including settled retraction while the head remains present.
type PistonSnapshot = {
  piston: string; // References the piston whose state is captured.
  extended: boolean; // Records the piston extension flag.
  head_present: boolean; // Records whether the piston head occupies its near contact.
  availability: PistonAvailability; // Records which piston contacts are currently available.
  revision: string; // Stores the revision as a decimal string without integer precision loss.
};
type ResourceSnapshot = {
  resource: string; // References the shared resource whose state is captured.
  selected_contact: string; // References the one contact currently selected by this resource.
  enabled: boolean; // Records whether the resource currently conducts power.
  revision: string; // Stores the revision as a decimal string without integer precision loss.
};
type JsonValue = null | boolean | number | string | JsonValue[] | { [key: string]: JsonValue }; // Defines JSON data without asserting an unimplemented trace event schema.
type TraceRecord = {
  sequence: string; // Stores the ordered sequence number as a decimal string.
  tick: string; // Stores the logical tick as a decimal string.
  phase: string; // Names the executor phase at this observation.
  kind: string; // Names the recorded event kind.
  details: JsonValue; // Stores event-specific JSON details without claiming a draft trace exists.
  snapshot: string | null; // References the captured state at this boundary when present.
};
type Diagnostic = {
  code: string; // Provides a stable machine-readable diagnostic category.
  message: string; // Explains the issue in ordinary language.
  positions: Position[]; // Supplies the positions involved in the diagnostic.
};
```

In the current `electrical_draft`, `graph.routes`, `node.output_route`, piston route bindings, wire route bindings, `trace`, and snapshot `execution` serialize as `null`. This marks missing program/runtime sections as unavailable instead of claiming that an empty section is complete.

`graph.dust_paths` is an additive v1.1 table. It is an empty array when no path is certified, and may be absent in older v1 exports; readers treat an absent table as empty. Each certificate's IDs must resolve, its input site must belong to its electrical input node, its path must contain at least two wire nodes, and no wire may be owned by multiple certificates. The `dust_path_certificates` capability appears when at least one certificate is present. The certificate exposes provenance and query bindings, not a promise that the original wire state slots or ordered Turbo execution have been removed.

## Tagged record examples

```json
{
  "id": "power_link:0",
  "from": { "site": "site:8", "port": { "kind": "conducted", "include_dust": false } },
  "to": { "kind": "input", "node": "node:3", "port": "main" },
  "face": "north",
  "attenuation": 0,
  "guard": "guard:4"
}
```

`id` identifies this dependency; `from` names its source and exact query channel; `to` names its receiver and input kind; `face` preserves the literal query direction; `attenuation` gives saturating signal loss; `guard` names its enabling expression. A conductor target similarly carries `site` and `include_dust`. Ports include stored, output, weak(face), strong(face), and conducted(include_dust); input ports include main, side, and quasi.

```json
{
  "id": "route:2",
  "deliveries": [
    { "ordinal": 0, "target": "site:5", "kind": "update", "direction": "east", "guard": "guard:0" },
    { "ordinal": 1, "target": "site:5", "kind": "update", "direction": "east", "guard": "guard:0" }
  ]
}
```

`id` identifies a callback procedure; `deliveries` retains its ordered invocations; `ordinal` records the procedure position; `target` names the delivery site; `kind` separates update and shape; `direction` preserves the literal direction or null; `guard` names delivery eligibility. Preserve operand view and additional metadata from actual route types when they exist. Duplicate deliveries are intentional.

## Validation and determinism

Validate unique typed IDs, all references, direction/port tags, 0-15 strengths, nonnegative attenuation under the graph's existing rules, guard structure, canonical neighbor length/order, snapshot bindings, and trace ordering. Reuse program validation where available; format validation also covers draft-only tables. Export counters/deadlines/revisions that can exceed JavaScript's safe integer range as decimal strings. No wall-clock timestamp is needed in semantic content.

Never derive ownership from position, erase disabled contact alternatives, flatten a guard into one saved Boolean, merge ordered callbacks into power edges, or duplicate resource state inside each piston. An export of a draft is useful even though it cannot run.
