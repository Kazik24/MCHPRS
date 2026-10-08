# Documentation

These documents describe the implementation in this repository. The Rust source
and runnable checks are the authority when a description and behavior disagree.
Minecraft compatibility, physical interpreter behavior, and compiled logical
behavior are separate claims.

## Simulator and compiler

Read the models before changing timing, power queries, spatial extraction, or
graph rewrites:

| Document | Purpose |
| --- | --- |
| [Redstone model](REDSTONE_MODEL.md) | Spatial signal algebra, component transitions, dust propagation, and scheduled work. |
| [Piston model](PISTON_MODEL.md) | Piston events, moving payloads, quasi-connectivity, BUD sampling, instant circuits, and nano/pico stepping. |
| [Redpiler architecture](REDPILER_ARCHITECTURE.md) | Compilation lifecycle, inference, conditional geometry solver, instant pistons, optimizer, backend, and handoff. |
| [Redpiler parser](REDPILER_PARSER.md) | Spatial extraction, recognition, ownership, boundaries, and admission. |
| [Redpiler optimizer](REDPILER_OPTIMIZER.md) | Actual pass order, rewrite rules, and correctness conditions. |
| [Redpiler model](REDPILER_MODEL.md) | Weighted graph execution, retained state, notifications, scheduling, and handoff contracts. |
| [Execution generalization scope](REDPILER_PARTIAL_COMPILATION.md) | Combined BUD presentation, qualified notification execution, and native/compiled ownership proposal with separate acceptance gates. |
| [Test suites](tests/README.md) | Checks, frozen references, capture procedures, and limits of the evidence. |

For an electrical change, start with the redstone model. For a movement or
notification change, also read the piston model. For a compiler change, follow
architecture, parser, optimizer, and compiled model in that order. Run the checks
for the affected contract, including rejected admission and interpreter handoff.

## Server references

| Document | Purpose |
| --- | --- |
| [Permissions](MCHPRS_PERMISSIONS.md) | Permission evaluation, action gates, configurable limits, and rank metadata. |
| [Plot Git](PLOT_GIT.md) | Commands, access, storage, checkout, and recovery. |
| [Wire pen](WIRE_TOOL.md) | Live dust routing, controls, safety checks, and search limits. |
| [RedstoneVC client parity](REDSTONEVC_CLIENT_PARITY.md) | Reference plugin commands, presentation, HID, and differences from Plot Git. |
| [Client synchronization](CLIENT_SYNC.md) | Authoritative state, interaction barriers, rendering, and outgoing queues. |
| [Minecraft data](../mc_data/README.md) | Pinned registry inputs and generated build assets. |

## Keeping the reference useful

Update the relevant model with any semantic change. Describe the general rule
and its actual guard conditions; a passing schematic does not establish a rule
for arbitrary circuits. Put fixture setup and reproduction commands under
`docs/tests/`, keeping exact binaries, protocols, and frozen expectations under
`test_data/`. Keep derived inspections and traces in their ignored output paths.

Implementation plans, task prompts, deployment diaries, and dated performance
reports are not reference documents. Git history retains deleted material.
Existing machine-readable provenance is evidence about its recorded revision,
not a description of the current runtime.

Check local documentation links with:

```sh
python tools/validate_docs.py
```
