Scope a dedicated prune_inert_callbacks.rs pass, limited to removing deliveries with no effective receiver.

Its purpose is to eliminate stored rows, guard evaluations and unsuccessful searches. It must preserve every callback that actually invokes a handler.

Eligibility

Verify the current dispatcher, then use this rule:

delivery.receiver is None
AND NOT (
    delivery is Update
    AND target site is Component
)

Component Updates can receive a fallback handler, so literal None alone is insufficient.

Keep every explicit receiver—even when its handler currently does nothing. Keep uncertain cases. Do not classify using current power, piston pose, selected contact or pending state.

Correctness argument

Prove the rule against both consumers:

1. In ordinary route delivery, an eligible row performs only pure checks before resolving to no receiver. It cannot write, schedule, sample, notify or produce host effects.
2. Turbo lookup requires an explicit receiver. Removing an eligible row cannot change its first selectable candidate.

Check for additional consumers before implementing. Canonical neighborhoods, captured sites, heading and queue bookkeeping must remain intact.

No SAT solver, temporal model checker or new certificate structure is needed.

Implementation

Create the pass under crates/core/src/redpiler/passes/, using the existing GraphPass API.

- Reuse the effective-receiver rule. Extract one small shared helper only if necessary.
- Retain surviving deliveries in their original order.
- Preserve every surviving field and sequence label.
- Keep empty routes and their IDs.
- Keep callback links unchanged: eligible rows have no explicit receiver and therefore no corresponding link.
- Preserve nodes, sites, profiles, guards, power links and imported work.
- Do not compact IDs or collect unused guards.
- Check cancellation while processing routes; never publish a partially prepared compilation.

The pass needs no parameters or additional runtime state.

Pipeline

Run after Opt-01 and before Turbo index construction. Route pruning invalidates existing lookup offsets; rebuild them afterward.

Test custom orders and preserve the agreed review gate for O1 enablement. O0 remains unchanged.

JSON must expose the resulting sparse routes accurately. Sequence gaps and empty routes remain valid.

Required tests

- Constant/contact targets with absent receivers, in Live and Captured views.
- Component Update with None remains because of fallback.
- Explicit receivers remain, including Contact Shape.
- Receiver-less alternatives before and between meaningful alternatives preserve first-match selection.
- Failed guards followed by passing alternatives behave identically.
- Meaningful duplicate notifications remain ordered.
- Empty routes retain valid owners and references.
- Callback links remain unchanged.
- Captures, headings, layers and power reads remain equivalent.
- BUD, observer, shared-contact, nested-walk and reset-continuation episodes agree with general Graph and interpreter.
- Disabled execution, composition, invalidation, cancellation and idempotence work.

Measurement

Compare O1 against O1 plus pruning, with identical Turbo-index configuration on both sides.

Run five fresh paired, uninstrumented 100-tick samples. Separately profile:

- Removed rows and retained bytes.
- Delivery rows examined.
- Guard operations.
- Remaining passing rows without effective receivers.

Semantic callback, scheduling, wire-walk and piston-action counts must remain unchanged. Search and guard counts may decrease.

The reported 3.38 million stored rows and 1.39 million passing visits are candidate estimates; remeasure eligibility on the current graph.

Stop after this rule, tests and benchmark. Callback deduplication, branch merging and stateful identity rules are separate work.
