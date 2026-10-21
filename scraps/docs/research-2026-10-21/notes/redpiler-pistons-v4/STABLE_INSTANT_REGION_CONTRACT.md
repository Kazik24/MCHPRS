# Stable-input instant regions

> Status, 2026-10-18: O3 stable-input composition and proxies remain. The finite
> and symbolic synthesis implementations described below have been removed;
> their last combined source is commit `576ae3137492dfa3dcacaf787ac6c9e482840d6e`.
> See [algorithms and recovery](ALGORITHMS_AND_RECOVERY.md) for current execution
> and restoration instructions. Historical evidence below is preserved.

This is an explicit, experimental O3 input contract, selected by `-O3 --stable-instant-inputs` (or `--optimize --stable-instant-inputs`). Ordinary O3 keeps its arbitrary-input-history contract and original ownership groups.

## Assumption and observation boundary

The caller promises that external inputs stay stable **from the activating falling edge until the region resets**. The falling edge establishes the input values for that activation; the interval starts immediately at that edge, not later when a queued piston action is accepted. Operand preparation must finish before the edge. Data, activation levels and independent external trigger/update events cannot change during this interval. A second external activation before reset is outside this contract.

This refers to the region's identified activation input, not every falling signal inside the graph. Internal carry, inhibition, resource contacts, reset signals and their causal notifications continue to change. The compiler must distinguish these derived signals from frozen external inputs. No additional input snapshot or runtime protocol monitor is introduced by the current grouping pass.

Reset completion must be established by the region's transition certificate: its reset obligations are discharged and the next input transaction is admissible. It is not inferred from a guessed tick count, the first visible output, or equality with every entry-state bit. Legitimate stored results may remain different after reset. A held-low input may cause internally generated repeat cycles; this assumption does not make every instant region a one-shot pulse.

A history violating this promise is outside this circuit contract. This never means Rust language undefined behavior or unsafe memory access. There is currently no runtime protocol checker or inferred ready output: the caller must establish the protocol. A shared trigger is a composition connection, not proof that its surroundings obey this promise.

Private intermediate values may differ in a future contracted implementation if no boundary consumer observes them and retained memory plus future work remains equivalent. A timing restriction must specify when consumers sample; it cannot be inferred merely from a common trigger. Observers, comparator updates, external power readers, downstream piston requests and reset/continuation are also observations. Final arithmetic alone is insufficient for a stateful circuit.

The present implementation preserves more than this relaxed contract requires: it retains causal execution and original mutable state. It does not yet exploit the promise to discard Turbo walks, replace a whole adder with arithmetic, or hide intermediate boundary effects.

## Autonomous execution target

Stable inputs let a closed region execute as a local state machine for the activation. Its inputs are the values at the falling edge and its existing persistent memory. A useful transition boundary is:

```text
transition(frozen_inputs, region_memory, due_phase)
    -> next_memory, ordered_boundary_effects, next_deadline_or_reset
```

Internal work can become Boolean/strength expressions and direct memory transitions rather than individual wire walks and callback dispatches. Only effects required by outside readers need to be published, at their original logical times and in their required causal order. The state machine advances through the shared scheduler; autonomy does not mean running future ticks early or making reset instantaneous.

Before replacing execution, certify that every read comes from owned memory or a frozen input, all mutable shared payloads are enclosed, and boundary effects preserve outside observation and continuation. Derived feedback through an external repeater/comparator must be included or handled explicitly; it cannot be frozen merely because that component is outside the current ownership group. External observers of an internal wire can make its intermediate values boundary effects. If closure or a transition cannot be proven, retain general ordered execution for that part.

Current regions remain the analysis/program boundary. The first bounded closed-selection transition kernel now covers ADDER1BIT; see [CLOSED_INSTANT_MACHINE.md](../../../redpiler-pistons-v4/CLOSED_INSTANT_MACHINE.md). Larger composed regions still use ordered proxies. Composition alone does not remove internal notifications, reset work or Turbo walks.

## Algorithm and representation

`analysis/instant_regions/composition.rs` composes existing ownership groups without fixture names or runtime World queries:

1. Infer actor/resource/contact/reset ownership closures.
2. Admit a group only if every actor has a recognized reset mechanism and every reset owner is a certified built-in reset.
3. Join dependencies through internal dust, conducted power, callback relations and shared variable-input fanout. Constant supplies do not merge unrelated circuits. Ordinary components remain boundary owners; composition does not pass through a comparator or repeater's logic.
4. Reconstruct contact/site ownership, attach internal wires, and rebuild power, callback, observer and geometry-reader boundaries in source order.
5. Compile internal live piston notifications into `Sample(actor)` and qualifying reset notifications into `ArmReset(node)`. Keep other rows as `Dispatch`.
6. Independently rederive boundaries and instructions during validation; lower only validated programs.

Ineligible groups remain unchanged. Unknown enabled passes invalidate these certificates; original draft routes remain available for general execution. Mutable component, piston, payload and reset state still has one owner. Global FIFO, resource revisions, action revalidation, deferred publications and scheduled deadlines remain authoritative.

An `InstantProxy` holds its region reference, independently certified private cells, and ordered route programs. Targeted instructions allow shared regions to retain private-lane occupancy shortcuts without pretending all payloads are private. Rust JSON export, Python and the viewer use the same operation objects. Older exports without the additive region/proxy tables remain accepted.

| Fixture    | Ownership groups | Composed regions | Actors | Internal wires | Independently private cells |
| ---------- | ---------------: | ---------------: | -----: | -------------: | --------------------------: |
| ADDER1BIT  |                6 |                1 |     13 |             32 |                           4 |
| ADDER11BIT |               65 |                1 |    142 |            295 |                          43 |

All actors have explicit internal sampling instructions. One region currently means one analysis boundary and ordered proxy program, not one runtime operation. Wire states and reset obligations still exist. Region count is not a speed result. Complete adder coverage does not establish complete Divider recognition.

The separately executed large-fixture census reports Divider has 2,055 actors, 1,389 recognized actors and 1,679 actors in ownership groups (closures can include unrecognized actors). Composition yields 382 regions, of which two are eligible composed programs containing 917 and 2 actors. The remaining groups stay general. The unrecognized 666 actors and incomplete mixed ownership groups prevent a whole-Divider contraction. No recognition condition was weakened to force a single region.

## Real-output counterexample to unordered settling

The pinned ADDER1BIT case `prepared-0-1-1` prepares all operand levers in one tick, waits the manifest's 32 ticks, changes the trigger and holds every input during the response. Expected arithmetic is sum 0, carry 1.

`fixed_point_reordered_adder_requests_change_the_sum_payload` now reads actual interpreter sum/carry repeaters for 24 ticks, rather than inferring correctness solely from a payload block:

| Response ticks | Interpreter `(sum, carry)` | Ordered Graph | Unordered experiment |
| -------------- | -------------------------- | ------------- | -------------------- |
| 1-2            | (0, 0)                     | (0, 0)        | (0, 0)               |
| 3-4            | (0, 0)                     | (0, 0)        | (1, 0)               |
| 5-7            | (0, 1)                     | (0, 1)        | (1, 1)               |
| 8-9            | (0, 1)                     | (0, 1)        | (0, 1)               |

Sum is sampled at ticks 3-7. The unordered version is wrong throughout that window. These outputs are active-low: the interpreter sum repeater is physically powered, while the unordered sum repeater is unpowered. Restoring only the first two piston requests' FIFO order repairs the first payload response. Divergence recurs later; agreement at ticks 8-9 does not establish continuation equivalence.

This disproves this unordered solver under stable inputs, not the possibility of deleting native callbacks. A region kernel may remove them if it preserves the required causal dependencies or proves the resulting boundary/state transition equivalent under its timing contract.

## Correctness checks

`backend/graph/tests/stable_region.rs` compares all 8 one-bit vectors and 40 eleven-bit manifest vectors. Each case asserts a real high-to-low trigger transition, records every external lever at that edge, and checks that both interpreter and Graph keep those inputs unchanged throughout the held response. At every preparation action and response tick, ordinary O3 and composed O3 are compared using complete `CircuitState`: wire/component memory, piston availability, heads, payload identity/location/enabled/revision and reset state. The comparison also includes clock/phase, FIFO requests, deferred obligations, dirty flags and pending scheduled work. Projected whole-fixture blocks, entities, deadlines and piston future are compared against the actual interpreter; arithmetic is checked at the declared windows.

Separate tests hand off to the interpreter at response cuts 0, 1, 2, 4, 8 and 16 for both adders, then compare full projected memory and future work for another 16 ticks. This uses a carry-producing vector, not every cut of every vector. Structural checks cover all actors/contacts, unchanged uncertified groups, stale instruction targets and missing contacts. Recording remains test-only and opt-in.

Before the falling-edge clarification and source-file cleanup, the full release Redpiler run passed 330 tests with 8 ignored; the ignored Divider census was then executed separately and passed. Core test/bench and server checks passed. Python loading passed 12 tests and the HTML viewer passed 20. Golden ADDER1BIT exports were regenerated for all four tiers.

After the clarification, 17 graph test files and 3 inline test modules were consolidated under `backend/graph/tests/`. Rust module identities and shared helper visibility were retained, and current documentation links were updated. Discovery still finds the same 137 graph tests; the only name change explicitly identifies the strengthened falling-edge replay. The complete current debug Redpiler suite passed 330 tests with 8 ignored, including all 48 falling-edge/input-stability cases. Core test/bench compilation, focused formatting and diff checks passed. Release tests and benchmarks were not repeated for this test organization and contract-only update; production execution was unchanged.

These fixture checks supplement local instruction refinement; they are not an exhaustive proof over all input histories. A future implementation removing state slots must compare its semantic projection rather than require identical internal arrays.

## Measurement protocol

The existing release benchmark accepts this flag only with `--graph-tier 3` and a supported stable protocol. For adders it prepares a pinned carry vector, then measures 512 ticks with all external levers held. Setup, inputs, compilation, publication, memory checks, reset and continuation remain outside tick timers. This differs from the ordinary round-robin protocol: their times must not be compared directly.

```text
cargo bench -p mchprs_core --bench cpus --locked -- --graph-short --all-devices --component io/adder_1bit --graph-tier 3 --stable-instant-inputs --iterations 5 --output target/stable-adder1.json
cargo bench -p mchprs_core --bench cpus --locked -- --graph-short --all-devices --component io/adder_11bits --graph-tier 3 --stable-instant-inputs --iterations 5 --output target/stable-adder11.json
```

CPU/Counter protocols are not assumed stable. Attribute composition's speed by comparing ordinary O3 and composed O3 under the identical held-input episode, not different protocols or unrelated earlier workloads.

For these stable episodes the harness automatically alternates ordinary `O3`, `O3-stable` and interpreter execution on fresh worlds and verifies them against the same trace, memory checkpoints and continuation. It records both graph variants separately in JSON.

### Measured composed-region results

Five paired, uninstrumented release samples per device passed interpreter comparisons, reset memory/future checks and native continuation. Every episode below measures 512 ticks. The adders hold all primary controls after the pinned preparation; Divider retains its existing long held-phase trigger protocol.

| Fixture    | Ordinary O3 median | Composed O3 median | Median paired time change | Interpreter median |
| ---------- | -----------------: | -----------------: | ------------------------: | -----------------: |
| ADDER1BIT  |         0.012610 s |         0.012516 s |                    +0.37% |         0.013714 s |
| ADDER11BIT |         0.041507 s |         0.038743 s |                    -4.69% |         0.042186 s |
| Divider    |         0.295939 s |         0.296163 s |                    +7.87% |         0.252737 s |

Paired change and ratio of medians differ. All five eleven-bit pairs favor composition, but ranges overlap (ordinary 0.039433-0.043480 s; composed 0.038496-0.041439 s). One-bit ranges overlap too (0.012464-0.012957 vs 0.012457-0.013215 s). Divider ranges are 0.270443-0.312827 vs 0.290129-0.333815 s. These results do not justify promising a large acceleration or enabling the contract by default.

| Fixture    | Actual callbacks, ordinary to composed | Wire walks (unchanged) | Estimated retained tables, ordinary to composed |
| ---------- | -------------------------------------: | ---------------------: | ----------------------------------------------: |
| ADDER1BIT  |                       43,246 to 36,250 |                    941 |                          87,816 to 94,792 bytes |
| ADDER11BIT |                     198,238 to 192,521 |                  2,651 |                        863,692 to 942,436 bytes |
| Divider    |                     793,124 to 787,694 |                 14,380 |                  13,128,348 to 13,402,644 bytes |

Scheduled ticks, piston samples, accepted actions and deferred publications also remain exactly unchanged between variants. Fused dispatch reduces handler overhead; it does not yet eliminate the costly wire/piston computation. Retained-table estimates increase and are not RSS. This is structural progress toward region contraction, with a limited measured eleven-bit improvement and no demonstrated one-bit/Divider benefit.

Raw reports are `target/stable-adder1.json`, `target/stable-adder11.json` and `target/stable-divider.json`. Per-pair changes, work assertions, table estimates and a copy of the measured executable are in `target/stable-instant-regions/`. The final ordinary, contract-disabled CPU smoke also matched interpreter RAM, memory checkpoints and pending continuation over 100 ticks; its one sample is not a new comparative speed benchmark. No stable-input CPU promise was assumed.

## Remaining contraction

The first bounded implementation is documented in [CLOSED_INSTANT_MACHINE.md](../../../redpiler-pistons-v4/CLOSED_INSTANT_MACHINE.md). Extending it to reusable subregions requires a certified region transition program absorbing internal wire/reset work. Its certificate must identify sampled outputs, retained resource/memory transitions and future obligations. It may preserve a small causal schedule rather than every native notification. Shared-resource arbitration and the counterexample above must be represented before deleting their machinery. This batch creates an inspectable boundary/program without a second state owner.
