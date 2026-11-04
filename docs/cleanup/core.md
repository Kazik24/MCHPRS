# Core cleanup review

Scope: crates/core/**, excluding crates/core/src/plot/** and crates/core/src/redpiler/** (reviewed by other agents). The inventory below covers 96 entries: 95 source/config/document text files and one binary benchmark asset. All listed text files were reviewed individually for small DRY/readability changes and missing comments. Stateful simulation paths and benchmark timing boundaries were kept intact.

## Applied changes

- crates/core/build.rs: split the workspace [package] table selection and the next-table boundary into named steps. This keeps the current lightweight parser and adds no dependency.
- crates/core/src/redstone/copper_bulb/mod.rs: replaced a stale TODO that suggested deleting item_transform; its caller in interaction::use_item_on_block applies honeycomb waxing and axe scraping, and copper-bulb tests cover the behavior.
- crates/core/src/world/mod.rs: corrected the tick-priority wording and described the X/Z/Y section iterator and bounds clipping.
- crates/core/src/waveform/recorder.rs: described the stages that enumerate Manhattan-radius-2 offsets.
- crates/core/src/permissions/mod.rs: explained parsing positive prefix nodes and selecting the highest priority.
- crates/core/src/server.rs: explained how queued arrivals are drained after a failed plot load.
- crates/core/src/container/mod.rs: explained slot-order conversion into saved inventory entries.
- crates/core/examples/bench_llvm_small.rs and check_llvm_zcore.rs: explained the cuboid/lever scan and the block-entity position merge.
- Additional long-chain comments cover benchmark allocation counting, player equipment mapping, permission filters, piston power/read order and trace projections, waveform index/offset mapping, and CPU RAM/reference decoding.

These edits change comments and the local spelling of build-script parsing steps only. No public signatures, serialized layouts, update ordering, error paths, or benchmark measurements were changed.

## Larger follow-up proposals

1. Several redstone, world, headless, and container tests construct a full empty PlotWorld with the same chunk grid. A shared test-only fixture constructor could reduce that setup duplication. Keep fixture-specific initialization and defaults explicit, and validate each caller's dimensions/chunks before migrating; this was not introduced because it touches many tests.
2. container/mod.rs combines protocol decoding, click/drag actions, inventory conversion, and furnace rules. If this file keeps growing, split protocol action handling from reusable inventory rules while keeping click as the stable integration point. Defer until a concrete second consumer or a focused behavioral test justifies the module boundary.
3. CPU/arithmetic/FPU benchmark examples repeat observation encoding for separate native and compiled worlds. A future small observation helper could reduce duplication, but it must preserve per-benchmark timing boundaries and fixture-specific decoders; no shared benchmark framework was added.

## Compatibility and validation

No signatures or call sites changed. The copper-bulb caller was searched and verified; all other edits are comments or equivalent named intermediate variables. git diff --check passed for the 17 edited core files. Root's workspace run completed with 849 passed, 2 failed, and 38 ignored tests. The failures were the known FourInARow trace mismatch at (12, 9, 10), action 1/tick 2 (power 0 vs 1), and a missing test_data/cpu-references/zcore12b_sierpinski.json fixture in frozen_cpu_inputs_and_initial_states. The last iterator comments were added after that run; root owns final incremental validation. No separate core cargo command was started by this reviewer. No rustfmt was run because the changes do not alter Rust formatting.

## File-by-file coverage

| File | Review result |
| --- | --- |
| crates/core/benches/bitbuffer.rs | Read; timing scope and benchmark setup retained. |
| crates/core/benches/chungus_mandelbrot_plot | Binary benchmark asset; not treated as source text. |
| crates/core/benches/chungus.rs | Read; timing scope and benchmark setup retained. |
| crates/core/benches/cpus.rs | Read; timing scope and benchmark setup retained. |
| crates/core/benches/piston.rs | Read; timing scope and benchmark setup retained. |
| crates/core/benches/support/cpu_timing.rs | Read; timing scope and benchmark setup retained. |
| crates/core/build.rs | Split package-table selection from the next-table boundary; same parser behavior, no dependency added. |
| crates/core/Cargo.toml | Read; no safe small refactor surfaced. |
| crates/core/examples/bench_anpu.rs | Read; timing scope and benchmark setup retained. |
| crates/core/examples/bench_arithmetic.rs | Read; timing scope and benchmark setup retained. |
| crates/core/examples/bench_chungus.rs | Read; timing scope and benchmark setup retained. |
| crates/core/examples/bench_cpus.rs | Read; timing scope and benchmark setup retained. |
| crates/core/examples/bench_fpu.rs | Read; timing scope and benchmark setup retained. |
| crates/core/examples/bench_llvm_small.rs | Added a comment for the cuboid scan and lever filter; benchmark work unchanged. |
| crates/core/examples/bench_llvm_zcore.rs | Read; timing scope and benchmark setup retained. |
| crates/core/examples/bench_redpiler_allocations.rs | Added a comment describing the dirty-section count across chunks. |
| crates/core/examples/census_sampled.rs | Read; timing scope and benchmark setup retained. |
| crates/core/examples/check_llvm_zcore.rs | Added comments for merging changed positions, block entities, neighboring diagnostics, and report bounds. |
| crates/core/examples/cpu_references.rs | Read; timing scope and benchmark setup retained. |
| crates/core/examples/inspect_zcore.rs | Read; timing scope and benchmark setup retained. |
| crates/core/src/chat_commands.rs | Read; no safe small refactor surfaced. |
| crates/core/src/chat.rs | Read; no safe small refactor surfaced. |
| crates/core/src/config_tests.rs | Read; no safe small refactor surfaced. |
| crates/core/src/config.rs | Read; no safe small refactor surfaced. |
| crates/core/src/container/mod.rs | Added comments for comparator fullness calculation and occupied-slot serialization; click behavior unchanged. |
| crates/core/src/container/tests.rs | Read; assertions and fixture-specific ordering retained. |
| crates/core/src/headless/mod.rs | Read; no safe small refactor surfaced. |
| crates/core/src/headless/tests.rs | Read; assertions and fixture-specific ordering retained. |
| crates/core/src/interaction.rs | Read; no safe small refactor surfaced. |
| crates/core/src/lib.rs | Read; no safe small refactor surfaced. |
| crates/core/src/messages.rs | Read; no safe small refactor surfaced. |
| crates/core/src/permissions/database.rs | Read; protocol, persistence, and externally visible behavior retained. |
| crates/core/src/permissions/mod.rs | Added comments for proxy-node filtering, stored-node precedence, group-root traversal, rank weights, and prefix priority; effective permission behavior unchanged. |
| crates/core/src/permissions/rank.rs | Read; protocol, persistence, and externally visible behavior retained. |
| crates/core/src/player.rs | Added a comment describing the protocol equipment slot mapping and small-model filter. |
| crates/core/src/player/client_sync_tests.rs | Read; protocol, persistence, and externally visible behavior retained. |
| crates/core/src/player/client_sync_unit_tests.rs | Read; protocol, persistence, and externally visible behavior retained. |
| crates/core/src/player/client_sync.rs | Read; protocol, persistence, and externally visible behavior retained. |
| crates/core/src/player/error_messages_tests.rs | Read; protocol, persistence, and externally visible behavior retained. |
| crates/core/src/player/error_messages.rs | Read; protocol, persistence, and externally visible behavior retained. |
| crates/core/src/profile.rs | Read; no safe small refactor surfaced. |
| crates/core/src/redbox/mod.rs | Read; protocol, persistence, and externally visible behavior retained. |
| crates/core/src/redbox/tests.rs | Read; assertions and fixture-specific ordering retained. |
| crates/core/src/redstone/adder_tests.rs | Read; state, storage, and update-order behavior retained. |
| crates/core/src/redstone/button.rs | Read; state, storage, and update-order behavior retained. |
| crates/core/src/redstone/command_block.rs | Read; state, storage, and update-order behavior retained. |
| crates/core/src/redstone/comparator.rs | Read; state, storage, and update-order behavior retained. |
| crates/core/src/redstone/copper_bulb/mod.rs | Replaced stale removal TODO with behavior docs; caller in interaction::use_item_on_block and tests confirm use. |
| crates/core/src/redstone/copper_bulb/tests.rs | Read; assertions and fixture-specific ordering retained. |
| crates/core/src/redstone/master_tests.rs | Read; state, storage, and update-order behavior retained. |
| crates/core/src/redstone/mod.rs | Added a comment describing the reconstruction of chunk coordinates for a fixture. |
| crates/core/src/redstone/noteblock.rs | Read; state, storage, and update-order behavior retained. |
| crates/core/src/redstone/observer_tests.rs | Read; state, storage, and update-order behavior retained. |
| crates/core/src/redstone/piston/cache.rs | Read; state, storage, and update-order behavior retained. |
| crates/core/src/redstone/piston/instant.rs | Added comments explaining native neighbor read order and its ordering assertion. |
| crates/core/src/redstone/piston/mod.rs | Read; state, storage, and update-order behavior retained. |
| crates/core/src/redstone/piston/tests/bud_reference.rs | Read; assertions and fixture-specific ordering retained. |
| crates/core/src/redstone/piston/tests/instant.rs | Added comments explaining snapshot delta encoding and callback extraction by event/tick/cohort. |
| crates/core/src/redstone/piston/tests/instant/io.rs | Read; assertions and fixture-specific ordering retained. |
| crates/core/src/redstone/piston/tests/instant/reference_contract.rs | Added comments explaining resource contacts, pending-work projection, manifest order, and trace truncation. |
| crates/core/src/redstone/piston/tests/mod.rs | Read; assertions and fixture-specific ordering retained. |
| crates/core/src/redstone/piston/trace.rs | Added comments explaining main and quasi-connectivity source scans. |
| crates/core/src/redstone/power.rs | Read; state, storage, and update-order behavior retained. |
| crates/core/src/redstone/repeater.rs | Read; state, storage, and update-order behavior retained. |
| crates/core/src/redstone/wire/mod.rs | Read; state, storage, and update-order behavior retained. |
| crates/core/src/redstone/wire/tests.rs | Read; assertions and fixture-specific ordering retained. |
| crates/core/src/redstone/wire/turbo_cache.rs | Read; state, storage, and update-order behavior retained. |
| crates/core/src/redstone/wire/turbo_tests.rs | Read; state, storage, and update-order behavior retained. |
| crates/core/src/redstone/wire/turbo.rs | Read; state, storage, and update-order behavior retained. |
| crates/core/src/server.rs | Added comments for plot player listing and queued-player preservation/diagnostics while draining a failed plot. |
| crates/core/src/server/chat_tests.rs | Read; protocol, persistence, and externally visible behavior retained. |
| crates/core/src/server/login.rs | Read; protocol, persistence, and externally visible behavior retained. |
| crates/core/src/server/skin_tests.rs | Read; protocol, persistence, and externally visible behavior retained. |
| crates/core/src/signed_velocity/mod.rs | Read; protocol, persistence, and externally visible behavior retained. |
| crates/core/src/signed_velocity/tests.rs | Read; assertions and fixture-specific ordering retained. |
| crates/core/src/sound.rs | Read; no safe small refactor surfaced. |
| crates/core/src/tests/circuit_geometry.rs | Read; assertions and fixture-specific ordering retained. |
| crates/core/src/tests/mod.rs | Read; assertions and fixture-specific ordering retained. |
| crates/core/src/utils.rs | Read; no safe small refactor surfaced. |
| crates/core/src/velocity.rs | Read; no safe small refactor surfaced. |
| crates/core/src/waveform/api.rs | Read; protocol, persistence, and externally visible behavior retained. |
| crates/core/src/waveform/api/tests.rs | Read; assertions and fixture-specific ordering retained. |
| crates/core/src/waveform/mod.rs | Read; protocol, persistence, and externally visible behavior retained. |
| crates/core/src/waveform/recorder.rs | Explained probe-index creation, Manhattan-radius offset generation, and filtering offsets to active probes; capture behavior unchanged. |
| crates/core/src/waveform/tests.rs | Read; assertions and fixture-specific ordering retained. |
| crates/core/src/world/mod.rs | Fixed tick-priority docs and explained X/Z/Y section enumeration and clipping. |
| crates/core/src/world/scheduler.rs | Read; state, storage, and update-order behavior retained. |
| crates/core/src/world/storage.rs | Read; state, storage, and update-order behavior retained. |
| crates/core/src/world/tests.rs | Read; assertions and fixture-specific ordering retained. |
| crates/core/src/world/wire_cache.rs | Read; state, storage, and update-order behavior retained. |
| crates/core/test_support/chungus.rs | Read; existing shared fixture helpers retained. |
| crates/core/test_support/cpus.rs | Added comments explaining RAM bit decoding and ordered reference-checkpoint selection. |
| crates/core/tests/cpu_references.rs | Read; assertions and fixture-specific ordering retained. |
| crates/core/tests/headless_api.rs | Read; assertions and fixture-specific ordering retained. |
| crates/core/tests/redpiler_admission.rs | Read; assertions and fixture-specific ordering retained. |
| crates/core/tests/review_graph_removal_probe.rs | Read; assertions and fixture-specific ordering retained. |
