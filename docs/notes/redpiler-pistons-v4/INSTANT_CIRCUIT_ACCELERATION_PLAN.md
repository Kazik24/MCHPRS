# Executable instant-circuit kernels

## First gate: CPU_BUBBLESORT interface

The 2026-10-20 source-graph audit finds one connected computation candidate:
15,200 instant actors, 41,353 wires, and an explicit physical perimeter. All
768 documented RAM cells have both read and write/notification references.
All 53 controls (52 buttons and one lever), including Start, are retained.
The merged perimeter contains every one of the 56,153 crossing power links
and 19,615 crossing callback occurrences; the audit finds zero omissions.

All 16,561 physical pistons and 13,118 resources remain represented. The
1,361 ordinary pistons include external memory/state machinery, whose effects
must be represented in the new kernel interface rather than discarded.
There are 41,354 physical wires in total; any wire outside the computation
candidate remains explicitly inventoried as peripheral transport.

This establishes a connected compilation candidate and retained source
dependencies, **not** an already accelerated executable. The current event
kernel owns zero wires. Its perimeter is not yet installed as a standalone
runtime interface, and memory transactions and scheduler effects are still
implicit in physical-node execution. The implementation below must close
those gaps.

Reproduction: ignored test
`kernel_interface::bubblesort_kernel_interface_is_complete_in_source_graph`.
Artifact: `target/instant-kernel-interface/cpu-bubblesort-interface.json`.
The artifact enumerates RAM cells, controls, peripheral nodes, typed perimeter
counts, resources, and crossing-edge checks against the source graph.

## Contract

Replace each admitted physical instant region with one executable stateful
graph, with original piston/resource identities and a complete typed interface.
Its inference must not need the original World or a physical block-neighborhood
walk. Keep logical state transitions and the causal relationships that affect
memory or exposed outputs. A connected group or renamed call into Turbo is
not this implementation.

Inputs include control actions and levels, clock advancement, memory/state
reads and readiness, external power/update events, and imported pending work.
Outputs include signal levels and falling events, delivered memory-write
samples/requests and accepted writes, reset/tick requests, deferred publications,
and exposed component effects. A level and its update notification are distinct.
The boundary must preserve route provenance, input sampling cuts, priorities,
deadlines, revision checks, and the ordering of dependent effects.

The new graph retains each piston as a stateful node, including both storage
and read pistons. Memory cannot be replaced by an unqualified Boolean bit while
its availability or in-flight writes matter. Stateful boundary primitives may
reuse existing piston/scheduler commit helpers; interior transport must execute
the new compiled representation.

## Algorithm

1. **Bind the complete region and interface.** Start from the validated
   BoundaryCatalog and merged RegionPartition/perimeter. Bind every owned
   actor, resource, contact selector, wire corridor, entry, and outside effect.
   Audit the interface against the original power/callback rows and stateful
   peripherals. No unexplained missing edge or state owner is admissible.

2. **Build two linked graphs.** A pure value graph describes power/guard
   expressions with attenuation and contact selection. An ordered event graph
   describes update delivery, piston sampling/acceptance, resource writes,
   reset, and future publication. The same value on two events does not make
   the events interchangeable. Keep original IDs in cold provenance tables;
   use dense local IDs in the executable arrays.

3. **Compile transport between piston nodes.** Trace fixed geometry once at
   compilation. Replace interior dust visits and passive block callbacks with
   transport programs whose leaves read typed live inputs and whose outputs
   deliver ordered receiver samples. Precompute reachability/attenuation where
   valid. A maximum-of-weighted-sources equation computes a settled value only;
   it is not, by itself, a replacement for update delivery or intermediate
   threshold crossings.

   For the first exact specialization, use bounded fixed-geometry corridors
   with queue-only receivers and a small source/entry-state family. Compile
   the exact reference transition into a compact event transducer: an entry
   guard, retained transport state, pure value operations, ordered sampling
   cuts, and next state. Preserve first-discovery observations and the first
   qualifying request/source route. Conditional entries must be checked before
   any optimized effect. Unsupported entries retain an explicit counted
   fallback; a region with such runtime fallback is not reported as fully
   replaced.

   A reference transition may be symbolically specialized or exhaustively
   compiled over a bounded finite family. Bound compile time, retained states,
   and memory; never assume that testing one driver vector certifies others.
   Inputs outside the certificate must fail entry before mutation. Any removed
   interior state needed for fallback or projection must be materialized from
   the transducer state, not read from a stale shadow circuit.

4. **Execute causal events on the compiled graph.** An input activates its
   compiled entry. Follow precompiled dependency/transport edges, enqueue
   target piston work, and commit through the original stateful semantics.
   FIFO successors remain behind already queued peers where that order matters.
   A chain must not run recursively to completion ahead of a sibling chain.
   No universal simultaneous-layer rule is assumed: shared resources and
   acceptance reads retain their explicit dependencies and arbitration.
   Graph construction is cold; runtime does not rediscover physical layers.

5. **Minimize only under the transition contract.** Fold constants, remove
   proven passive work, hash-cons pure expressions at the same sampling cut,
   remove dead reads/edges, and fuse equivalent typed reads. Do not remove a
   piston/buffer merely because its settled Boolean value is an identity;
   it can contribute a causal step. Do not merge observations across a write
   or capture boundary. An event may be removed only if its reads, effects,
   and generated pending work are redundant under the certificate.

6. **Pack for CPU execution.** Store opcodes, operands, successor ranges,
   state slots, guards, and output bindings in contiguous arrays. Reuse local
   scratch with nested-invocation isolation. Avoid hot-path block reconstruction,
   physical-neighbor discovery, hash maps, allocation, and indirect dispatch
   per dust block. Start with efficient scalar operations; add SIMD only to
   measured batches of independent operations, preserving event dependencies.

7. **Keep entry, exit, and fallback honest.** Import control, piston/resource,
   and pending state. Export interface effects and continuation state. Keep
   original physical projection cold and separate from inference. Count every
   general transport fallback and expose the uncovered region; installed
   kernel count is not a coverage or performance claim.

## Why this preserves NANOTICK_EXAMPLE

The pinned supplied example has five piston actors. Its measured first-wave
order is `delay-1, ordinary, delay-2, downstream, inhibit`: the ordinary branch
queues downstream before delay-2 queues inhibit. Downstream therefore retracts
before the inhibitor becomes effective. The replacement must reproduce that
behavior, including reset/reuse; it must not repair the designer's race by
running the entire delayed branch first or evaluating only settled Boolean
values. Keep the unequal actor chains and their activation/acceptance edges.

## Implementation and acceptance order

1. Add a **separate pass**, typed plan output, independent validation, lowering,
   runtime, interface export, coverage and operation counters. Reuse current
   state/scheduler owners. Keep it experimental until its certificates pass.
2. Deliver an executable small region first, with actual dust traversal removed:
   single-wire controls, NANOTICK_EXAMPLE, ADDER1BIT and its prepared-0-1-1
   ordering counterexample. Cover rotations, rising/falling inputs, shared
   resources, interrupted/reset entries, and native continuation.
3. Grow through ADDER11 and Counter, including Counter's sixteen labeled BUD
   cells. Preserve whole input/output histories, not only final settled bits.
4. Lower BubbleSort's one candidate with the audited full interface. Report
   exactly which transport is compiled, which state and effects remain, and
   whether any general walks/callbacks still execute. Validate its entire
   10,036-tick four-bank RAM trace, availability, final sort, external controls,
   reset/pending work and continuation. Extend input histories before promotion.
5. Validate FPU_MROWW with the green-wool trigger and seeded random histories.
   Do not claim arithmetic equivalence from a native output bank that stays
   0xffff, or call ordinary-piston probes a certified FPU memory map.
6. Measure fresh alternating production-release pairs after correctness passes.
   Compare interpreter, current correct maximum compilation, and the new pass
   on identical inputs. Report inference TPS separately from preparation and
   projection; target a material BubbleSort gain (at least 10% lower median
   inference time), and explain work removed. A slower prototype or uncovered
   whole region is unfinished, even if it groups into one kernel.

Each transport certificate must give a state mapping and a transition argument:
at every exposed sampling/commit boundary the same relevant values are read and
the same dependent output/future effects are produced. Internal queues need not
be byte-identical if a different representation has that correspondence. The
memory-only strict/relaxed experiments are useful evidence, not the certificate.
