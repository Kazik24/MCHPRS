# Step 02: derive local gate and resource contracts

Status: planned; requires approved step 01. Stop after the local-contract review.

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
