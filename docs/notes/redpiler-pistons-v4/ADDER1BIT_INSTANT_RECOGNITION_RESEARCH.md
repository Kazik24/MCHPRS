# ADDER1BIT instant recognition: sources and mechanism map

Research checkpoint: 2026-10-14. All 13 ADDER1BIT pistons are instant actors; recognizing zero is a limitation of the existing matcher. Instant response, delayed restoration, shared ownership, and externally visible callbacks are compatible. The first task is to recognize the actual mechanisms, rather than weaken the private two-wire certificate until it happens to match this schematic.

For implementation and completed verification, see [the recognition results](ADDER1BIT_INSTANT_RECOGNITION_RESULTS.md). The source-only research below predates that change.

This investigation read documents and inspected the saved O3 graph using Python. It ran no simulation, Rust build, or benchmark. Source documents below include historical results and proposals; those are distinguished from the graph facts checked here.

## Source index

Paths are relative to the repository root. Older v4 notes have moved to `scraps/docs/redpiler-pistons-v4/`; links inside those notes can still point to their former locations.

| Source | What the recognition work must take from it |
|---|---|
| `docs/PISTON_MODEL.md`, sections 2, 4, 5 | Separate delivered power sampling, request creation, accepted action and settlement. Preserve live acceptance, FIFO, quasi-connectivity, ownership and interruption semantics. Instant means downstream actions can execute in the same action phase; restoration remains delayed. |
| `docs/INTERPRETER_ARCHITECTURE.md` | Current ownership, phase dispatch, native instant route qualification and profiling boundaries; current source remains authoritative over historical captures. |
| `docs/REDSTONE_MODEL.md` | Directional weak/strong/conducted power, dust shape, callback directions/capture and the shared scheduler. A power edge is not a delivered update. |
| `scraps/docs/redpiler-pistons-v4/ADDER1BIT_PROXY_ANALYSIS.md` | Complete 13-actor role table, shared-resource groups, both XOR inhibition stages, all 32 wire roles, eight saved operand histories and the proposed composed cut. Its first-response predicates are historical bounded evidence, not an all-history runtime replacement proof. |
| `scraps/docs/redpiler-pistons-v4/FIRST_INSTANT_PROXY_CONTRACT.md` | Original narrow TEST_PISTION_2 contract, exact callbacks at reentrant cuts, existing state ownership, imported work, reset/continue and the reason atomic adder evaluation loses cancellation. |
| `scraps/docs/redpiler-pistons-v4/TEST_FIRST_PROXY_ANALYSIS.md` | Concrete private redstone/base-observer/two-wire example. This is one family, not the definition of instant. |
| `scraps/docs/redpiler-pistons-v4/INSTANT_REPLACEMENT_RESEARCH.md` | Observer reset, dust reset, torch reset, externally rearmed and blocked mechanisms differ. A held input can cause recurring reset activity. A final truth table does not establish pulse width or timer replacement. |
| `scraps/docs/redpiler-pistons-v4/INSTANT_RESET_RESULTS.md` | Limits and measurements of the first implementation; ADDER11BIT recognition was absent. |
| `scraps/docs/redpiler-pistons-v4/GRAPH_FIXTURE_STRUCTURE_ANALYSIS.md` | Full dependency closure includes conducted reads, guards, contacts, shared resources and watches. Callback-only SCCs miss dependencies. IDs are local to each export. |
| `scraps/docs/redpiler-pistons-v4/steps/opt-03-piston-specialization.md` | Local mechanism certification and partial evaluation, keeping authoritative state and global scheduling. Unsupported optimization leaves general execution intact. |
| `scraps/docs/redpiler-pistons-v4/steps/opt-04-instant-region-contraction.md` | Later absorption into value computation and timed transitions; complete boundary and interference proof required. |
| `docs/notes/redpiler-pistons-v4/DUST_FIXED_POINT_INVESTIGATION.md` | Current ordering counterexample: swapping P6/P12 initial requests changes ADDER1BIT's sum. Notifications also reach electrically disconnected wires. |
| `scraps/docs/merge-domains/docs/REDPILER_PARSER.md`, reset-family and ownership sections | Observer, torch, under-head and lateral-dust reset seeds; union-find of overlapping possible contacts; reset must work for every possible shared owner. A reset seed is a starting point, not a certificate. |
| `scraps/docs/merge-domains/docs/REDPILER_COMPOSITION.md` | One authoritative owner, independent value/update channels, original delivery order, scheduling and transactional takeover/reset. Do not import the old runtime. |
| `scraps/docs/merge-domains/docs/REDPILER_OPTIMIZER.md`, Boolean simplification | Pure expressions may be shared and simplified after dependency extraction; notification/reset proof remains separate. |
| `scraps/docs/merge-domains/test_data/piston-research/test-prefix-20261008/REDPILER_ADJUSTMENT_PLAN.md` | Historical failure: cached truth tables erased held-input reset waveforms. Reset exposure includes moved payloads and conditional conductors, not just observer outputs. |
| `scraps/docs/merge-domains/test_data/piston-research/pc-counter-minimal-20261009/ADMISSION_REPORT.md` | QC data and callback activation can arrive independently. Do not classify every QC-only actuator as BUD memory or synthesize callbacks on data changes. |
| `scraps/docs/REDPILER_PISTON_ATTEMPTS_SUMMARY.md` and `scraps/docs/REDPILER_REGRESSION_TESTS.md` | Evidence provenance: inherited logical tests and old native timings do not certify the final historical runtime. Concrete adder inhibition and shared-OR regressions remain useful. |

Implementation references: `passes/simplify_instant_resets.rs`, `program_graph.rs`, `backend/graph/instant.rs`, `backend/graph/pistons.rs`, `backend/graph/wire.rs`, `backend/graph/tests/adder.rs`, `backend/graph/tests/instant_reset.rs`, and the independent interpreter's `redstone/piston.rs` and `redstone/instant_piston_tests/` under `crates/core/src/`.

## Checked graph facts

Pre-implementation input: `test_data/graph-fixtures/adder-1bit/adder-1bit-o3.graph.json`, SHA-256 `5765ef3751e0f47923fd774148b6f932163b74d86ed2f06d493142bdb7eb4684` at inspection. This saved graph contains 107 nodes, 13 sticky actors, 10 resources, 32 wires, nine observers, 36 contacts and 192 power links. It is a static artifact; it was not regenerated in this investigation.

Coordinates below subtract fixture origin `(40,30,40)`. The node IDs and resource IDs match the historical proxy map in this export. They are witnesses for tests and diagnostics, never matching rules.

| Actor / local base / facing | Payload and ownership | Reset dependency checked in graph | Mechanism role from historical proxy map |
|---|---|---|---|
| P0 `(9,1,2)` east | R1 wool, shared with P10 | Own base observer n63 through fixed cap site315; extra QC wire n58 | First carry-OR arm, first response `G=A∧B` |
| P1 `(5,2,1)` east | R2 RB, shared with P4 | Own observer n69 through site424; additional Main contact26 and QC wire n68 | First OR arm, first response B; reset source for the first AND pair |
| P2 `(11,2,1)` east | R3 RB, shared with P5 | Own observer n71 through site430; additional Main wire n50 and conducted shared contact29, QC wire n70 | First XOR result/second OR arm, first response `X=A⊕B`; reset source for second AND pair |
| P3 `(16,2,1)` east | R4 RB, private | Own observer n72 through cap site435; fixed normal conductor site168 | Sum result actor, first response `X⊕C` |
| P4 `(7,2,3)` north | R2 RB, shared with P1 | Own observer n77 through site467; Main direct wire n64 | Other first OR arm, first response A |
| P5 `(13,2,3)` north | R3 RB, shared with P2 | Own observer n80 through site473; Main conductor site207 | Other second OR arm, first response C |
| P6 `(1,3,1)` east | R5 RB, private | Own observer n90 through site532; extra QC trigger wire n89 | B activation wrapper |
| P7 `(6,3,1)` east | R6 wool, private | No own base observer; QC includes n69's cap site424 and junction wire n93; Main includes shared contact4 | First AND inhibitor, first response G; changes ascent before P2 acceptance |
| P8 `(12,3,1)` east | R7 wool, private | No own base observer; QC includes n71's cap site430 and junction wire n95; Main includes shared contact7 | Second AND inhibitor, first response `H=X∧C`; changes ascent before P3 acceptance |
| P9 `(5,3,2)` down | R0 RB, private | No own base observer; QC includes n69's site424 and junction n102/n93/n92; Main includes n76 | First AND/carry transfer, first response G |
| P10 `(11,3,2)` down | R1 wool, shared with P0 | No own base observer; QC includes n71's site430 and junction n103/n95/n94 | Second AND/carry transfer, first response H |
| P11 `(1,3,3)` east | R8 RB, private | Own observer n98 through site563; extra QC trigger wire n97 | A activation wrapper |
| P12 `(9,3,5)` north | R9 RB, private | Own observer n101 through site593; two Main conductor branches site381/site368 | C activation wrapper |

The graph directly establishes reset power connectivity. Calling a connection a functioning reset for every history still requires the operation/ownership proof below. The historical first-response role column was not replayed here.

All three shared resources have three allowed contacts:

- R2: P1/P4, contacts4/5/13; one common Far and two owner-specific Near positions.
- R3: P2/P5, contacts7/8/15; the same shared-OR arrangement in the second stage.
- R1: P0/P10, contacts1/2/29; wool carries the OR result through occupancy/conduction rather than emitting redstone power itself.

The seven remaining resources have two contacts. Resources are conserved tokens with one location, enabled flag and revision. Neither shared arm may invent a separate token. A private token may also temporarily detach or remain at Far after an early retraction.

## Why the existing condition misses the adder

`check_candidate` in `simplify_instant_resets.rs` does not recognize general instant mechanisms. It proves a particular conductor rewrite and optionally an even narrower private alternating two-wire kernel. Its current requirements exclude:

1. Vertical actors: P9/P10 fail the horizontal condition.
2. Non-redstone payloads: P0/P7/P8/P10 move wool, whose solidity/conduction changes are the computation.
3. Shared resources or any additional resource actor: P0/P1/P2/P4/P5/P10 cannot have private Near/Far ownership.
4. Actors without their own observer: P7/P8/P9/P10 reset from another actor's observer and junction.
5. Additional valid Main or Quasi branches: several actors sample direct dust, shared contact power and reset simultaneously. Requiring exactly one branch is not an instant criterion.
6. Observer cap used by several actors: n69 and n71 are intentionally shared reset sources.
7. Exactly two collinear output wires and south-only runtime shape: the adder's outputs branch, ascend and affect topology, QC and pending acceptance.

Removing these requirements from the old runtime certificate would be unsound. Keeping them as the definition of instant is also wrong. Separate mechanism recognition from certification of a specific executable shortcut.

## Minimal sufficient recognition model

Recognition should retain a witness for each existing PistonId and refer to existing resources, input branches, reset drivers and actor groups. It must not allocate duplicate runtime state or produce a new scheduler. A role can overlap: P1 is both a shared-OR arm and a reset driver for two other actors.

Use three overlapping structural families:

1. **Base-observer reset actors:** P0/P1/P2/P3/P4/P5/P6/P11/P12. Validate the downward watch, base-change/restoration route, fixed conducting cap and actual return path to actor power. Keep every additional Main/Quasi branch. A control on the cap is a live input, not a failed shape match.
2. **Shared-token OR groups:** `{P1,P4}`, `{P2,P5}`, `{P0,P10}`. Close over every possible contact and all capable actors. Establish one resource authority and compatible pull/restore paths for each owner. Include shared wool's electrical/shape dependencies. Do not require independent reset observers on every member.
3. **Shared-reset AND/inhibitor groups:** P7/P9 driven by the first junction and n69; P8/P10 by the second junction and n71. Follow conducted QC through the neighbor cap, not observer proximity. Include the driving shared-resource group, the two-input junction and the moving inhibitor support/conductor. P7/P8 change wire connectivity; P9/P10 transfer the corresponding carry event. Their first response remains action-phase computation despite delayed shared reset.

These families cover all 13 actors. Their closures overlap; a single exclusive enum selecting one family per actor would obscure ownership and reset sharing. A region can own multiple actor witnesses and one shared resource witness.

### Recognition algorithm and checks

1. Index incoming power by target, observer watch targets, routes by receiver, and actors by every allowed resource contact. Include conducted-query dependencies and guards.
2. For every sticky single-resource actor, resolve its material and all compatible owners. Admit the existing entity-free inert-conductor and RB material subset. Preserve facing, Near/Far and all contact alternatives.
3. Extract **all** Main/Quasi expressions. Follow conducted branches to their actual strong source or changing conductor. Keep source guards, dust policy, attenuation and port identity. Never replace a disjunction with one convenient reset observer.
4. Locate reset drivers from those expressions. An observer watching this actor is one source. A neighboring actor's observer reached through a cap and junction is another. Require a causal reset witness, not merely a nearby observer.
5. Close owner and reset dependencies until stable. If a closure spans the adder, that is an honest region boundary. SCC membership alone does not establish an executable proof.
6. Check the candidate response uses existing same-action-phase callbacks/requests rather than an ordinary component tick. Keep delayed reset work explicit. Static reachability alone establishes a candidate, not inevitability of acceptance; live cancellation remains legal.
7. Emit diagnostics separating `recognized mechanism actors`, `proved input rewrites`, `certified wire kernels` and `absorbed lifecycle actors`. The acceptance test is 13 mechanism witnesses; reporting 13 fully eliminated proxies when only links are simplified would be misleading.

An observer plus a resource is not by itself an instant certificate. The witness must identify the response channel affected by immediate resource unavailability and distinguish it from a channel that waits for restored occupancy. Use `test_data/graph-fixtures/adder_non_instant_pistons_4bit.schem` and ANPU as controls: reject a particular instant shortcut when its response requires completion, while retaining their valid ordinary graph execution. Family discovery and proved execution eligibility need separate result fields if the implementation cannot yet establish that distinction.

For the smallest useful safe optimization, simplify a fixed conducted edge independently of motif recognition when its full expression is proved identical for every strength/guard/state. The actor can be recognized even if no such edge is reducible. Keep the old two-wire kernel unchanged unless its own complete proof succeeds.

### Required proof before eliminating work

Recognition can be structural while the existing exact executor remains authoritative. Eliminating callbacks or actor lifecycle work additionally requires:

- Every boundary power read, update, shape callback and observer watch represented, including reentrant/captured reads and electrically disconnected wire recipients.
- Shared ownership/drop/recapture and material-dependent wire shape/conduction preserved in every admitted resource location/enabled state.
- Request FIFO, duplicate membership, live revalidation and both kinds of retraction preserved. An inhibitor can cancel an already queued request; an accepted action cannot be retroactively erased.
- Original observer/repeater pending ticks, priorities, deadlines, stale target behavior and actor/resource revision checks retained.
- Every legal external control history remains supported; no silently imposed fixed pulse/rearm protocol.
- Projection and reset/continue reproduce all relevant cells/entities and ordered future work at host and synchronous reentrant cuts.

The historical arithmetic composition is `U=A∨B`, `G=A∧B`, `X=U∧¬G`, `V=X∨C`, `H=X∧C`, `Sum=V∧¬H`, `Cout=G∨H`. It is a useful first-response lemma. It is not an atomic replacement contract: pending requests and reset state can distinguish identical current operands.

## Tests that make the new condition meaningful

Use the pinned ADDER1BIT graph/fixture rather than synthetic names or saved coordinates as production input. Assert the mechanism witness set is exactly P0..P12 and all three shared resources retain one authority. Repeat for horizontal rotations and translations; world-up QC stays world-up.

Add rejection/decline witnesses for a severed reset return, wrongly facing observer, unaccounted resource owner, moving/foreign writer, unsafe support alternative and added external observer. A valid nonmatch leaves general execution intact. A guard-dependent extra input should be preserved, not automatically rejected.

Run existing one-bit operand, rearm/reversal, cancellation, inhibition and every-cut continuation tests with the new pass alone, then O0/O3 and the interpreter. Retain the exact n69/P7/P2 and n71/P8/P3 causal inhibition checks. Include the current P6/P12 FIFO counterexample; separate payloads do not imply commuting effects. After recognition reaches 13/13, test ADDER11BIT recognition to expose coordinate or one-bit-boundary assumptions before broadening lifecycle absorption.

No speedup follows from 13/13 recognition by itself. Report which compiled queries or callbacks were actually removed, then measure paired runtime after correctness gates.

Not checked here: fresh graph generation, replay, tests, benchmark or an all-history replacement proof. The coverage map is static evidence and an implementation contract, not permission to replace the adder with a truth table.
