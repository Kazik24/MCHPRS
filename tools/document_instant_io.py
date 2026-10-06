"""Build the I/O revision dossiers and manifest evidence from frozen captures.

Reviewed mechanism statements below are separate from automatically indexed observations.
py tools/document_instant_io.py --write
py tools/document_instant_io.py --check
"""
import argparse
import json
from pathlib import Path
from inspect_instant_pistons import ROOT
from validate_instant_pistons import load

PACK = ROOT / "test_data/instant-pistons-io"

MECHANISMS = {
    "instant_observer": (
        "Observer-reset horizontal instant",
        "The south-facing sticky base (0,1,5), head (0,1,6) and redstone payload (0,1,7) start extended. Down-facing observer (0,2,5), initially unpowered, watches the base; its pulse supplies reset through cap (0,3,5). A falling input withdraws the payload and removes the raw output's power. Observer reset re-extends it, then loss of reset power permits another internal cycle while the root input remains low.",
        "Recurring response, not a quiescent ready state under held activation. Base cycles recur six ticks apart; no complete-state period is certified unless the trace index gives a three-cycle witness. Fresh snapshots define independent cases. The four horizontal rotations preserve the measured named-port response.",
        "Recognize the observer/cap feedback and retain exposed repeater timing. Do not collapse its complete response into one permanent Boolean output."),
    "instant_torch": (
        "Torch-reset horizontal instant",
        "The base/head/payload are at (0,1,5/6/7). Ground reset torch (0,2,4), initially unlit, supplies a finite re-extension response, unlike the repeated observer clock. The input lever powers its support, and the input torch at (0,1,2) turns off after its normal delay. The payload is pulled toward the base, then restored to the output location.",
        "The reuse episode waits for an empty scheduled/event/motion queue and two equal completed boundaries, switches the trigger off, proves the same readiness condition, then switches it on again. Two actual retractions establish reuse; an arbitrary fixed wait is not its proof.",
        "Retain torch reset, input delay and the repeater pulse adapter. This is a reusable synchronized episode under the tested rearm rule."),
    "instant_reset_redstone": (
        "Dust-reset horizontal instant, revised reset geometry",
        "This revision raises the base/head/payload to y=2 and places reset dust (0,1,6) below the head. A retracted payload powers that dust, whose notified changes reset the piston. The lever is at (0,2,0). This is a changed construction: the earlier no-under-head-dust, one-shot classification does not apply to this binary.",
        "Held activation retains recurring internal reset/retraction work. No repeated external-computation protocol is certified; use fresh snapshots. Preserve the lower dust's consumer-dependent callbacks.",
        "Recognition must inspect the actual reset dust and payload supply; the filename is not a reset-type identifier."),
    "instant_reset_redstone_2": (
        "Lateral dust-reset horizontal instant",
        "The base is (0,1,5). Dust (1,1,6), climb (1,2,5) and support (1,1,5) form a lateral reset route from the retracted redstone payload. The Output sign at (1,0,9) describes the nearby repeater connection, not a block directly below that sign.",
        "The held input continues reset work; bounded capture termination is not settlement. Independent cases use fresh snapshots; reuse is unresolved.",
        "The climb connection and conducting support are reset guards. Removing them would create a distinct diagnostic construction."),
    "instant_down": (
        "Observer-reset downward instant",
        "The down-facing base (0,3,5), head (0,2,5) and redstone payload (0,1,5) move vertically. The raw output at (0,1,7) feeds a delay-two repeater (0,1,8). Down-facing observer (0,4,5), initially unpowered, and cap (0,5,5) reset the base; the source lever is (0,3,0).",
        "A held falling input releases recurring reset work. Fresh snapshots are the validated next-use rule. This vertical construction is measured separately from horizontal rotation.",
        "Recognize vertical payload/support effects and the explicit output path; a horizontal-family rotation is insufficient."),
    "instant_down_torch_reset": (
        "Torch-reset downward instant",
        "The vertical base/head/payload are (0,3,5), (0,2,5), (0,1,5). South-facing wall reset torch (0,4,6), initially unlit, restores the payload after the first withdrawal. The output dust and repeater remain horizontal at y=1, distinct from the actuator direction.",
        "The measured reuse episode proves quiescence after activation and after trigger rearm, then produces another retraction. The payload is retained and returned; no free-running clock is assumed.",
        "Preserve the separate vertical motion and horizontal consumer adapter, with the tested torch/rearm protocol."),
    "instant_blocked": (
        "Constant-power inhibition diagnostic",
        "The source lever can extinguish the input torch, but the redstone block (0,3,5) continues quasi-connected power to the base (0,1,5). The base stays extended and no retraction event is applied. The blocked object is the piston activation, not the lever or repeater.",
        "The imported fixture remains blocked through all 24 response boundaries. It supplies no saved control to remove the cap. The older pack contains a separately named conducting-cap diagnostic; it is not silently substituted for this revision.",
        "This negative construction guards against recognizing a base from only its apparent input wire. Include every effective power/inhibit path."),
    "instant_chain": (
        "Two-stage instant propagation",
        "Bases (0,1,5) and (0,1,8) face south, with down-facing reset observers (0,2,5)/(0,2,8) and caps (0,3,5)/(0,3,8). Payloads initially occupy (0,1,7)/(0,1,10). The first payload vacates its supplying position, and its nested neighbor callback reaches the second base. In response tick 2, operation 3 executes the first retraction and enqueues the second; operation 4 executes the second. Propagation depth is two bases in one completed game tick, with ordered operations rather than simultaneous movement.",
        "Both stages reset through the supplied geometry and continue internal work under held activation. Independent cases use fresh snapshots. The first repeater fall is delayed to tick 6.",
        "The physical relation execute_event(0,1,5) → callback(0,1,8) → enqueue(0,1,8) → execute_event(0,1,8) is measured, not inferred from a pico snapshot. A compiled synchronized wave may compose these stages while preserving the consumer response."),
    "or_1": (
        "Legal shared-payload OR with an output stage",
        "West-facing base (2,1,7) and north-facing base (0,1,9) meet at shared redstone payload (0,1,7). Either input event withdraws the same supply. The subsequent north-facing output base (0,1,5) converts this into consumer dust/repeater/lamp transitions. There is one payload with permitted drop/recapture and ownership transfer, not two independent redstone outputs.",
        "Both input orders and single-input events are captured from fresh ready states. Continuing observer/reset work is part of the same root episode. General external rearm/reuse is not established for this revision.",
        "Group the shared payload and reset closure into one region; independent per-piston ownership would reject legitimate transfers."),
    "and_1": (
        "Conjunction by two independent electrical contributors",
        "Input bases (0,1,10) and (2,1,10) feed a shared connection leading to output base (1,1,5). Either remaining payload keeps that connection positive; it falls only after both contributors withdraw. The output repeater is (1,1,1), lamp (1,1,0). This electrical join is a maximum of strengths, whose falling-event decoder yields conjunction.",
        "Both tested input orders produce the first conjunction pulse. Reset restores the contributors and creates later work; no unrestricted external reuse is certified.",
        "Recognize both contributors, the output stage and reset paths. A Boolean formula describes the certified first wave, not its whole held-input waveform."),
    "and_2": (
        "Conjunction by combined piston power",
        "Two input routes supply the upstream base (0,1,10). Removing only one leaves it powered; removing both allows retraction. Output base (0,1,5) supplies repeater (0,1,1) and lamp (0,1,0). This differs physically from AND_1's two moving contributors even though their tested first-wave tables match.",
        "Both tested input orders produce the first conjunction pulse; fresh snapshots isolate cases. Reset/next external computation beyond those snapshots remains unresolved.",
        "Recognition must retain both power routes, including QC, rather than match the AND_1 payload pattern."),
    "and_3": (
        "BUD-like conjunction with sampled history",
        "IN1 at (0,2,14) removes local power and delivers the relevant base update; IN2 at (0,5,14) removes remote QC power. The sign above (0,2,10) explicitly calls it a BUD switch that is reset. IN2-before-IN1 permits retraction, while IN1-before-IN2 leaves the base extended even though final electrical inputs match.",
        "The successful case is events-11-21. events-11-12 is a history diagnostic, not an order-independent AND failure. The reset sign expresses intent; these fresh cases do not prove removal of all stored history for arbitrary next computations.",
        "Model the independent live data and qualifying update, or reject an unsupported order-independent simplification. A data-only DAG omits this dependency."),
    "not_1": (
        "Synchronized inhibition with an explicit output stage",
        "Historical NOT naming denotes trigger IN2 (0,2,14), upstream base T=(0,2,10), inhibited by IN1 (2,2,14), base I=(2,2,10). I's sandstone payload uncovers a lower powered connection when it retracts. Output base O=(1,2,5), repeater (1,2,1) and lamp (1,2,0) are new explicit boundary components. First-wave result is IN2 AND NOT IN1.",
        "Both same-boundary source orders are measured. In events-11-21, T executes at tick 2/op 4 and enqueues O, I executes at op 5, and O executes at op 6 with restored power: it is not applied. Thus inhibitor movement need not precede target movement. Four rotations preserve the named consumer result. Delayed-input nanotick constructions were explicitly skipped for this revision.",
        "The supported condition is effective_inhibit before acceptance of O's retraction. The author allows equal-time firing and warns about extra delay pistons on the negating side. Redpiler may omit internal nanoticks for certified synchronized regions; it still needs the actual consumer adapter and reset contract."),
    "xor_simple": (
        "First-wave XOR with an unresolved reset-order discrepancy",
        "IN1 is right lever (2,2,15), IN2 left (0,2,15). West base (2,2,8), north base (0,2,10) and down-facing inhibitor (1,4,7) interact through shared payloads/dust; output base is (0,2,5). The four first-wave results match XOR at repeater tick 4 and lamp tick 8.",
        "Whole-response behavior has unresolved spatial/order and conformance differences: at MCHPRS origin (40,30,40), events-11-12 produces a later consumer fall at tick 8 and lamp dark at 12; events-11-21 stays powered. Initial Java captures at origins (6784,40,128)/(6848,40,128) show these later behaviors for the opposite orders. Additional Java captures at exactly (40,30,40) agree for order12 but still produce the later pulse for order21, differing from MCHPRS. The high-origin Java order12 is independently replayed with the same no-pulse response. Thus location matters in Java, but it does not explain all differences. Preserve every version; no location-independent constant-XOR reset contract or diagnosed interpreter-only root cause is claimed. Fresh snapshots isolate cases; no reuse claim.",
        "Do not enable a complete XOR boundary abstraction from the first truth table. The inhibitory payload must become effective before the output event is accepted, including reset-generated waves. Preserve the spatial/order differences; interpreter fixes are outside this assignment."),
    "or_interpreter_illigal": (
        "Author-excluded payload-dependent reset counterexample",
        "The saved west base (2,2,7), north base (0,2,9), matching heads and single redstone payload (0,2,7) are strictly representable. Reset dust below the heads assumes that its attached output supplies power. The other shared-output piston can take that payload away, leaving the dust-reset piston without its reset supply. The author calls that participant non-instant under this outcome and excludes this construction from scope.",
        "The six imported-state episodes are diagnostics, including an OR-like first consumer fall. Strict idle representability and observed pulses do not prove normal notified construction reachability or a valid reset. No normalized fixture or passing compiler oracle is substituted. Position/direction/random dependence is author-reported undefined behavior, not a general property inferred from the filename.",
        "Reject payload-dependent reset without closure for every permitted owner. This is a reset-supply condition, not a blanket ban on matching shared heads or legal OR payload transfer."),
    "adder_1bit": (
        "Prepared one-bit full adder with sum and carry consumers",
        "A=(1,6,3), B=(1,6,1), Cin=(8,3,6), trigger=(0,5,5). Data lever powered=false encodes one; the saved trigger is powered=true and is switched false after preparation. Sum payload (18,2,1) uses air=one, stationary redstone block=zero, moving=invalid. Sum repeater (19,2,1) has delay one; carry repeater (11,1,0) delay two. The Cout sign (12,0,0) describes that lateral connection, not its own below block.",
        "All eight prepared vectors pass S=(A+B+Cin) mod 2 and Cout=floor((A+B+Cin)/2). Raw sum is valid at response boundaries 1..3, sum repeater 3..7, carry repeater 5..9; their common consumer window is 5..7. Preparation proves quiescence with the trigger held, independent cases use fresh snapshots, and external repeated calculation is unproven.",
        "Separate prepared data from the root event and separate sum/carry publication windows. The moving/reset states are not arithmetic zeros. BUD/inhibition interactions inside the adder remain part of the synchronized-family guard."),
    "adder_11bits": (
        "Corrected eleven-bit adder with lever banks and repeater bank",
        "This I/O binary is 23×7×46, distinct from the corrected older 21×7×46 and the initial unsigned 21×7×45 revisions. A_i=(4,5,43-4i), B_i=(4,5,41-4i), i=0..10; bit zero has largest Z. Trigger=(2,5,45), separate from A/B signs, starts powered=true. Sum payload_i=(20,2,41-4i), consumer_i=(21,2,41-4i). All eleven output signs say O1: cloned text does not define bit order. Cin is fixed zero; output is modulo 2048, with no declared overflow port.",
        "Forty fresh prepared calculations cover zero, every A/B single bit, carry chains, maxima, overflow, 0x555+0x2aa in both orders and eight reproducible LCG cases (seed 0x1a57). All match the established decoder. Raw sum is valid 1..3; repeater bank 3..7. Java independently captures six calculations plus idle, not all forty. External reuse is unproven; old GWIEZDNY discrepancy traces retain old provenance.",
        "Recognize bank direction, width and explicit trigger; do not reuse old dimensions/offsets/coordinates. Preserve output reset/validity through the repeater adapter."),
    "counter_basic": (
        "Released free-running counter with BUD memory and output bank",
        "The corrected trigger lever is (7,8,0), on support (7,8,1), with torch (7,8,2). One on-transition releases the update generator at (2,11,19). Upper memory bases (5,11,4+2i) are LSB-first, stationary retracted=one/extended=zero. Real output dust is (17,1,4+2i), repeaters (18,1,4+2i); an earlier candidate at y=3 is cyan wool, not a second memory bank.",
        "Stored counts n=1..16 appear at completed boundaries 6n. The delay-one repeater bank publishes count n in conservative windows [6n+5,6n+8], bounded at capture tick 102. Outside those windows the exposed bank has resets and partial values: tick 22 decodes 1, tick 23 decodes 3. Those are not new root computations. Memory may already contain a newer wave than the consumer bank. Persistence is storage within the running sequence, not a stationary free-clock count. Clear/restart/high carry/full 16-bit wrap are unverified.",
        "Requires explicit BUD state and an owned update generator. Preserve wave identity and exposed propagation timing; a combinational node or one external event→one increment is incorrect."),
    "bud_noninstantinputs": (
        "BUD storage with separate ordinary data and update wires",
        "Down-facing memory M=(0,3,3), head (0,2,3), redstone payload (0,1,3), delay-two repeater (0,1,4). Data lever (0,7,0) negates through a torch into QC dust (0,6,3). Update lever (1,4,0) negates into EW wire (0,3,2). Its side connection delivers a callback without powering the south-adjacent memory directly. Data=true means power removed and logical one.",
        "Data-only leaves M extended despite piston_power=false. Update-before-data also holds old zero. Preparing data, proving quiescence, then updating samples zero or one: sample-1 moves at ticks 2/3, becomes stationary retracted at 4 and turns the repeater off at 6. Store-hold-resample restores data power while retaining one, then the opposite update-lever edge samples zero and restores the repeater by tick 8. All four rotations and all six Java episodes agree.",
        "A BUD port must represent both data and update. Data changes alone are not samples; either qualifying update edge can recheck power. Reuse is proved for the tested quiet store/hold/resample sequence."),
    "bud_pistonupdate": (
        "BUD storage updated by an ordinary piston head",
        "Down memory M=(0,3,4), head (0,2,4), payload (0,1,4), repeater (0,1,5). Data lever=(0,7,1), update=(1,3,0). Ordinary west-facing piston U=(1,3,3) moves its head at (0,3,3), adjacent to M. Head removal/restoration delivers the independent update that samples the QC condition.",
        "The same six storage episodes and validity points as BUD_NonInstantInputs are observed: data-only holds, sample-1 is stationary at 4/consumer off at 6, restored QC retains one until an update samples zero, consumer on by 8. Four rotations and all six Java captures agree. This establishes a quiet repeated sampling protocol with a physically different update adapter.",
        "An ordinary piston is an update owner, not an instant data formula. Recognize the changed head cell and callback route separately from data power."),
    "bud_instantpistonupdate": (
        "BUD storage with a recurring piston/observer update generator; missing saved control",
        "Saved M=(0,3,3), output=(0,1,4), data lever=(0,7,0). West-facing update piston U=(1,3,2), head=(0,3,2), down observer=(1,4,2) and cap=(1,5,2) form feedback. The Update sign is on (1,4,0), but the wall torch at (1,3,1) has no saved lever on its support. The downloaded file remains hash 8dd372…d44 after the author's correction notification.",
        "Original idle is strictly preserved. Active cases explicitly add a notified wall/north lever at (1,3,-1), prove quiet setup, then prepare data and activate it. This is a derived source adapter outside the selection, not acceptance of the original missing input. Sample-1 memory is stationary retracted at 4, consumer off at 6 and retained through 36. The update piston recurs; a six-tick complete watched-state period is witnessed over three cycles. Derived Java episodes agree.",
        "Do not enable original-fixture acceptance until the actual saved control arrives. For the derived construction, model generator state and memory, and retain normal piston callbacks at the boundary."),
    "bud_instantmemorycellobserverupdate": (
        "Instant-produced data sampled by a delayed observer update",
        "Corrected controls are Data=(1,6,3), Update=(1,4,0), both saved powered=true. Data instant=(0,6,4), with its own observer/cap, supplies QC dust (0,6,8). Memory M=(0,3,8), head (0,2,8), payload (0,1,8), repeater (0,1,9). Down generator=(1,3,5), reset observer=(1,4,5), cap=(1,5,5); north-facing observer (1,3,6) watches the generator and drives EW update wire (0,3,7). The memory is not directly powered by that wire.",
        "Prepare Data=false for one while holding Update=true and prove quiescence. Releasing Update=false launches the data wave: QC dust is zero at response tick 1, M still extended at 2; observer update wire turns positive at 3 and M starts moving. M is stationary retracted at 5, repeater off at 7, stored one retained through 36. The full watched state has a demonstrated six-tick period over three cycles. Data-only does not sample; prepared zero samples zero. Four Java episodes agree. Unrestricted new external data during the recurring response is not certified.",
        "The required logical dependency is data-wave availability before qualifying observer sample. Collapse synchronized internal order into a transaction, while preserving BUD state, generator deadlines and consumer validity."),
}


def coord(p):
    if p and isinstance(p[0], list):
        return ", ".join(coord(x) for x in p)
    return "(" + ",".join(map(str, p)) + ")"


def readable(sign):
    return " / ".join(v for face in sign["text"].values() for v in face["readable"] if v)


def mappings(m, info):
    result = []
    inp, out = m["ports"]["inputs"], m["ports"]["observations"]
    for sign in info["signs"]:
        label, p = readable(sign), sign["pos"]
        role, chosen = None, []
        if label in inp:
            role, chosen = label, inp[label]
        elif label == "IN":
            role, chosen = "trigger", inp["trigger"]
        elif label in ("Data", "Update"):
            role, chosen = label.lower(), inp[label.lower()]
        elif label == "Trigger":
            role, chosen = "trigger", inp["trigger"]
        elif label in ("OUTPUT LAMP", "Output") and "lamp" in out:
            role, chosen = "lamp", out["lamp"]
        elif label == "Output" and "sum_repeater" in out:
            role, chosen = "sum_repeater", out["sum_repeater"]
        elif label == "Output":
            role, chosen = "repeater", out["repeater"]
        elif label == "Cin":
            role, chosen = "Cin", inp["Cin"]
        elif label == "Cout":
            role, chosen = "carry_repeater", out["carry_repeater"]
        elif label.startswith(("A", "B")) and label[1:].isdigit():
            role = label[0]
            bank = inp[role]
            chosen = bank[int(label[1:])-1] if isinstance(bank[0], list) else bank
        elif label.startswith("O") and label[1:].isdigit():
            role = "sum_repeater" if "sum_repeater" in out else "repeater"
            bank = out[role]
            chosen = next(v for v in bank if v[2] == p[2])
        elif "BUD-Switch Below" in label:
            role, chosen = "memory", [5,11,p[2]]
        elif "Update generator" in label:
            role, chosen = "generator", out["generator"]
        elif "BUD-switch" in label:
            role, chosen = "upstream BUD-like base", [0,2,10]
        assert role is not None, (m["id"], label)
        below = [p[0],p[1]-1,p[2]]
        result.append(dict(sign_position=p, normalized=label, original_text=sign["text"],
            candidates=[dict(position=below,role="geometric support candidate; not automatically a port"),
                        dict(position=chosen,role=role)], port_position=chosen, role=role,
            evidence="Reviewed input attachment/electrical path or consumer connection; independent port stimuli and watched geometry. Repeated O1 labels use physical Z spacing, not text index.",
            status="derived control absent from saved selection" if chosen==[1,3,-1] else "resolved for declared episode"))
    return result


def operation_order(t, response_ticks=3):
    origin = t["coordinates"]["origin"]
    result = []
    for s in t["samples"]:
        for e in s["callbacks"]:
            if not t["start_tick"] <= e["tick"] <= t["start_tick"]+response_ticks:
                continue
            if e["kind"] not in ("event_enqueue","event_execute","event_applied"):
                continue
            d = e["data"]
            result.append(dict(tick=e["tick"]-t["start_tick"],phase=e["phase"],operation=e["operation"],
                external_action=e["action"],event=e["kind"],piston_action=d["action"],
                actor=[d["pos"][k]-origin[i] for i,k in enumerate("xyz")]))
    return result


def primary_case(m):
    ids = [c["id"] for c in m["cases"]]
    for name in ("activate", "sample-1", "release", "events-11-21", "prepared-1-1-1", "prepared-1023-1-0"):
        if name in ids:
            return name
    return ids[-1]


def decoder(fid):
    if fid.startswith("adder"):
        return dict(sum_payload="stationary air=1, redstone_block=0, moving=invalid",sum_repeater="unpowered=1; sum window ticks 3..7",carry_repeater="unpowered=1; one-bit carry window ticks 5..9",raw_valid_window=[1,3])
    if fid.startswith("bud"):
        return dict(memory="stationary retracted=1, extended=0, moving invalid",repeater="unpowered=1; ordinary/piston update sample-1 valid from tick6, observer-update from tick7; separate zero-resample timing")
    if fid=="counter_basic":
        return dict(memory="LSB increasing Z; stationary retracted=1/extended=0, moving invalid; count n at tick6n",repeater="LSB increasing Z; unpowered=1; nth count window [6n+5,6n+8], bound102")
    if fid.startswith("instant"):
        return dict(raw_output="positive-to-zero fall is event1; held zero is not a new root",repeater="first unpowered boundary6 for activation, delay2; blocked fixture stays powered")
    return dict(repeater="unpowered=1 at first-wave boundary4",lamp="dark=activation; first-wave boundary8",consumer_input="falling-event wire, not a static permanent Boolean result")


def build():
    index = load(PACK/"trace-index.json")
    manifests = {p.stem:load(p) for p in sorted((PACK/"fixtures").glob("*.json"))}
    assert set(manifests)==set(MECHANISMS)
    mch_count = sum(a["engine"] != "Java" for a in index["artifacts"])
    java_count = len(index["artifacts"])-mch_count
    lines = ["# Instant-piston schematic I/O revision", "",
        "This catalog characterizes the **separate lever-input and repeater-output revision** in `test_data/instant-pistons-io`. It includes four standalone BUD constructions. Historical binaries, port coordinates and traces remain in the [older catalog](INSTANT_PISTON_SCHEMATICS.md). Compiler abstractions belong in the [compact mathematical model](INSTANT_PISTON_REDPILER_MODEL.md); this document records the physical evidence.", "",
        f"Coverage: {len(manifests)} exact binaries, 140 declared fresh/ordered cases, {mch_count} MCHPRS case/rotation captures and {java_count} primary independent Java captures, plus two [origin-aligned XOR diagnostic versions](../test_data/instant-pistons-io/diagnostics/xor-origin-comparisons.json). [Validation and commands](INSTANT_PISTON_IO_VALIDATION.md) distinguish agreement, mismatches and incomplete controls. [Download hashes](../test_data/instant-pistons-io/download-manifest.json), [trace index](../test_data/instant-pistons-io/trace-index.json) and every per-fixture manifest preserve provenance.", "",
        "## Coordinate and setup contract", "",
        "Coordinates are selection-local `(x,y,z)` from the saved minimum corner. MCHPRS capture minimum is `(40,30,40)`. The actual loader offset is recorded separately; paste anchor = minimum + loader offset, so `paste_clipboard` places a local cell at anchor − offset + local. The [loader](../crates/core/src/plot/worldedit/schematic.rs) and [actual paste implementation](../crates/core/src/plot/worldedit/mod.rs) are the coordinate authority; there is no separate clipboard.rs here. Rotation transforms geometry, state facings, lever attachments and observed ports together.", "",
        "All originals use strict saved-state paste with saved entities and empty tick/event/motion queues. A 24-tick idle test preserves every watched cell and empty work queues. This proves representability and idle stability, not normal-placement reachability. Exact palette states, decoded volume, block entities, offsets and original/normalized front/back/legacy sign fields are in each inspection artifact. Input supports and consumer connections were reviewed separately from sign positions; the label mapping includes rejected support candidates.", "",
        "Lever changes use the real interaction notification semantics: write the changed lever, notify around the lever, then notify around its attachment block. An unchanged requested lever value performs no interaction. For prepared arithmetic/BUD data, MCHPRS `wait_ready` has a 32-tick bound and requires empty scheduled/event/motion work plus two equal completed boundaries. Java uses eight observed preparation ticks; command-only capture cannot certify its internal queue emptiness. Java lever changes are actual vanilla player UseItemOn interactions, not replacement by `setblock`.", "",
        "Ticks below are completed game-tick boundaries relative to the final action, after any preparation. Traces also contain the imported state, every after-action state, operations within ScheduledTicks/PistonEvents/MovingEntities, action index, nested callbacks, ordered requests and active motions. Pico observations alone do not establish nested callback order; event/callback instrumentation is test-scoped and disabled in server execution. Game/nano/pico agree at aligned completed boundaries for all declared MCHPRS episodes.", "",
        "Logical one at a triggering connection is a newly positive-to-zero event. Prepared lever polarity and stored BUD bits are separately decoded. Held-zero internal resets remain part of the original episode. No external input changes during instant reset, unsynchronized extra-delay construction or unrestricted next calculation is certified. A bounded recurring response ends because of its capture limit, not because it settled.", "",
        "## Overview", "", "| Schematic | Purpose | MCHPRS / Java episodes | Status |", "| --- | --- | --- | --- |"]
    for fid,m in manifests.items():
        arts=[a for a in index["artifacts"] if a["fixture_id"]==fid]
        comparisons=[c for c in index["comparisons"] if c["fixture_id"]==fid]
        status = "captured discrepancy" if any(c["status"]=="mismatch" for c in comparisons) else "declared projections agree"
        if fid=="bud_instantpistonupdate":status="derived control only; original missing lever"
        if fid=="or_interpreter_illigal":status="author-excluded; imported-state diagnostics"
        lines.append(f"| [{Path(m['fixture']).name}](#{fid.replace('_','-')}) | {MECHANISMS[fid][0]} | {sum(a['engine']!='Java' for a in arts)} / {sum(a['engine']=='Java' for a in arts)} | {status} |")
    lines += ["", "## Shared observation windows", "",
        "| Family | First physical response | First ordinary-consumer result | Scope |", "| --- | --- | --- | --- |",
        "| Single instants and chain | Input torch off/retraction at tick2, moving2/3, stationary retracted4 | Delay-two repeater off6; first restoration11 | Raw wire low2..6; internal cycles retained |",
        "| Gates with active output stage | Consumer-input wire falls2 | Delay-one repeater off4..8; lamp dark8 then lit9 | First-wave table, including zero-event snapshots; later reset separately |",
        "| Adders | Raw sum valid1..3, moving reset4/5 | Sum bank3..7; one-bit carry5..9 | Common sum/carry window5..7 |",
        "| Ordinary-wire / piston-update BUDs | Memory moving2/3, stationary4 | Sample-one repeater off6 | Stored bit holds until qualifying update; zero-resample consumer on8 |",
        "| Observer-update BUD | QC data falls1; update samples3; memory stationary5 | Repeater off7 | Data-before-update dependency; repeating generator |",
        "| Counter | Stored count n at6n | Count n during [6n+5,6n+8] | Associate each output with its wave; reset/partial values outside window |", ""]
    for fid,m in manifests.items():
        purpose, physical, reset, guard = MECHANISMS[fid]
        info=load(ROOT/m["inspection"])
        arts=[a for a in index["artifacts"] if a["fixture_id"]==fid]
        chosen=primary_case(m)
        primary=next(a for a in arts if a["engine"]!="Java" and a["case_id"]==chosen and a["rotation"]==0)
        trace=load(ROOT/primary["artifact"])
        ordered=operation_order(trace)
        labelmap=mappings(m,info)
        comparisons=[c for c in index["comparisons"] if c["fixture_id"]==fid]
        evidence=dict(schema_version=1,catalog="docs/INSTANT_PISTON_IO_SCHEMATICS.md#"+fid.replace("_","-"),
            authority=dict(author_intent="ANSWERS.md plus supplied Data/Update/port signs",imported_state=m["inspection"],
                observed_mchprs="frozen traces, not independent Minecraft oracles",observed_java="SHA-pinned vanilla 1.21.5 captures with real lever interactions",inferred_mechanism=physical,proposed_compiler_guard=guard),
            label_mappings=labelmap,observation_decoder=decoder(fid),reset_and_next_use=reset,
            initial_state=dict(source=m["inspection"],scheduled=[],piston_events=[],active_motions=[],
                observed_ready="Strict imported idle is unchanged and quiescent for 24 ticks; physical construction reachability is a separate claim"),
            ordering=dict(supported_episode=chosen,measured_operations=ordered,
                contract="Certified synchronized physical family; compiled execution omits internal nanoticks. Logical data/sample dependencies and exposed reset/consumer timing remain.",requirements=guard),
            cases=[dict(id=c["id"],artifacts=[a["artifact"] for a in arts if a["case_id"]==c["id"]],
                independent_status=[dict(engine="Java",rotation=x["rotation"],status=x["status"]) for x in comparisons if x["case_id"]==c["id"]],
                expected_status="mathematical expectation after verified decoder" if "expectation" in c else "observed declared episode; not an invented Boolean oracle") for c in m["cases"]],
            status="Two primary whole-reset differences. Exact-origin Java order12 agrees, order21 differs. Spatial effects measured; conformance root cause unresolved" if fid=="xor_simple" else "observed declared projections agree where independently captured",
            trace_index="test_data/instant-pistons-io/trace-index.json")
        if fid=="xor_simple":evidence["additional_versions"]="test_data/instant-pistons-io/diagnostics/xor-origin-comparisons.json"
        m["evidence"]=evidence
        lines += [f"## {fid.replace('_','-')}", "", f"**{Path(m['fixture']).name}: {purpose}.** {m['classification']}.", "",
            f"Binary SHA-256 `{m['sha256']}`; dimensions `{coord(m['dimensions'])}`; NBT/Sponge version `{info['sponge_version']}`, data version `{info['data_version']}`, decoded `{info['decoded_cells']}` cells. Loader offset `{coord(info['loader_offset'])}`, paste anchor `{coord(m['coordinates']['paste_anchor'])}`, absolute position = `(40,30,40)` + local for saved orientation.", "",
            f"[Manifest](../{m['fixture'].replace(Path(m['fixture']).name,'fixtures/'+fid+'.json')}) · [Exact inspection/sign entities](../{m['inspection']}) · [Measured {chosen} operations](../{primary['artifact']}).", "",
            "### Port and initial-state map", "", "| Role | Local position(s) | Saved state |", "| --- | --- | --- |"]
        cells={tuple(c["pos"]):c["state"].removeprefix("minecraft:") for c in info["nonair_cells"]}
        for role,p in list(m["ports"]["inputs"].items())+list(m["ports"]["observations"].items()):
            pp=p if isinstance(p[0],list) else [p]
            states=list(dict.fromkeys(cells.get(tuple(x),"air / absent in saved selection") for x in pp))
            lines.append(f"| {role} | `{coord(p)}` | {'; '.join('`'+v+'`' for v in states)} |")
        pistons=[(p,s) for p,s in cells.items() if s.split('[')[0] in ("piston","sticky_piston")]
        if len(pistons)<=8:
            lines += ["", "Actuator states: "+"; ".join(f"`{coord(p)} {s}`" for p,s in pistons)+". Heads and payload destinations are included in the full watched cell trace, including initially empty cells."]
        lines += ["", "| Sign position and readable text | Referenced port/mechanism | Mapping evidence |", "| --- | --- | --- |"]
        for lm in labelmap:
            lines.append(f"| `{coord(lm['sign_position'])}` {lm['normalized']} | {lm['role']} `{coord(lm['port_position'])}` | {lm['status']}; support candidate separately retained in manifest |")
        lines += ["", "### Stimulus and decoder", "",
            "Cases use equivalent fresh imports, except explicitly ordered reuse/storage histories. Exact changed blocks and action order are in the manifest; every requested unchanged lever is recorded as held, with no new notification. Inputs, observation roles and polarity are:", "",
            "```json",json.dumps(m["ports"]["roles"],indent=2),"```", "",
            "Decoder: `"+json.dumps(decoder(fid),sort_keys=True)+"`.", "",
            "| Case | Inputs | Ordered actions | Bounded response ticks |", "| --- | --- | --- | --- |"]
        for c in m["cases"]:
            ops=[]
            for op in c["actions"]:
                if op["op"]=="lever":ops.append(f"lever{coord(op['pos'])}={'on' if op['powered'] else 'off'}")
                elif op["op"] in ("wait","wait_ready"):ops.append(f"{op['op']}≤{op['ticks']}" if op["op"]=="wait_ready" else f"wait{op['ticks']}")
                else:ops.append(f"{op['op']}{coord(op['pos'])}: {op['state']}")
            lines.append(f"| `{c['id']}` | `{c['inputs']}` | {' → '.join(ops) or 'no external action'} | {c['ticks']} |")
        lines += ["", "### Physical response, reset and next use", "", physical, "", reset, "",
            "### Ordering and recognition", "", guard, "",
            "The following event operations are measured in the representative episode. Tick, phase, operation and ordered external-action indices identify them; full nested power callbacks are in the linked trace. Enqueue is a request; execution without event_applied is not accepted movement.", "",
            "| Response tick | Phase / operation | External action | Measured event | Actor |", "| --- | --- | --- | --- | --- |"]
        for e in ordered[:30]:
            lines.append(f"| {e['tick']} | {e['phase']} / {e['operation']} | {e['external_action']} | {e['event']} {e['piston_action']} | `{coord(e['actor'])}` |")
        if not ordered:lines.append("| — | — | — | no applied/requested piston operation in this first response | — |")
        lines += ["", "### Evidence status and limits", "",
            f"MCHPRS: {sum(a['engine']!='Java' for a in arts)} complete episodes, declared rotations `{m['rotations']}`. Java: {sum(a['engine']=='Java' for a in arts)} independently captured episodes; {sum(x['status']=='match' for x in comparisons)} matching projections and {sum(x['status']=='mismatch' for x in comparisons)} discrepancies. Comparison covers the declared named ports at aligned response boundaries, not Java internal callback order.", "",
            "Period witnesses: "+("; ".join(f"`{a['case_id']}` r{a['rotation']}: period {a['period_witness']['period_ticks']} ticks from logical tick {a['period_witness']['first_tick']}, three complete cycles" for a in arts if a["period_witness"] and a['rotation']==0) or "none in the captured horizon; recurring actuator cadence is not a complete-state settlement/period claim")+".", "",
            "Explicit unknowns: "+" ".join(m["unknowns"])+" Normal-notified reconstruction, arbitrary consumer changes, strength-one adapters and positive-to-positive analog changes are not certified by these binary-lever episodes. Existing old-pack diagnostics retain their own provenance.", ""]
    lines += ["## BUD geometry and synchronization", "",
        "```mermaid", "flowchart LR", "  D[Prepared data lever] --> Q[QC power condition]", "  U[Independent update lever] --> W[Wire callback or piston head change]", "  Q --> M[Down-facing BUD memory]", "  W --> M", "  M --> R[Delay-two repeater output]", "  I[Instant-produced data] --> Q", "  G[Observer update generator] --> W", "```", "",
        "For the ordinary BUD families the measured partial order is prepare_data → quiet_endpoint → qualifying_update → memory_retraction → stationary_memory → delayed_consumer. The store/hold diagnostic has data restoration → quiet retained old memory → independent update → new sample. In the observer-update construction, data wave reaches QC at tick1 before the sampling update at tick3; the repeater publishes after memory becomes stationary. These logical dependencies remain in Redpiler even when internal nanoticks are omitted.", "",
        "For new NOT_1, both event orders satisfy effective_inhibit(I) before accepted_retraction(O). An enqueue from T may occur before the inhibit; FIFO validation cancels the later output event when power has returned. No relation requiring I's movement completion before T's first retraction is claimed. XOR's reset order is unverified across engines, so its whole boundary contract remains unresolved.", "",
        "## Versions, author corrections and deferrals", "",
        "The corrected bad BUD/observer/counter saves were fetched before freezing protocols. Their earlier exact binaries are retained in [rejected imports](../test_data/instant-pistons-io/rejected-imports) with hashes in download-manifest previous_revisions. BUD_InstantPistonUpdate was rechecked after the correction notice and remained unchanged; its external lever must not be mistaken for an author-saved control. A future correction needs a new hash/version and fresh captures.", "",
        "The old `ADDER_GWIEZDNY_TEST.schem` remains an obsolete historical alias, with its old discrepancy reference. This new lever-bank adder is neither that binary nor a renamed old trace. The [historical corrected adder](INSTANT_PISTON_SCHEMATICS.md#adder-11bits) and both edge-case versions remain separate.", "",
        "Per the latest request, NANOTICK_EXAMPLE, PM1_SORT, Q2CK_LyCore5_for_sorting and MCHPRS_REDSTONE_UPDATE_EDGECASE were skipped when creating this I/O pack. They are inventoried in the old catalog; CPU analysis is deferred. Standalone BUD examples are now supplied and partly validated. No additional nanotick family or general BUD acceptance family is invented. Remaining material questions are the missing instant-update control, XOR reset-order/spatial behavior, repeated adder/gate computation, counter clear/restart/wrap and arbitrary consumer adapters.", ""]
    return manifests, "\n".join(lines)


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--write",action="store_true")
    parser.add_argument("--check",action="store_true")
    args=parser.parse_args()
    manifests, document=build()
    products={PACK/"fixtures"/(fid+".json"):json.dumps(m,indent=2)+"\n" for fid,m in manifests.items()}
    products[ROOT/"docs/INSTANT_PISTON_IO_SCHEMATICS.md"]=document
    for path,content in products.items():
        if args.check:assert path.read_text(encoding="utf-8")==content,path
        if args.write:path.write_text(content,encoding="utf-8",newline="\n")
    print("Verified/generated 22 I/O dossiers and evidence annotations")


if __name__=="__main__":main()
