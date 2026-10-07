# CPU_BubbleSort: interpreter behavior and Redpiler admission

Research date: 2026-10-07. The unchanged downloaded CPU completes its sorting program in the MCHPRS interpreter. Redpiler rejects it before constructing an executable graph because its horizontal empty ordinary pistons do not fit the implemented clock protocol. The missing piston head is a separate defect; it did not prevent the measured program from sorting and stopping.

This report distinguishes measured interpreter behavior, actual compiler errors, structural recognition limitations and proposed compiler work. It does not certify a compiled CPU or independent Java conformance. Production compiler and interpreter code were not changed by this research.

## Fixture and source identity

| Item | Recorded value |
| --- | --- |
| Actual server filename | `CPU_BubbleSort.schem` |
| Server source | `ssh urmom`, `/srv/mchprs/data/schems/CPU_BubbleSort.schem` |
| Download | [schematic](../test_data/piston-research/cpu-bubblesort/CPU_BubbleSort.schem), [download manifest](../test_data/piston-research/cpu-bubblesort/download-manifest.json) |
| SHA-256 | `907e74d6c4882d4c7065d55e0f8ec4cca8885a34f7af90ca51c060953badaf2c` |
| File size | 59,872 bytes |
| Format | Sponge v2, DataVersion 4325 |
| Dimensions | 225 × 42 × 254 |
| Legacy WorldEdit offset | `[-224,-42,0]` |
| Rust loader offset | `[224,42,0]` |
| Saved nonair blocks | 160,429 |
| Saved block entities | 52 signs; no carried/container entities |
| Tested source revision | `9381112`, isolated audit checkout |
| Protocol and observation map | [cpu_bubblesort.json](../test_data/piston-research/fixtures/cpu_bubblesort.json) |

The author described a fully instant von Neumann CPU. Its measured storage words are 12 bits wide. The program's successful sorting behavior supports the CPU description, but this research did not reconstruct or certify the complete instruction set.

The shared checkout was changing during earlier work. Runs used an isolated snapshot at revision `9381112`; source sidecars record the full revision, working-tree status and SHA-256 of the relevant Rust sources. The snapshot contained an unrelated change to `plot/client_sync_tests.rs`. Research additions are test/tool code; no executable admission guard was bypassed. Each capture's sidecar identifies its own harness revision. Earlier captures precede the added long-run recorder.

All coordinates below are **selection-local**. In the main captures, selection minimum is world `(1,8,1)`, so add that vector to obtain world coordinates. This distinction explains why the compiler's first error names world `(220,15,4)` while this report names local `(219,7,3)`.

## Import and placement

The usual research placement at `(40,30,40)` would clip this build. Its 254-block Z extent leaves very little room in a 256-block plot. The harness now checks the whole imported bounding box before pasting.

The frozen execution uses:

- Selection minimum `(1,8,1)` and maximum `(225,49,254)`.
- Paste anchor `(225,50,1)`, obtained by adding the Rust loader offset.
- An empty plot, strict paste, no notifications and no implicit settling.
- Full-plot bounds for compiler analysis, rather than only the selection box.

There is also a placement limitation: 23 piston update-port records request possible notification positions two cells beyond the selection's west or north edge. With the capture's one-cell margin these positions fall outside the plot. They are structural context requests, not evidence that the imported CPU was clipped or that those callbacks occurred during execution.

For future compiler/manual tests, prefer **selection minimum `(2,8,2)`**, maximum `(226,49,255)`, paste anchor `(226,50,2)`. A focused Rust check imports this alternative placement and verifies that all piston update-port `outside_bounds` lists are empty. Full execution timing and traces in this report remain from `(1,8,1)`; the relocation check is structural, not a second full execution at the new position.

## Controls and operating procedure

| Control | Actual interactive block | Nearby label | Measured action |
| --- | --- | --- | --- |
| Load program | Lever `(150,17,119)` | Sign `(150,16,120)` | Switch ON; wait until ticking stops |
| Reset PC | Stone button `(166,7,158)` | Sign `(167,8,158)` | Click once; wait until ticking stops |
| Start | Stone button `(166,7,156)` | Sign `(167,8,156)` | Click once; tick until execution stops |

The author specified load → wait → PC reset → wait → start → run until the program ends. The author also confirmed that ticking stopping is the only completion indicator. The successful measured protocol leaves the load lever **ON** throughout; a switch-OFF transition is not required by this particular successful run. Whether every intended use should leave it ON remains an operating-contract question.

The native button helper powers the button, schedules release, and notifies both its neighbors and its attachment support. `schedule_tick(...,10,...)` means ten redstone ticks, or twenty game ticks in this world. The import regression checks actual release after those twenty game ticks. No artificially short button pulse was substituted.

The other 50 buttons are not assigned CPU reset semantics merely because they exist. Their physical groups are:

- Twelve floor buttons `(175+4b,10,166)`, `b=0..11`.
- Fourteen wall buttons `(175,12,192+4r)`, `r=0..13`.
- Twelve floor buttons `(178+4b,13,168)`.
- Twelve wall buttons `(177+4b,18,192)`.

They are useful later for operand, register and instruction probes. No separately labeled general-reset control was found. This report's successful sequence uses only the three controls above.

## Memory layout and decoding

There are four physical RAM layers, each with sixteen rows of twelve downward sticky-piston storage cells:

```text
bit b, row r, layer y:
base = (175 + 4b, y, 62 - 4r)
b = 0..11, r = 0..15, y = 7,17,27,37
```

This is **64 × 12 physical storage bits = 768 cells**. The observation order is row 0 at `z=62`, then decreasing Z; bit 0 starts at `x=175`, then increasing X. This physical map is not a complete proof of the processor's global address decoder.

For these storage cells, decode a stationary retracted piston as one and an extended piston as zero. Labels identify Cell 0/Cell 1 and `2^0`/`2^1` in the top layer. Their saved occupancies and the final sort agree with this decoding. A moving or missing base produces a **null word**, not zero; decoding transient moving geometry as a stored value would corrupt the trace.

The initial and final settled bank values are:

| Physical layer | Initial words, rows 0..15 | Final result |
| --- | --- | --- |
| `y=37` | `15,14,13,12,11,10,9,8,7,6,5,4,3,2,1,0` | `0,1,2,3,4,5,6,7,8,9,10,11,12,13,14,15` |
| `y=27` | Sixteen zero words | Unchanged |
| `y=17` | `3872,22,1358,1088,1614,1072,3600,0,843,3872,10,3600,0,795,3872,8` | Unchanged |
| `y=7` | `272,48,528,15,797,801,3344,6,832,1053,1359,1072,1615,3600,0,1627` | Unchanged |

The lower two layers contain nontrivial saved words consistent with program storage; their semantic instruction decoding has not been proved. The manifest also watches thirteen twelve-bit rows at `(177+4b,16,194+4r)` and one twelve-bit row at `(176+4b,17,188)`. Their values change during execution and return to saved values at the end. They are register/control candidates, not established PC or instruction-register assignments.

## Measured load, reset, execution and stop

Two complete captures independently restarted from the unchanged save and reached the same sorted memory and stop boundary. One retains compact memory/register traces; the second additionally retains the 65 ordinary-generator positions and read-only diagnostics at each quiet boundary.

The recorder waits for **twenty consecutive completed game ticks with no scheduled work, queued piston events or moving entities**. This verifies the author's ticking-stops criterion without confusing a temporarily quiet wire or restored piston with completion.

| Stage | Action/start tick | First fully quiet tick | Recorder boundary after 20 quiet ticks |
| --- | --- | --- | --- |
| Load | Lever ON at 0 | 302 | 321 |
| Reset PC | Button click at 321 | 345 | 364 |
| Execute | Start click at 364 | 10,400 | 10,419 |

The last active ticks are therefore 301, 344 and 10,399. The CPU's top RAM bank first reaches its final ascending sequence at **tick 10,164**. Internal activity continues afterward, so sorted values alone are not the stop indicator. The main execution stage is 10,055 captured ticks, including the quiet confirmation; the complete protocol is 10,419 game ticks.

Early RAM transitions show the swaps rather than an intrinsic sorter replacing the CPU:

| Tick | Top RAM, first four words | Interpretation |
| --- | --- | --- |
| 0 | `15,14,13,12` | Saved descending input |
| 450 | `15,null,13,12` | Second row has a moving base |
| 452 | `15,15,13,12` | First half of the swap is visible |
| 462 | `14,15,13,12` | Other row receives the displaced value |
| 528 | `14,15,null,12` | Next swap starts |
| 540 | `14,13,15,12` | Largest value has advanced another row |
| 10,164 | `0,1,2,3` | Whole bank is ascending |

A transient duplicate value during a two-write swap is expected in this measured implementation. Whole-bank multiset conservation is an end/transaction-boundary check, not an invariant of every physical tick.

The complete run records:

- **39,213,843 total internal trace entries**, represented by an ordered-operation SHA-256 and count per tick, plus explicit entries at the observed positions.
- **11,428 RAM power samples** at 588 of the 768 cell positions. Of these, 10,404 sample an extension matching the requested power state; 1,024 sample a mismatch. A mismatch sample is not itself an accepted write.
- **512 accepted RAM piston events**, across 64 storage-bit positions. All accepted RAM events execute in the piston-event phase. RAM power sampling also occurs during moving-entity completion.
- **21,571 samples and 3,922 accepted events** at 50 of the 65 empty ordinary pistons. The remaining fifteen are structurally present but receive no samples in this program.
- No samples or accepted events at the missing-head actor `(179,6,165)`.

At the final quiet boundary three additional downward sticky pistons remain retracted: `(176,13,155)`, `(192,13,155)` and `(196,13,155)`. All are currently powered. The whole CPU is quiet even though these and the stored-one RAM cells are not extended. **Ready to compile cannot mean every piston is extended** for this architecture.

The short diagnostic experiment is intentionally separate. It switched load ON for only 64 ticks, then OFF, waited another 64, reset PC, waited another 64 and started. At tick 192 there were 1,420 motions; compiling correctly rejected active movement. At tick 320 some lower RAM words had changed. This is not a valid CPU failure result: it violated the author's wait-until-stopped procedure. It is retained as evidence against fixed short waits and starting during unfinished initialization.

## Actual compilation results

The tested options use `optimize=true` and `io_only=true`:

- Physical mode, budgets 1×, 2×, 4× and 8×.
- `--assume-instant`, budget 8×.

Each setting rejects at import and at all three quiet boundaries: ticks 321, 364 and 10,419. None activates the compiler. Before and after every attempt, the harness hashes physical blocks, entities, scheduler and piston state to verify that failed admission preserves the running/interpreted world.

The first error is identical at each quiet boundary:

```text
ordinary piston at BlockPos { x: 220, y: 15, z: 4 }
is not a ready empty observer-clock generator;
independent piston update samplers are not implemented
```

The local base is **`(219,7,3)`**, with an East-facing ordinary piston, a matching normal head at `(220,7,3)`, empty far position `(221,7,3)`, a Down-facing observer above and a fixed white-wool cap. This is not a missing head or unsupported moving material.

The compilation path is:

1. [Compiler::compile](../crates/core/src/redpiler/mod.rs) runs bounded live analysis.
2. [program::prepare](../crates/core/src/redpiler/instant/program.rs) validates global entry issues and splits coupled moving/reset/sampling regions.
3. `prepare_region` calls [clocked::recognize](../crates/core/src/redpiler/instant/clocked.rs) before validating individual sticky mechanisms.
4. That recognizer collects ordinary pistons and requires a **Down-facing**, extended, empty generator with its supported observer context. The CPU's first **East-facing** generator fails this guard.

`--assume-instant` omits some physical power/reset requirements but does not bypass this shape/ownership restriction, retained-state admission, head validation or arbitrary feedback checks. Increasing the budget cannot make an unsupported protocol legal.

Analysis itself succeeds at **all four budgets**. The import requires 1,339,392 inspected cells and 3,218,953 dependency steps; it discovers all 16,561 pistons. The first failure is therefore not a dependency-budget exhaustion. Quiet-boundary compiler attempts took approximately 0.54–0.68 seconds in this audit; these are local measurements, not a general performance guarantee.

No later executable guard or Boolean cycle check is claimed to have been reached after that rejection. Additional limitations below come from the structural report and source inspection.

## Structural inventory and what the diagnostics mean

| Property | Count |
| --- | ---: |
| Sticky pistons | 16,496 |
| Ordinary pistons | 65 |
| Observers | 11,229 |
| Redstone dust | 41,354 |
| Repeaters | 235 |
| Fixed target blocks | 408 |
| Mobile ownership groups | 13,183 |
| Single-owner groups | 9,837 |
| Two-owner groups | 3,314 |
| Three-owner groups | 32 |
| Candidate electrical output interfaces | 124 |

Per-owner saved payloads are 15,733 redstone blocks, 278 light-gray wool, 278 gray wool, 59 orange wool, 49 black wool, 49 white wool, 48 cyan wool, two lime wool and 65 empty ordinary payloads. These are actuator associations, not independent physical block counts; shared ownership matters. All sticky payload materials are already supported by the material layer.

The 408 targets are fixed, saved with zero power and not carried payloads. They influence dust connectivity through the shared wire rules. They are not the moving-target limitation seen in FPU. This run does not establish projectile-generated target behavior.

The initial recognition report has:

| Recognition failure | Count | Meaning here |
| --- | ---: | --- |
| `NoResetPath` | 6,174 | Local reset certificate missing; not proof that the CPU lacks a working return mechanism |
| `NonconductingSupport` | 3,899 | A local reset template cannot use the support as required |
| `ForcedPower` | 671 | Power contributor defeats that template's release proof; provenance matters |
| `RetractedEntry` / `UnsampledEntry` | 171 each | Saved ones and other retained states do not fit the extended-entry template |
| `NoReturnPath` | 132 | Recognizer cannot prove a particular reset route |
| `MismatchedHead` | 66 | One actual missing head plus 65 ordinary heads excluded by the sticky reset matcher |
| `NonSticky` / `UnsupportedPayload` | 65 each | The empty ordinary update actors, not 65 broken sticky mechanisms |
| `AdditionalResetWriter` | 24 | Local reset template encounters another writer |
| `ActiveReset` | 9 | A candidate reset path is currently active according to the template |

10,149 actors have no local recognition failure. That is a local certificate count, not the number of executable graph nodes or a proof that their larger region compiles. Counts overlap: one actor may have several failures.

The descriptor reports exactly **one** missing/mismatched physical head. Do not turn the reset matcher's count of 66 into a list of 66 schematic repairs. Likewise, retracted entry is valid storage in the CPU and must not inherit the FPU author's statement about its two specifically broken retracted mechanisms.

602 observer caps are saved redstone blocks. For example, base `(2,12,93)` has a Down observer at `(2,13,93)` and redstone block at `(2,14,93)`. Redstone blocks emit power but are not primitive strong-power conductors through which the ordinary observer-cap template can prove reset. These cases need their full surrounding mechanism analyzed; replacing the caps with wool would alter the circuit. They are compiler-research candidates, not an established repair list.

## New or important mechanisms

### Horizontal empty ordinary update/reset generators

Sixty-four have bases `(219,y,3+4r)`, `y=7,17,27,37`, `r=0..15`. The remaining one is at `(220,13,156)` near the execution controls. All are East-facing, extended, have normal heads, no far payload, Down observers above and white-wool caps.

For each RAM bit, a horizontal sticky writer sits at `(175+4b,y,3+4r)` beside the downward stored bit `(175+4b,y,2+4r)`. At the row's last X position, the corresponding update actor is ordinary and empty. An empty payload does not mean no effect: head changes and notifications can sample the adjacent storage. The complete trace demonstrates that these actors participate in execution.

The current Counter protocol supports one particular generator and one independently sampled bank within each coupled region. Independent regions are now supported, but that does not automatically legalize a coupled CPU with many write/update generators. Their actual dependency and ownership graph must determine the protocol.

### Stored bits initially in both states

Of the initial 171 retracted sticky pistons, 152 belong to the mapped RAM: 62 in `y=7`, 58 in `y=17`, none in `y=27`, and 32 in `y=37`. The other nineteen are outside that RAM map. Treating every retraction as unready would discard the program/data already stored in the world.

Compilation must initialize retained state from actual stationary occupancy and distinguish data power from an accepted update. Data changes alone do not justify overwriting a BUD cell. The measured no-op samples, mismatch samples and accepted events provide separate evidence for these channels.

### Computation, write feedback and sampled state boundaries

This CPU repeatedly reads RAM, computes, writes selected rows and changes control state. Its state boundaries cannot be represented by one first-wave combinational substitution over all 16,561 actuators. Feedback through storage is expected; a remaining combinational cycle after correctly cutting storage/update boundaries needs a separate diagnostic.

Ideal compiled synchronization is still allowed. The physical trace provides accepted memory/update dependencies and useful boundary expectations; it does not require retaining millions of internal callback operations, piston animation or an interpreter event-loop backend in Redpiler.

### Shared ownership and reset context

There are 3,346 groups with multiple owners, including 32 groups with three owners. Their near positions, dropping/transferring payload behavior and reset writers must remain tied to the shared physical block. A per-piston Boolean that duplicates the payload or loses its supplying contributor is insufficient.

Observer-above, under-head/nearby dust, horizontal observers and reset writers appear together. Local failed reset matching does not disprove legal logical behavior. Physical mode needs a certified joint protocol where required; ideal mode still needs correct power/geometry/update dependencies and ownership.

## Missing head and the fail-safe parser requirement

The saved sticky piston at `(179,6,165)` is extended East. Its expected head position `(180,6,165)` is air, while its redstone payload remains at `(181,6,165)`. The author considers it probably broken but requires that a noncritical defect should not prevent compilation or break the useful circuit. The author subsequently clarified: **the parser should be fail-safe and should not require every piston to be correct**.

In both successful runs, this actor is powered and extended, its head position stays air, its far redstone block remains present, and it receives no recorded samples or accepted events. No repair was made. The CPU still sorts and halts.

That is strong evidence that this defect is noncritical for the supplied sorting episode. It is not a general deadness proof: analysis finds mobile power dependencies from groups 1526 and 1832, plus wire and adjacent-head update inputs. Other opcodes or manual operand controls may exercise it. Its location is near the ALU's NOT-output label; the separate sign saying that a module serves no purpose is elsewhere and does not label this actor.

The required parser policy is role- and effect-based:

1. Inventory actual geometry and retain per-actor diagnostics, including malformed actors. A diagnostic need not immediately abort the whole plot.
2. Discover observable electrical channels, retained memory, update/clock effects and shared ownership before deciding which mechanisms need executable legalization. Memory writes and notifications are effects even when there is no lamp or repeater output.
3. Prove that a defective actor is inert under the admitted input/state contract, or that its effects cannot reach the required roots. Preserve its actual static base/head/payload blocks and any fixed electrical contribution. Do not invent a head, delete a redstone block or substitute an ideal actuator for malformed geometry.
4. Apply motion, entry and reset requirements to live roles that actually need those capabilities. Stored-one BUD cells and empty notification pistons require their own legal roles, not the extended sticky-payload template.
5. If an unsupported actor can affect the admitted behavior, explain that dependency and the unsupported role. Do not silently omit it. Unrelated excluded geometry should remain visible in diagnostics without making an otherwise proven region fail.
6. Preserve unrelated static geometry and stored state on failed admission and on interpreter handoff. A mixed interpreter/event-loop fallback is not introduced by this policy.

An always-powered actor with a proved invariant power source can sometimes be retained statically even with a defective head. A presently powered actor whose sources are mobile does not have that proof merely from its current state. Similarly, “not sampled in this run” is a research observation, not permission for unconditional dead-code elimination.

The existing compiler still checks matching stationary heads for every admitted extended actor and has no general effect-based exclusion pass. The policy above is **required implementation work**, not a claim that this research already changed those guards. It also cannot solve the CPU's earlier horizontal-generator rejection by itself.

No manual geometry repair is required to reproduce the successful interpreted sort. Repairing the missing head can be useful as a separately named diagnostic fixture, but it must not replace this original or become the prerequisite for proving the compiler's failure tolerance.

## Sequential compiler work suggested by this CPU

| Milestone | Concrete work | Acceptance evidence |
| --- | --- | --- |
| 1. Role-aware diagnostics and safe exclusion | Separate inventory defects from live-role admission; retain actual static geometry; establish roots for electrical outputs, memory and notification effects; prove any excluded malformed actors irrelevant/inert | A supported circuit plus unrelated headless/blocked actors compiles and behaves identically; a defect feeding a live update or memory path remains diagnosed; failed compile changes no world/work |
| 2. Horizontal empty generators | Generalize supported notification-generator orientation/context through shared power and update analysis; keep each generator's actual clocks and ownership; do not declare all ordinary pistons one Counter clock | Small CPU-derived row with East-facing normal head and no payload samples its neighboring BUD cells correctly on the supported edges |
| 3. Retained-state entry | Recognize storage roles separately from ready computation actuators; initialize zero/one from stationary geometry; support legal retracted memory and corresponding handoff | Mixed saved RAM patterns survive compile/reset; prepared data without update holds state; selected updates write both values and preserve other rows |
| 4. Coupled writers and CPU state boundaries | Admit several coupled update generators; establish data-before-sampling dependencies, accepted transactions, retained read/control state and logical feedback cuts | Small two-row read/write loop and register feedback preserve selected writes, unchanged samples and stop/restart state; existing Counter/ANPU/RILAX tests remain distinct |
| 5. Remaining reset and shared-group roles | Research redstone caps, dust/follower return paths and extra writers; certify physical reset families or retain required ideal-mode logical dependencies | Minimal slices cover a redstone-capped observer, three-owner payload and independent reset writer without blanket template bypass |
| 6. Scale and error reporting | Profile per-region Boolean extraction, actor/source limits and temporary memory; report all actionable unsupported roles with positions | This build's import remains bounded; any region exceeding `1024×budget` actors has an explicit size diagnostic; limits are raised only with measured memory/time evidence |
| 7. CPU acceptance | Lower the supported state/update graph; run the native load/reset/start protocol; preserve observed RAM values, write selection, useful availability, stop behavior and handoff | Unchanged save sorts `15..0` to `0..15`, leaves the other physical RAM banks unchanged and stops; validate the required memory transaction trace, not just the final array |

The total actor count exceeds the 8× expression limit of 8,192, but the limit is per prepared region. This research did not bypass the earlier clock rejection to measure successful extraction or certify region sizes. Consequently, actor capacity and combinational-cycle errors are prospective concerns, not measured next errors for this CPU. Independent region support from revision `9381112` must be reused.

Useful next fixtures are a complete single memory row including its input/read/update context; two rows with independent selection; a register feedback/reset slice; one redstone-cap mechanism with all its supplying owners; and a live circuit with an irrelevant malformed actor. Cropping away supplies or update paths would turn those into different circuits, so retain their context and declare the inputs/observable effects. The complete CPU already supplies a working integration test; new examples should isolate unsupported mechanisms rather than replace it.

## Evidence and reproduction

### Benchmark and correctness target beside PM1 and ANPU

The existing [CPU benchmark](../crates/core/benches/cpus.rs) and
[integration tests](../crates/core/tests/cpu_references.rs) now include
`cpu_bubblesort`. They reuse the common loading, checkpoint and replay helpers.
The benchmark places this large fixture at `(2,8,2)`, performs load and PC reset
outside timing, and presses Start from the prepared state. Benchmark tick zero
corresponds to world logical tick 364.

The new [frozen reference](../test_data/cpu-references/cpu_bubblesort.json) contains
nine whole-world checkpoints and all **416 RAM transitions** across the four
physical banks, including moving-word nulls. Its final RAM change is execution
tick 9,800, equivalent to absolute tick 10,164. Replay checks the sorted top bank,
unchanged lower banks, exact RAM transition trace and automatic stop boundary.
The active timing window is execution ticks 1..10,035; the complete replay still
validates 50,000 ticks. The [source sidecar](../test_data/cpu-references/cpu_bubblesort.source.json)
records provenance. PM1 and ANPU reference files remain unchanged.

```powershell
cargo bench -p mchprs_core --bench cpus -- --cpu cpu_bubblesort --iterations 3
cargo test -p mchprs_core --test cpu_references --release bubblesort_frozen_reference -- --ignored --exact
```

These are interpreter benchmarks and correctness checks. They supply a reference
for subsequent Redpiler acceptance; they do not make the CPU compile. No release
throughput result is claimed by this research. See the
[CPU reference README](../test_data/cpu-references/README.md) for the common harness.

Validation passed the new 50,000-tick frozen replay and the initial-state check
for all three CPUs (two tests, zero failures). `cargo check -p mchprs_core --bench
cpus --locked` passed. All 416 benchmark RAM transitions were also compared with
the complete research capture after translating its absolute ticks by 364.

Frozen artifacts retain the original schematic hash and source identity:

| Capture | Purpose | Projection |
| --- | --- | --- |
| `cpu-bubblesort-entry-9381112` | Full structural analysis, all budget/mode attempts at import | [summary](../test_data/piston-research/references/cpu-bubblesort-entry-9381112.summary.json), [source](../test_data/piston-research/references/cpu-bubblesort-entry-9381112.source.json) |
| `cpu-bubblesort-short-protocol-9381112` | Deliberately insufficient waits and active-motion rejection | [summary](../test_data/piston-research/references/cpu-bubblesort-short-protocol-9381112.summary.json) |
| `cpu-bubblesort-held-load-9381112` | First complete sort and stop | [summary](../test_data/piston-research/references/cpu-bubblesort-held-load-9381112.summary.json) |
| `cpu-bubblesort-completed-9381112` | Reproduced complete sort, all RAM/ordinary observation traces and quiet-boundary diagnostics | [summary](../test_data/piston-research/references/cpu-bubblesort-completed-9381112.summary.json), [source](../test_data/piston-research/references/cpu-bubblesort-completed-9381112.source.json) |

Each ID also has its raw `.json.gz` capture beside the projection. Short/full diagnostic captures retain detailed structural reports. Long-run tick samples retain full ordered trace count/hash and explicit ordered entries at manifest observation positions; they do not store all 39 million internal entries. Summaries are projections, not a replacement for operation order. No historical FPU/RILAX/ANPU evidence was regenerated or relabeled as this CPU.

From the repository root, choose a new output filename for each capture:

```powershell
py tools/inspect_instant_pistons.py --pack-dir test_data/piston-research/cpu-bubblesort --write

py tools/capture_piston_research.py --fixture cpu_bubblesort --case diagnostics --output E:/cpu-entry-new.json --target-dir E:/mchprs-redpiler-outputs-target

py tools/capture_piston_research.py --fixture cpu_bubblesort --case execution_held_load_diagnostics --output E:/cpu-completed-new.json --target-dir E:/mchprs-redpiler-outputs-target

py tools/summarize_piston_research.py --ingest E:/cpu-completed-new.json --id cpu-bubblesort-new-run
py tools/summarize_piston_research.py --check

cargo test -p mchprs_core --lib redpiler::analysis::tests::research:: --locked -- --test-threads=1
```

The long-run opt-in test asserts the ascending final bank and absence of scheduled work, events and motion. The enabled import regression checks saved descending RAM, complete plot placement, native button release and a relocation containing all potential update positions. The research module passed **13 tests, zero failures, three ignored capture tests**; the completed-sort capture was run explicitly despite being opt-in.

These observations establish MCHPRS behavior for this exact save and protocol. They do not establish every instruction, alternate RAM input, reset during active computation, restart after completion, arbitrary manual control use or independently validated Java timing. Those are subsequent acceptance cases, not conditions needed to report the successful supplied BubbleSort run.
