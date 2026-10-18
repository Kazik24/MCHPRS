# Step 05: reduce layers into a compact immutable program

Status: planned; requires the reviewed ADDER1BIT layer artifact. Stop before runtime activation.

## Goal

Turn gate/layer analysis into live input/state/output expressions and necessary boundary
effects. Original internal wires, layer queues and reset propagation must not survive as
the executable body. Layers remain useful cold evidence and viewer data.

## Files and API

- Keep Boolean construction/reduction in a focused `analysis/instant/expressions.rs`.
- Add immutable program types to `program_graph/instant_program.rs`.
- Extend `PlanOutput` with the typed layered result and publish it only when validated.
- Extend `program_graph/validation.rs` and `lowering.rs`; use existing position/source IDs.
- Do not restore `redpiler_opt` or add a new crate/solver for basic Boolean identities.

## Reduction algorithm

1. Hash-cons topological Boolean nodes. Normalize commutative operands and implement only
   well-tested constant/complement/select identities initially.
2. Declare roots from boundary values, meaningful local state, accepted-use consequences,
   reset continuations and boundary effects. Do not root every physical wire or pose write.
3. Retain transient roots when they schedule a repeater pulse, activate an observer/BUD,
   change an observed geometry profile or affect a later accepted use.
4. Remove unreachable expressions and declarations. Remap input IDs and state/effect roots
   deterministically; exported cold witnesses keep explicit source-to-lowered mappings.
5. Pack ordered effects. Delete only effects proved ineffective or unobservable; preserve
   meaningful same-value callbacks and distinguish two occurrences at the same position.
6. Lower source power/geometry reads to direct boundary bindings. Avoid storing draft
   power-link ordinals after sorting; rebind typed operands or apply an explicit mapping.
7. Separate runtime arrays from cold cell/layer/materialization tables. Allocate evaluator
   scratch once per nesting depth or invocation frame, sized to the actual region program.
8. Budget nodes, retained bytes, effect occurrences and compilation work. A large operand
   count is not itself a rejection criterion. Unknown simplification means keep the expression.

## Validator obligations

Validate operand ordering and bounds; gate arity; use/cell/input/output references;
causal predecessor references; resource/reset ownership; strength/profile encodings;
continuation targets; root widths; effect ordering; source witness validity; and cancellation.
Validate that every contracted node/read/event has one complete replacement owner.

No builder may publish a usable-looking kernel with unimplemented effect variants. An
unsupported geometry/output adapter is a structured decline, not a no-op placeholder.

## Tests

- Exhaustively check local algebraic rewrites across small Boolean functions.
- Compare pre/post reduction gate evaluation on small generated layered circuits.
- Preserve ordered duplicate roots and effects, including `high -> notify -> low -> notify`.
- Reject tampered IDs, cyclic expression references, stale source bindings and illegal selectors.
- Confirm unreachable input declarations do not cause runtime sampling work.
- Compile ADDER1BIT and ADDER11BIT; report expressions/effects/bytes versus actors/gates/SCCs.
  No runtime speed claim is expected, but missing actual cell coverage cannot be hidden.

## Gate

A real compact program exists, independently interpretable in tests. No internal wire walk,
full-state transcript or generic callback-dispatch bytecode appears in its hot representation.
The program's size and construction behavior are recorded before runtime code is expanded.
