# G5 — Ordinary circuit parity audit

Status (2026-10-13): the retained supported subset passed the focused differential gate in this batch; broader histories remain untested, and the result awaits human review. Read [the migration plan](../GRAPH_PIPELINE_MIGRATION.md) and existing reference-contract requirements.

## Goal

Establish differential evidence for ordinary behavior under the unified Graph executor. Backend identity is part of every claimed execution test. Record explicit unsupported components and untested histories.

## Cases

- Feedforward compare/subtract comparators with analog rear and side strengths, live eligible override sources, saved output state and same-value notifications.
- Shared-source forks/reconvergence, comparator feedback, delayed repeater/torch loops, oscillator restart and initially nonzero state. A directed cycle is not automatically an error or proof of eligibility.
- Dust decay/rise, branches, loops, ascending/descending connections, direct/conducted lamp power, stored-wire diode reads and regulated sides.
- Nested Turbo walks, synchronous component callbacks, observer-visible intermediate changes and callback duplicates/directions.
- Imported stale and matching ticks, deadline ties, priorities, pending-work multiplicity, and compilation with ordinary work already queued.
- Lever/button/plate controls, comparator/bulb state, the explicitly approved trapdoor admission policy and reset/continue with pending work.
- Batch 02 adds command impulse/repeating/conditional/chain behavior, synchronous result/comparator feedback and reset, plus note rising-edge/blocked/instrument/sound checks. Trapdoor compilation rejection is the specifically accepted regression and must preserve the input.
- Existing fixture family plus ordinary electrical portions of the adder schematics. Do not demand full FPU coverage solely for this migration gate.

## Implementation instructions

Reuse `analysis/tests/redstone_fuzz.rs`, graph regression helpers, the reference-contract comparator and pinned fixture loader. Tests assert Graph identity for public compilation and compare against independently stepped interpreter episodes. Keep future coverage incremental; passing focused histories do not establish full CPU or all-world parity.

Preserve first-divergence reporting. Compare eligible callback order, component/resource state, pending work and reset continuation at appropriate boundaries. Revision IDs need not numerically match the interpreter; cancellation validity must match. Normalize only documented unobservable records. Same settled outputs are insufficient.

## Review evidence

- Matrix of cases attempted, admitted/executed, rejected, divergent and incomplete, with explicit backend identity.
- Minimal regression for each confirmed bug; changes must fix the shared semantic owner, not a fixture-specific path.
- Debug and release runs of new focused regressions, relevant existing parser/executor suites, formatting and diff checks.
- One ordered trace explaining a comparator/feedback interaction and one exact handoff comparison.

## Verification and remaining matrix

The 202 discovered redpiler tests cover imported tick deadlines/duplicates, comparator feedback from a synchronous command result, note-block edge behavior, command host errors, selected Hopper/trapdoor rejection, ordinary feedback/controls, observers, dust, piston, TEST-family, and adder episodes. Public compilation tests assert Graph identity. The command/result test compares an independently stepped interpreter, including live comparator feedback and the later scheduled output. The command reset handoff is compared with uninterrupted interpreter work.

The repaired CPU_BubbleSort interpreter reference passed its full 50,000-tick frozen replay. An explicit Graph episode also sorted the selected RAM bank after 10,035 active ticks and matched interpreter RAM at ticks 0, 10, 100, 1,000, 5,000, and 10,035. For this benchmark only, the same in-memory removal of the wall sign at `(188,10,186)` was applied to both inputs; the stored schematic keeps the sign, and compilation with it selected still returns the structured support-loss error. The repaired schematic includes the correctly oriented sticky piston head at `(181,14,166)`.

The final release harness runs five fresh sign-free input pairs for 100 active ticks; it compiles Graph and checks backend identity, RAM, and counters outside the timed section. Graph median was 1.284687 s (1.241218–1.301638 s), interpreter median was 0.649654 s (0.642933–0.695880 s). Each Graph sample recorded 20,005 scheduled ticks, 2,059,909 callbacks, 32,505 wire walks, 302,784 piston samples, and 27,884 piston actions/publications. The prior production baseline was Graph 4.215202 s (4.045955–4.592488 s) and interpreter 0.626306 s (0.590885–0.736583 s). These timings use release code with `cfg(test)` traces absent; the code changes reduce callback-route discovery/allocation and heap-bearing state copies, but no per-function profiler was run, so the exact share of elapsed time is unknown.

This is a focused matrix, not proof of every possible history. Long CPU histories, exhaustive analog fork/reconvergence sequences, all command failure/output combinations, changing note support topologies, and every scheduler wrap/type-replacement history were not tested in this batch. No unexpected divergence occurred in the exercised cases. The saved CPU sign remains intact and its support-loss state is deliberately rejected; the benchmark removes it in memory from both inputs only.

## Non-goals

No routing changes, output-only sampling shortcuts, source-manifest changes, full CPU benchmark campaign, or later circuit-specific optimization. A newly found divergence in admitted behavior blocks that behavior until fixed.
