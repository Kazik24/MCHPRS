# Classic redstone implementation progress

## 2026-10-16: read-only discovery started

The existing graph has no committed-signal net implementation. The current
`contract-static-dust-paths` pass retains Turbo, intermediate wire writes and
callbacks, and therefore is not the requested optimization. Historical
`7470205` DirectBackend packed a node, channel and attenuation into each forward
link; that representation is useful but supplies no current callback-order proof.

The first census runs on the complete prepared graph, before changing execution.
Potential electrical wire relations define whole regions, independently of the
saved contact choice. Discovery follows conducted power to component leaves and
inventories canonical callback-only wire neighbors, variable ingresses, receiver
inputs, observers and synchronous boundary targets. Decline counts are
overlapping, so one candidate can explain more than one failed condition.

`classic_discovery_census` is an explicitly ignored test; its output is
`target/classic-discovery.json`. Structural candidate counts are not accepted
coverage: saved geometry/strengths, imported work and exact boundary effects must
still be certified. No runtime pass, proof bound or default enablement is chosen
from these preliminary counts.

The intended runtime state is one committed four-bit signal per accepted net,
updated at its original source-route entry occurrence. Retained tables must bind
that occurrence, owned wire IDs with attenuation, complete receiver inputs and
ordered effective scheduling operations. The boundary proof determines the
operation operands; the implementation must not infer correctness from final
strengths or device delays.

## 2026-10-16: opt-in implementation and admission contract

The pass is registered after preparation and before instant plans at O2+, behind
`CompilerOptions.classic_redstone` / `--classic-redstone`. It remains disabled by
default. It owns complete fixed wire regions, never a convenient subset. Local
pistons elsewhere do not reject a region; observer/contact/independent-input or
private-work boundaries do. Existing dust and instant ownership cannot overlap.

The compact certificate retains source, owned wire attenuations, complete direct
receiver inputs, and route-delivery ordinal bindings. Dense lowering produces
input/op ranges and one byte of committed signal per net. Each retained op holds
one receiver, sampled nonzero polarity, and a 256-bit old/new selection mask
(index `(old << 4) | new`). These masks select an ordered operation stream; they
are not 256 retained execution transcripts. Runtime executes those operations,
preserves live receiver powered/pending state and the existing scheduler, and
performs no private Turbo walk, callback replay or intermediate dust write.

Receiver queries combine committed signal attenuation with any proved direct
live ingress contribution. Wire blocks, inspection, flush/reset and continuation
reconstruct private strengths on demand. Private scalar storage intentionally
remains stale; native comparison normalizes that storage through the same public
projection. Export JSON adds typed net ownership, inputs, occurrence bindings,
compact masks and execution/proof edge markers. The viewer displays direct net
edges while retaining inspectable proof topology.

Admission does not infer that a delayed receiver sees only final wire power.
A cold Graph reference recorder checks all 256 ingress old/new strength pairs,
including each meaningful receiver sample and its order. Powered state cannot
change during the admitted synchronous episode; pending membership can only
become present, so first samples of each polarity suffice for all receiver
powered/pending states. Independent validation replays the supplied ordered
certificate against fresh samples rather than invoking the builder's compaction.
Cancellation is checked during proof and validation. Repeated graph validation
repeats this finite proof; it is a deliberate compile-time cost, not a cached
unchecked certificate.

The source output route must reach its first Live private Update through a
prefix containing only fixed/inert shape handlers and ordinary inert/delayed
handlers. Foreign wire propagation and other potentially synchronous emitters
before that occurrence decline. Only this first occurrence commits. Duplicate
later source occurrences and external ordinary output-event occurrences are
proved inert for all 16 stable signal strengths. Their closed origin plus the
non-reentrant prefix closes the interval where source newest and net committed
signals differ: pending jobs do not run inside synchronous route dispatch.
Ingress repeaters additionally require a constant-zero side query and saved
unlocked state, excluding silent locking output changes. Captured Site guards
without matching captured context are false; captured external wire entries
are unsupported. Shape entries do not commit signals.

Bounds were selected after the structural census: 32 wires per complete region,
2,000,000 reference-work units per attempted net, and 32,000,000 cumulative units
including declined proofs. The largest preliminary ANPU structural candidate
was 15 wires. Exceeding a bound leaves the whole region on general execution.

## 2026-10-16: measured coverage and acceptance limits

The initial prepared ANPU census found 34 structural candidates containing 381
wires and 35 receivers among 34,781 wires / 2,971 electrical regions. These were
not certified coverage. Independent upstream inputs at boundary callbacks are
common: downstream wire notifications also update a driving torch/repeater whose
own input depends on an unrelated signal. Such a receiver has two independently
variable inputs and cannot be represented by this first family's one-ingress
query. The implementation declines it; no ANPU speedup or general usefulness is
claimed. The pinned graph fixture `simple_and_gate` supplies a real accepted net.

The ordinary component census was actually executed with the test-only prepare
hook: 93 passed / 1 ignored, observing 134 preparations across 87 test names,
283 wire occurrences / 63 regions and 28 preliminary structural candidates.
These are repeated prepared-graph observations, not unique fixture coverage;
manual draft-only tests do not enter that hook. Raw records are in
`target/classic-ordinary-component-census-20261016.jsonl`.

Focused acceptance covers lengths 1/2/4/8/16, bent branches and fanout, delayed
torch feedback, powered/pending receiver combinations with imported ticks,
short/repeated pulses, FIFO/deadline/priority equality, projection/reset and
interpreter continuation cuts. Negative cases cover observers, contacts,
locking, independent upstream inputs, callback-only wires, stale strengths,
private imported work, captured guards, bounds and corrupt certificates.
The positive JSON fixture is exercised by 15 Python loader tests and 23 viewer
JS tests. Parent independently ran the ignored native DLL classic+O3 test with
Rust query/state verification successfully; its extended version additionally
checks code-cache sharing with independent mutable engine state.

The first paired release measurement on `simple_and_gate` used five alternating
pairs, each 2,000 lever interactions plus two ticks per interaction. Setup/proof
and verification were outside inference timing. Median O2 was 6.1823 ms,
classic 4.6286 ms, interpreter 7.0160 ms. This is a small real positive fixture,
not ANPU evidence. That run preceded the final stable-entry refinement and must
be refreshed before treating its exact counts/timing as final acceptance.

An attempted broad pre-existing experimental suite failed
`compiled_adder_regions_replace_internal_propagation_and_preserve_outputs`
(expected symbolic coverage 1, actual 0 under the existing default-disabled
synthesis path), then spent over five minutes in the explicit arithmetic-history
synthesis test. At the parent's instruction that process was stopped after
preserving output. The unrelated experimental failure was not rewritten.

## 2026-10-16: final conservative admission rerun

The refreshed complete census still accepts only one graph-fixture net and no
ANPU net. ANPU exact first-decline counts are: external/callback-only private
wire 2,372; contact-dependent geometry/power 357; wire bound 155; independent
main input 56; locked saved repeater 10; ingress count 9; potentially reentrant
source prefix 7; side/quasi reads 3; stale saved geometry 2. No reference pair
work is needed after these structural declines. Preparation plus census/proof
reporting took 28.99 seconds for ANPU in the debug test executable.

A concrete positional independent-input witness is the one-wire candidate at
(42,16,12), ingress (41,17,12), receiver (42,15,13). Private callbacks also reach
the ingress receiver, whose main query reads unrelated component (40,17,12).
The exact query cannot be encoded by one ingress signal. The candidate report
retains all source positions in `target/classic-discovery.json`.

After the stable-entry change, 10 focused tests passed (2 ignored) and the
ordinary Graph namespace passed 95 tests (1 ignored). This includes the existing
fixed-point adder FIFO counterexample. The latest debug real-fixture rerun also
passed, regenerating the positive export. Its exact work counts are 20,000 to
16,000 callbacks, 2,000 to 1,000 walks, 1,000 classic entry invocations and 1,000
receiver ops per sample. The accepted direct program uses 584 bytes and one
state byte. The remaining walks belong to an unowned region. Debug preparation
pipeline times were 0.351 ms O2 and 5.106 ms with the pass, including repeated
independent certificate validation; these are compile costs, not inference.
The debug timing output is explicitly labeled `profile: debug` and preserved in
`target/classic-real-fixture-debug.json`. The old release timing above is not
silently relabeled as a measurement of the final code.

## 2026-10-16: final release measurement and DLL integration

The final release executable was rebuilt after the conservative stable-entry
change. Five alternating O2/classic pairs on the pinned `simple_and_gate`
fixture passed final block and ordered-future checks. Per 2,000 interactions
(two ticks each), medians were O2 **6.5703 ms**, classic **4.9148 ms**, interpreter
**7.1444 ms**: classic used 25.20% less inference time than O2 (1.337x ratio).
Raw evidence is `target/classic-real-fixture-release-latest.json`; it is explicitly
labeled release. Exact counts/storage remain those reported in the final debug
rerun above: 20,000→16,000 callbacks; 2,000→1,000 walks; 1,000 classic entries
and receiver ops; 584 direct-program bytes plus one state byte. Repeated
certificate checking raises this tiny fixture's release pipeline time from
0.2732 ms to 4.1186 ms. This compile/inference tradeoff and one positive fixture
do not justify default enablement or an ANPU performance claim.

The current native-demo DLL integration also passed (1 test, 0.63 seconds),
including specialized query verification, O3/classic coexistence, reset and
interpreter continuation, and a second backend hitting the same immutable code
cache without sharing mutable state. The final positive export passed Python
15/15 and viewer 23/23 again. `git diff --check` passed.

Broader targeted checks passed 13 program-graph tests and 74 pass tests; the
remaining pass test exposed its expected default pipeline list missing the new
disabled classic registration. That test-only expectation is now corrected;
it must be rerun in the parent's next debug test cycle. A separate before/after
measurement isolating the new default-disabled hot-path branches was not made.
The pass remains opt-in while broader coverage/performance work is outstanding.
## 2026-10-16: parent follow-up checks complete

The corrected `optimization_levels_select_rewrites_before_dependent_plan_builders` test passed in the parent's feature-enabled debug test binary. The final native classic/cache test passed again after the native guard/context and capture-index work (0.65 s), and `cargo check --workspace --locked` passed including the server and feature-disabled core. These supersede the earlier pending pipeline-list rerun. The C BubbleSort experiment independently completed all 10,036 native shadow ticks plus its paired full sort; it accepted zero classic nets, so its native throughput gain is separate from this pass's small-fixture gain. Native performance details and remaining limits are in `LLVM_NATIVE_INFERENCE_DEMO.md`.
