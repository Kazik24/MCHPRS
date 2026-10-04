# Master feature audit after the 1.21.5 piston repair

Date: 2026-10-04. Compared the current `port/piston-1.21.5` implementation with the pinned `origin/master` (`76b9b8c`) and `upstream/master` (`d492e43`) histories described in [CURRENT_VS_MASTER.md](CURRENT_VS_MASTER.md). That earlier document compares the recovered 1.18.2 baseline; it is preserved as historical evidence.

## Compatible changes

| Surface | Result in this branch |
| --- | --- |
| Client version / registries / configuration / chunks / items / acknowledgements | Already ported using exact 1.21.5 data, protocol 770, DataVersion 4325; copying master's 1.20.4 codecs would regress compatibility |
| Front/back sign NBT, player synchronization, generated sound IDs | Already covered by the protocol port and independent client tests |
| Automatic Redpiler default | Changed to false; explicit existing configuration remains effective. Piston/observer plots retain the interpreter guard |
| Plot selection | Added `/plot select` and `/p sel`, including permission check and advertised command nodes |
| Build version | Added `/version`, `--version` and `-v`; package version comes from the workspace manifest |
| Rates | Removed the 100000 RTPS ceiling; added no-argument WSR query and zero to disable periodic sends. Guarded zero-duration arithmetic and large-rate monitor calculations |
| Timing monitor | Fixed reset-counter underflow when starting/stopping or changing rates; widened report sums to avoid overflow |
| Empty-container comparators | Missing/wrong block entities now produce zero signal instead of panicking; existing inventory import already handles empty contents |
| End portal frame | Eye property supplies comparator strength 15, otherwise 0; interpreted and compiler identification share the override |
| Binary pressure plates | Added interpreted power, wire connectivity, support checks, player movement and compiler flush for wood and polished blackstone; generated data also supplies pale oak |
| Signs / stone bricks / glowstone | Added bamboo, cherry and mangrove sign placement, support checks, entities, annotation lookup and rotation/flip; added solid stone bricks. Glowstone and other existing block definitions were already present |
| Repeater short pulses | Scheduled the missing off transition with the selected delay; verified all four delays and optimized/unoptimized handoffs |
| Ground torch support power | Excluded downward weak power in both interpreter and compiler input search |
| Trapdoor defaults / opening | Existing bottom-half default and powered-to-open state mapping already implement the applicable fixes; arbitrary imported open/powered combinations remain outside the typed legacy model |
| Note-block tuning | Plays the newly selected note when the space above is air |
| Compiler correctness | Removed AnalogRepeaters; protected pending ticks before optimization; preserved initial transitions and coalesced only matching states; prevented newly created constants from being removed when graph indices are reused. Orphan pruning already retains inputs |
| Compiler synchronization | Existing advancement/reset/interaction flush and pending-tick transfer retained; stale typed ticks are rejected before compilation |
| Sponge / offsets / containers | Existing v2/v3 validation, lowercase entity IDs, WEOffset precedence, missing offsets and Count/count tolerance retained |
| WorldEdit flags | Combined flags are removed once; missing flag arguments are rejected before indexing |
| WorldEdit paste / undo / help | Added `//paste -s`, inclusive selection bounds, reverse application of overlapping undo clipboards and alias-aware help; `//count` already parses a mask |
| Plot failures / migrations / shutdown | Existing handled load errors, preserved originals and atomic backed-up migrations retained; format 4 stores piston timing and typed work |
| Lazy change tracking | Adapted in the performance milestone; untouched sections no longer allocate a 4096-entry change array |
| CI / data generation | Existing locked builds, data reproducibility and format checks retained; removed obsolete known-failing-test wording |

## Changes excluded because their interfaces conflict with this fork

These are deliberate exclusions from task 6, not claims that all upstream functionality is present.

| Change | Reason |
| --- | --- |
| Master's 512-wide / 384-high world geometry and save readers | Would reinterpret existing plot coordinates/section arrays. Production's plot-scale build option remains separate; upstream format 2/3 files need their own registry/geometry migrations |
| Fractional RTPS / WSR | The fork persists integer rates and uses integer atomics/timing APIs. Adopting master's float layout would need another versioned save migration and revised timing semantics |
| Velocity replacing Bungee forwarding | Master's forwarding configuration and login plugin/HMAC exchange replace the existing proxy contract. Keep the existing path until a separate proxy migration is selected and tested |
| Signal-set analysis, custom graph, multiple-block nodes, pass registry, RIL diagnostics and standalone compiler | Depend on the newer compiler/world API extraction. Selective correctness fixes above preserve the fork's shared scheduler and nano/pico behavior |
| New display compiler flags / backend link layout / batched APIs | Depend on those graph/backend replacements; not equivalent independent patches to this fork |
| Standalone world exporter / Rayon traversal | Reads master's save geometry and registry IDs; cannot safely consume this fork's version-4 piston state without a separate exporter adaptation |
| Crate extraction, Rust-2024 conversion, Nix/dev tooling | Coupled upstream development architecture, not required runtime fixes; kept this branch's build and module interfaces |

## Evidence and limits

The focused regressions exercise short-pulse duration, scheduled handoff preservation, missing-container behavior, frame eyes, torch direction, all 14 binary plate species, sign support, constant-index reuse and monitor overflow. The independent protocol runner also exercises version/rate/selection commands, combined paste flags, alias help and overlapping rstack undo/redo.

The Chungus handoff check now compares visible outputs with uninterrupted interpreted execution. Its old whole-world hash included internal optimized-away nodes and the old lost-pulse behavior; the other historical Chungus hashes remain unchanged. This does not claim compiled piston support or full upstream compiler equivalence.

Movement rules and adhesive graphs remain deferred by user instruction. General new vanilla block gameplay, weighted/entity-driven plate behavior, graphical-client animation checks and unsupported external save histories are separate work.
