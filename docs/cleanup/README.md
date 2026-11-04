# Repository cleanup review

This pass reviews authored source files for duplicated knowledge, readability,
useful comments, and iterator pipelines with more than three stages. Small,
behavior-preserving changes are applied; larger composition changes remain
proposals with their compatibility and verification requirements.

The detailed coverage is split by ownership:

| Scope | File-by-file review |
| --- | --- |
| Blocks, network, registry macros, persistence, API, viewer and entry point | [Supporting code](supporting.md) |
| Core outside plot and Redpiler, including tests and examples | [Core](core.md) |
| Plot lifecycle, commands, Git, WorldEdit and tools | [Plot](plot.md) |
| Sampled compiler, runtime, LLVM boundary and tests | [Redpiler](redpiler.md) |
| Capture, generation, validation scripts and historical source evidence | [Tools](tools.md) |

Registry JSON, binary schematics, lockfiles and recorded evidence are inputs,
not alternative implementations to deduplicate. Frozen migration structs also
deliberately repeat old layouts: changing their field order would change the
save format. Source files under historical evidence are reviewed as evidence,
without rewriting their recorded implementation or hashes.

## Applied changes

- Removed unused block-name and item-stack-limit macro inputs; generated
  registries already provide the runtime values.
- Reused direction rotations and face decoding, and shared the absent-property
  sentinel handling across boolean block accessors.
- Named NBT serialization values and comparator arithmetic; explained legacy
  layouts, interpolation progress and inventory thresholds.
- Shared packet angle encoding and reused the existing empty-packet macro.
- Removed three unreferenced files outside the Rust module graph: the old
  network NBT map and two obsolete migration prototypes.
- Shared RedGear baseline traversal without changing coordinate order.
- Shared viewer signal-row defaults, command output text and canvas row lookup;
  consolidated identical selected-row and tooltip CSS declarations.
- Added targeted explanations of longer iterator pipelines and retained the
  execution order and allocation behavior of simulation/compiler hot paths.

## Larger proposals

| Proposal | Concrete benefit and affected code | Compatibility and verification |
| --- | --- | --- |
| Named generated block definitions | Replace the five-field registry tuple with `BlockDefinition { name, registry_id, first_state, last_state, default_state }`. Update the generator, blocks and all `generated::BLOCKS` consumers together; `definition.2` and `.4` would become explicit field names. | Public generated type changes. Update fingerprint encoding and preserve the existing canonical fingerprint; run all registry and block round trips. |
| Sign-side composition | Model each face as `SignText { rows, color, glow }`, so front/back parsing and output share the same vocabulary. Callers include sign edits, packet encoding, schematic import and saved block entities. | `SignBlockEntity` uses positional bincode serialization. Keep the old layout through a serialization adapter or introduce an explicit migration; test both faces, colors, glow, legacy saves and NBT round trips. |
| Shared button/lever attachment vocabulary | `LeverFace` and `ButtonFace` repeat floor/wall/ceiling parsing, display and IDs. A shared attachment type could replace this repetition while leaving the enclosing button and lever structs distinct. | Account for public type names and Debug/error text. Check all legacy state IDs, defaults, rotations and property parsing before replacing them. |
| Viewer persistence ownership | Extract layout restoration/persistence from `App.tsx` into one hook with a clear plot/capture owner. Keep capture subscription and row actions separate. | Preserve the guard against writing the previous plot's layout. Existing layout/API/session tests cover data; add a browser test for switching plots before moving effects. |
| Viewer canvas composition | Split drawing and pointer navigation in `Waveform.tsx` into focused functions. Reuse `rowAt` and one viewport mapping vocabulary instead of expanding the component's props further. | Preserve device-pixel scaling, pointer capture, same-tick pulse visibility, scroll anchors and keyboard accessibility; validate in a browser. |
| Named visual counters | `RunTiming.visual_counts` and related tuple fields could share a small named counter struct if more consumers are added. | Serialized benchmark JSON currently uses arrays in some paths; preserve that schema or version consumers together. No new type is needed just to replace a single tuple access. |

Additional proposals and reasons to retain existing structure are recorded with
their owning files in the linked reviews.

## Verification

- Blocks: 13 tests passed; the manual property timing benchmark stayed ignored.
- Network: 36 tests passed, including byte-angle boundaries; the manual sender
  benchmark stayed ignored.
- Registry macros: four tests passed, including fingerprints of generated tables.
- Save data: six migration and backup-preservation tests passed.
- Viewer: 41 tests passed, including the shared command text check; TypeScript
  checking and the Vite production build passed after installing locked packages.

The combined `cargo test --workspace --locked --no-fail-fast` run completed with
849 passing tests, two failures and 38 ignored tests. The failures are:

- `redstonedb_levers_match_interpreter`: the Four In A Row fixture retains wire
  power 1 where the interpreter has 0 at `(12,9,10)`, action 1, tick 2. This is
  the same disagreement described in the existing
  [regression report](../redpiler-regressions/2026-11-02/four-in-a-row.md).
- `frozen_cpu_inputs_and_initial_states`: the checkout lacks
  `test_data/cpu-references/zcore12b_sierpinski.json`. The reference loader fails
  while opening the expected file; no reference was regenerated or substituted.

After the final source edits, the workspace run passed with 849 tests, no
failures, 39 ignored tests and only the two failures above filtered out:

```powershell
cargo test --workspace --locked -- --skip redpiler::sampled::redstonedb_tests::redstonedb_levers_match_interpreter --skip frozen_cpu_inputs_and_initial_states
```

The final run emitted no compiler warnings. The ignored-test count changed
because concurrent regression work added an ignored diagnostic test.

All 16 tracked Python files also passed syntax parsing without executing their
capture/download actions. No deployment, live-server capture, browser interaction
test or performance benchmark is part of this source cleanup.

`cargo fmt --all -- --check` remains red across the repository (482 diff
sections, including untouched benchmark/example files). Edited supporting Rust
files were formatted individually; a whole-repository formatting rewrite was
kept out of the targeted cleanup. VS Code's corrected package/binary target was
checked against `cargo metadata --no-deps --locked`.

Concurrent edits to `crates/core/src/redpiler/tests/sampled/cpu_trace.rs`, new
regression modules, and the dated `docs/redpiler-regressions/2026-11-04/` evidence
belong to separate work and are preserved without claiming them as cleanup
changes. Coverage follows the tracked source inventory at the start of this pass.
