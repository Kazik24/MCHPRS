# Repository tools

[Test-suite documentation](../docs/tests/README.md) describes runnable checks,
prerequisites, and the evidence each suite compares. Start there when changing
the interpreter or compiler.

| Tools | Purpose |
| --- | --- |
| `validate_docs.py` | Check local Markdown links and heading anchors. |
| `generate_mc_data.py` | Rebuild pinned registry/tag binary assets; see [Minecraft data](../mc_data/README.md). |
| `inspect_instant_pistons.py` | Decode schematics without simulation. |
| `instant_piston_protocols.py`, `instant_piston_io_protocols.py` | Define fixture actions and port protocols. |
| `capture_instant_pistons.py`, `capture_instant_pistons_java.py` | Capture new MCHPRS/independent Java physical episodes. |
| `analyze_instant_pistons.py` | Index captures and build observation projections. |
| `validate_instant_pistons.py`, `validate_instant_io.py` | Check pack hashes, coverage, coordinates, comparisons, and documentation links. |
| `capture_piston_research.py`, `summarize_piston_research.py` | Capture and summarize larger declared protocols. |

The [instant-piston guide](../docs/tests/INSTANT_PISTONS.md) gives exact commands
and distinguishes the original pack from the I/O revision. The
[CPU guide](../docs/tests/CPU_REFERENCES.md) covers replays and benchmarks.

Schematics, fixture protocols, provenance, and frozen CPU/Java expectations remain
versioned under `test_data/`. Inspections, detailed traces, projections, and trace
indices are ignored local outputs. Use fresh destinations for captures and retain
their source identities. Generated artifacts cannot replace independent expected
outputs simply because a comparison fails.

Capture, inspection, protocol, analysis, and validation tools remain the supported
workflow. One-off experiment captures and diagnostics are archived under
`../scraps/tools/`.
