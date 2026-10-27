# Documentation

These are the maintained general references. Source and runnable checks decide
behavior when a description disagrees with the implementation. Interpreter
compatibility, logical execution, and individual fixture results are separate claims.

| Document | Purpose |
| --- | --- |
| [Redstone model](REDSTONE_MODEL.md) | Electrical rules, component transitions, notifications, and scheduled work. |
| [Piston model](PISTON_MODEL.md) | Physical movement, BUD transactions, reset, and instant ordering. |
| [Interpreter architecture](INTERPRETER_ARCHITECTURE.md) | Physical execution, caches, verification, and reproduction commands. |
| [Redpiler architecture](REDPILER_ARCHITECTURE.md) | Graph and logical programs, ownership, timing, and interpreter handoff. |
| [Optimization levels](REDPILER_OPTIMIZATION_LEVELS.md) | O0–O6 selection, stable-input policy, and graph inspection. |
| [Development plan](REDPILER_DEVELOPMENT.md) | Consolidated compiler direction, unfinished work, and acceptance gates. |
| [Permissions](MCHPRS_PERMISSIONS.md) | Permission checks, configurable limits, and rank metadata. |
| [Waveform analyzer design](../scraps/docs/WAVEFORM_ANALYZER_DESIGN.md) | Interpreter capture, binary probes, public HTTP/SSE API and React viewer. |
| [Waveform analyzer plan](../scraps/docs/WAVEFORM_ANALYZER_PLAN.md) | Implementation sequence and acceptance gates. |
| [Waveform API specification](../scraps/docs/WAVEFORM_API_SPEC.md) | Public loaded-plot reads, compact signal/text events, command permissions and rank limits. |
| [Waveform viewer specification](../scraps/docs/VAPORVIEW_VIEWER_SPEC.md) | Screenshot layout, context menus, tick navigation and VaporView scrolling behavior. |
| [Waveform deployment](../scraps/docs/WAVEFORM_DEPLOYMENT.md) | Separate Docker frontend, private backend routing and proposed redstonefun.pl integration. |

For compiler work, read the architecture and development plan alongside the
physical models. The architecture and tier guide distinguish tagged v0.6.0 from
the original checkout's experimental work; their command policies differ.
The root [TODO](../TODO.md) tracks remaining work.
[Minecraft data](../mc_data/README.md) documents registry inputs and generation.

Keep only maintained, general references here. Put dated reports, measurements,
superseded proposals, and detailed investigation notes in [scraps/docs](../scraps/docs/README.md).
The [2026-10-21 archive](../scraps/docs/research-2026-10-21/README.md) preserves the
research consolidated into these guides, including previous TODO and benchmark
plans. The [architecture review](../scraps/docs/reports/REDPILER_REVIEW_2026-10-21.md)
records the release assessment and cleanup priorities.

Update the owning reference with semantic changes rather than creating another
status report here. Keep frozen inputs and machine-readable provenance under
`test_data/`, and generated traces in ignored output directories. Historical
measurements establish behavior only for their recorded revision and protocol.
