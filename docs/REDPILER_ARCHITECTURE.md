# Redpiler architecture

Redpiler compiles supported, piston-free redstone selections into an electrical
graph. Any build containing a piston is rejected before graph preparation; the
piston and its circuitry remain under the physical interpreter. This document
describes the current implementation. See the [execution model](REDPILER_MODEL.md),
[analysis and graph preparation](REDPILER_PARSER.md),
[graph passes](REDPILER_OPTIMIZER.md), and [physical piston model](PISTON_MODEL.md)
for details.

## Implementation map

| Responsibility               | Implementation                                                                                                                                            | Result                                                                  |
| ----------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------- |
| Compile lifecycle and options | [redpiler/mod.rs](../crates/core/src/redpiler/mod.rs)                                                                                                      | Admission, diagnostics, statistics, direct/native backend, reset        |
| Read-only analysis            | [analysis/](../crates/core/src/redpiler/analysis/mod.rs)                                                                                                   | Inventory, piston detection, resource limits, diagnostics                |
| Graph preparation             | [passes/](../crates/core/src/redpiler/passes/mod.rs)                                                                                                       | Spatially derived ordinary electrical graph                              |
| Direct backend                | [backend/direct/](../crates/core/src/redpiler/backend/direct/mod.rs)                                                                                       | Graph propagation and scheduled component transitions                    |
| Native backend                | [backend/native.rs](../crates/core/src/redpiler/backend/native.rs)                                                                                         | Interpreter callbacks for ordinary selections needing native ordering    |
| Classical redstone tests      | [legacy_regressions.rs](../crates/core/src/redpiler/passes/legacy_regressions.rs), [redstone_fuzz.rs](../crates/core/src/redpiler/analysis/tests/redstone_fuzz.rs) | Repeater regressions and generated graph comparisons                     |

## Compile lifecycle

[`Compiler::compile`](../crates/core/src/redpiler/mod.rs) first performs
read-only analysis over the selection. It stops immediately if the analysis
finds a piston. The compile error is `build contains pistion, pistions are not
supported`; the command highlights the first piston in the selection. No
backend is activated and scheduled work stays with the interpreter.

For piston-free selections, graph preparation identifies nodes, searches
spatial signal paths, and runs the fixed pass sequence. The compiler then
lowers the graph to the direct backend or, for ordinary comparator-ordering
cases that need it, the native backend. The new backend is published only after
successful preparation and lowering. Reset exports compiled state and
remaining scheduled work to the interpreter.

Read-only [`/rp analyze`](../crates/core/src/plot/commands.rs) reports the
selection and checks whether it can compile without transferring world
ownership. `--graph` prepares a diagnostic candidate only; it cannot be
executed. Analysis may inventory piston geometry for diagnostics, but no
analysis path creates an executable piston program.

## Commands and options

`/rp` and `/redpiler` are aliases.

| Command or flag                  | Behavior                                                         |
| -------------------------------- | ---------------------------------------------------------------- |
| `/rp compile [flags]`, `/rp c`   | Compile the current plot; piston builds are rejected and highlighted |
| `/rp analyze [flags]`            | Read-only analysis and compile-admission check                   |
| `/rp analyze --graph [flags]`    | Prepare a non-executable diagnostic graph                        |
| `/rp inspect`, `/rp i`           | Inspect the compiled node under the player's ray                  |
| `/rp reset`, `/rp r`             | Return compiled state and scheduled work to the interpreter       |
| `--optimize`, `-o`               | Enable optional graph rewrites                                    |
| `--io-only`, `-i`                | Restrict display writes; with optimization, allow orphan pruning  |
| `--update`, `-u`                 | Update blocks in the selection after reset                         |
| `--export`, `-e`                 | Export a supported ordinary graph                                 |
| `--export-dot`                   | Export the backend graph as DOT where supported                   |

`--assume-instant` is no longer a supported option. Unknown flags are rejected.
Graph optimization has local eligibility checks and is not a universal
equivalence proof. Binary and DOT export can reject graph features their
formats cannot represent.

## Tests

Classical redstone regression coverage is in
[`passes/legacy_regressions.rs`](../crates/core/src/redpiler/passes/legacy_regressions.rs),
including the repeater deadline regression. Generated ordinary-redstone
comparisons are in
[`analysis/tests/redstone_fuzz.rs`](../crates/core/src/redpiler/analysis/tests/redstone_fuzz.rs).
They exercise optimized and unoptimized graphs and compare behavior with the
interpreter. The `redpiler_opt` Boolean optimizer is a separate crate and is
not used by the server's graph-pass pipeline.
