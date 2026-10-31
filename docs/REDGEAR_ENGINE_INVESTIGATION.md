# RedGear engine investigation

**Historical investigation:** this report describes Redpiler as it existed on
27 October 2026, before the sampled runtime replaced the Graph backend. See
[current architecture](REDPILER_ARCHITECTURE.md) for the implementation now in
this repository.

27 October 2026. RedGear was examined at commit `14a5966da55296bb839500f9034043fb5ff0042f` in the separate checkout `E:/codex-builds/RedGear-reference`. Its [repository](https://github.com/Brzechuu/RedGear) is a Paper plugin that runs either its MCHPRS/MROWW adapter or a separate compiled-engine adapter in a separate process. The adapters accept JSON lines for region load, compile, tick, use, and report. The MCHPRS adapter can run the physical interpreter; the compiled-engine adapter does not silently fall back.

RedGear's own LICENSE is MIT. The compiled engine's referenced source repository returned 404, and the checkout does not establish a separate license for its bundled binary. No separate PDK or PDK documentation was found in the checkout; the inspected interface was the compiled engine DLL and its RedGear wrapper. Do not assume RedGear's MIT license grants permission to reuse that engine's code. The DLL's assembly metadata identifies source commit `2e7be125bc4dc8a21bd05a60785a262cf2342f85`; this does not identify the sibling MCHPRS version used to build the Rust binary.

## What the bundled compiler does

These observations come from inspecting the shipped compiled-engine DLL, SHA-256 `DD963AA133042E70550E3C96D4AB0886D7DE460F49204B985FA34A01BF1A4DF1`, not from the unavailable source repository. `ilspycmd` 11.2.0.10050 was installed to `E:/codex-builds/redgear-ilspy` for inspection:

```powershell
dotnet tool install ilspycmd --tool-path E:/codex-builds/redgear-ilspy --add-source https://api.nuget.org/v3/index.json
```

The compiler builds a netlist of component nodes and conditional input links. For dust sources it performs a bounded breadth-first walk, keeping the minimum attenuation from each source; its dust traversal stops after 14 edges. It separately discovers the effect of moving piston payloads: scratch worlds place candidate tokens in alternate positions, extract each affected node or wire's inputs, and compare those links. Interacting dependencies are grouped and their combinations recorded; a group exceeding 4,096 arrangements is rejected.

For ordering, the compiler builds a scratch quiet world with dynamic sources off, while preserving dust power from constant redstone-block sources. It selects sources that feed at least two potentially racing targets and toggles each source on and off. An interpreter callback hook records the first observed reaction of each target; separate rising and falling orders are attached to the compiled netlist. For a token move, it also records neighboring updates on leaving and entering each possible cell. These observations are samples, not an exhaustive proof that the same order holds with arbitrary simultaneous inputs or feedback. The compiler also explicitly rejects several piston/payload configurations, including moving redstone components and non-sticky pushes that permanently displace payloads. RedGear does not simulate command-block execution.

This differs from the current Redpiler's source-derived power links, callback routes and guarded resource ownership. Interpreter probing could supplement source facts and expose actual update order, but a quiet-world trace alone cannot justify suppressing events for other live states. A useful prototype should construct scratch worlds without modifying the input, record source transitions and exact native callback occurrences, compare the resulting predictions under independent and simultaneous source changes, and decline to Graph when its modeled state or boundaries are not covered. State-dependent guards, shared resources, pending work, and native effects remain part of that boundary. Start with a bounded, closed wire-to-receiver region and test rises, falls, multiple sources, attenuation, piston alternatives, and unsupported neighbors before attempting the saved FPU.

## Executed checks and limits

The shipped Linux NativeAOT executable ran under WSL, and the bundled managed wrapper and DLL ran under Windows .NET 10. A JSON-lines lever–wire–lamp smoke test compiled to two nodes, one link and one wire, and the lamp turned on after the lever was used. Inputs and output are `E:/codex-builds/redgear-smoke-input.jsonl` and `E:/codex-builds/redgear-smoke-output.jsonl`.

A loader converted `test_data/piston-research/FPU_MROWW.schem` (SHA-256 `90035d2285c964f92150ee74686465150680e005c672fb39287e7a59b9f6bf1a`) into RedGear's load protocol. The compiled engine accepted all 119,766 submitted non-air blocks and reported 31,822 nodes, 36,106 links, 25,060 wires and 948 BUD nodes; one compile took approximately 4.9 seconds. This is a schematic, not the saved plot, and no command-block effects are included.

A 192-frame alternating-input run was active: it reported 3,897,761 projected changes and two distinct observed dust words. With the standard managed wrapper, tick and flush calls totaled 4.17 seconds; the driver including input calls and output reads took 8.40 seconds. An isolated wrapper timing `Engine.Tick` separately and flushing only once per six ticks measured 1.003 seconds in Tick and 2.00 seconds including driver calls. These are one-off JIT process measurements, with different projection behavior; neither is a server TPS figure or a native correctness result. The scripts and reports are `E:/codex-builds/redgear-fpu-active.*` and `E:/codex-builds/redgear-fpu-core.*`.

An additional isolated compiled-engine wrapper used `CompileOptions(OutputsOnly: true, HidePistons: true)`, disabled heat, and flushed once per six ticks. It measured 0.778 seconds in `Engine.Tick` and 0.847 seconds in batched tick-plus-flush over 1,152 ticks. The standard wrapper, with heat enabled and flush on every tick, spent 4.058 seconds in tick-plus-flush during the same interleaved comparison. Both saw the same 16 added terminal lamp states at all 192 sampled frames, but every lamp remained on, so this is weak output coverage. Repeaters, wires and piston presentation are hidden in output-only mode. The fixture was modified by adding repeaters and lamps and the timings are not comparable to the saved-plot server benchmark. See `E:/codex-builds/redgear-fpu-outputonly.*`.

A separate paired replay used RedGear's bundled MCHPRS interpreter and the compiled engine with the same schematic input and 16 added terminal repeaters. The 36 control levers began with matching powered states; uses were checked against requested states and the trigger was off. However, 15 of 490 sampled palette entries formatted differently after load, including top slabs whose properties were omitted by the interpreter adapter. Block entities were not supplied by this experiment. The first 120 per-tick samples after warmup reported different dust or repeater words. A second trace starting immediately after control setup found its first dust difference at tick 1 and its first added-repeater difference at tick 8; at least one word differed on 143 of 144 ticks. The running clock does not identify settled cuts and the load differences are not ruled out as a cause. This is an observed wrapper-level difference, not a demonstrated engine defect or evidence about the current MCHPRS revision. See `E:/codex-builds/redgear-fpu-audited-timeline.*` and `E:/codex-builds/redgear-fpu-from-entry.*`.

## Compilation and execution stages

The decompiled binary under `E:/codex-builds/RedGear-reference-decompiled` shows a
component-and-token simulation, not a whole-circuit Boolean frame. The entry
point in `Rsc.Compilation/Compiler.cs` scans blocks, describes nodes,
builds power links and wire sources, discovers conditional links under token
arrangements, samples source ordering, and tracks BUD inputs. Dust search uses
minimum attenuation and a 14-edge bound. Candidate token arrangements are
grouped locally; the implementation rejects groups with more than 4,096
arrangements and has additional limits on token clusters.

The scratch interpreter is used for more than turning every source off. The
compiler probes individual and combined token positions to discover conditional
links. For selected racing sources, it records the first qualifying downstream
callback for both rising and falling changes in a quiet world. Token departure
and arrival are probed separately to build movement notification programs. A
sampled order is evidence for that sampled environment, not a proof over every
possible simultaneous input or resource arrangement.

The `Rsc.Engine/Engine.cs` runtime keeps state for component nodes,
pistons, movable tokens, scheduled ticks and motions. A game tick processes
priority buckets, then queued piston events, then motion landings. Events are
deduplicated by piston and event kind, dequeued in order, and revalidated
against current power and movement state. Token changes switch conditional
link arrangements and invoke precomputed departure/arrival operations. The
observed piston-keyed fired-output bookkeeping is not evidence of the user's
once-per-movable-resource-per-cycle rule.

The BUD mechanism in the inspected binary is not simply a precomputed Boolean
register. Compilation marks reactive nodes and records nearby dust and its
power sources. Runtime updates link strengths and separately rechecks BUD
nodes on relevant wire or neighbor changes. BUD state and notifications remain
part of the simulated model. `OutputsOnly` and `HidePistons` reduce projected
changes; they do not establish that hidden internal computation is skipped.
The engine exposes its own sync/reset behavior, but the inspected RedGear
wrapper discards the engine on reset. Do not infer seamless native handoff
from that wrapper behavior.

For a Redpiler experiment, its existing physical Graph backend could provide
scratch probes without executing the interpreter for every sample. Validate
representative Graph results against the interpreter and retain explicit
declines for unsupported state, unobserved boundary work, non-quiescent entry,
and interactions a sample does not cover. No probe or inferred netlist should
suppress native work until the executor and its ownership contract are tested.

## Corrected FPU_27 observations

On 28 October the bundled engine was also tested with the corrected isolated
`target/build-audit-tps/prod/FPU_27-2026-10-27.schem`, SHA-256
`b6182c6c0541d692d635d72d23ecc8f33cef2fb6071f4ba4be66485ff6892bc1`.
It loaded 119,782 blocks without a reported unsupported block and compiled
31,832 nodes, 36,117 links, 25,061 wires and 948 BUD nodes in one 5.18-second
run. This is a schematic experiment, not the saved plot or a server benchmark.

After the build directory was cleared, the same schematic was downloaded again
from the server to `test_data/piston-research/FPU_27.schem`; its SHA-256 matches
the original. The current server plot was also copied to
`test_data/piston-research/FPU_27_plot.pdat`, SHA-256
`701d79050c1acb5485d2d2076db0beb01c823a6c0932b1d52d0a7a5be451af0b`.
This is a newer plot snapshot, not the earlier pinned snapshot, and has not
yet been validated against its replay protocol.

The wrapper read back all 35 requested lever states and ran the user-confirmed
`0x412345678` input and `0x558e1d715` input. With sixteen added repeaters,
the clock's accepted retract events occurred every six game ticks; at the end
of those ticks the output dust read `0xfc00` for both inputs. The first input's
user-confirmed ideal result is `0x84f3`, and the user said an all-ones result
is invalid for the second input. The wrapper exposes the event after the game
tick, not the exact instant of retraction, and the added repeaters may affect
the circuit. Therefore this observation does not establish the engine's result
at the requested falling-edge boundary or prove its arithmetic correct. The
wrapper does not simulate command-block output. The local probe and log are
`target/redgear_fpu27_oracle_probe.py` and
`target/redgear_fpu27_oracle_probe.log`.

The inspected implementation maintains a per-piston/event-type queued marker;
we did not find evidence that this enforces the separately user-specified rule
that each movable resource may move only once per cycle. A compatible Redpiler
implementation needs to establish that behavior rather than infer it from the
foreign engine's ability to compile the fixture.

## Detailed compiler model

The following is a functional description inferred from the bundled managed
assembly, not a reconstruction of its original source or a proof of behavior
on every state. Source references below identify local decompiler output under
`E:/codex-builds/RedGear-reference-decompiled`; the assembly hash is recorded above.

1. **Inventory and initial state.** `Compiler.cs::ScanBlocks`,
   `AnalyzePistons`, and `DescribeNodes` index supported components, wires,
   piston bases and movable tokens. The description includes initial component
   power and delays. Hidden nodes and watcher lists represent some changes
   that are not themselves ordinary visible components. An input is described
   by its source node, attenuation, side and notification behavior, not just a
   Boolean value.
2. **Electrical links.** `Extractor::CollectSignal`, `CollectWeak`,
   `CollectStrong`, `CollectDiodeRear`, and `GetWireSources` walk source and
   conductor relationships. The bounded dust search keeps the best distance
   for each source, then emits links to component inputs and wire source lists.
   Constant sources are retained even when a scratch world disables dynamic
   sources. Side inputs and conducted power remain distinct from ordinary rear
   inputs.
3. **Movable geometry and conditional links.** `GroupProbe::ProbeSingles`
   compares extracted links before and after moving a token to alternative
   positions. It associates changed dependencies with the tokens involved.
   `ProbeJointly` grows groups of interacting token alternatives and enumerates
   combinations, up to the observed 4,096-arrangement limit. A group records
   the inputs and wire sources for each arrangement; its current arrangement is
   represented by a mixed-radix index updated when a token moves. Discovery is
   local to candidate neighborhoods: a change visible only in an unprobed
   combination may be missed. It must not be treated as a completeness proof.
4. **Ordering and movement observations.** `SourceOrdering` chooses sources
   that may feed multiple racing targets. `OrderRecorder` toggles each chosen
   source in a quiet scratch world and records the first qualifying downstream
   notifications, separately for rise and fall. `MoveRecorder` observes
   departure and arrival effects at token cells. These observations provide
   practical orderings for compiled callbacks; they do not prove arbitrary
   simultaneous or state-dependent histories.
5. **BUDs and unsupported cases.** `TrackBudded` and related wire-watch logic
   retain reactive piston and wire relationships. The compiler has explicit
   unsupported geometry and piston cases, and the wrapper does not execute
   command-block effects. These restrictions help make its representation
   manageable and must not be mistaken for universal support.

## Detailed runtime model

`Engine.cs` retains state and processes events rather than evaluating one
monolithic FPU expression. Its node data include current output and counts of
active input levels. Propagation updates relevant link contributions and
recalculates targets; dedicated wire/BUD handling can trigger a reaction even
when a component is not a conventional clocked logic gate. Node update
deduplication is per update phase, not the requested per-resource cycle rule.

One `Tick` advances scheduled work through priority buckets, runs queued
piston events, then advances outstanding motions. Requests are FIFO, deduped
while queued by piston and event kind, and checked against live power, piston
state and movement before execution. A token transition updates the relevant
arrangement groups, applies precomputed conditional link changes, and executes
departure/arrival operations. It can therefore preserve event causes and
dynamic geometry that a final-value-only Boolean expression can erase.

Output projection is separate from the internal simulation. The engine tracks
dirty nodes and moved tokens, and flush/sync writes selected visible changes to
the host world. Output-only and hidden-piston settings affect what is projected;
they are not evidence that internal piston or wire computation disappears.
The engine has state-import and reset facilities, but the wrapper's reset
behavior alone does not prove that arbitrary native handoff is correct.

## Rust port design

Implement the behavior independently using MCHPRS types and observable tests;
do not copy the foreign assembly's code. The useful starting points are the
physical `GraphBackend`, its circuit graph and source/delivery routes, piston
and resource state, the scheduler, and the native interpreter. The former
event_netlist.rs and `graph/wire_probe.rs` were small prototypes,
not a complete implementation. The event-netlist compiler and executor have
since been retired; their recorded findings are historical. The current private
target is the sampled reference compiler using native ScratchWorld probes and its
independent runtime. Graph-derived discovery is not part of that compiler. See
the [architecture](REDPILER_ARCHITECTURE.md) and
[current status](REDPILER_SAMPLED_ENGINE_PROGRESS.md).

Suggested boundaries, each independently testable:

| Stage | Input and output | First meaningful check |
| --- | --- | --- |
| Inventory | World snapshot and selected region to component, site, piston and resource records | Stable IDs, initial state and all possible movement destinations on a small `TEST_` schematic |
| Electrical discovery | Captured world to bounded source/receiver links with attenuation and input side; Graph facts only as a temporary adapter | Compare each source and relevant simultaneous combinations with interpreter power |
| Resource alternatives | Selected resources and candidate contacts to guarded links and affected receivers | Shared token, blocked movement, two influencing tokens and an unsupported arrangement |
| Event/order discovery | Native scratch interpreter to source-qualified updates and move effects | Rising/falling controls, unchanged final level pulse and two competing updates |
| Lowering | Validated inventory, links and event records to compact arrays and adjacency lists | Deterministic output, bounds checks and explicit declines for unrepresented effects |
| Runtime | Inputs plus explicit clock event to a bounded request worklist and visible output state | Successful-only resource consumption, live revalidation, shared contention and cycle rearm |
| Host boundary | Owned selection and output adapters to plot state | Lamps, repeaters and BUD behavior; unsupported crossings decline before suppression |
| Lifecycle | Entry snapshot plus owned positions/work to one restore operation | Stop, edit-before-write, error, replacement, save/unload; outside work survives |

The earlier idealized FPU investigation required each movable resource to move
at most once per declared computation cycle. That is stronger than the foreign
runtime's observed phase deduplication and is not a demonstrated rule of the
reference engine. Keep it separate from the base reference event port. If that later mode is
implemented, define its cycle event explicitly and use a consumed marker keyed
by the shared resource, set only after a represented move succeeds and cleared
at the next declared cycle. Do not clear persistent memory or invent a cycle
from an arbitrary game tick. For the FPU, the requested publication boundary
is the falling clock edge; other devices need a supported controller or an
explicit boundary. Do not manufacture activation from a held power level.

Graph-based probing can be cheaper than repeatedly running the full world
interpreter, but it must use an unoptimized physical configuration and must not
mutate the live plot. Compare representative observations with the interpreter.
Sampled source changes alone do not prove behavior under untested source or
token combinations. Use bounded local combinations, source-derived routes
and explicit fallback/decline where interactions are unrepresented. Do not
drop commands, lamp changes or outside callbacks silently.

The separate Graph wire probe enumerates combinations of at most eight declared
lever sources for simple wire/repeater/lamp inputs and rejects unsupported
stateful graphs. The prototype runtime consumes tables only when all sampled
rows match its weighted-link model. These are narrow experimental building
blocks; they have not compiled the FPU or demonstrated plot speed. Test them
first on inspectable `TEST_` gates, then COUNTER, CPU_BUBBLESORT and FPU_27.
For every benchmark report inputs, outputs, actual runtime entries, work
replaced, build/options and end-to-end game TPS. Do not multiply an isolated
kernel rate and call it plot throughput.

## Acceptance and unresolved questions

- The two user-provided FPU vectors are essential: `0x412345678` must give
  `0x84f3`; `0x558e1d715` must not give `0xffff`. The fixed-point result
  `0x871e` for the second vector is not independently confirmed.
- A simple one-change-per-feedback-bit proxy returned `0x88f3` for the first
  vector and is not a correct resource-event model. The compact 15 feedback
  bits are selected equation cuts, not the complete set of movable resources.
- Establish repeated frames, clock-edge identification, rearm, shared-resource
  behavior and visible interface before measuring speed. The target is actual
  changing-input FPU plot throughput around 35,000 game TPS or better.
- Unknown crossings, command effects, initial moving/pending work and reset
  ownership remain open. Any omitted behavior must be visible to the user.
- COUNTER's prior multi-million-TPS result and the old FPU kernel rates are
  historical measurements of retired paths; they do not describe the new
  engine.
