"""Print measured piston operations, not an inferred order from pico snapshots."""
import argparse
from pathlib import Path
from validate_instant_pistons import load


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("trace", type=Path)
    args = parser.parse_args()
    t = load(args.trace)
    origin = t["coordinates"]["origin"]
    for sample in t["samples"]:
        for callback in sample["callbacks"]:
            if callback["tick"] > t["start_tick"] + 3:
                continue
            data = callback["data"]
            if callback["kind"] == "callback" and "piston" not in str(data.get("block", "")):
                continue
            if callback["kind"] not in ("callback", "event_enqueue", "event_execute", "event_applied"):
                continue
            pos = data.get("pos")
            if isinstance(pos, dict):
                data = dict(data, pos=[pos[k] - origin[i] for i, k in enumerate("xyz")])
            print(callback["tick"], callback["phase"], callback["operation"], callback["action"], callback["kind"], data)


if __name__ == "__main__":
    main()
