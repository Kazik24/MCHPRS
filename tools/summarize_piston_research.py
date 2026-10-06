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


def summarize(capture):
    origin = capture["origin"]
    local = lambda p: [a-b for a, b in zip(position(p), origin)]
    analyses = []
    for attempt in capture["diagnostics"]["analysis"]:
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
    return dict(schema_version=1, fixture=capture["fixture"], fixture_sha256=capture["fixture_sha256"],
                coordinates="selection-local in summaries; full captures retain absolute operation/work positions",
                memory_word_order="increasing x; logical address is 7-index; bit 0 at z=24",
                moving_word="null marks a moving base; it is not a stored zero",
                analysis=analyses, compile=capture["diagnostics"]["compile"], episodes=episodes)


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
        destination = references/(args.id+".json.gz")
        source = references/(args.id+".source.json")
        summary = references/(args.id+".summary.json")
        if any(p.exists() for p in (destination, source, summary)):
            parser.error("frozen evidence requires a new id")
        raw = args.ingest.read_bytes()
        capture = json.loads(raw)
        shutil.copyfile(args.ingest.with_suffix(".source.json"), source)
        destination.write_bytes(gzip.compress(raw, mtime=0))
        summary.write_text(json.dumps(summarize(capture), indent=2)+"\n", encoding="utf-8", newline="\n")
        print(destination, destination.stat().st_size, "bytes")
    if args.check:
        for path in references.glob("*.json.gz"):
            capture = json.loads(gzip.decompress(path.read_bytes()))
            fixture = ROOT/capture["fixture"]
            assert hashlib.sha256(fixture.read_bytes()).hexdigest() == capture["fixture_sha256"], path
            summary = path.with_suffix("").with_suffix(".summary.json")
            assert json.loads(summary.read_text()) == summarize(capture), summary
            manifest = json.loads((PACK/"fixtures"/(fixture.stem.lower()+".json")).read_text())
            cases = {c["id"]: c for c in manifest["cases"]}
            for episode in capture["episodes"]:
                assert episode["case"] == cases[episode["case"]["id"]], path
                assert all(s["state"]["phase"] == "BetweenTicks" for s in episode["samples"] if s["label"] == "tick"), path
            source = path.with_suffix("").with_suffix(".source.json")
            assert json.loads(source.read_text())["revision"], source
            print("Verified", path.name)


if __name__ == "__main__":
    main()
