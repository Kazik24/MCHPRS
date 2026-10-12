# Graph backend native-reference tests

Tests live in `crates/core/src/redpiler/backend/graph_tests.rs`, registered as `redpiler::backend::graph::tests`. Test names describe behavior; no stage-number module or nested regression folder is used.

Eight additional tests cover the five executor findings and the admitted subset boundary:

| Test | Independent expectation |
| --- | --- |
| `boundary_torch_power_is_not_silently_compiled_as_zero` | A sampled external emitter supplies the same initial power as the native query, or compilation reports its unsupported source. |
| `imported_stale_and_inert_ticks_are_retained_or_explicitly_rejected` | Imported work is retained with its deadline or explicitly rejected without consuming the native queue. |
| `removed_repeater_tick_keeps_its_deadline_until_native_stale_dispatch` | A removed repeater's tick remains pending and is dispatched at the fourth half-tick; native scheduler snapshots are compared at every advance. |
| `observer_shape_then_explicit_update_matches_native_for_every_orientation` | Shape alone does not start a pulse; the separate watched-face Update does. All six orientations, toward/away cases, and four subsequent ticks are checked. |
| `torch_shape_does_not_sample_power_before_the_update_phase` | Native support checking does not schedule the torch's electrical recheck; the explicit Update is tested separately. |
| `captured_repeater_lock_rewrite_keeps_native_output_strength_and_consumers` | Both captured/live powered-state disagreements preserve native block properties, emitted strength, and downstream lamp behavior. |
| `bulb_far_comparator_callbacks_match_native_order_and_multiplicity` | Four far comparators receive exactly the native North/South/East/West callback sequence, with literal directions and no extra handler calls. |
| `piston_schematics_require_an_unsupported_component_error` | Complete piston fixtures cannot claim executable support before piston/dust integration. |

The schematic samples come from `INSTANT_TORCH.schem`, `INSTANT_OBSERVER.schem`, and `ADDER_1BIT.schem` in the frozen IO pack. Each load verifies manifest size and SHA-256, reuses the schematic loader, and prints the selected component's original local position and properties. Focused episodes relocate those samples and apply explicitly controlled state/orientation stimuli; they do not run the whole schematic. The copper-bulb episode is a small native-reference circuit because this pack does not supply a suitable bulb schematic.

The native expectations use existing interpreter procedures, scheduler snapshots, and the ordered callback recorder. Captured repeater checks exercise handler semantics; they do not claim full Turbo replay.

Run the combined suite:

```powershell
cargo test -p mchprs_core --lib --locked redpiler::backend::graph::tests:: -- --list
cargo test -p mchprs_core --lib --locked redpiler::backend::graph::tests:: -- --test-threads=1
```

Observed results: all eight additional tests passed in debug and release against the concurrently fixed executor. After file/module consolidation, all 21 combined graph tests passed in debug. The final unsupported-schematic test received a descriptive name after that run; its behavior was unchanged. Focused formatting and diff checks pass.

Not checked here: full core/CPU suites, execution of complete piston schematics, full Turbo replay, or public activation/reset continuation. External boundary source strengths are compilation-time samples; these tests establish the initial sampled value, not an external-input synchronization protocol.
