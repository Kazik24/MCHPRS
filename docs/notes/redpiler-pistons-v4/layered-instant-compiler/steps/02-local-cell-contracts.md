# Step 02: derive local gate and resource contracts

Status: stationary first-response contracts implemented; broader primitive/lifecycle
coverage remains in progress. No compiler pass or hot runtime activation yet.

## Goal

Recognize physical instant mechanisms as bounded logical cells/groups with explicit
polarity, activation, accepted-use effects and reset rules. Do not trace the entire region.

## Files and responsibilities

- Add focused analysis under `analysis/instant/`, initially `cells.rs` and `mod.rs`.
- Put immutable contract types next to `program_graph/instant_regions.rs`, or in a focused
  `program_graph/instant_cells.rs` if size warrants it. Do not put analysis code in those types.
- Reuse/refactor `passes/utils/instant_mechanisms.rs`; do not copy its recognizer into a pass.
- Add `backend/graph/tests/instant_cells.rs` with fixture-derived semantic comparisons.
- No new registered compiler pass yet; step 03 is the first new pass entry.

## Algorithm

1. Build indexed incoming-power, update, observer-watch and contact/resource lookups.
2. Start from recognized reset families, preserving actor/resource/reset IDs and all
   allowed contacts. Unsupported geometry remains ordinary Graph with a reason.
3. Group shared payload/reset ownership before deriving independently executable uses.
   Close only actual shared identity/dependency, not common outside fanout.
4. Derive value predicates from prepared power paths, their guards and attenuation.
   Example: disappearance of every powered contributor is AND in a falling decoder;
   disappearance of a conserved shared payload can yield OR over accepted owners.
5. Derive activation independently from values. A negated predicate is not a new event.
   Preserve main/QC separation, blocking, inhibition and sample-versus-acceptance conditions.
6. Normalize local value equations to Buffer/Invert/All/Any, composing virtual equations
   when needed. Bind them to logical pins; never write a formula for an adder by name.
7. Attach accepted-use rules, compact owner-selection changes and reset/wake contracts.
   If a shared OR's value is simple but reset depends on which actor won, retain that selector.
8. Revalidate each emitted contract against the source graph witness. Contracts may rely on
   the declared protocol but not on an incidental saved wire strength or initial payload copy.

Only a local cell's semantic state may be explored. Do not restore generic symbolic Turbo,
region-wide Boolean state, world cloning or production enumeration of input masks.
Small exhaustive local valuations are tests of the contract, not its production derivation.

## Proof and tests

For each cell family, write its decoder, activation condition, accepted transition and reset
obligations beside the implementation. Compare with native/ordinary Graph over all small
legal input combinations, both input arrival orders, cancellation and blocked cases.

Required cases include private redstone payload, inert conductor payload, AND with late
second input, shared OR owner competition, inhibition arriving before/after sampling,
observer-triggered reset, and an ordinary BUD which must not become an instant cell.

Tamper witnesses: missing resource identity, wrong polarity, omitted guard, wrong callback
direction and ownership overlap must fail validation. Record unsupported families honestly.

## Gate

Primitive fixtures have graph-derived contracts with no raw wire-state inputs. Every
recognized ADDER1BIT actor has either a valid local/group contract or a precise unsupported
witness. Full all-actor coverage is required before claiming adder support; it is not
acceptable to call a few recognized private cells the compiled adder.

## Current implemented slice

`analysis/instant/cells.rs` derives typed first-response equations and resource-absence
outputs. Shared ownership lives in `analysis/instant/ownership.rs`, reused by existing
region discovery instead of duplicating that closure. Payload admission reuses geometry's
material predicate. Immutable, documented pin/gate/use/resource types live in
`program_graph/instant_cells.rs`. No World reads or input-mask enumeration enter derivation.
The new types and derivation are test-gated until the boundary pass consumes them; shared
ownership already serves the existing production discovery path. Remove those staging
gates when connecting the pass, rather than shipping unused code or blanket lint suppression.

The admitted entry requires extended, ready bases and enabled payloads at a common Far
contact. Activation, sampled power, requested retraction and live acceptance are separate
predicates. Base/head/resource obstructions remain guarded dependencies. Reset observers
provide source provenance; this is **not** yet an accepted-effect or reset-lifecycle proof.
Early drops, ordinary co-owners and unsupported common-source rules cannot be silently
compiled with these stationary predicates.

The seven focused tests passed on 2026-10-18:

| Fixture | Local ownership cells | Covered actor uses | Typed Graph query comparisons |
| --- | ---: | ---: | ---: |
| ADDER1BIT | 6 | 13 / 13 | 792 |
| ADDER11BIT | 65 | 142 / 142 | 8,479 |

These cells are small shared-token ownership units, **not** the final instant-region
partition. Independent BUD memory remains outside. Tests reject malformed expressions,
wrong polarity, weakened contribution guards, omitted resource outputs, broken reset
provenance and conflicting common sources. Existing private-proxy/recognition tests also
passed after the shared ownership refactor.

The physical-entry test also replays canonical data preparation for all eight one-bit
vectors and all forty eleven-bit histories. Before activation, every admitted base is
extended with a restored head and every payload is physically present at its contract's
source. This establishes reachability under the actual fixture preparation protocol;
testing only the imported saved pose would not establish it.

Power-link leaves still reference prepared physical sites with exact port, attenuation
and guard provenance. Later corridor extraction must replace internal reads; no internal
wire or callback is removed by this slice. Accepted transition effects, reset/wake plans,
construction budgets, full primitive family coverage and native lifecycle differential
execution remain required before this step's complete gate or public activation.
