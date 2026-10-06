"""Reviewed coordinate maps and bounded experiments for the supplied pack.

Run once to generate manifests; subsequent analysis augments the JSON evidence.
These protocols describe operations, not a truth-table oracle based on names.
"""
import json
from pathlib import Path
from inspect_instant_pistons import ROOT, PACK, inspect


def action(op, pos=None, **kw):
    return dict(op=op, **({"pos": pos} if pos is not None else {}), **kw)


def case(name, actions=(), inputs=None, ticks=24, **kw):
    return dict(id=name, inputs=inputs, actions=list(actions), ticks=ticks, **kw)


MAPS = {
    "INSTANT_OBSERVER": ({"input": [0, 1, 0]}, {"output": [0, 1, 6]}),
    "INSTANT_TORCH": ({"input": [0, 1, 0]}, {"output": [0, 1, 6]}),
    "INSTANT_RESET_REDSTONE": ({"input": [0, 1, 0]}, {"output": [0, 1, 6]}),
    "INSTANT_RESET_REDSTONE_2": ({"input": [0, 2, 0]}, {"output": [0, 2, 6]}),
    "INSTANT_DOWN": ({"input": [0, 3, 0]}, {"output": [0, 1, 5]}),
    "INSTANT_DOWN_TORCH_RESET": ({"input": [0, 3, 0]}, {"output": [0, 1, 5]}),
    "INSTANT_BLOCKED": ({"input": [0, 1, 0], "blocker": [0, 3, 3]}, {"output": [0, 1, 6]}),
    "INSTANT_CHAIN": ({"input": [0, 1, 0]}, {"output": [0, 1, 9]}),
    "OR_1": ({"A": [2, 2, 9], "B": [0, 2, 9]}, {"output": [0, 2, 1], "payload": [0, 2, 4]}),
    "AND_1": ({"A": [2, 2, 9], "B": [0, 2, 9]}, {"output": [1, 2, 1]}),
    "AND_2": ({"A": [2, 2, 9], "B": [0, 2, 9]}, {"output": [0, 2, 1]}),
    "AND_3": ({"A": [0, 2, 6], "B": [0, 5, 6]}, {"output": [0, 2, 1]}),
    "NOT_1": ({"A": [0, 2, 6], "B": [2, 2, 6]}, {"output": [1, 2, 1]}),
    "NANOTICK_EXAMPLE": ({"trigger": [0, 2, 0]}, {"output": [1, 2, 12], "inhibited_net": [1, 2, 8]}),
    "XOR_Simple": ({"A": [2, 2, 7], "B": [0, 2, 7]}, {"output": [0, 2, 0], "inhibit_payload": [1, 2, 1]}),
    "OR_Interpreter_illigal": ({"A": [0, 2, 5], "B": [2, 2, 5]}, {"shared_payload": [0, 2, 0], "reset_A": [0, 1, 1], "reset_B": [1, 1, 0]}),
    "MCHPRS_REDSTONE_UPDATE_EDGECASE": ({"source": [6, 5, 6], "lever": [7, 5, 6]}, {"payload": [2, 2, 2], "released": [2, 1, 2], "comparator": [1, 3, 2]}),
    "ADDER_1BIT": ({"A": [1, 5, 3], "B": [1, 5, 1], "Cin": [9, 5, 5], "trigger": [0, 4, 5]}, {"sum": [18, 2, 1], "carry": [11, 1, 0], "carry_consumer": [12, 1, 2]}),
    "ADDER_11BITS": ({"A": [[3, 5, 43 - 4*i] for i in range(11)], "B": [[3, 5, 41 - 4*i] for i in range(11)], "trigger": [2, 4, 45]}, {"sum": [[20, 2, 41 - 4*i] for i in range(11)]}),
    "COUNTER_BASIC": ({"trigger": [7, 8, 0]}, {"memory": [[5, 11, 3 + 2*i] for i in range(16)], "candidate_output": [[20, 6, 3 + 2*i] for i in range(16)], "generator": [2, 11, 18]}),
}


def generate():
    dest = PACK / "fixtures"
    dest.mkdir(exist_ok=True)
    for path in sorted(PACK.glob("*.schem")):
        name = path.stem
        info = inspect(path, include_cells=False)
        inputs, outputs = MAPS.get(name, ({}, {}))
        cases = [case("saved-idle", ticks=24)]
        if name.startswith("INSTANT_"):
            p = inputs["input"]
            cases += [case("falling", [action("destroy", p)], [1]),
                      case("held-zero", [action("destroy", p), action("wait", ticks=12), action("notify", p)], [1], diagnostic="An additional notification of held zero; no new external falling event"),
                      case("rearm-diagnostic", [action("destroy", p), action("wait", ticks=12), action("set_notified", p, state="redstone_block"), action("wait", ticks=20), action("destroy", p)], [1, 1], diagnostic="Reuse becomes supported only if rearm is demonstrated")]
            if name == "INSTANT_BLOCKED":
                cases += [case("unblocked", [action("destroy", inputs["blocker"]), action("wait", ticks=12), action("destroy", p)], [1], diagnostic="Remove constant QC source before root stimulus")]
                cases += [case("unblocked-conducting-cap", [action("set_notified", inputs["blocker"], state="stone"), action("wait_ready", ticks=32), action("destroy", p)], [1], diagnostic="Derived reset variant replaces constant source with conducting cap")]
        elif name in ("OR_1", "AND_1", "AND_2", "AND_3", "NOT_1", "XOR_Simple", "OR_Interpreter_illigal"):
            for a, b, order in [(0, 0, "AB"), (1, 0, "AB"), (0, 1, "AB"), (1, 1, "AB"), (1, 1, "BA")]:
                vector = {"A": a, "B": b}
                ops = [action("destroy", inputs[key]) for key in order if vector[key]]
                cases += [case(f"events-{a}{b}-{order.lower()}", ops, [a, b])]
            if name in ("NOT_1", "XOR_Simple", "AND_3"):
                cases += [case("misaligned-ab", [action("destroy", inputs["A"]), action("wait", ticks=1), action("destroy", inputs["B"])], [1, 1], diagnostic="Input during reset, outside initial conformance")]
            if name == "OR_Interpreter_illigal":
                cases += [case("reconstruct-from-retracted", [action("construct_retracted", bases=[[0,2,2],[2,2,0]]), action("wait_ready", ticks=32)], diagnostic="Derived construction: stationary saved geometry, retracted bases, no heads, ordinary self-rechecks; compare naturally extended result")]
            if name == "NOT_1":
                for order in ("AB", "BA"):
                    ops=[action("set_notified", [2,1,1], state="stone"),action("set_notified",[2,2,1],state="repeater",properties={"facing":"west","delay":"1","locked":"false","powered":"true"}),action("wait_ready",ticks=32)]
                    ops += [action("destroy",inputs[k]) for k in order]
                    cases += [case("probe-repeater-"+order.lower(),ops,[1,1],observations={"probe":[2,2,1]},diagnostic="Derived consumer: West-input repeater at (2,2,1), stone support (2,1,1); notified writes and measured readiness")]
        elif name.startswith("ADDER_"):
            vectors = [(a, b, ci) for a in range(2) for b in range(2) for ci in range(2)] if name == "ADDER_1BIT" else [(a, b, 0) for a, b in [(0, 0), (1, 0), (0, 1), (31, 1), (1023, 1), (1024, 1024), (2047, 0), (0, 2047), (2047, 1), (2047, 2047), (0x555, 0x2aa), (0x2aa, 0x555)]]
            if name == "ADDER_11BITS":
                vectors += [(1 << i, 0, 0) for i in range(11)] + [(0, 1 << i, 0) for i in range(11)]
                seed = 0x1a57
                for _ in range(8):
                    seed = (1664525 * seed + 1013904223) & 0xffffffff
                    a = seed & 2047
                    seed = (1664525 * seed + 1013904223) & 0xffffffff
                    vectors.append((a, seed & 2047, 0))
            for a, b, ci in dict.fromkeys(vectors):
                ops = []
                # Prepared inputs are storage writes followed by all notifications.
                for key, value in [("A", a), ("B", b)] + ([("Cin", ci)] if name == "ADDER_1BIT" else []):
                    bank = inputs[key] if isinstance(inputs[key][0], list) else [inputs[key]]
                    ops += [action("raw", p, state="stone" if value & (1 << i) else "redstone_block") for i, p in enumerate(bank)]
                ops += [action("notify", op["pos"]) for op in ops.copy()]
                ops += [action("wait_ready", ticks=32), action("destroy", inputs["trigger"])]
                cases += [case(f"prepared-{a}-{b}-{ci}", ops, [a, b, ci], ticks=18)]
        elif name == "MCHPRS_REDSTONE_UPDATE_EDGECASE":
            cases += [case("falling", [action("destroy", inputs["source"])], [1], ticks=48)]
        elif name == "COUNTER_BASIC":
            cases += [case("released-clock", [action("destroy", inputs["trigger"])], [1], ticks=96)]
        elif name == "NANOTICK_EXAMPLE":
            cases += [case("falling", [action("destroy", inputs["trigger"])], [1], ticks=24)]
        else:
            cases = []
        rotations = [0, 90, 180, 270] if name in ("OR_1", "AND_1", "AND_2", "AND_3", "NOT_1", "NANOTICK_EXAMPLE", "XOR_Simple", "INSTANT_CHAIN", "MCHPRS_REDSTONE_UPDATE_EDGECASE", "OR_Interpreter_illigal") else [0]
        manifest = dict(schema_version=1, id=name.lower(), fixture=info["path"], sha256=info["sha256"], dimensions=info["dimensions"],
                        inspection=f"test_data/instant-pistons/inspection/{name.lower()}.json", classification="deferred CPU" if not cases else "observed diagnostics; function pending analysis",
                        coordinates=dict(convention="selection-local relative to saved minimum", origin=[40, 30, 40], loader_offset=info["loader_offset"], paste_anchor=[40+info["loader_offset"][0], 30+info["loader_offset"][1], 40+info["loader_offset"][2]], orientation="saved orientation; clockwise Y rotations transform geometry and states"),
                        ports=dict(inputs=inputs, observations=outputs, status="geometry-derived candidates; see dossier for per-port evidence"),
                        setup=dict(mode="paste_clipboard(ignore_air=false); raw storage then entities; no notifications", initial="exact binary states and block entities; empty queues, tick 0 BetweenTicks", readiness="saved-idle episode must remain unchanged with empty work; prepared cases wait for no queued work, bounded at 32 ticks"),
                        protocol=dict(logical_one="nonzero-to-zero event", cases_independent="fresh equivalent strict-import snapshots", input_during_reset="undefined; only labeled diagnostics exercise it", held_zero="no new external event; internal cycles retained", repeat_use="pending measured rearm condition; use fresh imports until proven"),
                        observation_decoder="raw state/property observations; moving states distinct; dust falling edge is event; air output payload means 1 only for verified arithmetic bank",
                        ordering_constraints=[], unknowns=[], java_status="not yet compared", rotations=rotations, cases=cases)
        (dest / (name.lower() + ".json")).write_text(json.dumps(manifest, indent=2) + "\n", newline="\n")


if __name__ == "__main__":
    generate()
