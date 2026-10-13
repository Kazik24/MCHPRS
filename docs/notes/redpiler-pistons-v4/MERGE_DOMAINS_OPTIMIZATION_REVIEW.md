# Historical optimization review: redpiler/merge-domains

Read-only source investigation, 2026-10-13. No checkout switch, old execution, or benchmark reproduction was performed. Historical commit sources were read with `git show`; current uncommitted work was preserved. This note identifies techniques, not a verified attribution of the reported 7–15 to 1000–1200 TPS improvement.

## Sources inspected

The branch tip is `70f6fcd`, with four commits after common ancestor `c21050f`:

| Commit | Relevant changes |
| --- | --- |
| `542d8ae` | Generalized composition architecture; base for the subsequent optimization commits |
| `794af67` | Classification of observable/boundary wires around the already existing cached decision evaluator |
| `ff46a21` | Dirty-root evaluation, region/root parallelism, work-size threshold |
| `70f6fcd` | Dirty-input capture and geometry invalidation; removal of root-level atomics/parallelism; revised internal-wire ownership and explicit pending-update dispatch |

Useful source commands:

```powershell
git show 70f6fcd:crates/core/src/redpiler/backend/direct/instant/logical.rs
git show 70f6fcd -- crates/core/src/redpiler/backend/direct/instant.rs
git show 70f6fcd -- crates/core/src/redpiler/backend/direct/mod.rs
git show 70f6fcd -- crates/core/src/redpiler/instant/program.rs
git show ff46a21 -- crates/core/src/redpiler/backend/direct/mod.rs
git show 794af67 -- crates/core/src/redpiler/instant/program.rs
```

## Techniques and applicability

### 1. Capture only inputs invalidated by a known change

At the final commit, `Plan` has `bindings_by_input`, `dirty_inputs`, and `input_dirty`. `mark_dirty` queues each affected binding once. `capture_dirty` reads those bindings instead of every input and reuses `captured_values` storage. Source changes, memory writes, and actor geometry changes explicitly invalidate their dependents.

Transfer the principle to the general Graph only where all dependencies can be identified. The immediate low-risk counterpart is compiled resource/contact/component-plan bindings: local queries should not discover dependencies by scanning the whole circuit. Persistent cached power/guard results are a later, separately proven change.

The old code captures frozen Boolean region inputs. Current Graph callbacks can mutate live state during the same tick or route. A per-tick dirty flag or one route-wide snapshot would therefore be an invalid direct translation. Invalidation would need to cover every relevant component output, piston availability/head pose, resource contact/enabled state, and capture context.

### 2. Cache shared pure expressions and invalidate only their users

`Plan` stores compact `BoundDecision` entries, input users, parents, roots-by-decision, cached values, and dirty-root queues. Changed inputs invalidate reachable dependents; evaluation visits the needed selected branches. The queue membership bit avoids repeated scheduling of the same pure computation.

This suggests shared guard representation and selective recomputation of pure dependencies, not suppression of semantic callbacks. Repeated callbacks still require their ordered activation, live sampling, acceptance, and observable side effects. An unchanged input value is not evidence that a delivered update can be deleted.

The old branch can prune unselected expression branches because it has a defined frozen expression-evaluation boundary. Do not assume that boundary exists around a general Graph route.

### 3. Keep hot storage compact and reuse scratch buffers

The final commit replaces root-level `AtomicU8` caches with ordinary `Option<bool>` entries and test evaluation atomics with plain counters. Evaluation uses a reusable explicit stack. Capture buffers are returned to the plan after use rather than repeatedly allocated.

Current Graph's straightforward counterparts are allocation-free route traversal, narrow scalar state reads, removal of unused runtime callback links, and reuse of scratch only where nested executions remain independent. Do not add atomics or threading to the serial Graph executor. Do not share one mutable wire snapshot between nested walks.

### 4. Skip internal wire execution behind a logical region boundary

This is a major change in the amount of work executed, rather than merely faster lookup. `794af67` classifies wires and retains observable/sampling/boundary-connected propagation. `70f6fcd` replaces that classification with more restrictive ownership: only external-input wires remain in `propagation_wires`; other internal wires are owned by the logical assembly. Shared internal cables across assemblies and external observers watching owned dust are rejected.

The associated World/Turbo hooks route updates into assemblies instead of executing ordinary dust propagation. This approach is tied to that branch's response-region model and changes which cases are admitted.

For current Graph, the corresponding future work belongs in a proven circuit/dust reduction pass. Require a complete boundary contract for power, callbacks, analog consumers, observer-visible changes, timing, cancellation, and reset reconstruction. Do not copy `owns_redstone_update`, the native composition adapter, or blanket internal-wire suppression into the baseline executor. Do not reduce current admitted observer support merely to reproduce the old speed.

### 5. Explicit work stacks and conditional parallelism

`70f6fcd` adds `PendingUpdate` and a reusable LIFO work vector to DirectBackend, including markers for native-delivery completion. The order of pushed entries encodes nested delivery order. This is an idea for avoiding recursive dispatch only if profiling or a concrete stack-depth problem justifies it; the native markers and old executor are not transferable.

`ff46a21` parallelizes regions and roots. The final commit removes root-level parallel evaluation and its atomics but retains conditional parallel region work. Thus the final acceleration cannot simply be attributed to Rayon. Current Graph does not yet have the certified independent region boundaries needed for that scheduling. Threading remains outside batch 03.

## What the archived measurements establish

The archived Counter reports under `scraps/docs/merge-domains/test_data/piston-research/test-prefix-20261008/` give:

| File | Workload | Median TPS | Samples | Executable graph nodes |
| --- | --- | --- | --- | --- |
| `perf-counter-before.json` | Counter, 60,000 game ticks | 1076.38 | 3 | 0 |
| `perf-counter-native.json` | Interpreter Counter reference | 1306.13 | 3 | Not applicable |
| `perf-counter-indexed.json` | Indexed compiled Counter | 1685.57 | 1 | 0 |

These are inherited native-domain results dated before the final logical composition commits. The reports' measurement-kind labels do not override the recorded zero executable graph/decision counts and the provenance documented in `REDPILER_PISTON_ATTEMPTS_SUMMARY.md`. They do not prove throughput of the final logical executor or reproduce a 7–15 to 1000–1200 TPS CPU acceleration.

No matched before/after artifact for that particular claimed jump was identified in the inspected sources. The user's in-game observation remains useful evidence to investigate, but this audit cannot attribute its multiplier to one commit. Fixture identity, active inputs, flush cadence, backend identity, tick units, output checks, and build mode would be needed for that attribution.

The historical `fpu_profile.rs` also calls the backend twice per reported game tick and can flush/print inside the measured loop. Current Graph uses explicit game-tick phases. Do not copy historical denominators or compare raw timings without checking these definitions.

## Recommendation for the current implementation

1. Complete batch 03's compiled local bindings, allocation-free route delivery, runtime-table cleanup, deliberate test recording, and verified legacy retirement.
2. Measure the same existing production 100-tick CPU workload before/after. Preserve RAM, graph identity, work counters, and ordering regressions.
3. Consider dependency-indexed pure guard/power caching separately, after documenting all invalidation owners. Keep all semantic callback deliveries.
4. Pursue large reductions through explicit passes that replace a certified internal subgraph while preserving its external event contract. This is the transferable idea behind the old logical assemblies.
5. Use the six-piston QC counter as a regression witness for separate data/update paths, rather than porting the old admission failure or memory classification. See `scraps/docs/merge-domains/test_data/piston-research/pc-counter-minimal-20261009/ADMISSION_REPORT.md`; the old branch rejects it and does not provide a verified complete output reference.

No old backend or instant architecture should be merged wholesale. Techniques are reusable after adapting their semantic boundaries; historical speed alone is not their correctness certificate.

Not checked: historical binaries, paired historical benchmarks, or complete branch equivalence. Risk: frozen-region caching and wire suppression are not valid general replacements for live Graph callbacks without additional proof.
