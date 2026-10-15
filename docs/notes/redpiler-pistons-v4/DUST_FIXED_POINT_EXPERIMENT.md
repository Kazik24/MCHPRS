# Experimental final-state dust solver

Date: 2026-10-14. Historical first experiment before the boundary fix. See [the follow-up investigation](DUST_FIXED_POINT_INVESTIGATION.md) for the corrected solver, matching CPU RAM and remaining ordering counterexamples. This replaces the removal experiment, not the production Turbo executor.

## Implementation

`passes/settle_dust_networks.rs` disables ordered-walk specializations while retaining boundary callback routes. The `unsafe-turbo-fixed-point` core feature appends this pass after the selected normal pipeline and selects the solver in `backend/graph/fixed_point.rs`. Normal builds and optimization tiers are unaffected.

For each changed seed, discover electrical wire neighbors using the same direct, ascent and descent conditions as general seed sampling. Sample non-dust input power once per wire. Build walk-local attenuation edges, solve from zero in temporary arrays with at most fifteen synchronous rounds, and publish final strengths together. Zero initialization eliminates stale self-supporting cycles; four-bit strength loses one per hop. Capture and admit boundary notifications from final changed wires and the triggering root, then deliver once per site. Nested boundary callbacks may start another solve.

This deliberately discards intermediate values, repeated deliveries and native order. Contact-dependent topology is sampled per solve, not cached across transitions. It does not certify observer, instant or BUD semantics. There are no Motion/progress objects or additional simulation state owners.

The first prototype incorrectly discovered networks through all 24 notification neighbors and repeatedly sampled general power during relaxation; its slow CPU run was stopped. The final prototype discovers electrical edges instead and samples external power once.

## CPU_BubbleSort result

Command:

```text
cargo bench -p mchprs_core --bench cpus --features unsafe-turbo-fixed-point --locked -- --cpu cpu_bubblesort --graph-short --graph-tier 3 --iterations 1 --output target/cpu-dust-fixed-point.json
```

One release sample, 100 tick calls, with setup and compilation outside the timer. The same approved in-memory sign removal is applied to both inputs; the schematic on disk is unchanged.

| Observation | Experimental solver | Interpreter |
|---|---:|---:|
| Timed execution | 0.0073849 s | 0.7337175 s |
| Distinct sampled RAM snapshots | 1 | 4 |
| Last data RAM | 15,14,13,...,0 | 14,15,13,12,...,0 |

Experimental work: 4,697 callbacks, 300 wire episodes, 1,699 piston samples, 281 accepted actions/publications and 261 scheduled ticks. A preceding normal O3 reference completed the same 100 ticks in 1.079654 s and passed its interpreter comparisons. These are single runs, not a paired speed benchmark. The large timing reduction is invalid as a performance claim because the CPU stalls.

RAM trace, continuation outputs, reset future, continuation future, reset checkpoint and continuation checkpoint all differ from the interpreter. This first run does not isolate an inherent limitation of final-state propagation: the follow-up found dropped external-wire notifications and corrected that implementation bug. The repaired solver still has ordering counterexamples documented in the follow-up investigation. It does not establish whether the first failure comes from event order, multiplicity or lost transients: the experiment changes all three.

Artifacts: `target/cpu-dust-fixed-point.json` and `.log`; normal reference `target/cpu-turbo-reference.json`. Feature builds record failures under `fixed_point_comparisons`; successful process exit is not a correctness pass.

## Verification and next use

The solver test covers 32 powered/unpowered and stale-strength combinations on an 18-wire line, including complete stale-power decay and attenuation beyond the power range. The pass test verifies retained boundary routes, unchanged entry nodes, structural validation and lowering. Three existing isolated-wire ordering/reset/nested-reentry tests pass in a normal build. Diff checks pass.

Keep this experiment disabled. The useful next direction is selective certification of private dust regions or full instant mechanisms, preserving the required boundary events. A whole-network final-state solver without such a boundary proof is not a substitute for instant proxies.

Not checked: full redpiler/workspace suites, exhaustive branched/contact-changing networks, full CPU sorting, repeated timing pairs or first-divergence causality analysis. No correctness or production speedup is claimed.
