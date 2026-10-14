# CPU_BubbleSort Graph inference profile — 2026-10-14

Capture: `target/cpu-bubblesort-current-o3.svg`; workload/checks: `target/cpu-bubblesort-current-o3.json`; extracted frame analysis: `target/cpu-bubblesort-current-o3-hotspots.json`.

Command: `cargo flamegraph -p mchprs_core --bench cpus --profile profiling --output target/cpu-bubblesort-current-o3.svg -- --cpu cpu_bubblesort --graph-short --graph-tier 3 --iterations 1 --output target/cpu-bubblesort-current-o3.json`.

The Windows sampler required elevation. The capture completed successfully. The existing harness passed its CPU RAM, reset and continuation comparisons against the interpreter, using the same in-memory sign removal. No source schematic or runtime code was modified.

## Inference costs

Only descendants of GraphBackend::tick_phases are counted here: 10,995 samples. Preparation, projection outside ticking and interpreter samples are excluded. Percentages are calculated from SVG frame intervals; recursive/nested occurrences of the same category are unioned rather than added. Results describe sampled stacks, not exact per-function timers.

| Trigger phase | Tick samples |
|---|---:|
| Piston action draining | 57.6% |
| Deferred piston publication | 37.3% |
| Scheduled tick draining | 5.1% |

| Nested work, inclusive | Tick samples |
|---|---:|
| General Turbo walk | 66.3% |
| Component input-strength queries | 23.9% |
| Lightweight site resolution | 15.6% |
| Full site resolution | 14.3% |
| Hashbrown operations | 11.6% |
| Recursive guard evaluation | 7.4% |
| WireWalk frames | 7.5% |
| Turbo callback selection | 5.5% |

These nested rows overlap each other and the trigger phases. Piston work is expensive mainly because it causes routes and wire propagation; 57.6% piston work and 66.3% Turbo work are not separate costs.

Largest individual self-time frames: lightweight site resolution 11.4%, full site resolution 9.4%, recursive guards 5.0%, query_power 4.6%, propagate_wire_changes 4.6%, hash-table find 4.3%, deliver_route_inner 4.0%, identify_wire_neighbors 3.8%, site_power 3.6%, capture_walk_site 2.9%. Inlining and sampling affect attribution.

## Implications

The principal target remains general Turbo propagation, particularly repeated site resolution, input/power queries, walk discovery and callback-driven reentry. Indexing delivery rows removes only one part of that work. Guard simplification also addresses a smaller fraction than the full propagation path.

The next substantial optimization should certify and replace complete private instant/reset regions or private dust episodes, retaining boundary behavior while eliminating their internal walks. A lower-risk runtime experiment can cache immutable site properties and reuse walk-local resolution where captured/live semantics permit; it must not share mutable observations across nested walks. The profile does not prove that any callback is redundant.

The single profiled sample took 1.436 seconds for Graph and 0.892 seconds for interpreter over 100 active ticks. These are instrumented run observations, not a speedup benchmark or a tier comparison. The capture includes additional Graph ticks during continuation checks; frame percentages therefore cover the bounded harness's Graph tick stacks, not exclusively its timed 100-tick window.

Not checked: repeat captures, a full CPU sort, per-tier comparison, or broad tests. Compilation dominates the complete SVG; zoom into GraphBackend::tick_phases before interpreting runtime costs.
