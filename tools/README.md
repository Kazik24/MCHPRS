# Repository tools

Start with [interpreter verification](../docs/INTERPRETER_ARCHITECTURE.md#8-verification-and-reproduction)
for physical checks and replay observations, or
[compiler verification](../docs/REDPILER_ARCHITECTURE.md#9-commands-limits-and-verification)
for Redpiler checks and admission limits.

| Tools | Purpose |
| --- | --- |
| `generate_mc_data.py` | Rebuild pinned registry/tag binary assets; see [Minecraft data](../mc_data/README.md). |
| `inspect_instant_pistons.py` | Decode schematics without simulation. |
| `instant_piston_protocols.py`, `instant_piston_io_protocols.py` | Define fixture actions and port protocols. |
| `capture_instant_pistons.py`, `capture_instant_pistons_java.py` | Capture new MCHPRS/independent Java physical episodes. |
| `analyze_instant_pistons.py` | Index captures and build observation projections. |
| `validate_instant_pistons.py`, `validate_instant_io.py` | Check pack hashes, coverage, coordinates, comparisons, and documentation links. |
| `capture_piston_research.py`, `summarize_piston_research.py` | Capture and summarize larger declared protocols. |

The protocol modules define each piston pack's actions and observation ports.
The [CPU reference guide](../test_data/cpu-references/README.md) covers frozen
replays and benchmark setup.

Schematics, fixture protocols, provenance, and frozen CPU/Java expectations remain
versioned under `test_data/`. Inspections, detailed traces, projections, and trace
indices are ignored local outputs. Use fresh destinations for captures and retain
their source identities. Generated artifacts cannot replace independent expected
outputs simply because a comparison fails.

Capture, inspection, protocol, analysis, and validation tools remain the supported
workflow. One-off experiment captures and diagnostics are archived under
`../scraps/tools/`.
