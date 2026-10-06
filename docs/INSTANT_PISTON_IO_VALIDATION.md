# Lever/repeater and standalone BUD validation

**Subsequent compiler acceptance:** the 11-bit adder from this exact I/O pack now compiles through the [implemented instant pipeline](INSTANT_PISTON_RUNTIME.md). Rust checks cover all 40 arithmetic cases under four optimize/I/O flag combinations, 24-tick repeater waveforms and interpreted continuation after handoff. This is additional compiled-versus-MCHPRS evidence; it does not replace the Java captures or resolve the XOR reset discrepancy recorded below. Standalone BUD execution remains a later milestone.

The [I/O revision catalog](INSTANT_PISTON_IO_SCHEMATICS.md) covers 22 exact schematic binaries and 140 declared cases, producing 203 MCHPRS episodes including selected horizontal rotations. There are 106 primary Java comparisons: **104 agree and two differ**, both XOR both-input reset responses. Three additional versioned Java diagnostics include one matching and one differing exact-origin comparison, plus a successful original-origin Java replay. A successful artifact validator checks this classification; it does not declare the differing episodes Minecraft-conformant.

## Source and import provenance

Read-only `ssh`/`scp` fetched the user-authorized files from host `urmom`, `/srv/mchprs/data/schems`, into a separate pack. [Download manifest](../test_data/instant-pistons-io/download-manifest.json) records exact SHA-256, byte counts, remote modification times, skipped files and the three superseded misclick saves. Those binaries are preserved under `rejected-imports`. The old pack's schematics and traces were not replaced.

The original captures used revision `50ab2f2f0433368f0981622462c220e64162997e` plus recorded shared-checkout changes. Each MCHPRS episode embeds the exact revision and relevant source hashes. The checked-in [capture baseline](../test_data/instant-pistons-io/source-baseline.json) is the discovery identity; the corrected counter and reproduced BUD captures embed their own isolated-worktree identities. [Validation baseline](../test_data/instant-pistons-io/validation-source-baseline.json) records the source used for the final Rust checks. This includes interpreter, piston, wire, world/phase, import/paste, interaction and recorder/protocol sources. No interpreter execution rules were changed by this characterization.

The shared checkout advanced through observed revision `fd996762d580530ce2bbf1600dc1f40f0487fbe4` while other parser and plot work continued. An attempted focused build then failed on unrelated unfinished parser code: missing `redpiler/analysis/tests.rs`, `RedstonePiston: Serialize` and `BlockFacing: Hash`. Instead of altering that work, initial validation used `target/instant-io-verification`, a detached worktree at `50ab2f…`, with this assignment's fixture files/helpers copied in and a separate build target. After the concurrent parser work advanced to `e22b57c`, the main checkout built and all 11 focused I/O regressions passed there as well. [Shared-checkout validation identity](../test_data/instant-pistons-io/validation-shared-source-baseline.json) records that later revision and exact source hashes separately from the trace and isolated-test identities. No applicable AGENTS.md was found in the repository or checked ancestors.

Java uses the official **1.21.5** server: SHA-1 `e6ec2f64e6080b9b5d9b471b291c33cc7f509733`, SHA-256 `ae7681dadce21b6b4017d28e7eb567d86b6c100a6969994f540b9e54f812dc29`. The tool verifies the JAR before launch. Every run creates an isolated frozen void world, binds only localhost, loads fresh fixture regions and uses strict saved-state `setblock` plus saved entity data. Preparation waits differ explicitly: MCHPRS proves queue/motion quiescence and two stable boundaries; Java records eight preparation ticks without internal queue inspection.

Lever changes are real vanilla player interactions. The local offline client uses `minecraft-protocol` 1.62.0 and Node v22.22.0; it sends UseItemOn after a verified teleport, with no chat or server plugin. RCON checks that the exact requested lever state was accepted while ticking remains frozen. Primary captures record client SHA-256 `6a92c84fa7ce8a9fd422341bc5d5c5bbacea561f93d5247f76bc9a0d8650f30f`; its exact [v1 source](../test_data/instant-pistons-io/reference-tools/java_lever_client-v1.cjs) is retained. The [current client](../tools/java_lever_client.cjs) additionally ignores stale login teleports and acknowledges client loading, SHA-256 `ec386109dff5fd06a6d6d96820e2f793a12ebc779c56baccb26b15087ae524a6`. This fixed immediate single-case capture startup; previously accepted lever episodes were not rewritten. Two early XOR replay attempts failed the explicit interaction-state check, produced no passing trace, and are not counted as completed references.

## Behavioral results

All strict imports preserve watched saved state and empty work for 24 ticks. Single/reset fixtures demonstrate their first retraction and attached repeater pulse; the torch variants prove quiescent rearm and reuse. The chain records nested callback propagation and distinct event executions in the same game tick. BLOCKED retains QC power and never applies a retraction.

OR and AND_1/2 match their first-wave event tables at the new consumers. AND_3 still depends on remote-data-before-local-update history. New NOT_1 allows the triggering actuator to retract first but rejects a downstream pending event after inhibition becomes effective; the test measures enqueue, inhibit callback and rejected execution. These observations support a synchronized logical abstraction without internal nanoticks, subject to the explicit boundary protocol.

The one-bit adder passes all eight A/B/Cin combinations. The 11-bit I/O adder is **23×7×46**, with 40 fresh calculations passing the derived modulo-2048 decoder; all A/B single bits, carry chains, overflow/maxima, `0x555+0x2aa` in both orders and eight LCG-seeded cases are included. Java covers all eight one-bit cases and six selected 11-bit calculations, plus idle. Raw sum is valid at ticks 1..3, sum repeater at 3..7, one-bit carry at 5..9. Mathematical expectations are applied only after polarity, bit order and consumer locations were established. These binaries do not inherit old GWIEZDNY traces or the prior corrected adder's coordinates.

The ordinary-input and piston-update BUDs hold their prior stored bit when live QC data changes without an update, sample the prepared bit on an independent update, retain stored one after power restoration and sample zero on the next qualifying update. All six Java cases per family agree; four horizontal MCHPRS rotations preserve the behavior. The observer-update construction produces data at tick1, samples at tick3 and publishes through its repeater at tick7, with stored state and a demonstrated six-tick recurring generator. Its four Java cases agree.

The counter's corrected saved lever releases a free-running clock. Counts 1..16 are stored at boundaries `6n`; the attached repeater bank publishes each wave later, in conservative windows `[6n+5,6n+8]`. The response was extended to tick102 to observe count16 at the consumer. The earlier y=3 candidate was wool, not output memory; the corrected harness observes dust at y=1 and the actual repeaters. Both new Java counter episodes agree. Discovery captures with the old observation scope remain under `target/instant-io-discovery`; they are not active references for the corrected manifest.

## Differences and unverified contracts

The [XOR diagnostics](../test_data/instant-pistons-io/diagnostics/xor-origin-comparisons.json) preserve the exact conditions:

| Episode | Origin | Whole reset projection |
| --- | --- | --- |
| MCHPRS order12 | (40,30,40) | Later repeater fall8, lamp dark12 |
| MCHPRS order21 | (40,30,40) | Consumer remains powered through24 |
| Primary Java order12 | (6784,40,128) | Consumer remains powered through24 |
| Primary Java order21 | (6848,40,128) | Later repeater fall8, lamp dark12 |
| Additional Java order12 | (40,30,40) | Agrees with MCHPRS |
| Additional Java order21 | (40,30,40) | Still differs from MCHPRS; later pulse |
| Replayed Java order12 | (6784,40,128) | Reproduces the primary no-pulse response |

Both engines have the same first-wave XOR table. Java order12 changes its reset projection with location, but location alone does not explain the remaining order21 difference. The actual stimuli, strict setup, binary hashes and accepted lever packets are retained. Java internal scheduled/callback order is not visible to this command-based recorder; no root cause or runtime repair is claimed. A location-independent complete XOR contract remains unverified.

`BUD_InstantPistonUpdate.schem` was rechecked after the author said the controls were fixed; it remained 629 bytes, SHA-256 `8dd37209093cd6ca4da9323e369ee4aaaa7cc406a81e8a06c00b51cef8f66d44`, with no update lever. Its active episodes explicitly add a notified control at selection-local `(1,3,-1)`. They characterize that derived construction, while original-fixture acceptance remains incomplete. The other two corrected BUD saves and corrected counter were downloaded and analyzed successfully.

The intentional illegal OR remains author-excluded: a shared owner can take away the powered payload that another participant requires for dust reset. Imported matching heads and an OR-like first pulse do not establish physical reachability, reset closure or compiler admissibility. No interpreter rules or fixture states were repaired to make it pass.

External repeated adder/gate computation, arbitrary data changes during recurring instant reset, counter stop/clear/restart/high-bit carry/wrap, generic consumer adapters and compiled/interpreter handoff remain unverified. Binary levers do not test positive-to-positive analog strength changes; existing old-pack analog diagnostics retain their separate source adapters and provenance. Horizontal Java rotations and independent Java callback instrumentation were not performed. NANOTICK_EXAMPLE, the two CPU schematics and the downloaded edge case were skipped in this revision as requested; their earlier inventories/evidence remain separate. Standalone BUD examples are now supplied, so the earlier missing-example statement applies only to the historical catalog.

## Commands executed and outcomes

The capture JAR path used was `C:/Users/Maciej/AppData/Local/Temp/mchprs-piston-audit-fd86695a/audit-references/server-1.21.5.jar`. Each output directory was new. The following are the completed extraction/capture commands; the first BUD capture is a subset retained unchanged when the full Java set was ingested:

```powershell
py tools/download_instant_io.py
py tools/download_instant_io.py --refresh
py tools/inspect_instant_pistons.py --pack-dir test_data/instant-pistons-io --write
py tools/instant_piston_io_protocols.py --write
py tools/capture_instant_pistons.py --pack-dir test_data/instant-pistons-io --output-dir target/instant-io-discovery
py tools/capture_instant_pistons_java.py --pack-dir test_data/instant-pistons-io --fixture bud_noninstantinputs --server-jar C:/Users/Maciej/AppData/Local/Temp/mchprs-piston-audit-fd86695a/audit-references/server-1.21.5.jar --output-dir target/instant-io-java-bud-discovery
py tools/capture_instant_pistons_java.py --pack-dir test_data/instant-pistons-io --server-jar C:/Users/Maciej/AppData/Local/Temp/mchprs-piston-audit-fd86695a/audit-references/server-1.21.5.jar --output-dir target/instant-io-java-v1
py tools/capture_instant_pistons_java.py --pack-dir test_data/instant-pistons-io --fixture counter_basic --server-jar C:/Users/Maciej/AppData/Local/Temp/mchprs-piston-audit-fd86695a/audit-references/server-1.21.5.jar --output-dir target/instant-io-java-counter-v2
py tools/capture_instant_pistons_java.py --pack-dir test_data/instant-pistons-io --server-jar C:/Users/Maciej/AppData/Local/Temp/mchprs-piston-audit-fd86695a/audit-references/server-1.21.5.jar --reference test_data/instant-pistons-io/traces/java-xor_simple-events-11-12-r0.json.gz --origin 40,30,40 --output-dir target/instant-io-java-xor-aligned-v2-12
py tools/capture_instant_pistons_java.py --pack-dir test_data/instant-pistons-io --server-jar C:/Users/Maciej/AppData/Local/Temp/mchprs-piston-audit-fd86695a/audit-references/server-1.21.5.jar --reference test_data/instant-pistons-io/traces/java-xor_simple-events-11-21-r0.json.gz --origin 40,30,40 --output-dir target/instant-io-java-xor-aligned-v2-21
py tools/capture_instant_pistons_java.py --pack-dir test_data/instant-pistons-io --server-jar C:/Users/Maciej/AppData/Local/Temp/mchprs-piston-audit-fd86695a/audit-references/server-1.21.5.jar --reference test_data/instant-pistons-io/traces/java-xor_simple-events-11-12-r0.json.gz --output-dir target/instant-io-java-xor-replay-v2
```

These MCHPRS commands ran from `target/instant-io-verification`, reusing its isolated build output:

```powershell
$env:CARGO_TARGET_DIR='F:/rustrepos/MCHPRS/target/instant-io-verification-build'
py tools/capture_instant_pistons.py --pack-dir test_data/instant-pistons-io --fixture counter_basic --output-dir F:/rustrepos/MCHPRS/target/instant-io-counter-v2
py tools/capture_instant_pistons.py --pack-dir test_data/instant-pistons-io --fixture bud_noninstantinputs --output-dir F:/rustrepos/MCHPRS/target/instant-io-bud-replay
cargo test -p mchprs_core --lib redstone::instant_piston_tests::io:: --locked --target-dir F:/rustrepos/MCHPRS/target/instant-io-verification-build -- --test-threads=1
cargo test -p mchprs_core --lib redstone:: --locked --target-dir F:/rustrepos/MCHPRS/target/instant-io-verification-build -- --test-threads=1
```

Each explicit capture test passed. Final focused result: **11 passed, 0 failed, 0 ignored**, 292 filtered, 29.16 seconds. The final affected redstone suite passed **95 tests, 0 failed, 2 ignored**, 206 filtered, 50.94 seconds. The ignored tests are explicit old-pack capture and optional generated-Java comparison. Earlier, before two additional causal regressions, it passed **93 tests, 0 failed, 2 ignored**. The two new regressions test same-tick chain callbacks and downstream event rejection after inhibition, rather than repeating schematic extraction assertions. Artifact validation reproduced all 24 BUD case/rotation operation episodes, and the final checks reproduced extraction, protocols, diagnostics and dossier annotations with no changes.

Ingestion and analysis kept discovery-counter captures outside the active index after its scope correction:

```powershell
py tools/analyze_instant_pistons.py --pack-dir test_data/instant-pistons-io --ingest target/instant-io-discovery
py tools/analyze_instant_pistons.py --pack-dir test_data/instant-pistons-io --ingest target/instant-io-java-bud-discovery
py tools/analyze_instant_pistons.py --pack-dir test_data/instant-pistons-io --ingest target/instant-io-java-v1 --exclude counter_basic
py tools/analyze_instant_pistons.py --pack-dir test_data/instant-pistons-io --ingest target/instant-io-counter-v2
py tools/analyze_instant_pistons.py --pack-dir test_data/instant-pistons-io --ingest target/instant-io-java-counter-v2
py tools/analyze_instant_pistons.py --pack-dir test_data/instant-pistons-io --write
py tools/record_instant_io_origin_diagnostics.py --write
py tools/document_instant_io.py --write
```

Final consistency/reproduction commands verify exact binary and trace hashes, cell counts, paste offsets, ports/sign candidates, manifest case/rotation coverage, classified Java differences, all operation snapshots in 24 repeated BUD episodes and Markdown links:

```powershell
py tools/inspect_instant_pistons.py --pack-dir test_data/instant-pistons-io --check
py tools/instant_piston_io_protocols.py --check
py tools/analyze_instant_pistons.py --pack-dir test_data/instant-pistons-io --check
py tools/record_instant_io_origin_diagnostics.py
py tools/document_instant_io.py --check
py tools/validate_instant_io.py --recapture-dir target/instant-io-bud-replay
rustfmt --edition 2021 --check crates/core/src/redstone/instant_piston_tests/io.rs
git diff --check
```

After the concurrent parser work built, validation also ran directly from the shared workspace:

```powershell
cargo test -p mchprs_core --lib redstone::instant_piston_tests::io::new_or_output_stage_preserves_shared_payload_and_illegal_reset_starvation --locked -- --exact --test-threads=1
cargo test -p mchprs_core --lib redstone::instant_piston_tests::io:: --locked -- --test-threads=1
cargo test -p mchprs_core --lib redstone:: --locked -- --test-threads=1
```

Final main-workspace results: **12 focused tests passed, 0 failed, 0 ignored**, 313 filtered, 27.59 seconds; the affected redstone suite passed **96 tests, 0 failed, 2 ignored**, 227 filtered, 54.31 seconds. The added I/O-revision regression verifies that a shared OR retains exactly one payload through both input orders and that the illegal variant restores only one reset participant after both inputs fire; its exact focused command also passed (1 test, 0 failures). Before that addition, the main workspace passed 11 focused tests and 95 affected-suite tests. These checks do not require local generated projection files.

The detailed inspection/trace/projection/index files follow the repository's existing generated-artifact policy and remain local. Binaries, protocols, evidence annotations, provenance, tools, dossiers and the three compact independent diagnostic references are versioned. [tools/README.md](../tools/README.md) provides regeneration instructions and prerequisites; Rust behavioral regressions need only the versioned schematics/manifests, not local projection files.
