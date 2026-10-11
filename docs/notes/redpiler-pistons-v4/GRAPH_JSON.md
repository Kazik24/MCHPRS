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
  capabilities: string[]; // Lists available sections such as power, routes, snapshots, or trace_replay.
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
  nodes: object[]; // Records circuit behavior, source position, specialized plan references, and output route.
  profiles: object[]; // Records raw block state, readable name, registry identity, and query facts.
  sites: object[]; // Records each immutable query location and its contact, component, or constant source.
  guards: object[]; // Records tagged always/state/site/not/all/any expressions with operand views.
  contacts: object[]; // Records guarded profile and receiver alternatives at each connection contact.
  pistons: object[]; // Records facing, sticky flag, base/near/lane references, and available route bindings.
  resources: object[]; // Records one shared material identity and all permitted contacts.
  power_links: object[]; // Records source site/channel, target input or conductor channel, face, attenuation, and guard.
  wire_plans: object[]; // Records the exact 24 canonical site references for each explicit wire.
  wire_relations: object[]; // Records guarded step/ascent/descent dependencies and their directions.
  comparator_plans: object[]; // Records rear site, literal/live overrides, far guard, and threshold precedence.
  routes: object[] | null; // Records ordered callback procedures, or null before route assembly exists.
};
type Snapshot = {
  id: string; // Identifies a captured state referenced by the trace.
  components: object[]; // Stores component or wire state keyed by its authoritative node ID.
  pistons: object[]; // Stores extended, availability, and revision keyed by piston ID.
  resources: object[]; // Stores selected contact, enabled flag, and revision keyed by resource ID.
  execution: object | null; // Stores reviewed scheduler/frontier data when available, otherwise no continuation claim.
};
type TraceRecord = {
  sequence: string; // Preserves exact order without JavaScript integer precision loss.
  tick: string; // Preserves the logical tick as a lossless integer string.
  phase: string; // Names the executor phase of the observation.
  kind: string; // Names the recorded callback, sample, request, acceptance, publication, or boundary event.
  details: object; // Contains the exact typed operands appropriate to this event kind.
  snapshot: string | null; // References the state captured at this boundary when one exists.
};
type Diagnostic = {
  code: string; // Provides a stable machine-readable diagnostic category.
  message: string; // Explains the issue in ordinary language.
  positions: Position[]; // Supplies the actors or locations involved in the witness.
};
```

The `object` placeholders above deliberately defer payload shapes to the implemented types; they are not permission to stringify structs or omit their semantics. A1 must publish concrete tagged record schemas/examples for every currently exported variant. Use adjacent one-sentence explanations for every field and variant. Complete-program-only fields remain null in drafts, never invented IDs or empty arrays that imply completeness.

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
