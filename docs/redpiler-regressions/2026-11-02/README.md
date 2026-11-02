# Redpiler regression evidence — 2026-11-02

This bundle preserves the four RedstoneDB disagreements, exact input traces, diagnostic logs and patches, MAIR comparisons, and LLVM validation. It does not claim that the four interpreter/compiler defects have been fixed.

**Krótkie porównanie MAIR i Redpilera po polsku:** [mair-comparison/README.md](mair-comparison/README.md). **Krótki status LLVM po polsku:** [STATUS_LLVM_PL.md](STATUS_LLVM_PL.md).

| Report | First checked block disagreement | Earlier evidence |
| --- | --- | --- |
| [Carry-cancel action 39](carry-cancel-action-39.md) | Action 39, tick 5, wire `(14,9,8)`: interpreter 1, sampled 0 | Lost comparator pulse at local tick 3 |
| [Carry-cancel action 43](carry-cancel-action-43.md) | Action 43, tick 1, wire `(11,15,9)`: 14 vs 0 | Comparator/torch FIFO reversed at action 18 |
| [Adder action 63](adder-action-63.md) | Action 63, tick 1, wire `(11,17,14)`: 13 vs 0 | Missing comparator tick at action 3, tick 4 |
| [Four In A Row](four-in-a-row.md) | Action 1, tick 2, wire `(12,9,10)`: 0 vs 1 | Post-input and tick 1 still match |

[LLVM status and performance](LLVM_STATUS.md) distinguishes backend parity from interpreter correctness. The longer [experiment history](../../REDPILER_NATIVE_BACKEND_EXPERIMENT.md) remains intact.

## Coordinate and time conventions

- All positions are world `(x,y,z)`, with schematic origin `(8,8,8)`.
- Action indices are **zero-based**. Local tick 1 is the first tick after that action's lever toggle.
- Absolute ticks start at zero after loading. Apply each toggle before its wait ticks, without extra settling ticks.
- Lever indices use ascending `y`, then `z`, then `x` order. Preserve loaded block states and block entities, including initial dust powers and comparator outputs.
- The first three fixtures have 17 levers: 34 forward/reverse actions, followed by 32 seeded short-pulse actions. Four In A Row has 84 levers: 168 plus 32 actions.
- The locked generator is Rust `rand 0.10.3` `StdRng`, seeded by the fixture ID's byte-fold. Index ranges use `usize`; waits use `i32`. Substituting .NET Random or changing range widths can change the trace.

Each report embeds its complete action table. Exact machine-readable action JSON is also saved under [evidence/mair](evidence/mair). Checksums, sizes and original locations of copied evidence are in [evidence-manifest.json](evidence-manifest.json). Copied files were verified byte-for-byte; diagnostic patches are saved separately.

## Reproduce the visible failure on baseline and current

Investigated baseline: `6ef1b1657c0e1754ac2293d6087a62765466ef4e`.
Investigated current: `ea297c163a21e33b75c43f271f19c108c99d0112`.

Both reproduce the same visible failures with the same full action prefixes. The baseline's original test lacks the seeded tail, so use the saved [replay harness](evidence/redstonedb-replay.rs) on **both** revisions. The harness checks wire power and other component block states; it does not independently compare pending order. Diagnostic patches below add those observations.

Use a fresh detached worktree and an external build target. F: ran out of space during the investigation; do not allocate another dependency cache there.

```powershell
$repo = 'F:/rustrepos/MCHPRS'
$bundle = "$repo/docs/redpiler-regressions/2026-11-02"
$repro = 'E:/codex-builds/redstonedb-reproduce-current'
$revision = 'ea297c163a21e33b75c43f271f19c108c99d0112'
$caseId = '8111b000090d8f0b3e1370448bcb64a3' # Replace for another report.
git -C $repo worktree add --detach $repro $revision
Copy-Item "$bundle/evidence/redstonedb-replay.rs" "$repro/crates/core/src/redpiler/tests/sampled/redstonedb.rs"
Copy-Item "$bundle/evidence/definitions.json" "$repro/test_data/redstonedb/definitions.json"
$testPath = "$repro/crates/core/src/redpiler/tests/sampled/redstonedb.rs"
$text = [IO.File]::ReadAllText($testPath)
$needle = 'for definition in definitions["circuits"].as_array().unwrap() {'
if ([regex]::Matches($text, [regex]::Escape($needle)).Count -ne 1) { throw 'Unexpected replay harness' }
$filter = $needle + "`n        if definition[`"id`"].as_str() != Some(`"$caseId`") { continue; }"
[IO.File]::WriteAllText($testPath, $text.Replace($needle, $filter), [Text.UTF8Encoding]::new($false))
$env:CARGO_TARGET_DIR = 'E:/codex-builds/redstonedb-reproduce-target'
cargo test --manifest-path "$repro/Cargo.toml" -p mchprs_core --lib redpiler::sampled::redstonedb_tests::redstonedb_levers_match_interpreter -- --exact --nocapture
```

Repeat with a different worktree name and baseline revision. Expect **one test executed and failed**, at the report's stated position/action/tick. A zero-test run is not reproduction. Existing worktrees and primary changes must not be reset or overwritten.

## Reproduce forensic diagnostics

Start another clean worktree at baseline `6ef1b165`, then apply the relevant saved diagnostic patch. These patches already contain their fixture filter and pulse harness; do not also apply the generic harness recipe above.

```powershell
git -C $repro apply --check "$bundle/evidence/action39-baseline-diagnostics.patch"
git -C $repro apply "$bundle/evidence/action39-baseline-diagnostics.patch"
# Run the same exact test command from the previous section.
```

Available patches: [action 39](evidence/action39-baseline-diagnostics.patch), [action 43](evidence/action43-baseline-diagnostics.patch), [action 63](evidence/action63-baseline-diagnostics.patch), [Four In A Row](evidence/four-in-a-row-baseline-diagnostics.patch). They add test-only diagnostics; they are not fixes. The original logs are preserved even when later instrumentation evolved. Unbuilt experimental fix proposals were not substituted for the tested baseline diagnostics.

## Reproduce MAIR comparisons

The available DLL is the unmodified decompiler rebuild, assembly version `1.0.0.0`, SHA-256 `1510a2596486a0df240750bc96c7b0f446697fdf9fe6f3039f9b73787c99d5ac`. The original shipped MAIR binary is unavailable. Its source was `E:/codex-builds/RedGear-reference-decompiled`; the tested DLL is `F:/rustrepos/MCHPRS/target/mair-crosscheck/bin/MAIR.Core.dll`.

Copy the saved [Program.cs](evidence/mair/Program.cs) and [project](evidence/mair/RedstoneDbCheck.csproj) to a fresh directory on E:. The project targets .NET 10 and references that existing DLL. Run with the already-exported action JSON, avoiding RNG reimplementation:

```powershell
$run = 'E:/codex-builds/mair-redstonedb-reproduce'
New-Item -ItemType Directory -Path $run -Force | Out-Null
Copy-Item "$bundle/evidence/mair/Program.cs","$bundle/evidence/mair/RedstoneDbCheck.csproj" $run
dotnet run --project "$run/RedstoneDbCheck.csproj" --configuration Release -- $repo $caseId "$bundle/evidence/mair/$caseId.actions.json" "$run/$caseId.result.json"
```

Saved `*.final.json` files include fixture hashes, lever coordinates, parsing counts, first differences and the per-tick timeline. No settling tick was inserted. Every MAIR compiled node/wire was checked after each wait tick; wire power was compared, not wire sides. No separate all-block tick-zero scan was run. MAIR loads directly into its plot, whereas MCHPRS pastes a clipboard; its known hopper-support semantics also differ. Agreement within MAIR is not an independent proof of Minecraft or MCHPRS interpreter correctness.
