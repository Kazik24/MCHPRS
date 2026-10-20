# Dense transport baseline — 2026-10-20

The accepted starting point for whole-circuit minimization is **300 TPS with
correct BubbleSort outputs**, with `compile-instant-transport` enabled at O5.
The current pass replaces eligible propagation episodes with dense indexed
transport execution. It does not yet replace the whole circuit with a minimized
input/output program.

## Correctness and coverage

The source compiler admits 2,138 closed queue-only transport programs covering
17,426 of the CPU's 41,354 wires. It preserves first-discovery captures, shared
receiver layer marking, heading/bias, the three rotating FIFO queues, live
callback admission and actual source routes. It caches only no-dust drives
certified independent of the changing interior wire state. Unsupported geometry,
receivers and overlapping execution owners retain their existing execution.

The full 10,036-tick sort matched all 768 native RAM cells at every tick and
finished with RAM `0..15`. Complete component/piston/resource state and ordered
pending execution matched the existing O5 graph at every tick. The regression
suite passed 563 tests, with 62 opt-in research tests ignored. Focused checks
include every three-wire strength combination at all three entry roots, the
order-sensitive adder history and interpreter handoff, forged plans,
cancellation, and direct/contact-mediated interior no-dust dependencies.

| Full-sort work | Existing O5 | Dense transport |
|---|---:|---:|
| General wire walks | 3,581,461 | 2,009,756 |
| Dense transport invocations | 0 | 1,571,705 |
| Dense internal wire visits | 0 | 25,305,265 |
| Callbacks | 202,834,986 | 202,834,986 |
| Piston samples | 35,415,396 | 35,415,396 |
| Piston actions | 3,295,083 | 3,295,083 |
| Scheduled component ticks | 2,344,242 | 2,344,242 |

The 43.88% reduction in general walks is not an inference speedup measurement:
the dense program still performs its own internal wire visits and boundary work.

## Enabled production measurements

Production `bench/release`, workspace fat LTO, O5, budget 8, no stable-input
promise. Each sample runs the frozen complete CPU sort for 10,036 game ticks.
Inference excludes preparation, activation, presentation, RAM observation,
reset and continuation. The benchmark checks the frozen RAM trace and handoff.

| Completed sample | Inference seconds | TPS |
|---|---:|---:|
| 1 | 33.641824 | 298.32 |
| 2 | 32.314489 | 310.57 |
| 3 | 31.290972 | 320.73 |

The user ended benchmarking during sample 4 and accepted 300 TPS as the next
phase's baseline. These are three completed samples, not a completed five-run
study, and there is no matched production comparison against the disabled pass.
The accepted baseline must not be described as a measured speedup ratio.

Artifacts: `target/instant-transport/enabled-baseline-summary.json`, the matching
log and provenance JSON, and `bubblesort-enabled-baseline.exe` (SHA-256
`D64C04F02C63F433574D42D44635CB865F8CB57D3780ED7A2E4795658CF4E695`).
Source HEAD was `8d7d23eb5e0db8869b197944bb862562094a68a1` plus the recorded
working-tree changes. Source hashes are in the provenance artifact.

## Next goal

Compile the whole circuit into the simplest practical stateful program. Preserve
controls, memory reads/writes, exposed signals and future effects. Replace the
interior's executable wire walks, general power queries and physical callbacks
with minimized value expressions and ordered state/effect operations. Retaining
cold identities for inspection does not authorize their use during inference.

Whole-circuit completion requires complete interface coverage and behavioral
equivalence, plus counters proving that replaced interiors no longer execute the
original transport/query/callback machinery. A connected group, a compact copy
of the physical graph or a correct final RAM snapshot alone is insufficient.
