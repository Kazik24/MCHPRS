# Agent task: document and test the instant-piston schematic pack

You are working in the MCHPRS repository at F:\rustrepos\MCHPRS. Your task is to establish reproducible, evidence-backed behavior specifications and tests for the supplied instant-piston examples. Finish the documentation, fixture manifests, trace capture and meaningful regression tests before implementing a Redpiler instant parser.

The result must explain what each circuit does, why it does it, when its output is valid, what reset does, and which update-order assumptions a future compiler must retain. A schematic filename or a final output bit is insufficient evidence.

## 1. Authoritative context and scope

Read the applicable AGENTS.md files and inspect the current working tree. This is a shared checkout with other work in progress. Preserve unrelated changes and record the revision and relevant modified-source hashes used for your observations.

Read:

- docs/INSTANT_REDPILLER.md
- docs/ANSWERS.md
- docs/INSTANT_PISTON_IMPLEMENTATION_PLAN.md
- docs/REDSTONE_MODEL.md
- test_data/instant-pistons/download-manifest.json

Inspect the actual interpreter, import and stepping code rather than assuming the documentation is current:

- crates/core/src/redstone/mod.rs
- crates/core/src/redstone/piston.rs and piston/tests.rs
- crates/core/src/redstone/wire/mod.rs and wire/turbo.rs
- crates/core/src/redstone/observer_tests.rs and adder_tests.rs
- crates/core/src/plot/mod.rs and plot/piston_tests.rs
- crates/core/src/plot/worldedit/schematic.rs
- crates/core/src/plot/worldedit/clipboard.rs, if present, and the actual paste implementation
- crates/world/src/lib.rs and the scheduled-tick/phase definitions it references

The implementation plan describes proposed compiler functionality. Redpiler currently cannot execute these pistons merely because a schematic appears logically combinational. This assignment establishes interpreter evidence and compiler requirements; it does not enable piston compilation.

The circuit author's latest clarifications take precedence over older planning assumptions:

1. Logical one is a nonzero-to-zero transition. The low state enables downstream instants to retract and schedule their update cycles. Static zero is not a newly generated external trigger; internally generated reset cycles still belong to the triggered response.
2. Computation is trigger-driven, beginning from a stable extended mechanism. Removing a powering wire or circuit is a common stimulus.
3. Outputs are found where instant-driven wires or update paths reach non-instant consumers, such as a repeater, lamp or BUD switch. Labeled physical outputs are also useful fixture observations.
4. Internal simplification is allowed if externally significant behavior is preserved.
5. New external inputs or another computation during reset are undefined for initial conformance. Do not use them as normal truth-table cases. Internal reset and propagation effects remain part of valid execution.
6. Shared-output OR mechanisms may drop the payload and transfer it between pistons.
7. There is no typical standalone NOT gate in this instant logic. A negation circuit inhibits or blocks other pistons when its input is active.
8. Negation requires update-order synchronization. The author specifically says: “negated piston must fire first, update-wise.” Identify the physical piston meant by this statement and what “fire” means operationally. Do not silently replace it with an assumption about which actor moves first.
9. Dedicated BUD examples and circuits demonstrating nanotick misalignment have not yet been supplied. Existing embedded memory or BUD-like mechanisms still need documentation.
10. Here “nanoticks” also refers to circuit synchronization and relative update ordering. MCHPRS's nanotick_advance API is an inspection mechanism with its own semantics. Advancing a correctly synchronized circuit with that API does not turn it into a supplied nanotick counterexample.
11. Gates, adders, counters, decoders and long wires are initial targets. Full CPU analysis and compilation are later work.
12. Compiled rendering may update infrequently and omit internal piston animation and internal wire-state display. Simulation timing and interpreter handoff remain separate correctness questions.

## 2. Files to account for

Inventory every schematic in test_data/instant-pistons. Give the following basic examples a full behavior dossier and supported tests:

| Group | Schematics |
| --- | --- |
| Single mechanisms and resets | INSTANT_OBSERVER.schem, INSTANT_TORCH.schem, INSTANT_RESET_REDSTONE.schem, INSTANT_RESET_REDSTONE_2.schem, INSTANT_DOWN.schem, INSTANT_DOWN_TORCH_RESET.schem |
| Inhibition and propagation | INSTANT_BLOCKED.schem, INSTANT_CHAIN.schem |
| Shared output and conjunction | OR_1.schem, AND_1.schem, AND_2.schem, AND_3.schem |
| Order-dependent negation and XOR | NOT_1.schem, XOR_Simple.schem |
| Arithmetic and state | ADDER_1BIT.schem, ADDER_11BITS.schem, COUNTER_BASIC.schem |
| Intentional counterexample | OR_Interpreter_illigal.schem |

Also document and test the downloaded MCHPRS_REDSTONE_UPDATE_EDGECASE.schem as its own version. Its hash and dimensions differ from test_data/MCHPRS_REDSTONE_UPDATE_EDGECASE.schem. Keep the older reference fixture separate.

ADDER_11BITS.schem is the canonical corrected replacement for ADDER_GWIEZDNY_TEST.schem. 

The refreshed canonical adder has dimensions 21 x 7 x 46 and six signs labeled A1, A2, B1, B2, O1 and O2. The initial downloaded revision was 21 x 7 x 45 without sign entities. Recheck the actual file and hash: existing test/capture helpers may still refer to the earlier revision's dimensions, offsets or coordinates. Find the actual trigger separately; the six observed signs do not include a TICK label.

The old ADDER_GWIEZDNY_TEST.schem produced a changed-input discrepancy in its historical reference. That observation is not an expected result for the corrected adder. The author will remove the old file later; do not delete it or relabel its frozen traces as belonging to the replacement.

PM1_SORT.schem and Q2CK_LyCore5_for_sorting.schem were also downloaded. Inventory them and explicitly mark full behavior coverage deferred. Do not turn this assignment into a CPU project.

## 3. Decode geometry and sign labels before designing stimuli

Build or reuse a deterministic schematic inspection tool. Decode the NBT version, dimensions, palette, varint block data, offsets and block entities. Check the decoded cell count and record the SHA-256 of the exact binary.

Extract all sign labels and explanatory text, including front/back text and supported legacy sign fields. Text can be a plain string, a JSON-encoded text component, an object with text/extra fields, or a list of components. Preserve the original text alongside normalized readable text.

Use one explicit coordinate convention: selection-local coordinates relative to the saved minimum corner. Record the paste origin, loader offsets and resulting absolute coordinates separately. Verify the coordinate mapping against the actual loader and paste routine.

For every label, distinguish the sign's position from the referenced block, dust, payload or net. Labels can describe nearby blocks, blocks below them or a redstone connection. Inspect geometry and electrical/update paths before assigning a port. Record the candidate positions, final mapping and evidence. Do not assume “nearest block” or a universal “block below sign” rule.

Resolve ambiguous or absent ports through source inspection and controlled experiments. Ask a focused author question when needed, while continuing independent fixtures. Mark unresolved mappings explicitly; do not invent expected behavior.

Signs may mention BUD or memory within a larger fixture. Document those roles without claiming the author has supplied standalone BUD acceptance examples.

## 4. Specify the protocol for each fixture

For each canonical fixture, document:

- Exact initial block/entity state, expected readiness and relevant queued work.
- The import/setup operation: strict paste, normal notified placement, raw storage writes, settling or a specified construction sequence.
- All inputs, inhibit/negation inputs, independent updates, trigger locations and outputs.
- Prepared conditions and physical signal encoding, separate from logical falling-event encoding.
- Exact stimulus operations in execution order, including changed blocks and which notification path is used.
- The output decoder, first valid observation point and duration/window of validity.
- Reset geometry, reset power and callback paths, payload positions and permitted owners.
- A demonstrated condition permitting the next external computation.
- Internal periodic behavior, if any, and its effects at outputs.
- Required ordering constraints and unsupported or undefined input sequences.

A fixed “settle eight ticks” copied from another fixture is not proof of readiness. Determine whether the mechanism actually becomes ready or enters a recurring cycle. If a repeated-use protocol is unknown, state that and test fresh snapshots separately rather than claiming reuse.

For outputs without an attached non-instant consumer, document the labeled output and the missing consumer boundary. If a derived probe fixture is needed, preserve the original and describe the added consumer, its notifications and any changed behavior. A probe can load the circuit or introduce callbacks.

## 5. Capture physical state and causality

Produce machine-readable traces with enough information to reproduce and diagnose the response. Capture the initial state, state immediately after each external action, completed game-tick boundaries, and finer operations around computation and reset.

Trace metadata must include:

- Schema version, fixture path and hash, case ID and input vector.
- Engine identity/version; MCHPRS revision and relevant local-source hashes, or Java server binary hash.
- Coordinate convention, origin, orientation, setup mode and settling/readiness condition.
- Ordered stimulus list, declared valid protocol and expected observation projection.
- Capture commands, stepping mode, limits and termination reason.

For relevant components capture:

- Actual block states, dust strengths and connection sides.
- Piston facing/sticky/extended state, distinguishing retracted, extended and moving.
- Head, carried block and payload occupancy; group ownership or drop/recapture.
- Moving entity progress/previous progress and identity where available.
- Observer powered state and pending pulses; reset torches/dust.
- Electrical power/inhibit conditions, including quasi-connectivity.
- Relevant scheduled requests, piston event actions/order and active motions.
- Discovered consumer states, output transitions and callback-dependent effects.

Identify each fine observation by logical game tick, phase, operation index and ordered external-action index. Preserve due/priority/type information for scheduled work where available.

Use game, nano and pico stepping to compare the same episode at aligned completed tick boundaries. Record termination explicitly and bound all loops; periodic behavior should have a demonstrated period over repeated cycles, not a false “settled” label.

Immediate neighbor callbacks are nested inside an operation. A pico snapshot cannot by itself prove their internal order. Obtain test-scoped callback instrumentation or another justified causal trace when an ordering claim requires it. Keep instrumentation isolated, deterministic and inactive in normal server execution. Clearly distinguish measured callbacks from order inferred from source code.

## 6. Tests by circuit family

Use fresh equivalent ready snapshots for independent input cases. Separate prepared data from trigger events. Derive fixture truth tables after establishing the port decoder and evaluation context.

- Single/reset fixtures: initial readiness, falling trigger, first retraction response, complete reset path, payload retention/drop behavior, output pulses, repeatability where supported, and comparison of the observer/torch/dust/downward variants.
- INSTANT_CHAIN: record activation order and propagation depth; verify whether stages respond in the same game tick and identify the intervening callbacks. Do not equate “same tick” with simultaneous motion.
- INSTANT_BLOCKED: identify what is blocked, by which input/geometry, and whether the fixture is an intended valid inhibition mechanism or a negative construction. Test the actual unblocked and blocked protocols.
- OR_1: all supported event combinations, A-only/B-only/both, permitted input orders and shared-payload transfers. Explain the zero-event observation context and distinguish output response from continuing oscillation.
- AND_1/2/3: all supported combinations and physical reason for conjunction. Compare their mechanisms and reset/update requirements. Document BUD-like behavior mentioned by signs, including whether reset removes stored history within the valid protocol.
- NOT_1: treat the name as historical labeling for negation/inhibition. Identify every participant, the enabling trigger, active blocking input and the required order. Capture one working ordered case and a deliberately reversed/misaligned diagnostic when meaningful. The latter is not automatically a valid-protocol truth-table failure.
- XOR_Simple: verify all supported combinations, identify inhibition paths, and establish whether XOR requires the same ordering constraints. Do not certify XOR from the filename.
- ADDER_1BIT: establish A, B, carry-in, sum and carry-out and test all eight prepared input combinations under its valid trigger protocol. Separate sum/carry availability and reset.
- ADDER_11BITS: derive current sign-to-port mapping, bank direction, bit order, trigger, width, carry/overflow policy and validity window. Check zero, single bits, carry chains, overflow, maximum values, 0x555 + 0x2aa in both orders, and reproducible seeded cases. Use mathematical expectations only after the encoding and width are established. Test repeated calculations only after proving the reuse protocol.
- COUNTER_BASIC: identify trigger, update generator, memory and output encoding. Measure the actual count sequence, persistence between valid triggers, carry/reset/wrap if present, and any required ordering. Treat the counter as stateful; do not force it into a combinational truth table.
- Downloaded MCHPRS_REDSTONE_UPDATE_EDGECASE: rederive ports and setup for its current binary, then trace drop/recapture and reset. Do not import old coordinates or waveform expectations without checking their provenance.

Test supported positive-to-positive changes and repeated observations of a held zero to ensure they are not described as fresh root computations. Retain any real internal cycles. Use strength-one-to-zero cases only where the fixture permits them.

Use horizontal rotations where meaningful to expose spatial update-order dependence. A vertical mechanism is a separate construction, not automatically a rotation of the horizontal family. Add narrowly chosen mutations or diagnostic variants only when they test a specific recognition guard.

## 7. Negation ordering is an explicit result

For every negation path, identify the required ordering relation using concrete positions and operations. For example, represent a proven relation as operation(actor_A) precedes operation(actor_B), specifying whether each operation is a callback, event enqueue, event execution or movement completion.

Do not decide the actors or required relation from a Boolean formula. Trace the physical mechanism and resolve the author's “negated piston” wording if necessary. Explain how an active negation prevents another piston from computing, and why a different order can break the result.

Provide a partial-order graph/table for the supported episode. State what fixes the order: geometry, source-operation order, notification traversal, scheduled priority, event FIFO, reset phase or another mechanism. Record rotations that change it.

A later compiler may use a guarded expression such as “target trigger and absence of an effective inhibit,” but only after validating the protocol and order. Report any constraints crossing region boundaries. A topological data DAG alone may omit these dependencies.

Do not invent missing nanotick-misalignment schematics or claim their family is covered. Record the smallest additional author example needed where existing evidence is insufficient.

## 8. Preserve intentional counterexamples

OR_Interpreter_illigal.schem is an author-supplied counterexample: its practical physical state is described as illegal, while a future Redpiler abstraction might admit it. Preserve and analyze its exact saved state.

Distinguish strict-import representability, physical consistency/reachability, behavior after notified placement or updates, logical function, reset and proposed compiler admissibility. “Illegal” must become a concrete, evidenced condition rather than a blanket judgment from the name.

Compare its response with the legal shared-output OR. Explain whether the issue is ownership, head/payload consistency, update ordering, construction reachability or something else.

If it fails or behaves differently in MCHPRS/Java, retain the trace and classify the discrepancy. Do not repair the fixture, change interpreter rules, invent a passing oracle or assume that a future logical compiler can safely accept it. Any normalization or allowed abstract representation needs an explicit proposed contract and preservation argument.

## 9. Independent references and existing tests

Reuse relevant patterns from:

- crates/core/src/redstone/piston/tests.rs
- crates/core/src/redstone/mod.rs in-module Java comparisons
- crates/core/src/redstone/adder_tests.rs
- crates/core/src/plot/piston_tests.rs
- tools/capture_piston_oscillator.py
- tools/capture_observer_piston_feedback.py
- tools/capture_adder.py
- test_data/piston-repair/reference-manifest.json and existing Java trace files

Use the repository's pinned Java reference version and verify the server hash for independent captures. Run capture servers in isolated local worlds with explicit frozen ticking and setup semantics. Ensure normal placement/destruction, strict paste and raw writes are not conflated between engines.

Keep separate evidence categories: author intent, imported state, observed MCHPRS behavior, observed Java behavior, inferred mechanism and proposed compiler contract. MCHPRS recordings are useful reproducibility evidence but are not independent Minecraft conformance oracles.

A hash, file-name key, stimulus or coordinate mismatch makes a reference unsuitable for a new fixture. Create a new versioned capture for the corrected adder and downloaded edge case. Preserve old references under their old provenance; do not merely rename their keys or overwrite them with current MCHPRS output.

Current adder helpers may be mid-migration. Inspect them and their reference JSON together, fix fixture-specific harness mappings as justified, and distinguish baseline failures from changes introduced by your tests. Runtime interpreter fixes are outside this documentation/testing assignment unless separately authorized.

If Java capture is unavailable, complete independent structural analysis, MCHPRS traces and meaningful tests, then explicitly list the missing comparison. Do not mark unperformed checks as passed or leave all other fixtures unfinished.

## 10. Deliverables

Create:

1. docs/INSTANT_PISTON_SCHEMATICS.md: a catalog with an overview table and a full dossier for every canonical basic example and the downloaded edge case. Account for aliases, intentional counterexamples and deferred CPU files.
2. test_data/instant-pistons/fixtures/<fixture-id>.json: versioned machine-readable manifests with ports, classification, setup, cases, valid protocol, ordering constraints, observation decoders and expected/observed status. Unknowns must be explicit.
3. test_data/instant-pistons/traces/: reproducible, engine-labeled traces linked by fixture hash and case ID. Keep useful summaries in the catalog and detailed operation data in artifacts.
4. A reusable inspection/capture utility and shared test helpers where useful. Add focused Rust regression tests in an appropriate redstone test module, registered through the existing test structure.
5. Targeted corrections to REDSTONE_MODEL.md when actual source behavior contradicts it, and to INSTANT_PISTON_IMPLEMENTATION_PLAN.md for fixture evidence, negation ordering, recognition guards and compiler boundaries.
6. A short final report listing what is verified, mismatches, remaining author questions, missing standalone BUD/nanotick examples and the exact test/capture commands executed.

Each dossier must state its purpose, coordinate/port map, initial state, ordered stimulus, logical result, physical timeline, causal explanation, reset/next-use rule, required update order, counterexamples, classification/recognition implications and evidence links. Include a compact diagram or annotated slice where geometry is hard to explain.

Fixtures with unresolved intent still need an observed-state dossier and clearly scoped diagnostics. Do not label a function validated when its ports or protocol remain ambiguous.

## 11. Validation and completion

Run focused tests first, then the affected redstone suite when justified. For example:

cargo test -p mchprs_core --lib redstone:: -- --test-threads=1

Use the actual module/test filters you add, and report exact pass/fail results. Keep tests behavioral: causal ordering, externally visible transitions, reset reuse, payload transfers and arithmetic/counter state. Avoid assertions that only restate the extraction implementation.

Verify manifest hashes, coordinate mappings, trace reproducibility, Markdown links and artifact references. Check that fine-stepping and game-tick results agree at equivalent boundaries without hiding intermediate effects.

The assignment is complete when all canonical basic fixtures and the downloaded edge case have evidence-backed dossiers, reproducible supported episodes and meaningful test coverage; aliases/CPU deferrals and unverified or undefined cases are explicit; and the negation/counterexample constraints are concrete enough to guide a parser. A discovered interpreter mismatch should be documented and reproduced rather than silently changed to obtain a passing result.

Proceed through the fixture pack independently. Ask focused questions only for ambiguity that materially affects a fixture's interpretation, and continue unrelated fixtures while waiting.
