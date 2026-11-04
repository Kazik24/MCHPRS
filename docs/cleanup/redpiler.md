# Redpiler cleanup review

Date: 2026-11-04. This pass reviewed all 51 files under `crates/core/src/redpiler/`, including the native C engine and its tests. The changes below stay behavior-preserving and keep the C ABI, runtime layouts, and update order intact.

## File coverage

| File | Review coverage |
| --- | --- |
| [`compiler.rs`](../../crates/core/src/redpiler/compiler.rs) | Public compiler lifecycle, compile entry points, control input, tick, flush/reset publication, and piston identity reconciliation. |
| [`mod.rs`](../../crates/core/src/redpiler/mod.rs) | Module exports and public redpiler surface. |
| [`options.rs`](../../crates/core/src/redpiler/options.rs) | Compiler options, defaults, and flag parsing. |
| [`scratch.rs`](../../crates/core/src/redpiler/scratch.rs) | World capture, read boundaries, input discovery, piston probes, worker snapshots, result order, and restoration tests. |
| [`task_monitor.rs`](../../crates/core/src/redpiler/task_monitor.rs) | Cancellation, phase timing, task status updates, and progress reporting. |
| [`sampled/commands.rs`](../../crates/core/src/redpiler/sampled/commands.rs) | Host command block ownership, runtime command inputs, and output publication. |
| [`sampled/compiler.rs`](../../crates/core/src/redpiler/sampled/compiler.rs) | Sampled compilation orchestration, source diffing, piston grouping, and update footprints. |
| [`sampled/executor.rs`](../../crates/core/src/redpiler/sampled/executor.rs) | Runtime tick loop, input accounting, scheduled work, component callbacks, and movement hot paths. |
| [`sampled/group_probe.rs`](../../crates/core/src/redpiler/sampled/group_probe.rs) | Piston group arrangement discovery, dense and fallback probe paths, and arrangement tests. |
| [`sampled/input.rs`](../../crates/core/src/redpiler/sampled/input.rs) | Input level accounting and strength histogram operations. |
| [`sampled/mod.rs`](../../crates/core/src/redpiler/sampled/mod.rs) | Sampled compiler types, entry points, error handling, and shared helper definitions. |
| [`sampled/program.rs`](../../crates/core/src/redpiler/sampled/program.rs) | Compiled program records, runtime index preparation, movement metadata, and structural validation. |
| [`sampled/routing.rs`](../../crates/core/src/redpiler/sampled/routing.rs) | Local routing graph construction, reader source lookup, and directed reachability. |
| [`sampled/scheduler.rs`](../../crates/core/src/redpiler/sampled/scheduler.rs) | Delay and priority queue behavior, ordering, and wraparound tests. |
| [`sampled/shapes.rs`](../../crates/core/src/redpiler/sampled/shapes.rs) | Wire shape state encoding and sampled shape transition construction. |
| [`sampled/passes/README.md`](../../crates/core/src/redpiler/sampled/passes/README.md) | Documented pass order, inputs, outputs, cancellation, and index rebuild expectations. |
| [`sampled/passes/arrangements.rs`](../../crates/core/src/redpiler/sampled/passes/arrangements.rs) | Arrangement discovery pass inputs and movement flip setup. |
| [`sampled/passes/electrical.rs`](../../crates/core/src/redpiler/sampled/passes/electrical.rs) | Electrical source discovery, links, component ports, and special reader inputs. |
| [`sampled/passes/finalize.rs`](../../crates/core/src/redpiler/sampled/passes/finalize.rs) | Final validation, index construction, and pass timing. |
| [`sampled/passes/mod.rs`](../../crates/core/src/redpiler/sampled/passes/mod.rs) | Pass context and inventory definitions shared by compilation stages. |
| [`sampled/passes/movement.rs`](../../crates/core/src/redpiler/sampled/passes/movement.rs) | Piston movement transition and notification recipient discovery. |
| [`sampled/passes/parse.rs`](../../crates/core/src/redpiler/sampled/passes/parse.rs) | Captured world parsing, entry validation, and initial inventories. |
| [`sampled/passes/reactive.rs`](../../crates/core/src/redpiler/sampled/passes/reactive.rs) | Reactive component flags and ordered wire/neighbor notification relationships. |
| [`sampled/passes/share_output_programs.rs`](../../crates/core/src/redpiler/sampled/passes/share_output_programs.rs) | Identical immutable rise/fall instruction range sharing. |
| [`sampled/passes/source_order.rs`](../../crates/core/src/redpiler/sampled/passes/source_order.rs) | Source race detection, quiet basis preparation, ordering probes, and reference comparisons. |
| [`sampled/llvm/abi.h`](../../crates/core/src/redpiler/sampled/llvm/abi.h) | C descriptor declarations, exported entry points, and ABI size/offset assertions. |
| [`sampled/llvm/cache.rs`](../../crates/core/src/redpiler/sampled/llvm/cache.rs) | Process-owned cache setup, compiler lookup, hashing, subprocess handling, and cancellation. |
| [`sampled/llvm/command_engine_test.c`](../../crates/core/src/redpiler/sampled/llvm/command_engine_test.c) | Standalone C command engine lifecycle, limits, notification, and error precedence checks. |
| [`sampled/llvm/command_engine.c`](../../crates/core/src/redpiler/sampled/llvm/command_engine.c) | Native command engine state, validation, execution, output limits, and notifications. |
| [`sampled/llvm/command_notifications.rs`](../../crates/core/src/redpiler/sampled/llvm/command_notifications.rs) | Rust encoding for command-triggered notifications and live comparator guards. |
| [`sampled/llvm/command.rs`](../../crates/core/src/redpiler/sampled/llvm/command.rs) | Command block and output-state encoding, native synchronization, and command tests. |
| [`sampled/llvm/effects.rs`](../../crates/core/src/redpiler/sampled/llvm/effects.rs) | Native effect and output metadata table encoding. |
| [`sampled/llvm/engine.c`](../../crates/core/src/redpiler/sampled/llvm/engine.c) | Native program blob validation, execution state setup, update processing, and exported API. |
| [`sampled/llvm/llvm_movement.c`](../../crates/core/src/redpiler/sampled/llvm/llvm_movement.c) | Native movement table validation, motion transitions, projection, and state synchronization. |
| [`sampled/llvm/mod.rs`](../../crates/core/src/redpiler/sampled/llvm/mod.rs) | Rust native loader, ABI checks, program encoding/validation, runtime synchronization, and parity tests. |
| [`sampled/llvm/movement.rs`](../../crates/core/src/redpiler/sampled/llvm/movement.rs) | Rust movement table encoding/decoding and native runtime handoff checks. |
| [`sampled/llvm/routing.rs`](../../crates/core/src/redpiler/sampled/llvm/routing.rs) | Rust local routing graph tables for the native program blob. |
| [`sampled/llvm/shapes.rs`](../../crates/core/src/redpiler/sampled/llvm/shapes.rs) | Rust wire shape transition table encoding. |
| [`tests/classic_fuzz.rs`](../../crates/core/src/redpiler/tests/classic_fuzz.rs) | Seeded classic circuit differential checks and fixture export coverage. |
| [`tests/compiler.rs`](../../crates/core/src/redpiler/tests/compiler.rs) | Compiler errors, controls, pending ticks, piston handoff, and host preservation tests. |
| [`tests/publication.rs`](../../crates/core/src/redpiler/tests/publication.rs) | Publication modes, piston projections, effect acknowledgement, flush/reset, and identity preservation. |
| [`tests/sampled/adder.rs`](../../crates/core/src/redpiler/tests/sampled/adder.rs) | Arithmetic manifests, repeated operations, focused samples, and output checks. |
| [`tests/sampled/admission.rs`](../../crates/core/src/redpiler/tests/sampled/admission.rs) | Admission boundaries, pressure plate/trapdoor updates, command lifecycle, and limits. |
| [`tests/sampled/core.rs`](../../crates/core/src/redpiler/tests/sampled/core.rs) | Sampled runtime/reference behavior for components, movement, routing, commands, and notifications. |
| [`tests/sampled/cpu_trace.rs`](../../crates/core/src/redpiler/tests/sampled/cpu_trace.rs) | CPU trace and reference-prefix comparisons; this file had a concurrent external change and was not edited in this pass. |
| [`tests/sampled/incremental_input.rs`](../../crates/core/src/redpiler/tests/sampled/incremental_input.rs) | Equal-strength source retention and conditional input revalidation. |
| [`tests/sampled/llvm_regressions.rs`](../../crates/core/src/redpiler/tests/sampled/llvm_regressions.rs) | Native parity regressions for randomized toggles, movement, queueing, and effects. |
| [`tests/sampled/passes.rs`](../../crates/core/src/redpiler/tests/sampled/passes.rs) | Pass cancellation, output sharing, pending work, and post-rewrite execution checks. |
| [`tests/sampled/redstonedb.rs`](../../crates/core/src/redpiler/tests/sampled/redstonedb.rs) | RedstoneDB lever circuit differential coverage. |
| [`tests/sampled/reviewed_fixes.rs`](../../crates/core/src/redpiler/tests/sampled/reviewed_fixes.rs) | Observer rearming and moving-token command reachability regressions. |
| [`tests/sampled/runtime_index.rs`](../../crates/core/src/redpiler/tests/sampled/runtime_index.rs) | Compact indexes, phase stamps, piston bindings, imported ticks, reset failure, and sealed program validation. |

## Changes made

- `options.rs`: derive `Default` for `CompilerOptions` instead of maintaining an equivalent field-by-field implementation.
- `sampled/passes/arrangements.rs`: remove redundant references passed to `group_probe::run`; its parameters already take references. The only caller is `sampled/compiler.rs`.
- `sampled/passes/source_order.rs`: name the control, repeater, and other output-source iterators; name the fixed and arrangement input strength iterators. The accompanying comments state which source strengths are included and why the maximum is used. The test-only reference calculation is staged the same way so its comparison remains explicit.
- `sampled/executor.rs`: clarify that runtime input strength is the maximum across fixed and arrangement contributions; this adds no work to the hot path.
- `sampled/program.rs`: name the three notification tables traversed by validation and explain their shared target constraint.
- `sampled/shapes.rs`: name token occupancy and piston anchor cell iterators before combining them for shape sampling.
- `scratch.rs`: name worker result batches, then flatten and restore caller job order after parallel probes. The comment records why the sort is required; the existing result vectors and order are unchanged.

No public API, serialized layout, C ABI, validation rule, scheduler order, or runtime instruction was changed. No abstraction or helper was added.

## Larger proposals deferred

| Proposal | Callers and compatibility | Validation before implementation |
| --- | --- | --- |
| Keep Rust and C native ABI table definitions easier to compare by extracting a small schema source or splitting validation by table family. | The encoder and validator in `sampled/llvm/mod.rs`, table encoders under `sampled/llvm/*.rs`, and `ne_create`/`ne_state_size` in `engine.c` consume the same blob. Any schema or field change must preserve offsets and counts or update both implementations and the ABI/semantic versions together. | Run the standalone C command engine test, Rust malformed-blob tests, Rust/native parity tests, and native build/runtime checks on supported platforms. |
| Split the largest private runtime implementation into subsystem files after identifying stable boundaries for movement, component callbacks, and publication/accounting. | `sampled/compiler.rs` constructs and drives `Runtime`; `redpiler/compiler.rs` publishes and hands state to/from it. Keep `Runtime` state private and retain the same public `Compiler` behavior. | Run sampled core, publication, compiler handoff, movement, and LLVM parity tests; compare hot-path performance before merging. |
| Revisit shared contact and reader discovery between `scratch.rs`, `sampled/group_probe.rs`, and the electrical pass only if differential tests can pin down their distinct source rules. | Callers include `passes::electrical`, `passes::arrangements`, `group_probe`, and runtime movement/input handling. A shared helper must preserve discovery order, directionality, boundary errors, and each caller's admitted source types. | Add or reuse pairwise reference tests for wires, readers, piston movement, fixed inputs, and arrangement fallback; run the redpiler differential suite and check probe-heavy compile time. |
| Extract a private reconciliation helper from `Compiler::publish` if publication changes continue to grow. | Called by flush/reset paths in `redpiler/compiler.rs`; identity and carried block-entity association must remain stable across repeated publication. No public API change is required. | Run `tests/publication.rs` and `tests/compiler.rs`, especially repeated unchanged motion, interrupted motion, and reset handoff cases. |

These are follow-up candidates, not required changes for this pass. The code remains in its current composition because the relevant ordering, ownership, and ABI constraints cross the proposed boundaries.

## Checks

- `git diff --check` passed for the edited redpiler source files.
- `rustfmt --edition 2024` was initially run on the owned source files, then its unrelated formatting churn was removed. The final diff keeps the repository's existing formatting style.
- No Cargo command was started by this pass. The parent agent is coordinating the shared Cargo checks.
