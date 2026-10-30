# Interpreter-sampled Redpiler engine plan

The current assignment targets the complete ADDER1BIT executable gate, including
its actual IO schematic, eight prepared vectors, token/motion execution and
rearm. The new active manifest test currently fails at admission; see progress
for evidence. None of the mechanism milestones below substitutes for that gate.

The goal is a separate experimental compiler and runtime inspired by the
reference engine. The compiler uses the native interpreter in isolated scratch
worlds to discover update behavior; the runtime executes a compiled netlist.
Keep the existing Graph backend available while this engine is developed.

Each step must deliver runnable code and a focused test. A successful discovery
pass or generated program is not evidence that accelerated execution occurred.
Use existing world, interpreter and scheduling mechanisms where their contracts
fit. Keep native fixtures and declared output boundaries when migrating useful
scenarios. The superseded event executor is retired; its results remain
historical evidence.

Status verified 30 October 2026: lamp input extraction now collects directional
weak sources, expands eligible dust through the shared extractor, and observes
strong sources through at most one solid conductor. It deduplicates source
paths by minimum attenuation and maps them to the current chain's lever-to-lamp
link. Unsupported executable inputs reject with their position. This does not
admit multiple sources. The sampled lever–dust–lamp gate passes for
initially-off and initially-on entry, including repeated transitions, scheduler
wrap, four-game-tick lamp-off timing, host preservation and positioned rejection
of reads outside capture. The compiler checks escaped reads before lowering and
updates discovered contributions separately from receiver deliveries.
Compile-time empty-piston inventory is
complete; the next unmet sampled gate is piston extension, retraction and
rearm. The former event executor result is historical: its final suite recorded
45 passed, 1 failed and 1 ignored, with ADDER1BIT failing and COUNTER ignored.
Those adapters do not count as MAIR coverage. Multiple-source admission,
world-derived watcher installation and piston execution remain later gates. See
[current progress](REDPILER_SAMPLED_ENGINE_PROGRESS.md).

Current verification: `cargo test -p mchprs_core redpiler::sampled --lib`
passed 19 tests; `cargo test -p mchprs_core redpiler::scratch --lib` passed 15
tests. Both had zero failures and ignored tests. `cargo check -p mchprs_core
--tests` and `git diff --check` passed.

## 1. Engine boundary

Complete the existing private sampled module with a compiler entry point,
immutable program and mutable runtime state; do not create a third executor.
Compilation accepts a read-only world selection and
explicit entry work. Do not install the new engine on player plots yet.

**Gate:** an empty supported selection compiles and ticks. Compilation leaves
the source world unchanged and rejects unrepresented pending work.

## 2. Sample source updates

Copy a small circuit into a scratch world. Toggle a lever through the native
interpreter and record delivered updates in order, with rise and fall captured
separately. Compile the observed behavior and execute it without calling the
interpreter at runtime. Preserve loaded source and output state.

Runtime scheduler phase order is implemented: drain old bucket, advance once,
drain new bucket. Lamp off delay is four game ticks, expressed by
`schedule_tick(..., 2, ...)`; do not shorten it. Keep the compiler self-check
and every pending-work assertion. Use live receiver instructions, not stored
sampled output values.

**Gate:** lever → dust → lamp matches native rising and falling behavior, with
actual compiled operations counted. Initially-off and initially-on entry, 12
alternating transitions, the 32-bucket scheduler wrap and tick-4 lamp-off timing
pass native comparison. Tight captures and excluded electrical sources reject
with positioned boundary errors while preserving host state. Unsupported
dependencies and boundary crossings decline before execution.

## 3. Piston actors

Compile-time inventory is implemented for static empty actors: deterministic
IDs, facing, sticky state, home/far cells, loaded extension/head state and
transitive shared-cell groups. The capture rejects malformed/occupied contacts,
unsupported entities, moving states and escaped contact reads. This does not
change the lamp-only admission path. Next, compile requests, queued-action
deduplication and live power revalidation. Support an empty piston action
separately from payload movement.

**Execution gate (unmet):** lever → dust → piston executes extension,
retraction and rearm against native execution. Assert actual accepted actions
and pending work at the declared boundaries. Inventory completion alone does
not pass this gate.

## 4. Observer updates

Sample piston base/head changes and compile shape notifications and delayed
observer pulses. Keep shape callbacks distinct from ordinary power updates.

**Gate:** an empty piston triggers an observer whose output reaches a second
piston. Compare against native execution and assert the compiled notification,
pulse and second-piston action occurred.

## 5. Movable resources

Give each movable block one identity, discover its possible cells and allow
multiple pistons to reference it. Sample departure and arrival effects in
separate scratch arrangements. Preserve resource occupancy and moving state.

**Gate:** a moving redstone block changes a repeater's input correctly, including
reset and repeated movement. Compare payload, piston, output and pending work.

## 6. BUD behavior

Keep power dependencies separate from delivered updates. A data change without
a qualifying sample must preserve stored piston state. Reuse the existing BUD
schematics and their declared observation protocols.

**Gate:** the applicable BUD fixtures pass store → hold → sample → resample,
including repeated operations and native-equivalent declared outputs.

## 7. Interacting arrangements and ordering

Probe combinations of resources that change shared dependencies. Compile
ordered movement and watcher programs from the observations. Preserve shared
resource identity and FIFO request behavior; reject unrepresented arrangements.

**Gate:** shared-resource contention and all ADDER1BIT cases pass, including
successive changing inputs. Assert compiled work actually ran.

Before admitting these circuits, represent repeater short-pulse retention,
locking and priorities, plus the piston in-flight actions the fixture needs.
Use separate rise/fall and departure/arrival delivery programs. A changed
electrical contribution must not automatically synthesize a receiver callback.

## 8. Real circuits

Run ADDER11BIT, COUNTER, CPU_BUBBLESORT and FPU_27 in that order. Use existing
fixtures, manifests and meaningful input sequences. Add a missing general
component or interaction when a gate demonstrates it is required.

**Gate:** declared outputs are correct across repeated operations. Count actual
compiled operations; do not infer acceleration from successful compilation.

## 9. Measurement and integration

After correctness, measure comparable changing-input workloads against native
and Graph execution. Report fixture throughput separately from server game TPS.
Connect the engine to an experimental plot backend only when its ownership and
publication contracts are ready. Preserve unrelated world work and support the
approved reset-only exit through stop, cancellation, edits, save and unload.

**Gate:** a player can compile a supported plot, observe declared outputs and
reset safely. Measure actual plot throughput and state its limits.

## Implementation rules

- Steps 1–2 are the first assignment. Do not stop at an inventory or plan.
- Use coordinates and schematic names only in fixtures and diagnostics.
- Interpreter sampling is evidence for the probed conditions, not proof for all
  possible arrangements. Represent those conditions or reject unsupported cases.
- Keep electrical contributions, update delivery and movement ownership
  distinct. Do not poll all pistons or manufacture BUD updates from power alone.
- Preserve intermediate behavior only where needed for correct declared output
  boundaries. Do not silently discard effects that affect those boundaries.
- The new runtime must not rely on Graph or the interpreter to calculate results.
  Existing code may inform implementation; avoid a permanent LoweredCircuit
  dependency in the new compiler.
- Preserve dirty work, use inspectable passes and clear positioned errors, and
  report exact test results and remaining compatibility limits.
