# Removal, replacement and reuse

This is part of the [implementation plan](README.md), not authorization to delete code now.
Retirement happens only when the replacement passes its correctness and runtime-work gates.
The target is one boundary analysis, one cell/layer compiler and one Graph executor.

## Already removed: do not restore

The checkout scoped on 2026-10-18 no longer contains these experiments:

| Removed area | Examples of old paths | What remains useful |
| --- | --- | --- |
| Generic symbolic event tracing | `analysis/instant_regions/compiler.rs`, its `compiler/` directory, `symbolic_region.rs` | Its recorded failed entry, ordered-effect requirements and edge cases. |
| Finite machines and global trajectories | `analysis/instant_regions/machine.rs`; `program_graph/instant_machine.rs` | The lesson that enumerating input combinations/complete states does not scale. |
| Old kernel runtime and expression plans | `backend/graph/symbolic.rs`, `symbolic_kernel.rs`, `machine_state.rs`; associated program types | Boundary timing, revision and handoff obligations. |
| Boolean optimization crate | `crates/redpiler_opt/` | Algebraic identities and truth-table verification ideas, not a dependency to reintroduce. |
| Native/C inference experiment | Removed native engine/query code | Separate historical performance evidence; it is not the new instant compiler. |
| Direct/Native compiled backends | Earlier legacy compiler graph and packed/block executors | Nothing needed for execution; Graph remains the only compiled executor. |

Do not reintroduce empty modules, dummy enum variants, dormant synthesis passes, test flags,
compatibility wrappers or counters simply to make an old document look current.

## Replace and then remove the superseded path

| Current code/responsibility | Replacement | Retirement gate |
| --- | --- | --- |
| `passes/identify_instant_regions.rs` | `identify_instant_boundaries.rs`, using one shared classification/ownership implementation | Step 03 validates all boundary witnesses; migrate callers, then remove old registration/file. |
| `analysis/instant_regions.rs`: mixed discovery, boundary scans and proxy construction | Focused cell classification and boundary routines under `analysis/instant/` | Move reusable routines instead of copying them. Remove old functions as their callers migrate. |
| `analysis/instant_regions/composition.rs`: topology-based stable composition | Region discovery from classified instant connections and shared ownership, followed by SCC analysis | Step 04 has deterministic region coverage and no merging through ordinary fanout. |
| `InstantRegion` fields used both as ownership proof and dispatch recipe | A cold boundary/cell witness plus a separate compact execution program | Keep one authoritative owner map; JSON compatibility is a serialization adapter, not a second graph. |
| `compile_instant_proxies.rs`, `InstantProxy`, `InstantRoute`, `InstantStep` and `deliver_instant_route` | Layer-derived expressions and boundary effects for accepted regions; normal Graph otherwise | Step 08 confirms coverage, ordinary fallback and performance. Remove the old route VM, its indices, validators/export-only scaffolding and execution counters together. |
| Stable-input composed-proxy branch inside `infer_proxies` | Explicit layered transaction contract | Do not retain both as alternative semantics for the same region. |
| Repeated scans for actor/resource/reset/cell membership | One indexed classification result reused by both new passes | Step 03/04 profiles preparation counts and verifies deterministic owner maps. |

Private-cell occupancy hints may be retained as small direct bindings if still useful to
normal Graph. They must not keep the entire old proxy route VM alive. Remove a helper only
after inspecting all callers and retaining its validation/error behavior.

## Reuse intact

| Existing area | Reuse | Do not duplicate |
| --- | --- | --- |
| Geometry, electrical and callback preparation | The prepared graph is the source of truth for possible contacts, power/QC, guards, shape and ordered routes. | A second world parser or coordinate-based redstone search. |
| `passes/mod.rs`, `planning.rs` | Existing read-only builders, publication, cancellation and graph validation. | A new pipeline runner or pass-specific orchestration in `Compiler`. |
| Typed graph IDs and position provenance | Source bindings, errors, witness export and display lookup. | Runtime position hashes to rediscover connectivity. |
| `utils/instant_mechanisms.rs` | Its indexed power/reset source recognition as a starting candidate finder. | Calling its narrow pattern a universal instant proof. |
| Existing power/guard/shape rules | Derive valid pin dependencies and attenuation under all admitted contact alternatives. | Flattening every power edge to a zero-cost Boolean connection. |
| Ordinary Graph components and scheduler | Repeaters, observers, comparators, commands, notes, BUD sampling and deadline dispatch. | Another simulator inside the kernel or a private clock. |
| Resource identity/validation | Conservation, allowed contacts, revision witnesses and shared ownership. | One payload clone per piston. |
| Projection/reset infrastructure | Materialization at a certified cut and existing host publication primitives. | Advancing only a private region during handoff. |
| Loaders, manifests, reference contracts and adder helpers | Same schematics/hashes and stimulus protocols for every executor. | New loaders, hand-labelled per-schematic gate logic or silently changed inputs. |
| Current benchmark/profile workflow | Paired uninstrumented samples, existing timers and external sampling tools. | A new timing suite or always-on per-event formatting. |

## Keep as complementary general optimizations

`simplify_guards`, `simplify_state_guards`, callback pruning, Turbo indexing, dust paths,
wire-query specialization and compiled wire hops still help uncontracted Graph execution.
They do not run as the execution body of an accepted layered kernel.

`compile_builtin_resets`, `simplify_instant_resets`, `compile_instant_resets`,
`analysis/instant_resets.rs` and `backend/graph/instant.rs` currently provide exact local
reset optimizations. Keep them for general fallback. Accepted kernels suppress their owned
reset work; no node may be handled by both paths. Delete these specializations later only
if all their useful cases are replaced and measurements justify the deletion.

Classic committed-signal nets remain a separate domain optimization. Their current fixed,
one-ingress and bounded proof restrictions must not become instant-compiler restrictions.
Reuse pure query indexing/attenuation helpers only after a real second consumer exists.
Do not copy a private helper into the new compiler, or use classic compilation to conceal
uncompiled instant wires. A classic-owned wire is a boundary, never a second instant owner.

## Integration sequence without duplicate semantics

1. Keep public behavior unchanged while steps 01-05 build/validate plans in explicit test pipelines.
2. In that experimental pipeline, select either old region/proxy discovery or the new two-pass
   flow, never both independent discovery engines on the same nodes.
3. Run all semantics-preserving graph rewrites before either analysis. There are no graph
   rewrites between boundary identification and layer compilation.
4. Existing classic-net plans establish exclusions before instant ownership. The layered
   compiler can use their published interface but cannot take their wires silently.
5. Step 06 introduces one lower-time owner map. Accepted kernels own their internal dispatch;
   declined regions retain normal Graph. Do not use old proxies as a hidden third kernel mode.
6. At step 08, replace O3 old boundary/proxy registration with the new boundary analysis and
   select successful synthesis at O4 with its explicit stable-input contract. Unsupported
   candidates remain general Graph. Runtime compilation/learning is never a fallback.
7. Remove old discovery/proxy APIs, metadata, exports and tests that only assert their obsolete
   representation. Retain semantic regressions, adapted to the new interface and real outputs.

The rollout gate includes checking that retiring the old proxy does not cause an unexplained
regression for uncompiled devices. If it does, retain the small proven shortcut in the general
handler or defer retirement with a named witness; do not keep two compilers indefinitely.

## Required deletion audit

Before declaring retirement complete, search source/tests/tools/docs for removed names and
check: module declarations, feature flags, enum variants, serialization capabilities, pass
statistics, lower-time bindings, constructor fields, reset/flush handling, CLI help, Python
loader/viewer schemas, generated fixture expectations and benchmark positive-kernel assertions.
Historical documents may retain old names if explicitly marked historical. Do not delete
reference fixtures or the fixed-point adder counterexample to reduce test failures.
