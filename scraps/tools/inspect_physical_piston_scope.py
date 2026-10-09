"""Read-only candidate scope; this is not a hybrid admission certificate.

Reuses the schematic inspector. Closes motion-sensitive dust under the native
wire executor's 24 possible notification offsets, independently of saved power.
"""
import argparse
from collections import Counter, deque
import json
from pathlib import Path
import sys

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "tools"))
from inspect_instant_pistons import inspect


def candidate_scope(cells):
    directions = {
        "west": (-1, 0, 0), "east": (1, 0, 0),
        "down": (0, -1, 0), "up": (0, 1, 0),
        "north": (0, 0, -1), "south": (0, 0, 1),
    }
    offsets = [(x, y, z) for x in range(-2, 3) for y in range(-2, 3)
               for z in range(-2, 3) if 0 < abs(x) + abs(y) + abs(z) <= 2]
    assert len(offsets) == 24

    def add(pos, offset):
        return tuple(a + b for a, b in zip(pos, offset))

    def name(pos):
        return cells.get(pos, "minecraft:air").split("[", 1)[0]

    bases = {p for p in cells if name(p) in ("minecraft:piston", "minecraft:sticky_piston")}
    motion = set(bases)
    for base in bases:
        props = dict(prop.split("=", 1) for prop in cells[base].split("[", 1)[1].rstrip("]").split(","))
        direction = directions[props["facing"]]
        near = add(base, direction)
        motion.add(near)
        # Empty ordinary heads do not own the foreign base at Far.
        if name(base) == "minecraft:sticky_piston" or name(near) not in ("minecraft:air", "minecraft:piston_head"):
            motion.add(add(near, direction))
    wires = {p for p in cells if name(p) == "minecraft:redstone_wire"}
    seeds = {add(p, d) for p in motion for d in offsets} & wires
    physical = set(seeds)
    pending = deque(seeds)
    while pending:
        pos = pending.popleft()
        for offset in offsets:
            neighbor = add(pos, offset)
            if neighbor in wires and neighbor not in physical:
                physical.add(neighbor)
                pending.append(neighbor)
    touched = {add(p, d) for p in motion | physical for d in offsets}
    counts = Counter(name(p) for p in cells)
    touched_counts = Counter(name(p) for p in touched if p in cells)
    return {
        "bases": len(bases), "candidate_motion_cells": len(motion),
        "dust": len(wires), "motion_sensitive_dust_seeds": len(seeds),
        "dust_notification_closure": len(physical), "dust_outside_closure": len(wires - physical),
        "initial_nonair_by_type": dict(sorted(counts.items())),
        "potential_callback_targets_by_type": dict(sorted(touched_counts.items())),
    }


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("schematic", type=Path)
    args = parser.parse_args()
    data = inspect(args.schematic.resolve())
    cells = {tuple(cell["pos"]): cell["state"] for cell in data["nonair_cells"]}
    print(json.dumps({"sha256": data["sha256"], "scope": candidate_scope(cells)}, indent=2))
