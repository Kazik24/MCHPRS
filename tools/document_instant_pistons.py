"""Render the reviewed dossiers and augment manifests with indexed evidence.

py tools/document_instant_pistons.py
Narratives below are analysis conclusions, not functions inferred from filenames.
"""
import gzip
import json
from pathlib import Path
from inspect_instant_pistons import ROOT, PACK, inspect

# Purpose, mechanism, next-use rule, recognition implication.
DOSSIERS = {
"instant_observer": (
"Horizontal observer-reset instant; one falling response followed by a recurring reset.",
"Base (0,1,3) faces South, head (0,1,4), payload (0,1,5). Input dust (0,1,1) strongly powers sandstone (0,1,2), which powers the base. Retraction removes the source beside output dust (0,1,6). Observer (0,2,3), facing Down, watches the base and emits upward into sandstone (0,3,3). Its scheduled pulse quasi-powers the base and its output callbacks deliver the recheck. First retraction executes at response tick 1; base/payload restoration completes at 3; reset extension starts at 4; output returns at 6. This repeats every six ticks under held low input.",
"Initial strict import is quiet and extended. There is no quiescent next-computation state under held zero. The rearm diagnostic restores the input after 12 ticks and becomes quiet, but that external action occurs in an active episode and is not a certified reuse protocol. Use fresh ready imports for conformance.",
"Recognize observer facing, conducting cap, actual QC and callback path, payload conservation and periodic output. A constant Boolean level cannot replace the exposed waveform."),
"instant_torch": (
"Horizontal torch reset with a single output pulse and a stable powered reset.",
"Base/head/payload and input/output positions match INSTANT_OBSERVER. The observer/cap are absent; an initially unlit floor torch is at (0,2,2) on the input conduction block. Falling input turns that torch on through a two-game-tick scheduled transition. Its power and notifications reset the base. Output is low at boundaries 1 through 5 and high again at 6; the torch then holds the extended mechanism powered with no continuing oscillator.",
"By tick 6 the response is quiescent and extended, with payload restored. The recorded reuse diagnostic waits to tick 12, restores the source, waits 20 ticks (with 18 unchanged quiet boundaries), then destroys it; it reproduces the pulse. Acceptance can use the measured quiescent extended/head/payload/torch condition, not the literal wait lengths.",
"Torch timing, attachment support and its own power condition are guards. This reset cannot share the observer family's persistent-low oscillator contract."),
"instant_reset_redstone": (
"One-shot retraction in this exact saved geometry; automatic reset is not demonstrated.",
"Base (0,1,3) faces South. The input has two dust cells at (0,1,1) and (0,1,2), on sandstone; there is no observer, torch or dust below the head. Payload moves from (0,1,5) to (0,1,4). Output dust (0,1,6) falls in tick 1 and remains zero. By tick 3 the base is retracted and the payload is stationary at the head position. The front payload alone does not power the base: should_piston_extend excludes the facing side, and the saved geometry provides no closed feedback route.",
"This is a quiet retracted endpoint, not an extended ready instant. Restoring the external source and allowing extension to finish does establish another extended snapshot; the diagnostic then reproduces retraction. Treat that as an external rearm requirement, not autonomous reset.",
"Reject an autonomous-reset match for this binary. Its name supplies no proof of a dust feedback path; compare the added under-head dust in variant 2."),
"instant_reset_redstone_2": (
"Dust feedback reset activated by the retracted redstone payload.",
"Base (0,2,3), head (0,2,4), payload (0,2,5), input dust (0,2,1..2). Reset dust (0,1,4), on sandstone (0,0,4), is beneath the head. Retraction pulls the redstone block above that dust; on stationary restoration it powers the dust, whose side power enters sandstone beneath the base (0,1,3). The resulting recheck extends the base. Moving payloads emit no source power; this accounts for the delay between computation and restored output. Output is low 1..5 and restored at 6, then repeats with period six.",
"Held-low response is periodic. The source-restoration diagnostic closes the oscillator, but is outside initial reset-time conformance. Fresh imports are the supported independent-case protocol; recurring internal work belongs to the original root event.",
"Require the under-head support/dust route and a powered movable payload. Feedback power depends on payload availability; it cannot be assumed for wool or a stolen shared payload."),
"instant_down": (
"Downward observer-reset construction with the same six-tick response envelope.",
"Base (0,3,3) faces Down, head (0,2,3), redstone payload (0,1,3). Output dust reaches (0,1,4) and the labeled endpoint (0,1,5). The source (0,3,0) feeds climbing dust at (0,3,1) and (0,4,2). Observer (0,4,3) faces Down; sandstone cap (0,5,3) supplies QC reset. Output falls in tick 1 and returns in 6, repeatedly. The vertical payload occupies y=1..2; it is not a horizontal rotation.",
"Strict import is extended and quiet; held-zero response is periodic. Reuse from an independently verified extended snapshot is supported; input restoration during the recurring episode remains diagnostic.",
"Validate the vertical support, motion cells, climb and cap/QC route separately from horizontal matching."),
"instant_down_torch_reset": (
"Downward torch/dust reset with a quiescent endpoint.",
"Base/head/payload and output match INSTANT_DOWN. Reset is a wall torch at (0,4,4), facing South, attached to sandstone (0,4,3), plus dust (0,3,4) on sandstone (0,2,4). Falling input removes power from the attachment path, scheduled torch power provides reset, and output returns at tick 6. It remains powered after reset rather than entering the observer oscillator.",
"The endpoint at 6 is quiet and extended. The source-restoration/retrigger diagnostic reproduces the pulse after the torch has switched back off and queues drain. Observe the full geometry and quiet condition before next use.",
"This is a distinct wall-torch attachment/reset family. Vertical orientation alone cannot justify applying horizontal guards."),
"instant_blocked": (
"An instant held powered by a constant quasi-connectivity source.",
"Base (0,1,3), observer (0,2,3), head/payload (0,1,4..5), and output dust (0,1,6). The explanatory sign at (0,4,3) refers to redstone block (0,3,3): that cap directly satisfies QC and keeps the base extended when the external input (0,1,0) is removed. The output remains strength 15, with no retraction event. The sign's 'const 1' describes the author's physical convention; it is not a newly generated falling-event one.",
"Removing the cap first allows retraction but also removes the observer's conducting reset path: the recorded 'unblocked' case ends retracted. The separate derived conducting-cap case replaces the redstone cap with stone, waits for measured readiness, and restores observer-reset behavior. Original and derived geometry remain separate cases.",
"Classify original as ForcedPowered. Do not accept a reset because an observer is present; constant cap power changes its role."),
"instant_chain": (
"Two instant stages propagate in one game tick through ordered callbacks.",
"Bases (0,1,3) and (0,1,6) face South, with heads one step ahead and redstone payloads two steps ahead. The first payload (0,1,5) directly powers the second base from behind; final output dust is (0,1,9). Removing (0,1,0) queues the first Retract. In tick 1 operation 1, its payload removal notifies base (0,1,6), measuring piston_power=false; nested callback depth 1 enqueues the second Retract. Event FIFO executes the second at operation 2, before movement completion. Both observers then perform their separate resets.",
"Depth is two actuator events, not two game ticks or simultaneous motion. Both output paths repeat with a six-tick physical cycle. Initial conformance uses fresh imports; the recorded source-restoration episode is a reset-time diagnostic.",
"Keep payload writer -> downstream callback -> event enqueue -> event execution edges, including those crossing a compiler region boundary."),
"or_1": (
"Legal shared-output OR at the first-wave observation window.",
"A source (2,2,9) feeds West-facing base (2,2,4); B source (0,2,9) feeds North-facing base (0,2,6). Heads (1,2,4) and (0,2,5) share payload (0,2,4); output dust (0,2,1) begins at strength 13. No events leaves 13 unchanged. A-only, B-only and both input orders remove the shared source from its extended position, making output zero in tick 1. Motion/entity traces conserve one payload across the shared position and either head position. Ownership is a group property; the first event can capture while another retracts without that block.",
"Output is zero at boundaries 1..5 and 13 at 6; nonzero cases repeat every six ticks. Each base has its own observer and solid cap, so reset power does not require possession of the payload. All cases use fresh snapshots; no held-zero observation is counted as another external computation.",
"Recognize both actuators, independent resets and all three allowed payload positions/owners. Do not impose permanent single-base ownership or accept the illegal dust-dependent group by analogy."),
"and_1": (
"Conjunction from two independently moved sources feeding one dust net.",
"Sources A/B are (2,2,9)/(0,2,9); North-facing bases (2,2,7)/(0,2,7) move payloads at (2,2,5)/(0,2,5). Both payloads power dust (1,2,5), leading to labeled endpoint (1,2,1), initially 11. One payload remaining keeps the net nonzero; only both falling events remove both sources. Both AB and BA produce zero in tick 1. The conjunction arises from falling-event decoding of an electrical maximum, not from treating a static dust join as a positive AND.",
"Independent observer/cap resets restore both payloads at 6; the both-event response repeats every six ticks. Single-event episodes can move internally without a terminal falling edge. No external consumer is attached to the labeled net.",
"Keep both sources, common net and attenuation. Do not lower this net to conjunction until falling-event context is declared."),
"and_2": (
"Conjunction from two power paths controlling one actuator.",
"North-facing base (0,2,7) moves one payload at (0,2,5). B (0,2,9) powers its rear wire (0,2,8); A (2,2,9) powers the side branch through (2,2,8), (2,2,7), (1,2,7). Either remaining path keeps the base powered. Both input removal orders allow retraction and the endpoint (0,2,1), initially 12, falls in tick 1. One observer/cap performs reset.",
"Both-event output is low 1..5 and restored at 6, with six-tick cycles under held zero. Independent input cases start from the same saved extended snapshot.",
"Single-actuator dual-power conjunction is structurally different from AND_1's two-payload conjunction; validate both power paths and callback delivery."),
"and_3": (
"Conjunction with a remote QC power input and BUD-like update dependence.",
"North-facing base (0,2,4), payload (0,2,2), output dust (0,2,1). A (0,2,6) delivers power and updates through (0,2,5). B (0,5,6) powers upper dust (0,5,5..4) and sandstone (0,4,4), contributing QC. B's removal changes power without delivering a base callback. A's callback must see B already low. BA retracts in tick 1; AB leaves the base extended and output 15 despite both sources gone. This is a supported synchronization guard and an update-history witness, not a Boolean-table failure under the BA protocol.",
"The sign at (0,3,4) explicitly says this is also a BUD switch but reset. Dust (0,1,3) beneath the head closes feedback when the pulled redstone block becomes stationary. Reset does not make a late B callback appear: the AB episode retains its saved history until an independent recheck. The BA response has six-tick cycles; no repeated external calculation is certified.",
"Require destroy(B)/power propagation before callback(A to base). Preserve the power/update separation and reset dependency on payload. This embedded memory example does not constitute standalone BUD acceptance."),
"not_1": (
"Historical NOT label for A activation inhibited by B, with explicit transient order.",
"A drives North-facing base (0,2,4) and its redstone payload (0,2,2), powering output (1,2,1) at 14. B drives base (2,2,4), whose sandstone payload (2,2,2) covers powered lower dust (2,1,2), supplied by redstone block (2,1,3). B retraction uncovers that dust and supplies an alternate nonzero path to the output. First completed boundary decodes A && !B: 00/01/11 remain positive, 10 falls. AB event execution briefly makes output zero before B restores strength 13; BA prevents that transient. Both occur in the same game tick.",
"The author's latest clarification allows simultaneous-wave arrival here; extra actuator depth on the negating input can break inhibition. 'Fire' is measured as event execution plus its immediate electrical/update effects, not movement completion. The original has no consumer on the labeled output, so it does not establish arbitrary consumer equivalence. A separate derived West-input repeater at (2,2,1), on new stone (2,1,1), remains powered for both AB and BA: its due recheck sees the restored positive input. Probe setup uses notified writes and measured quiet readiness. It can load the wire; its result is scoped to that probe.",
"Each base resets through its own observer/cap and recorded responses cycle every six ticks. First-wave interpretation is valid at boundaries 1..3. New external changes during reset are undefined. Original physical actor nomenclature 'negated piston' is not pinned to a unique base by the author; the concrete actors/paths and required consumer relation are pinned here.",
"Retain inhibit-effective-before-downstream-evaluation constraints across regions. A data DAG or an assumption that B physically completes movement first is insufficient."),
"xor_simple": (
"First-wave XOR, with a reset-phase ordering limitation.",
"A/B (2,2,7)/(0,2,7) drive the shared OR payload (0,2,2), via bases (2,2,2) West and (0,2,4) North. A third Down-facing base (1,4,1) pulls sandstone (1,2,1), uncovering dust (1,1,1) powered by (2,1,1). Its control net joins both input routes; only both removal events enable this inhibit path. The unlabeled terminal wire at (0,2,0) is reached from shared-output dust (0,2,1). First completed boundary: 00 positive 14; 10/01 zero; 11 positive 13. This establishes XOR only in the stated window.",
"For 11 AB, first-wave execution is A/shared OR (2,2,2), inhibit (1,4,1), then B/shared OR (0,2,4): raw output is zero after operation 1 and restored to 13 after operation 2. BA executes B, A, then inhibit: zero persists through operation 2 and restores after operation 3. Both completed first boundaries match XOR, but both expose a transient before inhibition. No attached consumer establishes filtering. For 11 BA reset reaches a six-tick recurring waveform. AB is deliberately retained: output falls again at response tick 5 and later internal reset times despite both-event first-wave inhibition. No three-cycle full-state period is demonstrated in the 24-tick AB capture. Those internal effects are valid execution, not a new external trigger. No consumer adapter or repeated-use protocol is certified.",
"The net has no output sign or attached non-instant sink. Its mapping is derived from the shared payload-to-dust path and the uncovered lower dust; all supported combinations and rotations confirm the first-wave decoder.",
"Preserve the AND-controlled inhibit path, callback depth and reset pulses. First-wave XOR does not justify suppressing later externally visible callbacks."),
"adder_1bit": (
"One-bit full adder with independent sum and carry encodings.",
"A1/B1 signs mark caps (1,5,3)/(1,5,1); CIn marks (9,5,5). A redstone cap means prepared zero, a conducting non-source cap means one. The real trigger is unlabelled source (0,4,5), feeding the input/update network. OUT marks payload (18,2,1): air means sum one; redstone block means zero; moving is invalid. Cout is a wall sign at (12,0,0), referring to wire (11,1,0), not the block below the sign. Carry one is its 14-to-zero response. The repeater (12,1,2) remains powered: it supplies the conducting wool bridge moved by base (9,1,2), rather than consuming the decoded carry. This observation corrected the capture harness's initial candidate-consumer description.",
"All eight prepared vectors are verified with fresh quiet snapshots. Sum is available at 1..3, becomes moving at 4..5 for active sum, and resets at 6. Carry is valid at 1..5. Observer reset and several inhibitory paths explain different sum/carry physical timing. Preparation writes all caps, then notifies all caps, then requires two unchanged quiescent boundaries (observed two ticks); no fixed eight-tick readiness assumption is used in MCHPRS.",
"Reset cycles are retained in full traces. No arbitrary external reuse or downstream consumer handoff is certified. The internal repeater is an upstream driver, not proof of a carry consumer boundary.",
"Validate caps, trigger network, carry wool bridge, width-one arithmetic and both observation windows; retain inhibition and reset order."),
"adder_11bits": (
"Canonical corrected 11-bit adder; sum modulo 2048.",
"Exact refreshed binary is 21 x 7 x 46, SHA-256 41476594941f234f8759c2b4b12dab72641fbda9fb5f0412a2b0089ba964464e. Six signs A1/A2, B1/B2, O1/O2 identify the first two bit positions of each bank. Caps A_i=(3,5,43-4i), B_i=(3,5,41-4i); payload output O_i=(20,2,41-4i), i=0..10. Signs are one block above these particular caps/payloads, as verified by the geometry and single-bit experiments. Increasing bit index decreases Z. The actual trigger is (2,4,45), with no TICK sign; loader offset is (2,4,44). Each prepared cap is redstone for zero or a conducting non-source block for one.",
"Fresh snapshots verify zero, every single bit in both banks, carry chains, 1024+1024 overflow, maximum operands, maximum-plus-one, 0x555+0x2aa in both orders and eight deterministic LCG-seeded pairs (seed 0x1a57; parameters in manifests). The eleven output payloads decode at 1..3; moving outputs at 4..5 are invalid; tick 6 is physical reset, not the arithmetic result. No twelfth observed output exists: overflow is discarded in the declared 11-bit decoder. Existing Java references also match the current hash; new pack captures independently cover the listed representative arithmetic episodes.",
"Prepared data use all raw writes before notifications, then two unchanged quiescent boundaries (normally two ticks); Java uses normal setblock and eight measured setup ticks. Those setup semantics are explicitly distinct. The input source is held high until trigger, so independent prepared cases do not launch computations. Repeated calculations require a separately proven rearm protocol; none is certified here.",
"Do not import the 21 x 7 x 45 signless predecessor's offsets or old changed-input discrepancy. Geometric observer seeds alone miss inhibitory, downward and state/update paths. Keep the whole trigger/reset/order interface until a boundary adapter is validated."),
"counter_basic": (
"Stateful, free-running counter released by one external falling trigger.",
"Author confirms source (7,8,0), beneath Trigger sign (7,9,0), releases the clock rather than requesting a count per source removal. Update generator sign (3,12,18) marks the ordinary Down-facing piston (2,11,18) and adjacent West-facing observer (3,11,18); it does not label an output bit. BUD/Memory/Cell sign (5,12,3) marks first memory base (5,11,3). Sixteen repeated memory bases are (5,11,3+2i); retracted is stored one, extended is zero. The bank increments Z from least to most significant. Wires (20,6,3+2i) are observed candidate output paths, but carry/reset pulses make them an unsuitable persistent count decoder.",
"Saved memory is all extended, decoding zero. Under held-low trigger, completed boundaries 6,12,...,96 decode counts 1..16, carrying through the first five cells. Boundaries 5+6n and 6+6n retain the same stored count; intervening moving states make the full bank temporarily invalid. The update generator emits further internal computation waves; repeated observation of the root's held zero is not their origin. Both engines reproduce this sequence. The full counter state is not periodic over 96 ticks.",
"The memory/reset/update generator state persists between internal clock waves. External stop/restart, explicit memory clear, high carry and 16-bit wrap are not verified by this bounded run. Do not certify overflow or reset based only on bank length. Fresh imports are the tested release protocol.",
"Keep BUD storage and clock state as explicit execution owners; only validated instant subsections may be simplified. This is embedded memory coverage, not a standalone BUD suite."),
"mchprs_redstone_update_edgecase": (
"Downloaded 8 x 8 x 7 update/drop/recapture example, separate from the older root fixture.",
"Trigger source is (6,5,6); its attached powered lever (7,5,6) is also removed by normal support callbacks when the source is destroyed. That source feeds the North-facing instant at (3,5,6), payload (3,5,4), and the Down-facing mechanism at (2,4,2), head (2,3,2), payload (2,2,2). Other bases at (3,3,1), (2,2,3), (0,3,0), four observers, a comparator (1,3,2) and furnace (1,3,3) complete feedback. The saved furnace has 13 redstone items and comparator output strength 1; block entities must be imported, not replaced with arbitrary constants. There are no signs.",
"Vertical timeline: 1..2 pull payload into moving (2,3,2); 3 it is stationary there with retracted base; 4 extension begins; 5 early retraction finalizes and drops the in-flight payload at (2,2,2), leaving (2,3,2) air and the base moving; 7 base is retracted; 8 creates an empty extending head; 9 pulls the dropped payload again. The vertical payload projection repeats every eight ticks over multiple cycles. Other feedback phases are retained; no full-state period is proven within 48 ticks. The observation 'released' at (2,1,2) is a below-payload emptiness guard, not the actual drop cell.",
"Strict import is quiet; ordinary source destruction launches the recorded response. Reset never reaches a certified quiescent next-use state in this capture. Four MCHPRS rotations preserve drop/recapture, and current-binary Java r0 matches the declared port waveform. Old Java rotations are references for a different binary, not this one.",
"Require inventory/comparator state, ordinary lever support destruction, in-flight early-retraction guards, payload finalization and actual callback paths. Reject one-payload-always-owned assumptions."),
"or_interpreter_illigal": (
"Intentional counterexample: shared payload makes dust-dependent reset unclosed; forbidden compiler scope.",
"North-facing base (0,2,2) and West-facing base (2,2,0) have matching heads (0,2,1)/(1,2,0) and shared redstone payload (0,2,0). Both input sources are at z=5. Under-head reset dust at (0,1,1) and (1,1,0) needs the pulled powered payload to become stationary. Strict import represents the saved state, and a derived self-recheck construction from retracted bases/no heads can reach an extended head arrangement. Thus the name does not prove a malformed NBT or orphan head.",
"The author's clarified illegality is operational: the other piston can take the powered payload needed for reset, leaving a member without an instant reset. The author excludes this undefined construction from scope and warns of direction/position dependence. MCHPRS and Java retain the exact imported state and both input orders. First OR-like payload removal occurs, but one base stays retracted after tick 6 while the owner cycles. One conserved payload is insufficient for a reusable group. Initial owners follow event order; the competing member's reset does not close.",
"No valid reusable truth table or compiler admissibility is assigned. Saved-idle, single/both events, four rotations and derived construction are diagnostics. The abstract shared-payload OR resemblance must not normalize or repair the fixture. Other directions/positions are not promised to be equivalent.",
"Classify IntentionalCounterexample/UnclosedReset, reject compiled activation for this construction, and preserve interpreted/undefined behavior without changing interpreter rules."),
"nanotick_example": (
"Author-supplied propagation-depth counterexample: inhibition arrives after a downstream instant has accepted a trigger.",
"Trigger (0,2,0) splits into ordinary base (0,2,5) and delayed negating path (3,2,0) -> (2,2,2) -> (2,2,5). The ordinary redstone payload (0,2,7) drives inhibited dust (1,2,7..8); the final inhibitor's sandstone payload (2,2,7) covers the powered lower dust (2,1,7), supplied by (2,1,6). The downstream South-facing base (1,2,9), payload (1,2,11), drives labeled output (1,2,12). The output sign (1,1,13) is beside this net, not its coordinate.",
"Measured tick-1 event execution order in saved orientation: delay-1 (3,2,0), ordinary (0,2,5), delay-2 (2,2,2), downstream (1,2,9), inhibit (2,2,5). The ordinary event's nested callbacks queue downstream before delay-2 queues inhibit. At downstream validation, its net is zero. Later inhibit restores that net to 13, but the downstream retraction has already happened and final output is zero. This is the author's unwanted activation even though the Boolean guard would predict inhibition. All four MCHPRS rotations and an independent Java capture reproduce the episode.",
"All bases have observer/cap resets. The output repeats with a six-tick full-state period modulo motion identity renaming. This is an actual supplied nanotick synchronization example; changing stepping APIs does not create it. No new external input is introduced during reset.",
"Reject the delayed-inhibit construction under the initial synchronized gate contract. Retain the physical diagnostic and the violated inhibit-before-validation relation; no parser or compiled execution acceptance is assigned. A Boolean topological DAG that erases actuator depth cannot certify this circuit."),
}


def mdlink(path,label):
    return f"[{label}](../{path})"


def scalar(v):
    if isinstance(v,list):
        if all(x["name"] in ("air","redstone_block") for x in v):
            return str(sum((x["name"]=="air")<<i for i,x in enumerate(v)))
        return "["+", ".join(map(scalar,v))+ "]"
    return v["properties"].get("power",v["properties"].get("extended",v["name"]))


def label_mapping(fid,m,sign,electrical):
    rows=[r for side in sign["text"].values() for r in side["readable"] if r]
    label=" / ".join(rows)
    key="".join(rows).replace(" ","").lower()
    inputs=m["ports"]["inputs"];outputs=m["ports"]["observations"]
    chosen=None;role="explanatory text; no external port"
    if key in ("input","trigger"):
        chosen=inputs.get("input",inputs.get("trigger"));role="external source/update trigger"
    elif key in ("ina","inb"):
        chosen=inputs[key[-1].upper()];role="input source; falling event supplies logical one"
    elif key in ("output","out"):
        chosen=outputs.get("output",outputs.get("sum"));role="labeled physical output; no sink implied"
    elif fid=="adder_11bits" and key in ("a1","a2","b1","b2","o1","o2"):
        bank=outputs["sum"] if key[0]=="o" else inputs[key[0].upper()]
        chosen=bank[int(key[1])-1];role="bank bit 0 or 1; geometry and single-bit cases"
    elif fid=="adder_1bit" and key in ("a1","b1","cin","cout"):
        chosen=outputs["carry"] if key=="cout" else inputs[{"a1":"A","b1":"B","cin":"Cin"}[key]]
        role="wire output beside wall sign" if key=="cout" else "prepared data cap; one cell below this sign"
    elif fid=="instant_blocked" and "const1" in key:
        chosen=inputs["blocker"];role="constant QC source; not a falling-event one"
    elif fid=="and_3" and "bud-switch" in key:
        chosen=[0,2,4];role="embedded BUD base; remote power and local update are separate"
    elif fid=="counter_basic" and "memory" in key:
        chosen=outputs["memory"][0];role="first BUD storage base, bank LSB"
    elif fid=="counter_basic" and "generator" in key:
        chosen=[outputs["generator"],[3,11,18]];role="ordinary Down piston and West observer, not an output bit"
    elif fid=="or_interpreter_illigal":
        chosen=[[0,2,2],[2,2,0],[0,2,0]]
        role="historical interpretation proposal; latest author clarification forbids this reset-unsafe group"
    if chosen is None:
        raise ValueError(f"unmapped sign {fid}: {label}")
    candidates=[c["pos"] for c in electrical if sum(abs(a-b) for a,b in zip(c["pos"],sign["pos"]))<=3]
    referenced=chosen if isinstance(chosen[0],list) else [chosen]
    candidates=sorted({tuple(p) for p in candidates+referenced})
    return dict(sign_position=sign["pos"],readable=label,candidate_positions=candidates,
                referenced_positions=chosen,role=role,status="mapped",
                evidence="saved geometry and controlled current-binary episodes; causal path described in dossier",
                original_text=sign["text"])


def decoder(fid):
    common=dict(timing="completed response boundary; tick 0 is immediately after ordered stimuli",
                held_zero="not a new external event",moving="distinct state, never silently treated as an output bit")
    if fid.startswith("adder_"):
        return dict(**common,encoding={"air":1,"redstone_block":0,"moving_piston":None},
                    prepared_caps={"redstone_block":0,"conducting_non_source":1},
                    sum_window_ticks=[1,3],carry_window_ticks=[1,5] if fid=="adder_1bit" else None,
                    carry_encoding="nonzero to zero = carry one" if fid=="adder_1bit" else None,
                    width=1 if fid=="adder_1bit" else 11,overflow="carry observation" if fid=="adder_1bit" else "discarded modulo 2048",
                    carry_in_policy="prepared Cin port" if fid=="adder_1bit" else "fixed zero; no mapped external carry-in",
                    bank_order="listed positions from LSB to MSB",consumer_boundary="labeled sum/carry observations; upstream carry_driver is not a consumer")
    if fid=="counter_basic":
        return dict(**common,encoding={"stationary_base_extended":0,"stationary_base_retracted":1,"moving_piston":None},
                    bank_order="listed 16 memory bases, increasing Z, LSB to MSB",
                    stable_sample_ticks="5+6n and 6+6n, n>=0, demonstrated through 96 ticks",
                    count="floor((tick+1)/6) at declared stable boundaries; stored memory, not candidate wire bank",
                    wrap="unverified",clear="unverified")
    if fid in ("or_interpreter_illigal","mchprs_redstone_update_edgecase"):
        return dict(**common,function="physical diagnostic; no Boolean function certified",
                    decoded_fields="payload occupancy/transport, reset dust and comparator state; see ports")
    functions={"or_1":"A or B","and_1":"A and B","and_2":"A and B",
               "and_3":"A and B with B removal before A callback","not_1":"A and absence of B",
               "xor_simple":"A xor B","nanotick_example":"unwanted output falling event, inhibitor arrives too late",
               "instant_blocked":"no response under constant QC power"}
    return dict(**common,encoding="new dust nonzero-to-zero transition = one; unchanged nonzero means no event",
                first_valid_boundary=1,first_wave_window_ticks=[1,3],
                function=functions.get(fid,"falling external trigger causes first retraction/output fall"),
                consumer_boundary="labeled/derived wire observation; attached non-instant consumer absent unless case declares a probe",
                later_observations="reset pulses and moving state remain physical observations; full episode is retained")


def negation_paths(fid):
    if fid=="not_1":
        return [dict(actor=[2,2,4],payload=[2,2,2],controlled_wire=[2,1,2],supply=[2,1,3],
                     ordinary_actor=[0,2,4],observation=[1,2,1],
                     required="alternate supply effective before receiving consumer validation; equal-wave roots allowed; raw AB transient preserved")]
    if fid=="xor_simple":
        return [dict(actor=[1,4,1],payload=[1,2,1],controlled_wire=[1,1,1],supply=[2,1,1],
                     ordinary_actors=[[2,2,2],[0,2,4]],observation=[0,2,0],
                     required="AND-controlled alternate supply before consumer evaluation; original has no sink; reset AB pulses retained")]
    if fid=="nanotick_example":
        return [dict(actor=[2,2,5],payload=[2,2,7],controlled_wire=[2,1,7],supply=[2,1,6],
                     ordinary_actor=[0,2,5],consumer=[1,2,9],delays=[[3,2,0],[2,2,2]],
                     required="inhibit effective before downstream Retract event validation",status="violated; diagnostic outside accepted scope")]
    if fid=="adder_1bit":
        return [dict(actor=[6,3,1],payload=[8,3,1],controlled_wire=[8,2,1],supply=[8,4,0],consumer=[11,2,1],stage="first XOR inhibition"),
                dict(actor=[12,3,1],payload=[14,3,1],controlled_wire=[14,2,1],supply=[14,4,2],consumer=[16,2,1],stage="sum XOR inhibition")]
    if fid=="adder_11bits":
        return [dict(actor=[a,3,41-4*i],payload=[a+2,3,41-4*i],controlled_wire=[a+2,2,41-4*i],
                     supply=[a+2,4,40-4*i] if a==8 else [a+2,4,42-4*i],consumer=[13 if a==8 else 18,2,41-4*i],
                     stage=f"bit {i} {'first' if a==8 else 'sum'} XOR inhibition") for i in range(11) for a in (8,14)]
    return []


def inhibition_evidence(paths,artifacts):
    if not paths:return
    # Record actual applied inhibition and the later pending validation. A queued
    # or attempted event is not necessarily a successful piston movement.
    for p in paths:p["measured_validations"]=[]
    for a in artifacts:
        if a["engine"]=="Java" or a["rotation"]!=0 or not a["case_id"].startswith("prepared-"):continue
        t=json.loads(gzip.decompress((ROOT/a["artifact"]).read_bytes()))
        origin=t["coordinates"]["origin"]
        entries=[e for s in t["samples"] for e in s["callbacks"] if e["tick"]==t["start_tick"]+1]
        def at(e,pos):return e["data"].get("pos")==dict(zip(("x","y","z"),(a+b for a,b in zip(origin,pos))))
        for p in paths:
            inhibit=next((e for e in entries if e["kind"]=="event_applied" and e["data"]["action"]=="Retract" and at(e,p["actor"])),None)
            if inhibit is None:continue
            attempt=next((e for e in entries if e["kind"]=="event_execute" and e["data"]["action"]=="Retract" and at(e,p["consumer"])),None)
            if attempt is None:continue
            applied=any(e["kind"]=="event_applied" and at(e,p["consumer"]) for e in entries)
            p["measured_validations"].append(dict(case_id=a["case_id"],artifact=a["artifact"],tick=inhibit["tick"],
                                                inhibit_applied_operation=inhibit["operation"],target_validation_operation=attempt["operation"],
                                                target_retraction_applied=applied,
                                                evidence="measured callback/event recorder; execute denotes attempt, applied denotes successful movement"))
    for p in paths:
        if "consumer" in p and "stage" in p:
            p["required"]="inhibit's dust connection/power update effective before pending target Retract validation; geometry, immediate Wire Turbo callbacks and event FIFO fix tested order"
            p["status"]="geometry plus recorded prepared episode(s); compiler recognition certificate still required" if p["measured_validations"] else "geometry only; no active inhibit/target witness under declared fixed-zero carry-in protocol"


def wave_orders(m,info,artifacts):
    if m["id"] not in ("not_1","xor_simple","nanotick_example"):return []
    bases=[c["pos"] for c in info["nonair_cells"] if c["state"].split("[")[0] in ("minecraft:piston","minecraft:sticky_piston")]
    orders=[]
    for a in artifacts:
        if a["engine"]=="Java" or not (a["case_id"].startswith("events-") or a["case_id"]=="falling"):continue
        t=json.loads(gzip.decompress((ROOT/a["artifact"]).read_bytes()))
        mapping={}
        for p in bases:
            x,y,z=p;w,h,l=m["dimensions"]
            for _ in range(a["rotation"]//90):x,z=l-1-z,x;w,l=l,w
            mapping[x,y,z]=p
        events=[];wire=[]
        for s in t["samples"]:
            if s["tick"]!=t["start_tick"]+1:continue
            if "output" in s["observations"]:
                wire.append(dict(operation=s["operation"],power=int(s["observations"]["output"]["properties"]["power"])))
            for e in s["callbacks"]:
                if e["kind"] not in ("event_execute","event_applied"):continue
                p=tuple(e["data"]["pos"][k]-v for k,v in zip(("x","y","z"),t["coordinates"]["origin"]))
                events.append(dict(operation=e["operation"],kind=e["kind"],actor=mapping[p],action=e["data"]["action"],phase=e["phase"]))
        orders.append(dict(case_id=a["case_id"],rotation=a["rotation"],artifact=a["artifact"],
                           tick=t["start_tick"]+1,events=events,output_at_operation=wire,
                           evidence="measured MCHPRS event order; source callbacks remain in linked fine trace"))
    return orders


def main():
    index=json.loads((PACK/"trace-index.json").read_text())
    observed=json.loads((PACK/"mchprs-projections.json").read_text())["cases"]
    header="""# Instant-piston schematic behavior catalog

This is interpreter evidence and a compiler requirements catalog. No piston parser or Redpiler execution support is implemented. Logical one is a **new nonzero-to-zero transition** from a declared ready state; positive power is a physical condition, not that logical one. Internal reset/clock cycles remain part of the accepted response. New external data or computations during reset are undefined unless a dossier establishes a separate ready protocol.

Evidence categories remain separate: author intent (including clarifications during this assignment), exact imported state, observed MCHPRS behavior, independent Java behavior, mechanism inferred from geometry/source, and proposed compiler guard. A passing boundary projection does not certify an arbitrary non-instant consumer or a generalized circuit family.

The initial checkout was clean at `38bda8c94c0ccaed6e0548e50118d07cce6f1e47`, with no applicable AGENTS.md. Shared-checkout commits and unrelated edits continued during work. Every MCHPRS trace embeds its actual revision, working-tree state and relevant source SHA-256 values; [source-baseline.json](../test_data/instant-pistons/source-baseline.json) is the latest capture baseline, not a replacement for older embedded identities. Test recorder hooks are cfg(test), deterministic and inactive in normal server builds. No interpreter rules were changed.

The shared generated-artifact policy excludes inspection JSON, detailed traces, projections and the trace index from Git. These local artifacts are fully present, indexed and hash-linked in fixture manifests. [tools/README.md](../tools/README.md) documents regeneration; the Java projection regression is an explicit opt-in and was run successfully. This catalog's detailed evidence links refer to the local generated artifacts, while schematic binaries and final fixture protocols remain versioned.

## Reproduction and coordinate contract

All ports use selection-local (x,y,z) coordinates relative to the saved minimum corner. NBT cells are x-fastest, z-next, y-slowest. `Schema::read` negates Sponge displacement (v2 WEOffset metadata takes precedence; v3 Offset); `paste_clipboard` subtracts that loader offset from the supplied anchor. At saved orientation our minimum is (40,30,40), anchor = minimum + loader_offset, and absolute = minimum + local. Four-way clockwise horizontal rotations transform both coordinates and block state directions; vertical mechanisms are distinct constructions. The test helper verifies dimensions and offsets against the actual loader. [worldedit/mod.rs](../crates/core/src/plot/worldedit/mod.rs) contains the actual paste implementation; there is no clipboard.rs.

Paste writes raw states, then entities, without redstone placement notifications. Initial tick is 0/BetweenTicks and queues are empty. Each saved-idle episode runs 24 bounded game ticks to test imported quiescence; it does not prove stability under arbitrary callbacks. Prepared arithmetic writes all data caps before notifying them in declared order, then waits for two unchanged, quiescent completed boundaries, bounded at 32. Raw storage, notified source destruction, ordinary Java setblock and construction diagnostics are explicitly different operations.

The [inspection utility](../tools/inspect_instant_pistons.py) independently decodes gzip NBT v2/v3, palette/varints, exact cell counts, offsets and entities; preserves front/back/legacy original text and normalizes strings, JSON components, text/extra objects and component lists. The per-fixture label table includes candidate nearby electrical/mechanical positions; final maps follow geometry and controlled experiments, not a nearest-block rule. Full saved state and raw sign entities are in inspection JSON. CPU files retain full palette/entity inventories; their nonair cell arrays and behavior are deferred.

MCHPRS [capture](../tools/capture_instant_pistons.py) uses pico operations and nested test-scoped callback instrumentation. Samples include initial state, every action, every operation (including administrative tick completion), completed boundaries, full initial watched cells and indexed cell deltas, raw states/properties/entities, QC predicates, ordered scheduled requests (relative due/priority/type), events, motion progress/previous_progress/identity and consumer observations. Callbacks include action index, operation index, logical tick, phase and nesting depth. The scheduled execution record itself has position/type; its priority/due is retained in the preceding queue snapshot. Callback order is measured in MCHPRS; Java command captures do not expose it. Source-inferred shape notifications use West,East,North,South,Down,Up; power notifications use West,East,Down,Up,North,South. Retractions can enqueue later events synchronously through nested callbacks.

Nano snapshots represent a phase's remaining operation count, not one physical circuit 'nanotick'; pico returns after one operation including its nested callbacks. The behavioral suite compares all watched cells and global scheduled/event/motion work at aligned completed boundaries for game/nano/pico, using SHA-256 to bound test memory. Loops have 200,000-operation/tick and case-specific tick bounds. Period witnesses require three complete repeated cycles of that captured state, including blocks/entities, queued work and normalized motion state; period 1 with no work is quiescence, not oscillation. A repeated output bit alone never establishes a full-state period.

Independent [Java capture](../tools/capture_instant_pistons_java.py) verifies the official 1.21.5 binary SHA-1 `e6ec2f64e6080b9b5d9b471b291c33cc7f509733` and records SHA-256. It uses a new frozen void world and a unique chunk region per fresh case. Strict states/entity merges preserve saved configurations; preparation uses normal setblock plus eight measured setup ticks; source removal uses normal setblock air. Each trace records commands and their responses. It observes all declared ports; detailed Java arithmetic internals cover the first two stages, counter internals the first three cells, while MCHPRS records all watched components. Internal Java queues/callbacks, other Java rotations and Java derived probes are not captured. MCHPRS evidence is not an independent Minecraft oracle.

## Inventory and version boundaries

"""
    lines=[header,"| Fixture | Dimensions | Result / classification | Evidence |","| --- | --- | --- | --- |"]
    for p in sorted((PACK/"fixtures").glob("*.json")):
        m=json.loads(p.read_text());fid=m["id"]
        purpose=DOSSIERS.get(fid,("Full behavior deferred; CPU inventory only.",))[0]
        lines.append(f"| [{Path(m['fixture']).name}](../{m['fixture']}) | {' × '.join(map(str,m['dimensions']))} | {purpose} | [manifest](../test_data/instant-pistons/fixtures/{fid}.json) |")
    old=inspect(ROOT/"test_data/MCHPRS_REDSTONE_UPDATE_EDGECASE.schem",False)
    historical=inspect(ROOT/"test_data/ADDER_GWIEZDNY_TEST.schem",False)
    lines += [f"\nThe older root [edge case](../test_data/MCHPRS_REDSTONE_UPDATE_EDGECASE.schem) is {' × '.join(map(str,old['dimensions']))}, SHA-256 `{old['sha256']}`; its [Java reference](../test_data/piston-repair/java-redstone-update-edgecase.json) stays under its original provenance. The downloaded file is independently versioned below.",
              f"\n[ADDER_GWIEZDNY_TEST.schem](../test_data/ADDER_GWIEZDNY_TEST.schem), SHA-256 `{historical['sha256']}`, remains historical. Its changed-input discrepancy is not an expectation for ADDER_11BITS. No old file or frozen reference was renamed or overwritten. The earlier corrected-but-signless 21 × 7 × 45 binary is recorded in download-manifest.previous_revisions, hash `de850aabc43084b578153ea9aeb900436f2c0852ecdfe9099b0f7613f7f6f9a8`.",
              "\nADDER_11BIT is a documented remote alias of the same build with a different compressed-file hash, `b15540895ca8b21f9765e5db2b8a9a58cf635855b8cc3b15696c5ae9dc9a6124`; it is not present locally. Alias equivalence is author/download-manifest evidence, not a binary-hash substitution. All local schematic hashes match the refreshed download manifest.",
              "\nPM1_SORT and Q2CK_LyCore5_for_sorting are inventoried with hashes, dimensions, complete sign text and palettes. Full behavior coverage is deferred; no CPU compilation claim is made. A standalone BUD acceptance example remains missing. NANOTICK_EXAMPLE was supplied during this assignment and now provides an actual depth-misalignment counterexample."]
    lines += ["\n## Negation partial orders and compiler boundary\n",
"The supported NOT first wave permits both roots before event processing. A at (0,2,4) removes the ordinary output source; B at (2,2,4) uncovers the alternate supply. The latest author clarification allows equal-wave firing, and describes failure when the negating path has two actuator delays while the ordinary path has one. It does not impose movement-completion order. The recorded term 'negated piston' has no unambiguous single-base assignment; the concrete inhibit path and evaluation relation replace that ambiguity without claiming an author-selected actor.",
"\n| Episode | Measured relation | What fixes it / implication |\n| --- | --- | --- |\n| NOT AB | enqueue A < enqueue B; execute A < output=0 < execute B < output=13 | external action order, event FIFO, nested wire callbacks; a raw-wire transient occurs |\n| NOT BA | execute B/uncover supply < execute A/remove ordinary source | event FIFO; raw output never reaches zero in the first wave |\n| NOT repeater probe | B supply effective < due repeater evaluation | scheduled delay and live-input recheck suppress the same-wave AB transient |\n| AND_3 BA | remove B/power propagation < A callback at (0,2,4) < Retract enqueue | source order; remote QC power change alone does not update the base |\n| NANOTICK_EXAMPLE | execute ordinary (0,2,5) < enqueue downstream (1,2,9) < execute downstream < execute inhibit (2,2,5) | two extra negating actuators and FIFO permit an unwanted accepted trigger |\n| Required inhibit contract | inhibit effective at consumer input < downstream event validation / timed consumer evaluation | retain as a cross-region order constraint, separately from the data DAG |",
"\nXOR 11 AB executes shared A (2,2,2) < Down inhibit (1,4,1) < shared B (0,2,4); BA executes shared B < shared A < inhibit. Both raw nets briefly reach zero. Arithmetic inhibition has a stronger measured cancellation witness: a wool mover's applied retraction/connection callbacks precede an already queued target's validation. Per-stage concrete coordinates and operation indices are tabulated in the adder dossiers and manifests; 'event_execute' is an attempted validation, while 'event_applied' records successful movement. The canonical LSB sum-inhibit geometry has no active witness under its fixed-zero carry-in protocol and is marked accordingly.",
"\n```mermaid\nflowchart LR\n  R[Root falling event] --> N[Ordinary actuator]\n  R --> D1[Negating delay 1]\n  D1 --> D2[Negating delay 2]\n  D2 --> I[Inhibit actuator]\n  N --> Q[Downstream enqueue]\n  Q --> C[Downstream validation]\n  I --> E[Alternate power and update effective]\n  E -. required before .-> C\n```\n",
"NANOTICK_EXAMPLE violates the dotted relation while every event still occurs in the same game tick. Rotations preserve the tested first-wave failure, but do not prove arbitrary position/direction independence. Immediate callback instrumentation measures dispatcher entry order and actual event enqueue/execution; mutation and shape traversal between entries is source-inferred. Algebraic simplification must retain externally significant reset pulses, update outputs and pending work. Original labeled nets mostly lack non-instant sinks; full consumer contracts remain separate work. Intentional unclosed resets are rejected, even if an abstract first-wave formula appears combinational."]
    for p in sorted((PACK/"fixtures").glob("*.json")):
        m=json.loads(p.read_text());fid=m["id"]
        if fid not in DOSSIERS:
            m["unknowns"]=["Full CPU behavior/port decoder/valid protocol deferred by scope"]
            p.write_text(json.dumps(m,indent=2)+"\n",newline="\n")
            continue
        entry=DOSSIERS[fid]
        purpose,mechanism,guard=entry[0],entry[1],entry[-1]
        reset="\n\n".join(entry[2:-1])
        info=json.loads((ROOT/m["inspection"]).read_text(encoding="utf-8"))
        artifacts=[a for a in index["artifacts"] if a["fixture_id"]==fid]
        comps=[c for c in index["comparisons"] if c["fixture_id"]==fid]
        m["classification"]="IntentionalCounterexample/UnclosedReset; forbidden compiler scope" if fid=="or_interpreter_illigal" else "Stateful counter with embedded BUD memory" if fid=="counter_basic" else "Synchronization counterexample; forbidden initial compiler scope" if fid=="nanotick_example" else "Observed first-wave protocol; no generic compiler certificate"
        m["dossier"]=f"docs/INSTANT_PISTON_SCHEMATICS.md#{fid.replace('_','-')}"
        m["purpose"]=purpose;m["mechanism"]=mechanism;m["reset_and_next_use"]=reset;m["recognition_guards"]=guard
        m["setup"]["readiness"]="saved-idle remains unchanged with empty work; prepared snapshots require two unchanged quiescent completed boundaries, bounded at 32 ticks"
        m["java_status"]=dict(compared_cases=len(comps),status="all declared projections match" if all(c["status"]=="match" for c in comps) else "mismatch",scope="current binary; response boundaries, not internal callbacks; setup difference retained")
        m["evidence"]=artifacts
        m["observation_decoder"]=decoder(fid)
        electrical=[c for c in info["nonair_cells"] if any(s in c["state"] for s in ("redstone","piston","observer","repeater","comparator","lever"))]
        m["label_mappings"]=[label_mapping(fid,m,s,electrical) for s in info["signs"]]
        m["negation_paths"]=negation_paths(fid)
        inhibition_evidence(m["negation_paths"],artifacts)
        m["first_wave_orders"]=wave_orders(m,info,artifacts)
        m["ports"]["status"]="geometry and controlled episodes verified; absent sink and temporal limits described in dossier"
        m["ordering_constraints"]=[{"relation": "remote B power propagation precedes A base callback", "positions":[[0,5,6],[0,2,4]],"valid_order":"BA","AB":"retains history; diagnostic"}] if fid=="and_3" else [{"relation":"effective inhibit precedes downstream validation; preserve recorded transients at exposed ports","evidence":"measured callback/event order; NOT probe and supplied depth counterexample","scope":"consumer-specific; equality of completed boundary bits is insufficient"}] if fid in ("not_1","xor_simple","nanotick_example") else []
        m["unknowns"]=["Arbitrary non-instant consumer/handoff equivalence not certified","External input or another computation during reset is undefined"]
        if fid=="counter_basic":m["unknowns"] += ["External stop/restart, explicit clear, 16-bit wrap and high carry not verified"]
        if fid=="xor_simple":m["unknowns"] += ["AB full-state period not demonstrated in bounded trace; reset pulses reach output"]
        if fid=="mchprs_redstone_update_edgecase":m["unknowns"] += ["Full-state period not demonstrated in 48 ticks; vertical payload repeats eight ticks","Java rotations for this downloaded binary not captured"]
        if fid in ("instant_observer","instant_down","instant_reset_redstone_2","instant_chain","or_1","and_1","and_2","and_3","not_1","xor_simple","adder_1bit","adder_11bits","nanotick_example"):
            m["protocol"]["repeat_use"]="fresh ready imports for independent computations; recurring internal work retained; external rearm during active reset only diagnostic"
        elif fid in ("instant_torch","instant_down_torch_reset","instant_reset_redstone"):
            m["protocol"]["repeat_use"]=reset
        for c in m["cases"]:
            c["evidence_status"]="observed diagnostic; no reusable truth-table acceptance" if fid in ("or_interpreter_illigal","nanotick_example") or c.get("diagnostic") else "observed supported episode within dossier's protocol/window"
            if fid=="and_3" and c["id"]=="events-11-ab":c["evidence_status"]="unsupported ordering diagnostic; base not rechecked after final QC power change"
            c["artifacts"]=[a["artifact"] for a in artifacts if a["case_id"]==c["id"]]
            c["expected_status"]="behavioral regression observation; arithmetic expectation independently checked where decoder validated"
            c["observations_reference"]={"mchprs":"test_data/instant-pistons/mchprs-projections.json",
                                         "java":"test_data/instant-pistons/java-projections.json" if any(x["case_id"]==c["id"] for x in comps) else None,
                                         "key":{"fixture_id":fid,"fixture_sha256":m["sha256"],"case_id":c["id"],"rotation":"artifact-specific"}}
        p.write_text(json.dumps(m,indent=2)+"\n",newline="\n")
        lines += [f"\n## {fid.replace('_','-')}\n",f"**{Path(m['fixture']).name}: {purpose}** Exact SHA-256 `{m['sha256']}`; dimensions {' × '.join(map(str,m['dimensions']))}.",
                  f"\nMinimum origin `{m['coordinates']['origin']}`, loader offset `{m['coordinates']['loader_offset']}`, paste anchor `{m['coordinates']['paste_anchor']}`. {mdlink(m['inspection'],'Exact imported state, palette, entities and original text')}; {mdlink('test_data/instant-pistons/fixtures/'+fid+'.json','machine-readable protocol')}."]
        lines += ["\n| Sign position | Readable label/text | Candidate referenced cells | Final map/evidence |","| --- | --- | --- | --- |"]
        for mapping in m["label_mappings"]:
            lines.append(f"| `{mapping['sign_position']}` | {mapping['readable'].replace('|','&#124;')} | `{mapping['candidate_positions']}` | `{mapping['referenced_positions']}`: {mapping['role']}; geometry and current episodes |")
        if not info["signs"]:lines.append("| none | no sign entities | source and feedback paths inspected directly | independent trigger/payload episode below |")
        lines += ["\n| Port/observation | Selection-local cell or bank | Role |","| --- | --- | --- |"]
        for kind in ("inputs","observations"):
            for name,positions in m["ports"][kind].items():
                display=positions if not isinstance(positions[0],list) else f"{positions[0]} ... {positions[-1]}, {len(positions)} cells in listed bit order"
                lines.append(f"| {name} | `{display}` | {'prepared/source/update input' if kind=='inputs' else 'physical observation; decoder and window below'} |")
        lines += [f"\n{mechanism}",f"\n**Initial state and setup.** Import the exact JSON-linked saved states/entities with no notifications, tick 0 BetweenTicks, empty scheduled/event/motion work. The 24-tick saved-idle episode is quiescent. For arithmetic preparation use the declared cap encoding and bounded readiness predicate; for original gate/single episodes destroy the mapped source(s) in the manifest's exact order through interaction::destroy. Destruction writes air and performs normal shape/power notifications; callback-dependent effects remain part of the action. Java command setup/removal semantics are recorded separately.",f"\n**Reset, validity and next use.** {reset}",f"\n**Classification / compiler requirement.** {guard}"]
        if m["negation_paths"]:
            lines += ["\n| Inhibit actor / moved payload | Controlled wire / supply | Receiving actor or net | Operation relation / measured witness |","| --- | --- | --- | --- |"]
            for n in m["negation_paths"]:
                witness=next((w for w in n["measured_validations"] if not w["target_retraction_applied"]),None)
                evidence=f"{witness['case_id']}: applied inhibit op {witness['inhibit_applied_operation']} precedes target validation op {witness['target_validation_operation']}; target canceled" if witness else n.get("status","see physical episode and declared consumer-specific limit")
                lines.append(f"| `{n['actor']}` / `{n['payload']}` | `{n['controlled_wire']}` / `{n['supply']}` | `{n.get('consumer',n.get('observation'))}` | {n['required']}; {evidence} |")
        if m["dimensions"][0]<=4:
            y=2 if m["dimensions"][1]>=3 else 1
            glyph={"sticky_piston":"P","piston":"P","piston_head":"H","observer":"V","redstone_block":"R","redstone_wire":"d","sandstone":"s","stone":"s"}
            cells={tuple(c["pos"]):c["state"].removeprefix("minecraft:").split("[")[0] for c in info["nonair_cells"]}
            rows=["x="+"".join(map(str,range(m["dimensions"][0])))]
            rows += [f"z={z:02} "+"".join(glyph.get(cells.get((x,y,z),"air"),"." if (x,y,z) not in cells else "?") for x in range(m["dimensions"][0])) for z in range(m["dimensions"][2])]
            lines += [f"\nSaved slice y={y}: P base, H head, R redstone source/payload, d dust, s inert support, V observer; other states '?'. Full layers are in inspection JSON.\n\n```text\n"+"\n".join(rows)+"\n```"]
        cases=[c for c in observed if c["fixture_id"]==fid and c["rotation"]==0]
        lines += ["\n**Recorded result and physical timeline.** Values below are completed response boundaries 0..12: dust strength, payload type, or decoded air/redstone output bank. Zero-event cases observe unchanged readiness, not a newly generated logical zero input. Moving values remain explicit.","\n| Case | Ordered operations / inputs | Named observations at response boundaries |","| --- | --- | --- |"]
        for c in cases:
            if fid=="adder_11bits" and c["case_id"] not in ("saved-idle","prepared-0-0-0","prepared-1023-1-0","prepared-1024-1024-0","prepared-2047-2047-0","prepared-1365-682-0","prepared-682-1365-0"):continue
            definition=next(x for x in m["cases"] if x["id"]==c["case_id"])
            timeline="; ".join(k+": "+", ".join(scalar(s[k]) for s in c["projections"][:13]) for k in c["projections"][0] if k not in ("memory","candidate_output"))
            if fid=="counter_basic" and c["case_id"]=="released-clock":timeline="memory at 6n: 1,2,...,16; moving bank invalid between storage updates; output dust records pulses, not stored count"
            ops=", ".join(op["op"]+(" "+str(op["pos"]) if "pos" in op else " "+str(op.get("ticks",""))) for op in definition["actions"])
            if len(ops)>180:ops=f"prepare {definition['inputs']}; all writes, all notifications; bounded ready; destroy actual trigger"
            lines.append(f"| [{c['case_id']}](../{c['artifact']}) | `{ops or 'none; observe saved snapshot'}` | {timeline} |")
        witnesses=[a for a in artifacts if a["engine"]!="Java" and a["rotation"]==0 and a["period_witness"] and a["case_id"] in ("falling","events-11-ab","events-11-ba","released-clock")]
        if witnesses:lines.append("\nThree-cycle full-state witnesses: "+"; ".join(f"{a['case_id']}: period {a['period_witness']['period_ticks']} from response tick {a['period_witness']['first_tick']} (identity-renamed)" for a in witnesses)+". Period-one quiescence must be read with the endpoint geometry above.")
        lines += [f"\n**Evidence / limits.** {len(artifacts)} frozen engine/rotation artifacts; {len(comps)} independent Java r0 episode projections match. The manifest links every case and artifact; [trace index](../test_data/instant-pistons/trace-index.json) supplies exact artifact hashes and comparison scope. Game/nano/pico complete-state agreement is tested for every declared episode and MCHPRS rotation. Fresh imports and diagnostic variants remain distinct. No arbitrary consumer boundary, external reset-time input, general repeated-use contract or backend handoff is implied."]
    lines += ["\n## Validation and remaining evidence\n",
              "The focused module tests physical causal order, shared payload conservation and reset starvation, the downloaded drop/recapture, all eight full-adder vectors, corrected-adder single bits/carries/overflow/seeded vectors, persistent counter sequence, consumer-specific inhibition filtering, analog 15-to-1 then 1-to-0 behavior, and the supplied nanotick failure. Independent Java port projections include 81 episodes; all match at aligned response boundaries. Rotational Java captures and Java derived probes remain missing, as do independent command-level scheduled/callback traces.",
              "\nThe analog test is a derived source adapter for INSTANT_OBSERVER: comparator at (0,1,0), North input, initially supplied by redstone at (0,1,-1), then the downloaded furnace's 13-redstone inventory (strength one). After scheduled preparation settles, replacing that rear source with air creates the strength-one falling response. This is not a claim that strength one traverses arbitrary attenuating fixtures. Positive-to-positive preparation produces no root computation; held-zero rechecks leave existing internal work intact.",
              "\nStandalone BUD examples are still needed for an independently declared power/update interface and reset/reuse contract. Counter high-bit carry/wrap and external stop/clear, generic sink adapters, parser recognition and interpreter handoff remain later work. The author resolved counter free-running intent and illegal shared-reset scope; the exact noun 'negated piston' remains unassigned to a single base, but the operational inhibit/consumer ordering is concrete. The newly supplied nanotick example removes the earlier missing-example limitation for this depth-misalignment episode.",
              "\nExact executed commands and pass/fail counts are in [INSTANT_PISTON_VALIDATION.md](INSTANT_PISTON_VALIDATION.md). Failed exploratory capture attempts and early harness assumptions were corrected without interpreter changes or passing-oracle substitutions. All old references remain separate."]
    (ROOT/"docs/INSTANT_PISTON_SCHEMATICS.md").write_text("\n".join(lines)+"\n",encoding="utf-8",newline="\n")


if __name__=="__main__":main()
