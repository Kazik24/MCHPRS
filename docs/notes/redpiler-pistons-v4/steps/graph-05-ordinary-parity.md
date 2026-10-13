# G5 — Ordinary circuit parity audit

Status (2026-10-13): focused differential work was added in batch 02; the G5 audit remains incomplete and awaits human review. Read [the migration plan](../GRAPH_PIPELINE_MIGRATION.md) and existing reference-contract requirements.

## Goal

Establish which ordinary circuits can move from legacy Direct/Native selection to Graph. Backend identity is part of every claimed execution test. Record unresolved component policy before legacy retirement.

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

Reuse `analysis/tests/redstone_fuzz.rs`, graph regression helpers, the reference-contract comparator and pinned fixture loader. Use an explicit Graph runner; do not use the public compiler as the sole selector while it can still choose Direct/Native. Compare against independently executed interpreter episodes, using existing fixed seeds and short targeted histories before larger campaigns. Passing legacy-versus-interpreter tests do not establish Graph parity.

Preserve first-divergence reporting. Compare eligible callback order, component/resource state, pending work and reset continuation at appropriate boundaries. Revision IDs need not numerically match the interpreter; cancellation validity must match. Normalize only documented unobservable records. Same settled outputs are insufficient.

## Review evidence

- Matrix of cases attempted, admitted/executed, rejected, divergent and incomplete, with explicit backend identity.
- Minimal regression for each confirmed bug; changes must fix the shared semantic owner, not a fixture-specific path.
- Debug and release runs of new focused regressions, relevant existing parser/executor suites, formatting and diff checks.
- One ordered trace explaining a comparator/feedback interaction and one exact handoff comparison.

## Batch 02 evidence and remaining matrix

The explicit Graph tests cover imported tick deadlines/duplicates, comparator notification scheduling from a synchronous command result, note-block edge behavior, command host errors, selected trapdoor rejection, and selected existing observer, dust, piston, TEST-family, and adder episodes. The command/result test now compares against an independently stepped interpreter, including the comparator's scheduled feedback and the later stored output. The command reset handoff is compared with uninterrupted interpreter work.

The repaired CPU_BubbleSort interpreter reference passed its full 50,000-tick frozen replay. An explicit Graph episode also sorted the selected RAM bank after 10,035 active ticks and matched interpreter RAM at ticks 0, 10, 100, 1,000, 5,000, and 10,035. For this benchmark only, the same in-memory removal of the wall sign at `(188,10,186)` was applied to both inputs; the stored schematic keeps the sign, and compilation with it selected still returns the structured support-loss error. The repaired schematic includes the correctly oriented sticky piston head at `(181,14,166)`.

Current CPU Graph retained-table estimates were 19.74 MiB after geometry, 194.70 MiB after electrical analysis, 411.08 MiB after callbacks, 405.30 MiB at lowering input, and 323.78 MiB after lowering. These are estimated table bytes, not process RSS. Deliveries (133.0 MiB), guards (102.7 MiB), and callback links (64.0 MiB) were largest. During the explicit test, clearing test-only trace vectors each tick kept sampled RSS near 0.54 GiB; the first test run without that clearing reached 12.45 GiB and was stopped. This confirms unbounded test instrumentation caused that run's growth. The older ~42 GiB preparation report remains unexplained because it lacks stage data and RSS sampling. The existing release CPU harness now runs five fresh sign-free input pairs for 100 active ticks; it compiles Graph and checks backend identity, RAM, and counters outside the timed section. Graph median was 4.396806 s (4.245987–4.582137 s), interpreter median was 0.634134 s (0.621264–0.646845 s). Each Graph sample recorded 2,059,909 callbacks, 32,505 wire walks, 302,784 piston samples, and 27,884 piston actions/publications, matching the `cfg(test)` run. The test trace formatting is omitted from the release library; the two timing paths are not identical (`Graph::tick_with_world` versus direct test-backend `tick`), so the timing difference does not isolate the instrumentation cost. Compilation time was not measured.

This is a focused subset, not the complete matrix above. Existing command/control fuzz tests that compile through the public compiler still select Native and do not count as Graph parity. Broad analog comparator feedback, longer delayed loops, button/plate histories, full note sound capture, every command failure/chain ordering mode, and all reset boundaries remain incomplete. No unexpected divergence was reported by the focused passing tests; this does not establish parity for those untested histories.

## Non-goals

No routing changes, output-only sampling shortcuts, source-manifest changes, full CPU benchmark campaign or Native deletion. An unresolved divergence blocks activation of its affected cohort.
