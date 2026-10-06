# Instant-piston pack validation, 2026-10-06

This report records the original physical characterization and its capture identities. Subsequent executable compiler work is documented separately in the [implemented pipeline](INSTANT_PISTON_RUNTIME.md): the lever/repeater 11-bit adder now passes compiled arithmetic, reset-waveform and interpreter-continuation checks. Historical statements below about missing compiler support or BUD fixtures apply to this capture stage; the later [I/O validation](INSTANT_PISTON_IO_VALIDATION.md) supplies the standalone BUD evidence. Those BUDs still require executable storage support.

The [catalog](INSTANT_PISTON_SCHEMATICS.md) contains dossiers for all 20 basic/diagnostic fixtures, including the newly supplied nanotick example and independently versioned downloaded edge case. Two CPU files have structural inventories with behavior explicitly deferred. The 22 binary hashes match the download manifest. There are 138 declared episodes, 306 MCHPRS rotation/episode captures and 81 independent Java captures, linked through [manifests](../test_data/instant-pistons/fixtures) and the [trace index](../test_data/instant-pistons/trace-index.json). All 81 declared Java port comparisons match. Traces occupy 29,688,434 compressed bytes.

Initial HEAD was `38bda8c94c0ccaed6e0548e50118d07cce6f1e47`, with a clean tree and no applicable AGENTS.md. Shared-checkout work continued; final MCHPRS capture HEAD is `a46bf74c0381697a2d7b18449f2b3dfdd9aa357a`, and final validation observes `e5285c670f305c66325bf0305a530784ad507069` after Git ignore commits. Each trace embeds the revision and source hashes actually used. The latest [source baseline](../test_data/instant-pistons/source-baseline.json) accompanies a fresh chain recapture, and does not replace older trace identities. No interpreter rules or Redpiler execution support were changed; recorder hooks compile only under cfg(test).

The shared checkout temporarily removed download-manifest.json during final validation. At the user's request it was restored from `a46bf74c0381697a2d7b18449f2b3dfdd9aa357a`; that Git blob has SHA-256 `3657ac84e1aac16037f8cfa3b2db93919b50cc96ffc899d58866d82c17a155d5`. The validator also supports reading that pinned blob when the download record is unavailable. The shared generated-artifact policy now excludes traces, inspections, projections and the trace index from Git. They remain available locally and their hashes/references are retained in fixture manifests. The [tools README](../tools/README.md) documents regeneration. Shared cleanup made the Java projection regression explicitly opt-in; it was run and passed after that change. The final test helper's SHA-256 is `b86892a82238bc9e781a63eb119f8b28f22899c13b2343ada10a1872131e40e3`; older embedded capture identities are preserved.

Java 1.21.5 server identity was verified before capture: SHA-1 `e6ec2f64e6080b9b5d9b471b291c33cc7f509733`, SHA-256 `ae7681dadce21b6b4017d28e7eb567d86b6c100a6969994f540b9e54f812dc29`. Captures use isolated frozen void worlds and fresh regions. Saved state uses strict import; Java prepared data uses normal setblock and eight measured setup ticks. MCHPRS uses raw data writes, explicit notifications and measured quiescence. These operations remain separately documented.

## Results and limits

The pack tests verify first-wave gate behavior within declared protocols, nested chain propagation, NOT transient order and repeater filtering, all eight full-adder vectors, corrected 11-bit arithmetic/carries/overflow/single bits/seeded inputs, counter memory counts 1..16, shared payload conservation, reset starvation in the intentional illegal OR, downloaded drop/recapture, analog positive-to-positive and strength-one falling responses, held-zero observations, strict-import quiescence, and supported single-fixture reuse. All declared episodes and MCHPRS rotations agree between game/nano/pico at aligned completed boundaries, including global work and watched physical states.

Observed limitations are part of the result: INSTANT_RESET_REDSTONE does not autonomously reset; AND_3 AB retains update history while BA computes; NOT AB has a raw-wire transient; XOR AB produces later reset-phase output pulses; some mechanisms oscillate under held zero. The illegal shared reset is author-excluded undefined behavior, and the delayed-inhibit nanotick example is outside the synchronized gate contract. Neither counterexample is repaired or assigned a passing Boolean oracle. No mismatch occurs in the captured Java projections. This does not prove equality of unobserved Java callbacks or arbitrary consumers.

Remaining evidence:

- A standalone BUD acceptance fixture with separate power/update ports and a declared clear/reuse protocol remains missing. Embedded AND_3/counter memory is documented. NANOTICK_EXAMPLE now covers the supplied depth-misalignment diagnostic; arbitrary position dependence and other misalignment geometries remain unverified.
- Counter stop/restart, clear, high carry and full 16-bit wrap are unverified. The stored memory bank is measured; the candidate wire bank is not certified as a persistent external count encoding.
- Generic consumer adapters, Java rotations for current binaries, Java derived probes, internal Java callbacks/queues and interpreter handoff remain unverified. Java counter details cover the first three cells plus all named observations; Java adder details cover the first two stages plus all outputs.
- There are no pending author answers required for the captured episodes. The author clarified equal-wave negation, extra-delay failure, free-running counter intent and illegal-reset exclusion. Their earlier noun “negated piston” is not assigned to a unique base; concrete operations and the required inhibit-before-consumer relation are specified instead. Future reusable BUD/consumer protocols need additional examples.

## Exact test and verification commands

Run from `F:\rustrepos\MCHPRS`. The only build diagnostic was the existing Windows linker message about creating the proc-macro import library.

| Command | Result |
| --- | --- |
| `cargo test -p mchprs_core --lib redstone::adder_tests -- --test-threads=1` | Baseline: 5 passed, 0 failed |
| `cargo test -p mchprs_core --lib redstone::instant_piston_tests -- --test-threads=1` | Before generated-artifact policy update: 15 passed, 0 failed, 1 capture test ignored, 271 filtered out; 27.57 s |
| `cargo test -p mchprs_core --lib redstone::instant_piston_tests::adder_inhibition_changes_dust_connections_before_pending_target_validation -- --exact --test-threads=1` | 1 passed, 0 failed; targeted causal cancellation check |
| `cargo test -p mchprs_core --lib redstone:: -- --test-threads=1 > target/instant-redstone-suite-final.txt` | Before opt-in policy update: 85 passed, 0 failed, 1 capture test ignored, 201 filtered out; 28.68 s |
| `cargo test -p mchprs_core --lib redstone:: -- --test-threads=1 > target/instant-redstone-suite-final-current.txt` | Final current tree: 84 passed, 0 failed, 2 ignored, 201 filtered out; 26.77 s |
| `cargo test -p mchprs_core --lib redstone::instant_piston_tests::frozen_java_port_waveforms_match_current_binaries_and_protocols --locked -- --ignored --exact --test-threads=1` | Final opt-in Java comparison: 1 passed, 0 failed, 286 filtered out; 2.56 s |
| `cargo check -p mchprs_core --lib` | Passed; normal build excludes recorder hooks |
| `rustfmt --edition 2021 crates/core/src/redstone/instant_piston_tests.rs` | Completed |
| `py tools/inspect_instant_pistons.py --write` | Generated 22 current structural inventories, including full original sign text |
| `py tools/inspect_instant_pistons.py --check > target/instant-inspection-check.txt` | Passed for all 22 exact binaries |
| `py tools/inspect_instant_pistons.py --check > target/instant-inspection-check-final.txt` | Final restored/shared tree: passed for all 22 binaries |
| `py tools/instant_piston_protocols.py` | Generated initial reviewed coordinates/actions; final analysis enriches them separately |
| `py tools/analyze_instant_pistons.py --write > target/instant-analysis.txt` | Indexed 387 artifacts; 81 comparisons, zero mismatches |
| `py tools/analyze_instant_pistons.py --check > target/instant-artifact-check.txt` | Passed; frozen projections/index reproduce exactly |
| `py tools/document_instant_pistons.py` | Generated UTF-8 catalog, final label maps, decoders, classifications and evidence links |
| `py tools/validate_instant_pistons.py --recapture-dir target/instant-chain-repro-final` | Binary/artifact hashes, complete case/rotation coverage, coordinates, links and 16 complete chain operation episodes checked |
| `py tools/validate_instant_pistons.py --recapture-dir target/instant-chain-repro-final2` | Passed after final source changes and restoration: 22 binaries, 387 artifact hashes, coordinates/links and 16 complete operation episodes |
| `py tools/validate_instant_pistons.py --recapture-dir target/instant-java-adder-repro-final` | Passed: corrected adder independently recaptured at its frozen Java origin; all recorded samples match |
| `git diff --check` | Passed; Windows line-ending conversion notices only |
| `git restore --source=a46bf74c0381697a2d7b18449f2b3dfdd9aa357a --worktree -- test_data/instant-pistons/download-manifest.json` | Restored only the accidentally removed file, as authorized by the user |

The inspection and analyzer checks do not change evidence. The validation utility compares chain recapture cells, callbacks, event/motion/scheduled work, action/operation indices and outputs exactly, allowing only updated provenance and descriptive annotations. Three-cycle period witnesses use all watched state/global work with motion identities renamed and last-tick fields relative to the boundary; they are not inferred from repeating output alone.

## Exact capture commands

These output directories are local build artifacts; frozen engine-labeled captures live in [traces](../test_data/instant-pistons/traces). Utilities refuse existing output directories. Replace a directory with a new name to reproduce. Do not rerun the protocol generator alone over final manifests: rerun analysis/document generation afterward and review changed semantics.

```powershell
py tools/capture_instant_pistons.py --output-dir target/instant-pack-first --fixture not_1
py tools/capture_instant_pistons.py --output-dir target/instant-pack-discovery
py tools/capture_instant_pistons.py --output-dir target/instant-pack-v1
py tools/capture_instant_pistons.py --output-dir target/instant-counter-probe --fixture counter_basic
py tools/capture_instant_pistons.py --output-dir target/instant-nanotick-v1 --fixture nanotick_example
py tools/capture_instant_pistons.py --output-dir target/instant-not-probes --fixture not_1
py tools/capture_instant_pistons.py --output-dir target/instant-illegal-construction --fixture or_interpreter_illigal
py tools/capture_instant_pistons.py --output-dir target/instant-onebit-final --fixture adder_1bit
py tools/capture_instant_pistons.py --output-dir target/instant-blocked-variant --fixture instant_blocked
py tools/capture_instant_pistons.py --output-dir target/instant-chain-repro-final --fixture instant_chain
py tools/capture_instant_pistons.py --output-dir target/instant-chain-repro-final2 --fixture instant_chain
py tools/capture_instant_pistons_java.py --server-jar C:/Users/Maciej/AppData/Local/Temp/mchprs-piston-audit-fd86695a/audit-references/server-1.21.5.jar --output-dir target/instant-java-v4
py tools/capture_instant_pistons_java.py --server-jar C:/Users/Maciej/AppData/Local/Temp/mchprs-piston-audit-fd86695a/audit-references/server-1.21.5.jar --output-dir target/instant-java-nanotick --fixture nanotick_example
py tools/capture_instant_pistons_java.py --server-jar C:/Users/Maciej/AppData/Local/Temp/mchprs-piston-audit-fd86695a/audit-references/server-1.21.5.jar --output-dir target/instant-java-adder-repro-final --reference test_data/instant-pistons/traces/java-adder_11bits-prepared-1365-682-0-r0.json.gz
```

The MCHPRS wrapper invokes exactly:

```powershell
cargo test -p mchprs_core --lib redstone::instant_piston_tests::capture_pack -- --ignored --exact --test-threads=1 --nocapture
```

It sets `INSTANT_CAPTURE_DIR` and optionally `INSTANT_FIXTURE`, records source identity, then writes deterministic gzip JSON. Each successful capture invocation passed its one explicit ignored test. The final chain recapture produced 16 episodes; it was checked against frozen evidence without overwriting it. Java `--reference` checks the current hash/actions/projection and reproduces the recorded absolute origin, so later inventory additions do not silently move a position-sensitive episode. The corrected `1365 + 682` capture reproduced all recorded Java samples independently; it remains a recapture check, not an overwritten reference.

Ingestion commands used the same utility with each corresponding successful capture directory. The corrected one-bit captures were retained first; later ingestion excluded their superseded candidate-consumer key:

```powershell
py tools/analyze_instant_pistons.py --ingest target/instant-onebit-final
py tools/analyze_instant_pistons.py --ingest target/instant-pack-v1 --exclude adder_1bit
py tools/analyze_instant_pistons.py --ingest target/instant-java-v4
py tools/analyze_instant_pistons.py --ingest target/instant-nanotick-v1
py tools/analyze_instant_pistons.py --ingest target/instant-java-nanotick
py tools/analyze_instant_pistons.py --ingest target/instant-not-probes
py tools/analyze_instant_pistons.py --ingest target/instant-illegal-construction
py tools/analyze_instant_pistons.py --ingest target/instant-blocked-variant
```

## Exploratory failures and preserved versions

The first whole-pack discovery stopped at a duplicate adder case filename; vector de-duplication fixed the harness, and capture restarted in a new directory. Early focused testing had three failed harness assertions: a counter persistence window, the carry repeater's incorrect consumer interpretation, and an invalid analog preparation. Physical inspection corrected those assumptions; the interpreter was unchanged. Later final assertions test the corrected mechanisms rather than hiding failures.

Exploratory Java commands used the same server-jar argument as above with `--output-dir target/instant-java-discovery`, then `target/instant-java-v1`, `target/instant-java-v2`, and `target/instant-java-v3`. They failed respectively on datapack metadata location, RCON response truncation, missing wool-state decoding, and startup heap exhaustion. The tool corrected packaging, bounded observation groups, state vocabulary and heap sizing before the successful v4 run. These were capture-tool failures, not circuit conformance failures; no failed attempt is counted as a passed comparison.

The first targeted Java recapture attempt rejected a tuple/list representation difference in the coordinate guard before starting the server. Normalizing that representation fixed the guard; the repeated command then captured successfully and matched every frozen sample. The link checker also caught the temporarily deleted download record; restoration and updated provenance text resolved it.

The historical ADDER_GWIEZDNY_TEST binary/traces, corrected adder's 21 x 7 x 45 predecessor provenance, and root-level 7 x 8 x 7 edge-case references remain separate. Current 21 x 7 x 46 adder and downloaded 8 x 8 x 7 edge-case captures carry their own hashes and coordinates. No old reference was renamed or overwritten with MCHPRS output.
