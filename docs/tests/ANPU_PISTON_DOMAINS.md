# ANPU Pong: piston domains and physical execution

Investigation of source revision `a0c5f293039981b365e058eae5c8a6d989104ee4`,
2026-10-08. The schematic remains
`test_data/Q2CK@Q2CK_Anpu1_Pong_KBTV.schem`, SHA-256
`f4068257797399531ce31d56c972af32d0f73f7d8d496ce1b7b8cd1c178decec`,
placed with selection minimum at `(8,8,8)`. Coordinates below are world positions.
The original schematic and frozen expectations were not modified.

**Conclusion:** ANPU's observed pistons need physical execution. The compiler
currently has no ordinary physical piston domain. Its rejection describes a
missing logical certificate, not a requirement that these ordinary mechanisms
should satisfy that certificate. Removing the admission checks would not supply
the missing executor or its electrical/notification boundary.

## Native role inventory

Reused and extended
[`anpu_native_motion_roles_distinguish_payloads_from_moving_bases`](../../crates/core/src/redpiler/analysis/tests/research/anpu_motion_scope.rs).
The diagnostic covers Start followed by 5,000 native game ticks without paddles.
Geometry is read before Start; motion identities are observed at tick boundaries;
the existing piston trace captures delivered samples and accepted events inside
each tick, including unchanged samples.

| Role/evidence | Count | Interpretation |
| --- | ---: | --- |
| Initial piston bases | 2,898 | 2,113 sticky and 785 non-sticky; these properties alone do not assign execution domains. |
| Downward cells beneath note blocks | 896 | Physical BUD memory selected by the existing BUD reference; each initially extended over black concrete. |
| Other sticky bases | 1,217 | Mixed ordinary actuators and retained geometry; 390 change pose in this episode. |
| Non-sticky bases | 785 | All have Air or their owned stationary head at Near; 546 cycle, 15 receive only unchanged samples, 224 receive no samples. |
| Non-sticky bases with a foreign base at Far | 728 | Far is an address alias, not evidence of transported piston payload. |
| Initial supported material associated with sticky bases | 967 redstone blocks, 1,146 black concrete blocks | Emitters versus conductors; memory concrete is included. |
| Structural reset matches | 0 | No certified instant mechanism is established by this inventory. |
| Observers / initial payload groups | 64 / 2,898 | Observer presence is not reset certification. |

Of the 896 memory cells, 230 extend and retract, 448 receive only unchanged
powered/extended samples, and 218 receive no callback in this episode. Stored
state and delivered sampling are separate observations; an unchanged sample is
still part of the reference.

| Boundary-observed moving entity | Extension | Retraction | Meaning |
| --- | ---: | ---: | --- |
| `source=false`, black concrete | 2,210 | 2,210 | Transported conductor/storage payload. |
| `source=false`, redstone block | 1,092 | 998 | Transported electrical supply. |
| `source=true`, piston head | 5,388 | 0 | The actuator's own extending head. |
| `source=true`, non-sticky base | 0 | 2,086 | Its own retracting base animation, not another piston being carried. |
| `source=true`, sticky base | 0 | 3,208 | Its own retracting base animation. |
| `source=false`, piston base | 0 | 0 | No transported piston body observed. |

All 17,192 observed motion identities disappear two tick boundaries after their
first observation. The scheduler advances progress by 0.5, then to 1.0, and
completes on the following movement phase. This includes sticky actuators;
stickiness does not make their movement instant. The trace accepts 5,388 Extend
and 5,294 Retract events, all in `PistonEvents`, and no `RetractWithoutPull`.
The head/base motion counts account for every accepted event in this episode.
Memory samples occur in `ScheduledTicks`, `PistonEvents`, and `MovingEntities`.
There is no observed instant response/reset episode in this run. Dormant
mechanisms, paddle inputs, and untested later motion are not classified as
universally instant or incapable of transporting a piston.

## Exact admission causes

Default maximum budget fails at `(117,30,74)`. This is initially a retracted
downward sticky actuator with black concrete at Near `(117,29,74)` and Air at
Far. Its power dependencies include a torch at `(117,30,75)` and a repeater at
`(117,32,76)`; dust at `(117,32,74)` supplies a coupled notification. Structural
recognition reports `RetractedEntry`, `UnverifiedTorchControl`, and `NoResetPath`.
It passes the earlier payload and input gates, but has no observer reset,
payload-following response, matched construction, recognized storage role, or
generator exemption at the final default-policy certificate gate in
[`prepare_region`](../../crates/core/src/redpiler/instant/program.rs).
The native trace extends this base at tick 22 and retracts it at tick 364;
later extension/retraction pairs include 422/428 and 492/498. Those are ordinary
held poses and timed transitions. A missing instant reset is not an error in
this schematic.

With `--assume-instant`, that final response/reset gate is skipped. Preparation
later reaches `(99,44,76)`, initially a retracted empty North-facing non-sticky
base. Its discovered power sources and wires are empty; its sole discovered
notification comes from dust `(99,44,78)` and is explicitly independent of power.
[`sampling::recognize`](../../crates/core/src/redpiler/instant/sampling.rs)
labels every non-sticky actor except a recognized clock as a sampling generator
and requires independently coupled control. That requirement fails here. The
native episode observes one unchanged off-state sample and no accepted motion
at this base. Independent notification is valid physical behavior; it is not a
certificate for the current logical generator model.

Budget 1 fails earlier at the dependency budget. Raising it cannot supply a
physical domain. Neither `--io-only` nor optimization selects such a domain.

## Trace through the compiler and runtime

1. [`analysis::analyze`](../../crates/core/src/redpiler/analysis/mod.rs) inventories
   every live base and attaches `PistonRuntimeUnavailable`, whose current text
   requests a validated instant runtime. Recognition supplies geometry and
   dependencies; it does not choose an ordinary execution owner.
2. [`Compiler::compile`](../../crates/core/src/redpiler/mod.rs) routes any report
   containing pistons to `instant::program::prepare`. The ordinary graph-only
   path is used only when there are no pistons in the report.
3. [`regions::split`](../../crates/core/src/redpiler/instant/regions.rs) groups
   geometry ownership, power reads, reset paths, observer dependencies, and
   independent update routes. It produces 27 ANPU regions, including regions
   of 737 and 736 bases. It partitions logical programs, not physical versus
   instant domains. Every region must pass `prepare_region`.
4. [`Boundaries`](../../crates/core/src/redpiler/instant/boundary.rs) hides owned
   geometry and binds logical electrical ports. The graph has `InstantInput`,
   `MobileSource`, and `InstantOutput`, but no physical piston node. Ordinary
   block identification does not lower a piston or moving entity.
5. [`DirectBackend`](../../crates/core/src/redpiler/backend/direct/mod.rs) advances
   graph queues and logical plans. `tick_with_world` advances logical time and
   command effects; it never drains physical piston events or motions. Plot
   tick dispatch exclusively chooses compiled or interpreted execution, and
   successful compilation clears the world's ordinary scheduled work.
6. Reset flushes graph state, materializes settled logical piston occupancy,
   and transfers graph deadlines to the world. It does not reconstruct a
   physical in-flight movement. Position bindings, observed cells, and logical
   Near/Far formulas were compiled against fixed geometry.

Thus the default failure reflects unsuitable domain routing for an ordinary
actuator; the assume-instant failure also exposes an unsupported logical
notification/control interaction. Both rejections remain necessary until a
physical boundary exists. They are not evidence that ANPU itself needs instant
pistons.

## Bounded hybrid feasibility and required contract

Reusing [`piston.rs`](../../crates/core/src/redstone/piston.rs) and the existing
`PistonState` is feasible in principle. A safe ANPU hybrid is not a small
admission-filter change: the current world interface has no callback ownership
router, and moving geometry has no compiled topology invalidation protocol.

| Boundary | Required behavior and current blocker |
| --- | --- |
| Compiled power to physical actors | Read current graph strengths, including analog comparator entities, directional strong/weak power, conduction, and QC. Display flushing is optional and cannot be the source of truth. Deliver qualifying notifications separately from power changes; do not poll every piston each tick. |
| Physical notifications to graph/instant regions | Route `piston::notify`, observer shape changes, head-to-base callbacks, and restored-component rechecks to exactly one execution owner. Today they call `redstone::update` and `interaction::change` directly; these can update dust and schedule electrical work in the interpreter while the graph owns that same work. A `schedule_tick` hook alone cannot suppress synchronous dust propagation. |
| Tick order and sampling | Share one logical clock; preserve scheduled priority/FIFO work, then piston events including newly enqueued events, then an identity-checked snapshot of moving entities. Movement-completion notifications may enqueue events for the next event phase. Record independent BUD deliveries, same-value samples, and causal ordering; never synthesize writes from data changes. Current logical sampling transactions are not automatically physical callback sequences. |
| Geometry and topology | Account for absent supply/conduction while material is a MovingPiston, dust shape/support and stepped connections, observers watching actual block-state changes, and refreshed input/binding maps. Static graph links and logical settled Near/Far guards do not cover those intermediate states. |
| Moving actors or compiled components | Preflight the full native payload line before mutation. Reject movement of graph-bound nodes, instant-owned cells, other bases, or unsupported material/support interactions unless their ownership/bindings can move atomically. Far containing a foreign base alone is not a reason to reject an empty Near actuator. Recheck the footprint on each operation. |
| Invalidation and handoff | Stop compiled execution before unsafe writes; materialize current electrical state and deadlines once; retain physical event order, motion identities/progress/carried entities, phase, and movement cursor. Never overwrite a live moving base/head with settled logical materialization. Resume the interpreter from the preserved phase, without reset-generated samples. |

The smallest useful next implementation would support an isolated ordinary
empty-head actuator with a fixed compiled control boundary, actual movement
completion, and an independently notified physical BUD cell; explicitly reject
topology-changing footprints and movement of compiled components. Validate
both directions and reset while progress is 0.5 before expanding to transported
conductors/redstone supplies. ANPU exercises the latter already, so success on
the empty-head subset alone would not establish admission or equivalence.

No hybrid was published and no certificate was weakened. This investigation
adds only native diagnostic evidence and a regression for pending-event/motion
rejection, reset, and interpreter continuation. A representative working hybrid
and a compiled ANPU replay remain blocked by the ownership router and dynamic
geometry contract above.

## Validation and performance

| Check | Result |
| --- | --- |
| Extended 5,000-tick native role diagnostic | Passed; inventory/timing above. |
| 20 admission attempts: budgets 1, 2, 4, 8; default, optimize, io-only, optimize+io-only, assume-instant | Passed rejection regression; inactive compiler, unchanged physical checkpoint and queued work. |
| Unchanged 50,000-tick Pong replay | Passed all whole-world checkpoints and ordered chat; complete per-tick screen-change trace (14 frozen frames, including the initial frame). |
| Unchanged physical BUD reference plus screen/checkpoints | Passed all 1,552 active sample-tick records, including ordered same-value samples and accepted events. |
| New pending-event/motion reset and continuation regression | Passed for sticky/non-sticky actuators, both compiler policies, and pauses before execution and after one/two movement steps; full checkpoints match an uninterrupted interpreter through tick 8. This resets an inactive compiler after transactional rejection, not an active hybrid. |
| Active-hybrid reset/handoff, compiled screen/BUD replay | Unavailable: no hybrid admitted or implemented. |
| Fixture/reference integrity; diff whitespace; changed Rust file formatting | Passed. |

Release interpreter measurements use Rust `1.98.1`, Windows, AMD Ryzen 9 5950X
(16 cores / 32 logical processors), the repository's release profile with fat
LTO, no connected clients, and the fixed harness active window of ticks 1..5,000.
Three samples were recorded sequentially after validation, with no concurrent
build/test process. An earlier run overlapping validation was discarded.

| Sample | 50,000-tick seconds | 50,000-tick average TPS | Active seconds (5,000 ticks) | Active TPS |
| --- | ---: | ---: | ---: | ---: |
| 1 | 4.7715095 | 10,478.9 | 4.7698940 | 1,048.2 |
| 2 | 4.8795332 | 10,246.9 | 4.8777986 | 1,025.1 |
| 3 | 4.7708760 | 10,480.3 | 4.7692811 | 1,048.4 |
| Median | 4.7715095 | **10,478.9** | 4.7698940 | **1,048.2** |

Each sample retains the frozen checkpoint/chat/screen assertions. Timed work is
`tick_interpreted`: scheduled electrical work, physical events/movement,
world mutations, normal dirty-state bookkeeping, and synchronous piston/block
action packet encoding. The harness has `fast_rendering=false` and
`screen_only=false`; `PlotWorld::block_action` encodes piston actions even with
an empty client list. Loading, Start input,
hashing, screen projection/comparison, and reference checks are outside timing.
`--flush-every` is zero: display/section-change flushing and its packet encoding,
and client/network delivery costs are excluded. The 50,000-tick
average includes the mostly idle tail and must not be substituted for active TPS.
No compiled/hybrid speedup is claimed.

Raw timing artifact: `target/anpu-piston-domain-interpreter.json` (generated,
not a frozen expectation). Reproduction commands:

```powershell
cargo test -p mchprs_core --lib --release --locked anpu_motion_scope -- --include-ignored --nocapture --test-threads=1
cargo test -p mchprs_core --lib --release --locked redstone::piston::tests::bud_reference::anpu_cannot_bypass_compiled_graph_admission -- --exact --nocapture
cargo test -p mchprs_core --test cpu_references --release --locked anpu_pong_frozen_reference -- --ignored --exact
cargo test -p mchprs_core --lib --release --locked redstone::piston::tests::bud_reference::anpu_interpreter_preserves_frozen_bud_updates_and_screen -- --ignored --exact
cargo bench -p mchprs_core --bench cpus --locked -- --cpu anpu_pong --iterations 3 --label anpu-piston-domain-investigation --output target/anpu-piston-domain-interpreter.json
```

The unit and integration validation commands above were also exercised directly
through the corresponding freshly built release test executables, avoiding a
concurrent unrelated Cargo build lock. Run performance after other work finishes.
