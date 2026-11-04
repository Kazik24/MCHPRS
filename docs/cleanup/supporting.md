# Supporting code and viewer review

Each source file below was reviewed for DRY, readability and long expressions.
“Retained” means no small complete change improved the existing structure.
Paths are relative to the repository root; removed files are listed explicitly.

## Supporting Rust code

| File | Result |
| --- | --- |
| `crates/blocks/src/block_entities.rs` | Shared equivalent container arms; named comparator and NBT values; documented persisted sign fields, progress encoding and compact inventory storage. Sign-side composition is a separate serialization-sensitive proposal. |
| `crates/blocks/src/blocks/mod.rs` | Reused rotation and face helpers; shared boolean sentinel lookup; removed 53 unused name expressions and their macro input; expanded opaque-state property updates and sign transforms into readable steps. |
| `crates/blocks/src/blocks/props.rs` | Retained explicit state encodings. Button/lever attachment enums are a possible shared type; piston-head and moving-piston properties differ and must stay distinct. |
| `crates/blocks/src/generated.rs` | Retained the macro entry point; edit the generator instead of expanded registry data. |
| `crates/blocks/src/items.rs` | Removed unused stack-limit macro inputs and documented the filled-container threshold; registry values remain authoritative. |
| `crates/blocks/src/lib.rs` | Removed a commented-out face decoder. The direction enums use different ID orders, so their conversions are not interchangeable. |
| `crates/network/src/generated.rs` | Retained the generated-protocol entry point. |
| `crates/network/src/inbound_tests.rs` | Retained ordered backpressure and disconnect tests; setup already uses local helpers. |
| `crates/network/src/lib.rs` | Corrected the connection-phase description; retained connection wrappers and acknowledgement lifetime handling. |
| `crates/network/src/nbt_map.rs` | Removed: no module declaration, callers or documentation references; the current registry encoder does not use it. |
| `crates/network/src/outbound.rs` | Retained packet barriers, compressed/uncompressed boundaries and close/error behavior. Flattening packet and block payloads would obscure ordering requirements. |
| `crates/network/src/outbound/tests.rs` | Retained byte-for-byte ordering, overflow and socket tests; manual benchmark remains separate. |
| `crates/network/src/packets/clientbound.rs` | Replaced eight copies of angle arithmetic with the encoder method. Packet field order remains explicit. |
| `crates/network/src/packets/components.rs` | Separated JSON parsing from the item-name iterator and explained registry-order dependence. Kept component depth, count and payload validation explicit. |
| `crates/network/src/packets/mod.rs` | Added shared angle encoding; boolean aliases use one implementation; removed dead packet-cache comments. |
| `crates/network/src/packets/serverbound.rs` | Reused the existing empty-packet macro for request and unknown packets; retained per-packet trust-boundary validation. |
| `crates/network/src/packets/tests.rs` | Added fixed expected bytes for zero, quarter/half/full turns, negative turns and truncation; retained existing wire-layout tests. |
| `crates/network/src/text.rs` | Retained recursive JSON/NBT conversion and homogeneous/mixed list handling; the branches encode distinct wire rules. |
| `crates/proc_macros/src/lib.rs` | Retained explicit derive expansion. A shared named-field parser is only worthwhile if tuple/unnamed-field diagnostics or further derives are added. |
| `crates/proc_macros/src/minecraft_data.rs` | Explained the four-stage target-state index. Retained input-order-sensitive registry generation and default-property merging. Named block definitions are proposed in the main review. |
| `crates/proc_macros/tests/minecraft_data.rs` | Retained canonical fingerprints; these protect generated values independently of implementation refactors. |
| `crates/redgear-api/src/main.rs` | Shared full-region baseline traversal between initial response-size validation and baseline projection; kept validation, draining and response limits at their original call sites. |
| `crates/redgear-api/tests/session.rs` | Retained end-to-end loading, repeated baseline, connection-slot and oversized-message checks. |
| `crates/save_data/src/atomic.rs` | Retained exclusive backup creation, sync-before-replace and temporary-file cleanup; the two write paths have different durability purposes. |
| `crates/save_data/src/lib.rs` | Retained public module exports. |
| `crates/save_data/src/plot_data.rs` | Retained validation and format headers. Avoid sharing frozen migration layouts with the live schema. |
| `crates/save_data/src/plot_data/fixer.rs` | Replaced speculative module commentary with the actual conversion, validation and backup contract. |
| `crates/save_data/src/plot_data/fixer/legacy_1_18.rs` | Retained frozen format-1/v0 representations and checked ID translation. |
| `crates/save_data/src/plot_data/fixer/legacy_1_21_5.rs` | Retained frozen format-3 decoding and occupied-destination protection. A named internal location struct could help if the three index components gain further consumers. |
| `crates/save_data/src/plot_data/fixer/pre_header.rs` | Removed: obsolete prototype outside the module graph, still constructing an outdated `PlotData` layout. The active dispatcher explicitly rejects missing headers. |
| `crates/save_data/src/plot_data/fixer/pre_worldsendrate.rs` | Removed: unreferenced prototype superseded by `legacy_1_18::decode_v0`. |
| `crates/save_data/src/plot_data/tests.rs` | Retained old-format fixture constructors and backup assertions; historical payloads deliberately differ. |
| `crates/utils/src/lib.rs` | Replaced uncertain macro commentary with the purpose of the qualified enum-variant path matcher. |
| `crates/world/src/lib.rs` | Retained serialized tick/piston order, snapshot identity and half-step progression; these are model contracts rather than incidental duplication. |
| `src/main.rs` | Retained startup and version handling; no abstraction needed for one initialization path. |
| `src/logging.rs` | Retained bounded rotation, Windows close-before-rename and restart test; no additional writer layer needed. |

## Waveform viewer

| File | Result |
| --- | --- |
| `waveform-viewer/src/App.tsx` | Reused shared signal-row defaults. Larger hook/effect ownership and canvas decomposition remain proposals. |
| `waveform-viewer/src/api.ts` | Explained the Retry-After seconds/date/backoff expression; kept sequence continuity and snapshot replacement checks. |
| `waveform-viewer/src/api.test.ts` | Retained exact u64 IDs, pagination, rollover and retry-state tests. |
| `waveform-viewer/src/bus.ts` | Explained decimal unknown fallback and partial leading-digit masks. Retained missing-bit slots and merge ordering. |
| `waveform-viewer/src/bus.test.ts` | Retained arbitrary-width, signed/inverted, membership and missing-probe coverage. |
| `waveform-viewer/src/busPlot.ts` | Explained bigint ratio scaling before Number conversion. |
| `waveform-viewer/src/busPlot.test.ts` | Retained unknown gaps and 64/1024-bit plot-range assertions. |
| `waveform-viewer/src/clock.ts` | Explained same-tick sequence ordering at clock edges; retained generic signal/bus sampling. |
| `waveform-viewer/src/clock.test.ts` | Retained clock-edge and unknown-transition contracts. |
| `waveform-viewer/src/components/BusPlot.tsx` | Named the marker X coordinate once for both SVG line endpoints. |
| `waveform-viewer/src/components/CommandEvents.tsx` | Shared command text formatting between tooltip, preview and VCD output. |
| `waveform-viewer/src/components/ContextMenu.tsx` | Retained menu rendering and keyboard navigation; no speculative menu framework. |
| `waveform-viewer/src/components/MaterialIcon.tsx` | Retained the existing path table and shared SVG renderer. |
| `waveform-viewer/src/components/Netlist.tsx` | Retained scope filtering and checkbox behavior; no additional tree abstraction. |
| `waveform-viewer/src/components/ResizeHandle.tsx` | Retained shared resize clamping and pointer/keyboard controls. |
| `waveform-viewer/src/components/Toolbar.tsx` | Retained the local button helper and existing action vocabulary. |
| `waveform-viewer/src/components/Welcome.tsx` | Retained the small empty-state presentation. |
| `waveform-viewer/src/components/Waveform.tsx` | Shared canvas row hit-testing and reused the sampled marker value; retained drawing and gesture order. |
| `waveform-viewer/src/components/drawBus.ts` | Retained clipping, bus segments and same-tick pulse markers. |
| `waveform-viewer/src/demo.ts` | Retained deterministic independent signal/event examples and pulse ordering comment. |
| `waveform-viewer/src/empty.ts` | Retained ordered empty-state selection and owner-state distinction. |
| `waveform-viewer/src/empty.test.ts` | Retained quiet/event-only visibility and status tests. |
| `waveform-viewer/src/layout.ts` | Reused row defaults directly instead of constructing a one-signal session during probe merging. |
| `waveform-viewer/src/layout.test.ts` | Retained persistence, capture changes, missing probes and malformed storage tests. |
| `waveform-viewer/src/main.tsx` | Retained the single application mount. |
| `waveform-viewer/src/plot.ts` | Retained bounded signed coordinate parsing. |
| `waveform-viewer/src/plot.test.ts` | Retained accepted formats and invalid/out-of-range coordinate cases. |
| `waveform-viewer/src/scroll.ts` | Retained explicit device/modifier precedence and shared wheel normalization. |
| `waveform-viewer/src/scroll.test.ts` | Retained pan, zoom and device detection coverage. |
| `waveform-viewer/src/state.ts` | Extracted `signalRow` for initialization, adding probes and layout merging; retained session validation. |
| `waveform-viewer/src/state.test.ts` | Retained selection, grouping and session validation scenarios. |
| `waveform-viewer/src/style.css` | Combined identical selected-row backgrounds and common tooltip declarations, retaining selector specificity and individual tooltip dimensions. |
| `waveform-viewer/src/trace.ts` | Shared command text formatting; named BigInt sequence operands to avoid repeated conversion. |
| `waveform-viewer/src/trace.test.ts` | Added fixed normal/truncated command-text expectations; retained sampling, zoom and tick labels. |
| `waveform-viewer/src/vcd.ts` | Reused command text without changing escaping, event grouping or timestamps. |
| `waveform-viewer/src/vcd.test.ts` | Retained exact same-tick signal/text output and safe escaping expectations. |
| `waveform-viewer/vite.config.ts` | Retained existing same-origin development proxy and relative asset base. |

## Build and deployment inputs

Reviewed root/crate Cargo manifests, `rustfmt.toml`, viewer package/TypeScript
configuration, HTML entry point, viewer Docker/Nginx files, Docker server variants,
Compose and the example server config. These define separate build/runtime
boundaries; this pass adds no dependencies, flags or deployment changes. Docker
runtime stages duplicate a few lines intentionally to keep the classic-builder
fallback independent from BuildKit cache mounts.

Also reviewed `.github/FUNDING.yml`, the build/Docker/test workflows, VS Code
launch/settings files, and the production Docker ignore file. Updated the VS Code
launch package, binary, filter and labels to the existing `mroww` Cargo target;
removed a stale port-version reference from the production Dockerfile comment.
The Docker publishing workflow still references an absent `docker/Dockerfile`.
Choose its intended default image geometry before repointing it to one of the
existing variants; this pass does not change publishing behavior. CI setup could
later share the existing `test.yml` toolchain/cache conventions, but no new
workflow abstraction is needed for a few setup steps.

The frontend lockfile and all registry/fixture inputs are preserved. The existing
documentation index remains a product/model reference; cleanup notes live in this
separate directory.
