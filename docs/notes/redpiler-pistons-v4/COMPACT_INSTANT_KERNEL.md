# Compact instant kernel

The O4-and-higher `--stable-instant-inputs` path now lowers a bounded whole-selection family to a shared effect/phase program. The old full-state transition table is retained only as compile-time proof data. `CircuitProgram` stores the compact kernel; `GraphBackend` owns its live control mask, phase/cycle position and required revision origins.

## Derivation and admission

The compiler uses graph ownership and observer analysis to require a closed selected inventory with supported node and host behavior. It tries graph-derived initially powered controls as falling-edge triggers and admits exactly one trigger whose entry proof succeeds. The actual falling-edge callback runs through ordinary Graph first; the runtime then checks the complete post-activation state against a synthesized canonical entry relation before entering the kernel.

For each admitted trigger, the compiler prepares each valuation of up to three remaining controls, using both forward and reverse control order from the fixed initial graph. This is at most eight data valuations. Each must reach the same certified quiescent entry family. Held controls are then evaluated by the ordinary ordered Graph executor until an exact normalized state repeats. The finite prefix and repeated cycle are synthesized into a hash-consed constant/Select expression DAG. Ordered boundary events use guarded occurrences so each valuation preserves duplicates and causal order. An incomplete, ambiguous or over-budget proof declines optimization and keeps ordinary Graph execution.

This finite enumeration is restricted to the small first family. It is not a strategy for ADDER11BIT or larger regions. Those remain on the general Graph path; the compiler does not increase the control limit or enumerate large operand spaces.

## Runtime and observation

On hot ticks, the kernel evaluates only the roots needed for selected Electrical component outputs, dirty-node deltas, ordered presentation effects and logical counters. The selected graph inventory is closed, so internal wire and piston callbacks have no independent runtime reader after their output effects have been compiled. Private wire, piston, resource, revision and scheduler values are cold reconstruction roots, evaluated for projection, inspection, flush, fallback or interpreter handoff.

Every external control change deoptimizes before its effects: the backend reconstructs the exact current `CircuitState` and future work, then lets ordinary Graph handle the action. The same reconstruction supports reset cuts, native continuation and direct projection. Dirty bits and queued presentation are append-only; hot phases OR and append their exact deltas to the live buffers. Near clock overflow the backend deoptimizes before deadlines stop being representable.

The promise applies only under the existing opt-in stable-input protocol, from the activating falling edge until certified reset. It does not reorder internal carry, reset or inhibition propagation. Graph-derived admission, the complete Electrical output set, guarded event order, and cold full-state/future reconstruction are validated before lowering. The full graph remains available as the fallback executor.

## Equivalence argument

For each frozen input assignment, the compiler records a deterministic ordered Graph trajectory until its complete normalized semantic state repeats. This includes private component values, actor and payload state, FIFO requests, relative scheduled deadlines and deferred revision witnesses. A matching repeated state therefore has the same next transition while the external inputs remain held. The common phase period is a bounded least common multiple of those individual periods; it preserves each trajectory rather than imposing a new propagation order.

The runtime's admission check establishes the entry relation. Each phase selects the recorded Electrical output changes and appends the exact dirty and presentation deltas; its cold expressions describe the complete successor state and future. This maintains the relation by induction for every subsequent held-input tick, including repeated cycles. Independent revision origins and the absolute clock are reconstructed using wrapping revision increments and relative deadlines; execution leaves the kernel before the clock boundary. A control change materializes the related full state before ordinary callbacks run.

Validation rejects malformed expression references and cycles before evaluation, then rederives the canonical compact program from the graph. Differential tests additionally compare the compiler's Graph evaluator with the interpreter. The argument depends on the admitted closed inventory and the executor's origin normalization; it does not justify substituting this kernel into a region with unproved intermediate readers or host effects.

## Current size and limits

The ADDER1BIT kernel has 17 phases and 584 shared expressions, with an estimated 34,100 bytes for the compact runtime kernel. The compile-time certificate contains 292 normalized states and 365 transitions (847,848 estimated bytes including the kernel). These are allocation estimates, not peak RSS; the lowered graph also retains the ordinary Graph fallback. Kernel tick benchmarks time tick calls and exclude flush, while flush currently materializes private state even for `io_only`; they do not establish whole-server throughput.

The compact kernel is certified for all eight prepared ADDER1BIT operand cases and tested against complete projected state/future behavior, reset cuts and interpreter continuation. Independent projection checks compare raw live Electrical slots and exercise repeated full/`io_only` flushes and active-control deoptimization. The old 13.24× snapshot-prototype benchmark in [CLOSED_INSTANT_MACHINE.md](CLOSED_INSTANT_MACHINE.md) is historical and does not measure the compact representation. A paired compact-kernel benchmark is tracked separately.

ADDER11BIT remains general execution: 1,083 nodes and 22 data controls plus a trigger exceed this compiler's explicit bounds. The next scaling work needs a structural compact derivation for reusable subregions and a causal composition proof; simply enlarging the finite control enumeration is not sound or practical.

## Release measurements and verification (2026-10-15)

Five alternating before/after pairs used the pinned snapshot executable and the compact executable on the same prepared ADDER1BIT carry case. Each fresh-world sample timed 512 `tick_with_world` calls with inputs held. Compilation, preparation, the ordinary activating falling edge, publication, observation, reset and interpreter continuation were outside the timer. Every comparison passed the harness's output, projected memory, pending-work and continuation checks.

| Execution | Median for 512 ticks | Range |
| --- | ---: | ---: |
| Previous snapshot kernel (historical O3) | 0.9691 ms | 0.9443–1.0699 ms |
| Compact kernel (O4 with stable inputs) | 0.0779 ms | 0.0756–0.1283 ms |
| Ordinary O4, in the new executable | 13.1419 ms | 12.9594–13.6643 ms |
| Interpreter, in the new-executable pairs | 13.4572 ms | 13.2060–14.1029 ms |

The compact/snapshot ratio of medians is **12.44×**, and all five pairs favored the compact kernel. The ordinary-O4/compact ratio is **168.7× on this held-input tick workload**. The previous executable selects its snapshot pass at its historical O3 tier; the new executable selects the corresponding compact pass at O4. These measurements do not establish arithmetic transaction latency, repeated-calculation throughput or server TPS: activation and flush still have costs, and the timed interval extends beyond the initial arithmetic sampling window.

Both kernels recorded exactly 512 accelerated ticks, 935 logical scheduled dispatches, 1,197 accepted actions and 1,190 deferred publications during the timed interval, with zero actual Turbo walks, callbacks or piston samples. Estimated retained lowered tables, including fallback, fell from **908,644 to 129,060 bytes (85.8% less)**. The compact kernel itself accounts for 34,100 bytes. These are retained allocation estimates, not RSS or peak memory measurements.

Five ADDER11BIT samples also passed the full benchmark comparisons at O4. Median times were ordinary O4 41.1594 ms, stable O4 40.9652 ms and interpreter 42.7317 ms per 512 ticks. Both Graph modes recorded zero compact-kernel hits; this confirms fallback behavior, not an eleven-bit kernel speedup. Stable O4 still executed 192,521 callbacks, 2,651 walks and 50,413 piston samples.

Three fresh 100-tick CPU_BubbleSort pairs passed per-tick RAM, reset and continuation checks at O4. Graph median was 0.697671 s [0.687911–0.702692 s]; interpreter median was 0.634609 s [0.606261–0.669476 s]. All recorded zero kernel hits. Graph work remained 20,005 scheduled ticks, 1,703,695 callbacks, 32,505 wire walks, 302,784 piston samples and 27,884 actions/publications. These are fallback measurements, not evidence of a CPU kernel speedup. The existing identical in-memory sign removal was used for both executors; source schematics were unchanged.

Divider passed three fresh O4/stable-O4/interpreter comparisons. Each exercised four 128-tick trigger phases, checked every output sample, required the prepared OFF response 170 and ON reset 0, and compared reset/future work plus 32 interpreter continuation ticks. The graph has 16,408 nodes and 2,055 actors; both Graph modes retained general execution with zero kernel hits. Median times per 512 ticks were O4 0.305102 s, stable O4 0.306023 s and interpreter 0.253057 s.

The BUD counter passed three fresh O4/interpreter comparisons over 1,800 active ticks after warm-up. The complete 16-bit consumer-bank waveform matched, including 309 distinct sampled words in this protocol, followed by reset/future and continuation checks. Its graph has 1,737 nodes and 163 actors and retained general execution, with zero kernel hits. Median times were Graph 0.629467 s and interpreter 0.753896 s. This does not prove all possible counter input histories or encode it as a compact BUD-memory kernel.

The explicit full CPU validity test also passed at O4: **10,035 active ticks**, final sorted RAM **0..15**, and RAM/register observations matching the interpreter at ticks 0, 10, 100, 1,000, 5,000, 10,000 and 10,035. After Graph reset, all 32 subsequent interpreted observations matched too. This is a test-build correctness run, not a production benchmark. It used the same sign-free copies as the short benchmark and recorded zero kernel hits. The full run discharged 2,344,242 scheduled ticks and executed 199,211,514 callbacks, 3,882,000 walks, 35,541,662 piston samples, 3,295,083 actions and 3,295,071 publications. Handoff occurs at the recorded boundary and is checked by the subsequent interpreted observations.

Final verification passed:

- Kernel implementation suites in debug and release: **343 passed, 8 ignored** in each, including all eight prepared one-bit operands, the existing eleven-bit vectors, malformed-program checks and arbitrary control histories. After moving compact compilation to O4, the debug suite again passed **343 with 8 ignored**, and the release benchmark exercised the new O4 tier. The full release unit suite was not repeated for the tier-only change.
- Tier selection checks cover O3 exclusion, O4 and higher inclusion, command parsing/completion and O4 JSON fixture generation.
- The separately added ignored full-sort test passed explicitly; adding this opt-in case raises the default ignored-test count by one. Its shared helper also retains the existing O0 100-tick test.
- Independent projection checks: repeated full/IO-only/double flush, raw live Electrical outputs, input changes during active execution, independent wrapping revision origins, shifted clocks and interpreter continuation.
- Core tests/benches checked with `graph-trace`; server checked with ordinary features.
- Python loader **14/14** and viewer **22/22**, including malformed required fields, ID namespaces and oversized decimal constants.
- Focused Rust formatting and `git diff --check`.

Reproduction commands:

```powershell
cargo test -p mchprs_core --lib redpiler:: --locked -- --test-threads=1
cargo test -p mchprs_core --lib redpiler:: --release --locked -- --test-threads=1
cargo check -p mchprs_core --tests --benches --features graph-trace --locked
cargo check -p mroww --locked
cargo bench -p mchprs_core --bench cpus --locked --no-run
& ./target/compact-kernel-bench/run.ps1
py -3 target/compact-kernel-bench/summarize.py
cargo test -p mchprs_core --lib cpu_bubblesort_o4_completes_sort_and_preserves_interpreter_continuation --locked -- --ignored --test-threads=1 --nocapture
& ./target/compact-kernel-bench/cpus.exe --graph-short --cpu fpu_divider --graph-tier 4 --stable-instant-inputs --iterations 3 --output target/compact-kernel-bench/divider-o4.json
& ./target/compact-kernel-bench/cpus.exe --graph-short --cpu counter_basic --graph-tier 4 --iterations 3 --output target/compact-kernel-bench/counter-o4.json
& ./target/compact-kernel-bench/cpus.exe --graph-short --cpu cpu_bubblesort --graph-tier 4 --iterations 3 --output target/compact-kernel-bench/cpu-o4.json
```

The alternating script, pinned executables, individual JSON reports and summary are local artifacts in `target/compact-kernel-bench/`; the previous executable remains in `target/closed-instant-bench/`. The script selects the old executable's O3 tier and the new executable's O4 tier. Compact executable SHA-256: `9af53977dccb5c3d4f6c418ed9b5ccc00f609229b168f99935b1759df02df111`. Previous executable SHA-256: `02f541d7e35b747f855445211f944e831a17e31ec4ad4bc0973d4ea00db1acdf`.

Not run: full-workspace tests, new sampled profiling, broad fuzzing, peak-RSS measurement or larger-region kernel extraction. Source schematics and Cargo dependencies were preserved; the O4 exported-graph fixture was added. Changes are uncommitted.
