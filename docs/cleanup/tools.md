# Tools and evidence source review

Scope: 21 tracked source files under `tools/**`, `scripts/**`, and source-code extensions under `docs/**` and `test_data/**`. Each was read individually. `tools/README.md` was also read as adjacent documentation, but is not part of the 21-file source inventory. Historical prose, JSON fixtures, logs, patches, and recorded outputs were excluded; those are provenance inputs, not duplicate implementations to rewrite. No scripts, captures, downloads, or external services were run.

## Findings and follow-up proposals

1. **Repair the active MCHPRS capture source inventory.** `tools/capture_instant_pistons.py` hashes `crates/core/src/redstone/piston.rs`, `crates/core/src/plot/worldedit/schematic.rs`, `crates/core/src/redstone/instant_piston_tests.rs`, and `crates/core/src/redstone/instant_piston_tests/io.rs`. These filesystem paths are retired, so `identity()` fails while reading source files before capture. Update the list to the current tracked files and verify every entry exists. The logical Cargo selector `redstone::instant_piston_tests::capture_pack` remains valid and should be retained. New source baselines should only be written as part of an explicitly requested fresh capture; old baselines stay frozen.

2. **Restore or replace two absent helper modules before using their entry points.** `capture_instant_pistons_java.py` imports `capture_piston_oscillator`, which is not tracked. `validate_instant_pistons.py` imports `validate_docs`; `validate_instant_io.py` reuses its `check_links` through the validator. Either restore those shared helpers or move the small required behavior into a tracked shared module. Before changing imports, search callers and preserve the validators' existing link-check policy. Add offline import/validation checks that do not launch a server or download artifacts.

3. **Fix the tools guide's unavailable Redpiler link.** `tools/README.md` links to `docs/REDPILER_ARCHITECTURE.md`, which is absent. Point it to an existing verification guide or remove the link if no current guide covers that material. Its interpreter architecture link resolves.

4. **Keep the two instant-piston protocol revisions separate.** The older and I/O protocol generators describe different fixture sets and schemas. Share a helper only if a second stable consumer appears and schema-specific output remains explicit. The copied Java lever client under `test_data/**` is frozen reference tooling and should remain independent of the active client.

5. **Keep dated evidence sources as evidence.** The Java capture, wire-shape replay, MAIR diagnostic, and Rust RedstoneDB replay files preserve particular experiments, local paths, setup choices, and conclusions. Do not normalize or deduplicate them with current tools. If a historical experiment must be rerun, first record the required environment and verify its inputs and destinations separately.

## Compatibility and validation

This was a read-only source review. No tools or evidence source was changed. Capture and download commands were not run because they start local servers, write generated artifacts, or access remote data. No tool-specific tests were run. The workspace validation is recorded in [the cleanup index](README.md): 849 passed, no failures, 39 ignored, and two known tests filtered out. That run does not establish that the capture and validator entry points work; the missing imports and stale source paths above remain unverified at runtime. No iterator-stage comment edits were made in these frozen evidence sources.

## File-by-file coverage

| File | Review result |
| --- | --- |
| `tools/analyze_instant_pistons.py` | Read; report/projection handling retained. No safe local change surfaced. |
| `tools/capture_instant_pistons.py` | Read; four source-hash paths are retired and break capture setup. Cargo test selector remains valid. |
| `tools/capture_instant_pistons_java.py` | Read; current entry point imports untracked `capture_piston_oscillator`. Not run. |
| `tools/capture_piston_research.py` | Read; writes new reference outputs and source identity. Not run. |
| `tools/download_instant_io.py` | Read; downloads and archives remote inputs. Not run. |
| `tools/download_piston_research.py` | Read; retrieves research inputs over SSH/SCP. Not run. |
| `tools/generate_mc_data.py` | Read; generates registry/tag assets. Not run. |
| `tools/inspect_instant_pistons.py` | Read; schematic inspection logic retained. No safe local change surfaced. |
| `tools/instant_piston_io_protocols.py` | Read; I/O revision episode generation retained as a separate versioned protocol. |
| `tools/instant_piston_protocols.py` | Read; original protocol generation retained separately from the I/O revision. |
| `tools/java_lever_client.cjs` | Read; local offline Minecraft packet client retained. |
| `tools/summarize_piston_research.py` | Read; analysis output and declared protocol handling retained. |
| `tools/validate_instant_io.py` | Read; reuses validator helpers; not runnable until the shared link-check dependency is restored or replaced. |
| `tools/validate_instant_pistons.py` | Read; imports untracked `validate_docs`. Not run. |
| `scripts/build-llvm-native.ps1` | Read; native build configuration retained. Not run. |
| `scripts/scrape-redstonedb.py` | Read; downloads and writes research data. Not run. |
| `docs/redpiler-regressions/2026-11-02/evidence/java-8111-capture.py` | Read; dated Java replay evidence retained without edits. |
| `docs/redpiler-regressions/2026-11-02/evidence/java-piston-wire-shape-capture.py` | Read; one-off replay with pinned local paths retained as historical evidence. |
| `docs/redpiler-regressions/2026-11-02/evidence/mair-anpu/Program.cs` | Read; dense diagnostic program retained as recorded experiment source. |
| `docs/redpiler-regressions/2026-11-02/evidence/redstonedb-replay.rs` | Read; dated replay test retained as evidence; no edits. |
| `test_data/instant-pistons-io/reference-tools/java_lever_client-v1.cjs` | Read; versioned offline reference client retained independently from the active tool. |

## Adjacent documentation

`tools/README.md` was reviewed for links and describes the supported workflow. Its Redpiler architecture link is stale as noted above. The rest of its workflow links were not changed.
