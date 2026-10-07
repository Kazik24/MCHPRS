"""Freeze and summarize the new compiler/BUD characterization evidence.

py tools/summarize_piston_research.py --ingest E:/capture.json --id rilax-episodes
py tools/summarize_piston_research.py --check
Raw operation order is retained in deterministic gzip; summaries are projections.
"""
import argparse
from collections import Counter
import gzip
import hashlib
import json
from pathlib import Path
import shutil

ROOT = Path(__file__).resolve().parents[1]
PACK = ROOT/"test_data/piston-research"


def position(pos):
    return tuple(pos[k] for k in ("x", "y", "z"))


def cpu_episode(capture, case, samples):
    origin = capture["origin"]
    previous = None
    transitions, boundaries, actions, broken, diagnostics = [], [], [], [], []
    counts = Counter()
    accepted = Counter()
    for sample in samples:
        state = sample.get("state", {})
        tick = sample.get("tick", state.get("tick", 0))
        words = sample.get("words")
        if words is None:
            words = {}
            for name, blocks in state.get("ports", {}).items():
                if not name.startswith(("ram_", "register")):
                    continue
                words[name] = [None if any(b["name"] not in ("piston", "sticky_piston") for b in blocks[i:i+12]) else
                               sum(1 << bit for bit, b in enumerate(blocks[i:i+12]) if b["properties"]["extended"] == "false")
                               for i in range(0, len(blocks), 12)]
        if words != previous:
            transitions.append(dict(tick=tick, step=sample.get("step"), words=words))
            previous = words
        if sample["label"] == "boundary":
            boundaries.append(dict(tick=tick, step=sample["step"], quiet_ticks=sample["quiet_ticks"], words=words,
                                   pending=len(state["pending_ticks"]), motions=len(state["motions"]), checkpoint=sample["checkpoint"]))
        if sample["label"] == "action":
            actions.append(dict(tick=tick, **sample["action"]))
        if "diagnostics" in sample:
            diagnostics.append(dict(tick=tick, **summarize(dict(fixture=capture["fixture"], fixture_sha256=capture["fixture_sha256"],
                                                               origin=origin, diagnostics=sample["diagnostics"], episodes=[]))))
        for entry in sample.get("operations", []):
            operation = entry["operation"]
            kind = next(iter(operation))
            event = operation[kind]
            pos = tuple(a-b for a, b in zip(position(event["pos"]), origin))
            role = "internal_or_register"
            if pos[0] in range(175, 220, 4) and pos[1] in (7, 17, 27, 37) and pos[2] in range(2, 63, 4):
                role = "ram"
            elif (pos[0] == 219 and pos[1] in (7, 17, 27, 37) and pos[2] in range(3, 64, 4)) or pos == (220, 13, 156):
                role = "empty_ordinary"
            counts[role+"_"+kind] += 1
            if kind == "Applied":
                accepted[str(pos)] += 1
            else:
                counts[role+("_unchanged_sample" if event["extended"] == event["powered"] else "_changed_sample")] += 1
            if pos == (179, 6, 165):
                broken.append(entry)
    return dict(case_id=case["id"], completed_ticks=sum(s["label"] == "tick" for s in samples), actions=actions,
                boundaries=boundaries, transitions=transitions, operation_counts=dict(counts), accepted_by_position=dict(accepted),
                suspected_broken_actor_trace=broken, diagnostics=diagnostics, final_words=previous,
                full_operation_count=sum(s.get("operation_count", len(s.get("operations", []))) for s in samples))


def divider_episode(capture, episode):
    transitions, actions = [], []
    previous = None
    counts, headless = Counter(), Counter()
    for sample in episode["samples"]:
        state = sample["state"]
        bits = "".join("0" if b["properties"]["powered"] == "true" else "1"
                       for b in state["ports"]["output_msb_first"])
        if bits != previous:
            transitions.append(dict(tick=state["tick"], step=sample.get("step"),
                                    label=sample["label"], negative_bits=bits, raw=int(bits, 2)))
            previous = bits
        if sample["label"] == "action":
            actions.append(dict(tick=state["tick"], **sample["action"]))
        for entry in sample["operations"]:
            kind = next(iter(entry["operation"]))
            counts[kind] += 1
            pos = tuple(a-b for a, b in zip(position(entry["operation"][kind]["pos"]), capture["origin"]))
            if pos[0] in (14, 15) and pos[1] == 15 and pos[2] in range(12, 41, 4):
                headless[kind] += 1
    return dict(case_id=episode["case"]["id"], actions=actions, transitions=transitions,
                operation_counts=dict(counts), headless_operation_counts=dict(headless),
                completed_ticks=sum(s["label"] == "tick" for s in episode["samples"]),
                final_pending=len(state["pending_ticks"]), final_motions=len(state["motions"]),
                final_events=len(state["piston_events"]))


def summarize(capture):
    if "samples" in capture:
        return dict(schema_version=1, fixture=capture["fixture"], fixture_sha256=capture["fixture_sha256"],
                    coordinates="selection-local; RAM row 0 at z=62, bit 0 at x=175; physical layers, not proven global addresses",
                    moving_word="null means moving or missing base, not stored zero",
                    trace_scope=capture["trace_scope"], episodes=[cpu_episode(capture, capture["case"], capture["samples"])])
    origin = capture["origin"]
    local = lambda p: [a-b for a, b in zip(position(p), origin)]
    analyses = []
    for attempt in (capture.get("diagnostics") or {}).get("analysis", []):
        if "error" in attempt:
            analyses.append(attempt)
            continue
        report = attempt["report"]
        failures = Counter()
        examples = {}
        for recognition in report["recognition"]:
            for failure in recognition["failures"]:
                kind = failure if isinstance(failure, str) else next(iter(failure))
                failures[kind] += 1
                examples.setdefault(kind, dict(base=local(report["pistons"][recognition["piston"]]["pos"]), failure=failure))
        analyses.append(dict(budget=attempt["budget"], elapsed_ms=attempt["elapsed_ms"],
                            inspected_cells=report["inspected_cells"], nonair_blocks=report["nonair_blocks"],
                            dependency_steps=report["dependency_steps"], pistons=len(report["pistons"]),
                            observers=len(report["observers"]), payload_groups=len(report["payload_groups"]),
                            group_sizes={str(k): v for k, v in Counter(len(g["members"]) for g in report["payload_groups"]).items()},
                            matched_reset_mechanisms=sum(not r["failures"] for r in report["recognition"]),
                            diagnostic_counts=dict(Counter(d for p in report["pistons"] for d in p["diagnostics"])),
                            recognition_failure_counts=dict(failures), examples=examples,
                            output_interfaces=len(report["ports"]["outputs"]), reset_exposures=len(report["ports"]["reset_exposures"])))
    episodes = []
    for episode in capture["episodes"]:
        if "FPU_DIVIDER" in capture["fixture"]:
            episodes.append(divider_episode(capture, episode))
            continue
        if "CPU_BubbleSort" in capture["fixture"]:
            episodes.append(cpu_episode(capture, episode["case"], episode["samples"]))
            continue
        transitions = []
        actions = []
        applied = []
        previous = None
        sample_counts = Counter()
        for sample in episode["samples"]:
            state = sample["state"]
            ports = state["ports"]
            if "memory" not in ports:
                continue
            words = []
            for index in range(8):
                row = ports["memory"][index*8:index*8+8]
                words.append(None if any(b["name"] != "sticky_piston" for b in row) else
                             sum(1 << bit for bit, b in enumerate(row) if b["properties"]["extended"] == "false"))
            output = sum(1 << bit for bit, b in enumerate(ports["output"]) if b["properties"]["powered"] == "true")
            observed = dict(words_in_increasing_x_order=words, output=output)
            if observed != previous:
                transitions.append(dict(tick=state["tick"], step=sample.get("step"), label=sample["label"], **observed))
                previous = observed
            if sample["label"] == "action":
                actions.append(dict(tick=state["tick"], **sample["action"]))
            for entry in sample["operations"]:
                operation = entry["operation"]
                if "Applied" in operation:
                    event = operation["Applied"]
                    applied.append(dict(tick=entry["tick"], phase=entry["phase"], pos=local(event["pos"]),
                                        sticky=event["sticky"], action=event["action"]))
                else:
                    sample_counts[str(local(operation["Sample"]["pos"]))] += 1
        episodes.append(dict(case_id=episode["case"]["id"], actions=actions, transitions=transitions,
                             accepted_events=applied, sample_counts=dict(sample_counts),
                             completed_ticks=sum(s["label"] == "tick" for s in episode["samples"])))
    if "FPU_DIVIDER" in capture["fixture"]:
        return dict(schema_version=1, fixture=capture["fixture"], fixture_sha256=capture["fixture_sha256"],
                    coordinates="selection-local; negative output bits in increasing x order; reset is all powered",
                    analysis=analyses, compile=(capture.get("diagnostics") or {}).get("compile", []), episodes=episodes)
    return dict(schema_version=1, fixture=capture["fixture"], fixture_sha256=capture["fixture_sha256"],
                coordinates="selection-local in summaries; full captures retain absolute operation/work positions",
                memory_word_order="physical layers; row 0 at z=62, bit 0 at x=175" if "CPU_BubbleSort" in capture["fixture"] else "increasing x; logical address is 7-index; bit 0 at z=24",
                moving_word="null marks a moving base; it is not a stored zero",
                analysis=analyses, compile=(capture.get("diagnostics") or {}).get("compile", []), episodes=episodes)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--ingest", type=Path)
    parser.add_argument("--id")
    parser.add_argument("--check", action="store_true")
    args = parser.parse_args()
    references = PACK/"references"
    references.mkdir(exist_ok=True)
    if args.ingest:
        if not args.id or not args.id.replace("-", "").isalnum():
            parser.error("supply an alphanumeric/hyphen --id")
        traces = PACK/"traces"
        traces.mkdir(exist_ok=True)
        destination = traces/(args.id+".json.gz")
        source = references/(args.id+".source.json")
        summary = references/(args.id+".summary.json")
        if any(p.exists() for p in (destination, references/(args.id+".json.gz"), source, summary)):
            parser.error("frozen evidence requires a new id")
        raw = args.ingest.read_bytes()
        capture = json.loads(raw)
        shutil.copyfile(args.ingest.with_suffix(".source.json"), source)
        destination.write_bytes(gzip.compress(raw, mtime=0))
        summary.write_text(json.dumps(summarize(capture), indent=2)+"\n", encoding="utf-8", newline="\n")
        print(destination, destination.stat().st_size, "bytes")
    if args.check:
        for path in [*references.glob("*.json.gz"), *(PACK/"traces").glob("*.json.gz")]:
            capture = json.loads(gzip.decompress(path.read_bytes()))
            fixture = ROOT/capture["fixture"]
            assert hashlib.sha256(fixture.read_bytes()).hexdigest() == capture["fixture_sha256"], path
            stem = path.name.removesuffix(".json.gz")
            summary = references/(stem+".summary.json")
            assert json.loads(summary.read_text()) == summarize(capture), summary
            manifest = json.loads((PACK/"fixtures"/(fixture.stem.lower()+".json")).read_text())
            cases = {c["id"]: c for c in manifest["cases"]}
            for episode in capture.get("episodes", []):
                assert episode["case"] == cases[episode["case"]["id"]], path
                assert all(s["state"]["phase"] == "BetweenTicks" for s in episode["samples"] if s["label"] == "tick"), path
            if "samples" in capture:
                assert capture["case"] == cases[capture["case"]["id"]], path
                assert all(s["state"]["phase"] == "BetweenTicks" for s in capture["samples"] if s["label"] == "boundary"), path
            source = references/(stem+".source.json")
            assert json.loads(source.read_text())["revision"], source
            print("Verified", path.name)


if __name__ == "__main__":
    main()
