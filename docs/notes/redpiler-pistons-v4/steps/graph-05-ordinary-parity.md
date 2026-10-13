# G5 — Ordinary circuit parity audit

Status: proposed; requires reviewed G4. Read [the migration plan](../GRAPH_PIPELINE_MIGRATION.md) and existing reference-contract requirements.

## Goal

Establish which ordinary circuits can move from legacy Direct/Native selection to Graph. Backend identity is part of every claimed execution test. Record unresolved component policy before legacy retirement.

## Cases

- Feedforward compare/subtract comparators with analog rear and side strengths, live eligible override sources, saved output state and same-value notifications.
- Shared-source forks/reconvergence, comparator feedback, delayed repeater/torch loops, oscillator restart and initially nonzero state. A directed cycle is not automatically an error or proof of eligibility.
- Dust decay/rise, branches, loops, ascending/descending connections, direct/conducted lamp power, stored-wire diode reads and regulated sides.
- Nested Turbo walks, synchronous component callbacks, observer-visible intermediate changes and callback duplicates/directions.
- Imported stale and matching ticks, deadline ties, priorities, pending-work multiplicity, and compilation with ordinary work already queued.
- Lever/button/plate controls, comparator/bulb state, applicable trapdoor behavior and reset/continue with pending work.
- Existing fixture family plus ordinary electrical portions of the adder schematics. Do not demand full FPU coverage solely for this migration gate.

## Implementation instructions

Reuse `analysis/tests/redstone_fuzz.rs`, graph regression helpers, the reference-contract comparator and pinned fixture loader. Use an explicit Graph runner; do not use the public compiler as the sole selector while it can still choose Direct/Native. Compare against independently executed interpreter episodes, using existing fixed seeds and short targeted histories before larger campaigns. Passing legacy-versus-interpreter tests do not establish Graph parity.

Preserve first-divergence reporting. Compare eligible callback order, component/resource state, pending work and reset continuation at appropriate boundaries. Revision IDs need not numerically match the interpreter; cancellation validity must match. Normalize only documented unobservable records. Same settled outputs are insufficient.

## Review evidence

- Matrix of cases attempted, admitted/executed, rejected, divergent and incomplete, with explicit backend identity.
- Minimal regression for each confirmed bug; changes must fix the shared semantic owner, not a fixture-specific path.
- Debug and release runs of new focused regressions, relevant existing parser/executor suites, formatting and diff checks.
- One ordered trace explaining a comparator/feedback interaction and one exact handoff comparison.

## Non-goals

No routing changes, output-only sampling shortcuts, source-manifest changes, full CPU benchmark campaign or Native deletion. An unresolved divergence blocks activation of its affected cohort.
