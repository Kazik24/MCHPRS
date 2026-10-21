# O6 logical device execution

Implementation scope and gates: [logical compiler plan](docs/notes/redpiler-pistons-v4/INSTANT_CIRCUIT_ACCELERATION_PLAN.md).

O6 automatically uses the valid-device contract. Preserve stable public outputs,
normal clock cadence, controls and persistent memory. Public values must match at
valid observable publication ticks. Transient glitches may disappear between
valid results. Internal previews, wire propagation and reset notifications may
disappear when they do not change required outputs or memory effects.
Retain logical write/update events separately from value changes: BUD memory can
react to an update even when its input level stays the same.
Preserve signal strength at ports whose outside consumers depend on it; use
Boolean values internally wherever the level cannot affect an observation.
Treat a shared redstone block as one logical pulled/unpulled bit with at most
five pull owners. Either pull request activates it; the winning piston and
physical destination choice do not belong to this O6 model.
O0–O5 retain the existing execution contract. No additional flag or input-case table.

- [x] Extract selected output and register roots into shared Boolean expressions.
- [x] Evaluate changed dependencies using cached current values and a dirty bitset.
- [x] Validate COUNTER's selected outputs/register equations against the full artifact.
- [x] Compile COUNTER timing into a small logical state machine with source-derived
  primitive delays; remove the remaining physical timing island from execution.
- [x] Execute actual O6 COUNTER with zero physical callbacks, wire walks, piston
  samples/actions and deferred publications; compare stable outputs through carries.
- [ ] Materialize logical registers and canonical internal state for interpreter
  handoff; verify normal stop/restart and native continuation.
- [ ] Deferred: COUNTER's added active-low observation bank returns to an idle
  zero pattern while O6 retains its last publication. The stored count is not
  reset; all six OFF cuts retain matching stored state and valid restart results.
  The user accepts this idle readout difference for now; it does not block FPU.
- [ ] Resolve FPU feedback into logical state/reset equations and fixed boundary
  geometry. Compile the 35 input bits and 16 far-side output bits into a kernel.
- [ ] Validate changing FPU outputs with the trigger held OFF against native runs
  at valid result-publication ticks. Diagnose raw tick differences separately;
  identify reset glitches from event state rather than assuming every `0xffff`
  word is an invalid result. Preserve the original valid-result cadence.
  Nonzero output-word checks alone do not establish strength-sensitive port parity.
- [ ] Bind BubbleSort's 768 RAM bits as explicit logical state and compile its
  output/next-state dependency cone. Preserve BUD update/write events even when
  their signal values are unchanged. Verify the full sort and interpreter handoff.
- [ ] Replace recursive source expansion with a bounded dependency worklist;
  simplify feedback before choosing state cuts, then lower consequential BUD
  samples, writes and delayed publication without internal physical callbacks.
- [ ] Execute a small ordinary BUD slice with zero internal physical work;
  preserve update-triggered writes, acceptance rechecks and observable tick timing.
- [ ] Compile ANPU with the same logical state/event executor; validate all
  5,000 screen/state ticks, consequential BUD transactions and native continuation.
- [x] Compile live Main/QC power predicates for every O6 piston actor in
  BubbleSort and ANPU, including ordinary memory. Keep request/acceptance cuts,
  scheduler order and same-value BUD notifications. Verify all 10,036 BubbleSort
  RAM ticks and handoff, and ANPU's screen and ordered BUD trace through 5,000 ticks.
  This replaces power-query traversal; whole-device wire/event contraction is
  separate work and must not be reported as complete from predicate coverage.
  Verified 16,561/16,561 BubbleSort actors (57,893 packed reads) and 2,898/2,898
  ANPU actors (6,288 reads), with zero whole-input fallback reads. BubbleSort
  matches every frozen RAM tick through 10,036 plus 32 interpreter handoff ticks;
  ANPU matches 14 screen frames and 1,552 ordered BUD update ticks through 5,000.
- [ ] Compare BUD trigger, QC sample and publication semantics in COUNTER,
  CPU_BUBBLESORT and ANPU before choosing the generic logical BUD operation.
  Source audits distinguish 16 coordinated COUNTER bits, 896 ANPU lower readers
  paired with upper storage pistons, and 768 BubbleSort RAM bits. Native requests
  sample power and acceptance rechecks it; focused raw traces must establish which
  cuts normal builds rely on before replacing them with logical events.
  Use `RILAX_MEMORY_BANK_BUD.schem` as the small ordinary-memory baseline:
  64 storage cells plus lower read gates, write sampling on both enable edges,
  and latched reads. Discover graph-derived update/sample events independently
  of instant-cell admission. Compare corresponding storage/read roles with ANPU;
  differing fixture observation labels do not establish different cell semantics.
- [ ] Generalize the same compiler/event executor across the library: track all
  96 schematic sources and duplicate aliases, plus saved worlds such as CHUNGUS.
  Distinguish structural extraction from functional native equivalence; missing
  port/protocol definitions must remain explicit coverage gaps.
- [ ] Replace the current qualified clock-family activation with graph-derived
  logical state/event operations after discussing newly found semantic blockers.
- [ ] Profile the resulting logical kernels; specialize or JIT only demonstrated
  expensive expression patterns, without storing whole input/state combinations.
- [ ] Measure production release game TPS: COUNTER >=1,000,000 and FPU >=1,000.
  Report whole-device rates separately from isolated kernel evaluations.

Current experiment: the one-BUD/one-output kernel and automatic O6 COUNTER
logical execution match native running cuts across 260 increments and tested
stop/restart memory cuts. The added bank's idle readout differs as accepted above.
The last broad Redpiler run passed 673 tests; workspace checking was
warning-free before the new focused BUD probes.
Three sustained six-million-game-tick runs measure 4.24M game TPS including
benchmark-loop overhead (4.97M tick-timer TPS), with zero physical update work.
This meets the COUNTER speed target for the tested normal clock family.
Handoff and FPU/BubbleSort whole-device execution remain unfinished.
The test-only FPU computation-frame extractor now folds observer/reset outputs
OFF before reduction, following the fully instant frame contract. The 353
undeclared observer inputs disappear: the closed candidate has 22,193 expressions,
53 control inputs and 15 feedback-state candidates. Fresh reset-frame evaluation
converges in one to four logical passes for all 16 held-input vectors and agrees
with the carried-feedback candidate. One vector still produces candidate 0x8446
while the native tail alternates 0x8000/0x8800 plus reset-like 0xffff pulses.
Other holds can also have multiple plateaus. The user explicitly accepts this
nonstable native behavior and the reported candidate difference for now; it does
not block FPU work. Preserve the exact vectors for reproduction rather than
requiring native parity on this case. Publication timing and production device
integration still need validation. Reproduction: `target/logical-inference/fpu-input-vectors.json`
and `.csv` (all 16 vectors, raw lever order and first-case positions).

Provisional release FPU kernel benchmark (Ryzen 9 5950X): three samples of
4,096 changing operands per raw selector pattern measure median 9,337–33,050
complete calculations/s across seven patterns with varying outputs. Frame reset,
fixed-point convergence and output collection are timed; physical simulation and
publication adapters are absent. Constant-output selector 111 is excluded from
that range. Cold fixture load is 0.155 s, graph preparation 6.978 s, and logical
extraction 0.701 s. This is candidate kernel throughput, not validated game TPS
or a correctness claim. See `target/logical-inference/fpu-kernel-benchmark.json`.
