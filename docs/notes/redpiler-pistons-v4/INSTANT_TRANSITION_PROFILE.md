# Minimized transition inference profile, 2026-10-20

The minimized O5 stable-input BubbleSort program now spends substantial time
outside its remaining exact wire walks. Compile pure boundary guards into the
existing compact Boolean representation before another broad propagation change.
Whole-circuit replacement remains the goal; this profile identifies retained
general graph work that the next compiled component must remove.

## Evidence and scope

Release O3 fat LTO with debug symbols, frozen executable/PDB, the same complete
10,036-tick CPU workload as the production measurements. Samply/ETW samples the
main thread at 1 kHz. The inference callsite is `cpu_bubblesort.rs:178`;
compilation, preparation, projection and RAM probes are excluded by stack/callsite
classification. There are 50,004 inference samples, weighted by positive thread
CPU deltas, totaling 36.916 sampled CPU seconds. ETW reports zero lost events
and buffers. All 10,831 application addresses resolve, 4,564 with inline chains.

The profiled run matches the minimized production executable's RAM hashes,
sorted output, active/reset/continuation checkpoints and all 27 exported work
counters. Profiling elapsed time is not a production TPS measurement. Other
desktop applications remain active on the host.

Artifacts: `target/profiles/transition-inference/`, including the frozen EXE/PDB,
`record.json`, `provenance.json`, `profile.kernel.etl`, `profile.json.gz`,
`symbols.json`, `hotspots.json`, `inference.folded`, and validation output
`episode.json`. The recorder omits unstable presymbolication; symbols are
resolved directly from the frozen matching PDB with inline frames.

## Exclusive partition

| Execution region | Inference CPU samples |
|---|---:|
| Other graph execution, including ordinary routes and commits | 44.42% |
| Stable-input logic | 19.71% |
| Shared compiled effect replay | 17.70% |
| Remaining exact dense transport | 10.47% |
| Transition entry checks | 7.70% |
| Separately visible signal-channel wrappers | 0.003% |

The shared replay helper executes both adaptive transitions and channels;
some thin wrappers disappear from PDB call chains. These are stack-defined
regions, not exact accounting by compiler pass. In particular, the inclusive
`run_instant_transport` parent includes replay and entry checks; its 35.45%
must not be called remaining dense-wire execution. Missing wrapper samples
also do not prove a wrapper's work is free.

## Retained costs

The nearest Graph owner partition assigns 13.83% exclusively to
`guard_matches_using`, 9.32% to `deliver_route_inner`, 5.34% to
`drive_condition`, 4.64% to `transition_entry`, 3.91% to `update_component`,
and 3.74% to `capture_transport_site`. Owner attribution includes work inlined
into those functions; it is not just function prologue/dispatch cost.

Overlapping inclusive groups give more context: general guard evaluation
18.96%, external capture 14.54%, live piston sampling 11.24%, compiled piston
predicates 5.87%, generic boundary dispatch 0.50%, request enqueue 1.09%,
pending-resource lookup 0.38%, and scheduler helpers 0.72%. Do not add these
percentages; their call trees overlap.

The largest remaining exact route region contains 8.15% of inference owned
by guard evaluation and 9.32% by the ordinary delivery loop. Replacing only
the generic piston dispatch adapter has weak justification. Reusing compact
typed predicates for boundary guards reaches route delivery, capture selectors,
replay admission and stable-input execution while preserving every sample cut.

## Next compiler component

Compile bounded pure guard predicates with the existing Boolean/DNF compiler,
preserving live state and supplied captures. Keep current CPU-predicate
semantics unchanged: a captured-site test without a capture remains false.
Unsupported or over-budget predicates retain the original evaluator, and any
fallback atom must call that original evaluator directly to avoid recursively
selecting its own compiled predicate. Validate the published plan independently
and benchmark against the frozen minimized transition executable.

A subsequent typed piston request operation can remove repeated samples after
the first qualifying request in a closed transport episode. This requires a
proof that actor/resource/deferred state and tick phase stay fixed until the
episode ends, plus fixed receiver identity and first-source preservation.
Changing wire power remains live. This stronger reduction should follow the
guard work rather than merely bypassing the inexpensive generic adapter.
