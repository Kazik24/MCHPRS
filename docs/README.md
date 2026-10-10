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
| [Piston model](PISTON_MODEL.md) | Physical transport, validation, ownership, BUD transactions and instant causality; shares Redstone state and scheduling. |
| [Interpreter architecture](INTERPRETER_ARCHITECTURE.md) | Physical execution domain, ordered callbacks, scheduling, dust and piston caches, and optimization benefits. |
| [Redpiler architecture](REDPILER_ARCHITECTURE.md) | Compilation lifecycle, inference, conditional geometry solver, instant pistons, optimizer, backend, and handoff. |
| [Redpiler parser](REDPILER_PARSER.md) | Spatial extraction, recognition, ownership, boundaries, and admission. |
| [Redpiler optimizer](REDPILER_OPTIMIZER.md) | Actual pass order, rewrite rules, and correctness conditions. |
| [Redpiler model](REDPILER_MODEL.md) | Region contract, graph transitions, memory, restricted activation, electrical boundaries and handoff. |
| [Interpreter verification](INTERPRETER_ARCHITECTURE.md#8-verification-and-reproduction) | Physical checks, frozen replay observations and reproduction commands. |

Each rule has one owner: Redstone defines shared electrical/state/time rules;
Piston defines physical movement and BUD transactions; Redpiler defines the
compiled projection and admitted protocols. Architecture references own data
structures, caches and lowering; parser/optimizer references own construction.

For an electrical change, start with the redstone model. For a movement or
notification change, also read the piston model. For a compiler change, follow
architecture, parser, optimizer, and compiled model in that order. Run the checks
for the affected contract, including rejected admission and interpreter handoff.

## Server references

| Document | Purpose |
| --- | --- |
| [Permissions](MCHPRS_PERMISSIONS.md) | Permission evaluation, action gates, configurable limits, and rank metadata. |
| [Minecraft data](../mc_data/README.md) | Pinned registry inputs and generated build assets. |

## Keeping the reference useful

Update the relevant model with any semantic change. Describe the general rule
and its actual guard conditions; a passing schematic does not establish a rule
for arbitrary circuits. Put fixture setup and reproduction commands under
`docs/tests/`, keeping exact binaries, protocols, and frozen expectations under
`test_data/`. Keep derived inspections and traces in their ignored output paths.

Implementation plans and dated investigations belong in `docs/notes/`.
The [general activation proposal](notes/NOTIFICATION_GATED_RESPONSES.md) describes
obligations beyond the restricted activation path in the active model.
Existing machine-readable provenance is evidence about its recorded revision,
not a description of the current runtime.

When moving definitions, update inbound links and heading anchors, including
references in tools and fixture reports. `tools/validate_docs.py` and some
historical test/notes guides are absent from this checkout. Present verification
references are linked above; missing guides are not advertised as runnable tools.
