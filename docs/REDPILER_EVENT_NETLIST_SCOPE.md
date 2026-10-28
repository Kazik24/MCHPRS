# Event-netlist compiler scope

## Status and target

This is an implementation scope, not a description of an existing executor.
The corrected saved FPU plus Divider still executes on Graph. The goal is
20,000–30,000 game ticks/s with changing inputs, with 40,000 as a stretch goal;
COUNTER must remain above 1,000,000 game ticks/s and adders must work. Compare
observable outputs and command messages against native redstone and report any
differences. The user explicitly permits the idealized Boolean output to differ
when native nanotick timing changes the result in the experimental mode; do not
describe that behavior as native equivalence. Preserve the state and future
needed for native continuation at any handoff the mode actually offers.

The retained research passes are disabled for production because discovery,
boundary analysis and equation lowering do not yet implement source-qualified
internal propagation, complete native effects or a safe handoff. The failed
bulk executor and temporary runtime interception were removed. See the
[research results](REDPILER_EVENT_NETLIST_RESULTS_2026-10-28.md) for measured
coverage, successful tests, failed shortcuts and reproducible commands.

The isolated baseline `event_domain_research_pipeline` in `passes/mod.rs`
prepares the existing Graph and runs only instant-boundary discovery. Every
other graph pass is off in that test-only pipeline. It installs no compiled
executor: the Graph remains authoritative. Normal optimization tiers, including
the existing COUNTER path, are unchanged. This baseline exposes the original
source relationships to the research compiler; it is not a new public mode or
a performance claim. If boundary discovery itself exhausts a budget, report
the failure; it is not evidence that a smaller region cannot be compiled.

On 2026-10-28, a test-only discovery census of the corrected saved plot
(`p-1,-1-FPU27-2026-10-27`, SHA-256
`261e7c67467d788023b0b809f439cf390e09d0e449ab103fcc5c75be75bcf319`)
completed with an explicit 8x analysis budget in the shared source. It found two
potential candidates:
the FPU candidate contains 8,016 actors and 24,857 wires, and the Divider
candidate contains 2,054 actors and 5,115 wires. Discovery took 7.82 seconds
and the sampled peak process working set was 1.16 GiB. After partitioning was
changed to use owner-qualified pairs reached by the greedy walk, a shared-source
rerun produced the same counts in 8.08 seconds with a 1.18 GiB sampled peak.
These are potential dependencies, not certified executable regions. The pass recorded 548
native-resource read rows; these do not constitute a complete boundary or
rejection report. No executor was installed or run. The same full-plot
pipeline previously hit its fixed 256 MiB boundary-table estimate before
discovery; the isolated run raised that estimate to 2 GiB and the work cap from
32 Mi to 256 Mi visits. The standalone Divider census also succeeds with the
unchanged shared limits. Its candidates are not evidence of acceleration.

## Semantics and boundaries

Inputs may be held stable during an admitted frame. In the experimental
idealized mode, internal timing may be ignored without a timing proof, and a
timing-dependent native output may intentionally differ from the chosen Boolean
result. Report that compatibility loss rather than claiming native equivalence.
Explicitly represent source transitions: a wire may fall and rise before the
cut, and an unpowered piston acts only if it receives a request. Final power
values alone are not a transition. Do not infer that a piston fires solely
because it is eligible.

The compiler must support legal timing-sensitive instant logic, not classify
it as broken just because one path arrives before another. Relative piston
steps can make a path behave as a buffer or an inverter depending on which
input arrives first. Derive arrival events from actual source-qualified routes,
guards and resource/reset behavior; piston count can be a useful clue, not a
general replacement for those dependencies. A bounded device may lower such a
pattern to a Boolean transition under the mode's stated assumptions. The user
permits an idealized experimental mode to skip a native timing proof, so the
result can differ from native when arrival order changes behavior. The observed
FPU fall/rise trace shows that event timing matters, but does not identify a
specific buffer or NOT gate or establish a physical defect in the corrected FPU.

### Experimental gate assumptions

An experimental fast mode may recognize a bounded physical pattern as a NOT
or buffer and compile the corresponding Boolean function under explicitly
reported assumptions about its inputs, reset, ownership and native effects,
without proving the native nanotick arrival order.
This is a compatibility tradeoff: visual similarity alone does not establish
the function when another input, arrival order or shared payload can affect it.
Test each supported pattern against its intended Boolean behavior and compare
native redstone, including both input orders where inputs are independent; a
native timing mismatch is an allowed, reported outcome in this mode. Unsupported
or conflicting non-timing cases remain Graph-owned. A passing sample does not
establish correctness for other states or patterns, and this research baseline
does not enable such a mode. Preserve native continuation at any offered handoff,
or explicitly restrict it until materialization is supported.

Keep these relationships distinct:

* **Read:** a region reads a native or compiled source. A read does not transfer
  ownership. Track changes at a supported boundary; otherwise decline that cut.
* **Event:** an original source occurrence can trigger an update, including its
  guard and live or captured receiver. Retain source identity and order where
  they affect observable behavior or future state.
* **Ownership:** a region exclusively updates its own actor, resource, wire and
  scheduled state. Every possible writer of a movable payload belongs to the
  same ownership group. An unsupported co-owner leaves that group native.
* **Effect:** an event or value crosses into Graph or an observable output.
  Native neighbors, lamps, repeaters and command blocks keep their effects;
  terminal dust is not automatically an output merely because it is a terminal.

Independent devices and clocks stay separate until a real mutable dependency
requires joining them. Unsupported work remains native without vetoing other
independent candidates. A boundary that cannot be represented safely remains
Graph-owned; do not split an already owned resource or discard pending work.

## Passes and artifacts

Each pass is separately inspectable, bounded, cancellable, and accepts a
validated artifact from the previous pass. Reuse `CompileGraph`, its original
routes, power links, guards, contacts and profiles, `BoundaryCatalog`, and the
existing region fans. Avoid a second general-purpose graph representation.

1. **Prepare.** Build the ordinary Graph geometry, electrical links, callbacks
   and imported pending work. Validate before optimization. This is the native
   fallback and the source of all later evidence.
2. **Discover and classify.** Reuse instant cells, resource owners, wire
   corridors and source-qualified activation occurrences. Record declines with
   location and reason. A recognized cell is a candidate, not an executable
   device.
3. **Grow and partition.** Start from real input/event sources and follow
   supported effects. Join complete shared ownership groups and supported wire
   dependencies; track external reads as ports rather than claiming their
   sources. Merge connected candidates only when mutable state or an internal
   event dependency requires it. Keep uncertain crossings native. Identify
   true observable and native-effect outputs and exclude dead terminal nets
   only when they have no relevant consumers.
4. **Derive transitions.** Compile source-qualified rise/fall requests, wire
   strength and geometry dependencies, guarded/captured receivers, payload
   alternatives and observer/reset effects. A signal can change more than once
   per frame; preserve the cuts needed by later samples. Reuse native probing
   in an isolated scratch world as evidence for links and order, never as proof
   outside its stated state/input domain. Enumerate only bounded local state;
   do not enumerate all 35 FPU controls or expand an unbounded symbolic FIFO.
5. **Certify.** Check complete writers, guard and receiver dependencies,
   external ingress/egress, interacting sources, cycles, resource arbitration,
   pending work and handoff. Experimental idealized mode does not require a
   native timing proof; it must identify that assumption and must not present
   its result as native-equivalent. A candidate may use a stronger, narrow
   contract, such as a quiescent entry or static geometry, if admission checks
   it. Reject unsupported non-timing cases locally, with a concrete reason;
   sampled histories are not a general proof.
6. **Lower and execute.** Compile certified domains to compact indexed
   actor/resource/signal state and source-to-consumer links, with explicit
   events and native effect ports. Batch or reduce independent work when proven
   safe. A future executor must actually replace internal Graph callbacks and
   piston work, refresh native reads at the right cuts, and materialize state
   and pending work at a permitted handoff. On an unexpected condition it must
   either hand back a complete state or decline before owning the region.
7. **Explain and measure.** Surface representative clickable block locations
   and short reasons. Separately report discovered, declined, installed and
   actually executed work. Record domain sizes, compile work/memory/time,
   callbacks removed, native effects, frame evaluations and throughput.

## Passes excluded from the research baseline

The baseline does not run old instant-region/proxy builders, event/PullWave
kernels, transport, signal-channel, value-region, stable-equation, settled
device, BUD-owner or CPU-kernel builders. It also skips Graph optimization
rewrites and classic optimizers; ordinary Graph execution remains available.
They are excluded only from this isolated baseline, not deleted or globally
disabled. Existing O6 Counter depends on boundary/BUD/settled machinery; do not
remove it until a replacement is measured and verified. Reintroduce a pass to
the research pipeline only when a concrete consumer and correctness test show
it is needed.

## Implementation gates

1. **Baseline:** the research pipeline discovers a real cell, installs no
   kernel or logical device, and its Graph execution matches native with reset.
2. **Event and ownership:** independent and connected devices, changing inputs,
   two sources, shared payload, separate clocks, unsupported neighbor, captured
   receivers and native observer reset. Include a timing-dependent buffer/NOT
   fixture with both arrival orders and unequal piston depths; compare settled
   outputs and native continuation. Check each pass's artifact and fallback.
3. **Actual execution:** a connected physical circuit must execute a compiled
   domain and eliminate substantial internal callbacks, with correct settled
   outputs, outward effects, reset and native continuation. A shadow comparison
   or generated plan alone does not satisfy this gate.
4. **Counter and adders:** verify Counter outputs, carry, start/stop and
   continuation; measure at least 1M game TPS. Check one-bit and multi-bit
   adders with changing operands and an unsupported adjacent region.
5. **Saved FPU and Divider:** use the corrected `FPU_27` plot, not only a
   standalone schematic. Build/pin a scale-5 replay; the workspace now defaults
   to `PLOT_SCALE=5`. Exercise changing and random inputs, 16
   repeater outputs, 35 lamps, command messages with nonzero observed output,
   and permitted handoff followed by native continuation. Record actual executed
   domains and work removed, compile budget/time, input and executable hashes,
   and at least three comparable server throughput samples.

The saved FPU's last validated random replay still admitted zero logical
devices and processed 13,321,604 callbacks in 1,152 ticks. The 1,021-region
event prototype passed one witness but was slower than its matched
instrumented Graph control and removed only 4.1% of callbacks. These are
reasons to require actual execution and large work reduction, not targets that
can be satisfied by recognizing cells or installing plans.
