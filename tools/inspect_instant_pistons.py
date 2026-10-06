"""Deterministic, dependency-free Sponge v2/v3 inspection (no simulation).

py tools/inspect_instant_pistons.py --write
py tools/inspect_instant_pistons.py --check
Selection-local coordinates: x fastest, then z, then y. All text is retained.
"""
import argparse
from collections import Counter
import gzip
import hashlib
import json
from pathlib import Path
import struct

ROOT = Path(__file__).resolve().parents[1]
PACK = ROOT / "test_data/instant-pistons"


class NBT:
    def __init__(self, data):
        self.data, self.at = data, 0

    def read(self, n):
        if n < 0 or self.at + n > len(self.data):
            raise ValueError("truncated NBT")
        result = self.data[self.at:self.at + n]
        self.at += n
        return result

    def number(self, fmt):
        return struct.unpack(">" + fmt, self.read(struct.calcsize(fmt)))[0]

    def string(self):
        return self.read(self.number("H")).decode("utf-8")

    def value(self, tag):
        if tag in range(1, 7):
            return self.number({1: "b", 2: "h", 3: "i", 4: "q", 5: "f", 6: "d"}[tag])
        if tag == 7:
            return list(self.read(self.number("i")))
        if tag == 8:
            return self.string()
        if tag == 9:
            kind, count = self.number("B"), self.number("i")
            if count < 0:
                raise ValueError("negative list size")
            return [self.value(kind) for _ in range(count)]
        if tag == 10:
            result = {}
            while (kind := self.number("B")) != 0:
                name = self.string()
                if name in result:
                    raise ValueError("duplicate NBT name")
                result[name] = self.value(kind)
            return result
        if tag in (11, 12):
            count = self.number("i")
            if count < 0:
                raise ValueError("negative array size")
            return [self.number("i" if tag == 11 else "q") for _ in range(count)]
        raise ValueError(f"unsupported NBT tag {tag}")


def text(component):
    if isinstance(component, str):
        try:
            decoded = json.loads(component)
        except (ValueError, TypeError):
            return component
        return text(decoded) if decoded != component else component
    if isinstance(component, list):
        return "".join(map(text, component))
    if isinstance(component, dict):
        return text(component.get("text", "")) + text(component.get("extra", []))
    return "" if component is None else str(component)


def inspect(path, include_cells=True):
    binary = path.read_bytes()
    reader = NBT(gzip.decompress(binary))
    if reader.number("B") != 10:
        raise ValueError("root must be compound")
    name = reader.string()
    root = reader.value(10)
    if reader.at != len(reader.data):
        raise ValueError("trailing NBT bytes")
    s = root.get("Schematic", root)
    version = s["Version"]
    if version not in (2, 3):
        raise ValueError("only Sponge v2/v3")
    dims = [s[k] & 65535 for k in ("Width", "Height", "Length")]
    if not all(dims):
        raise ValueError("zero dimension")
    blocks = s if version == 2 else s["Blocks"]
    palette = {i: state for state, i in blocks["Palette"].items()}
    if len(palette) != len(blocks["Palette"]) or min(palette) < 0:
        raise ValueError("invalid palette")
    encoded = blocks["BlockData" if version == 2 else "Data"]
    cells, at, counts = [], 0, Counter()
    for i in range(dims[0] * dims[1] * dims[2]):
        value = 0
        for shift in range(5):
            if at == len(encoded):
                raise ValueError("truncated varint")
            byte = encoded[at] & 255
            at += 1
            if shift == 4 and byte & 248:
                raise ValueError("varint overflow")
            value |= (byte & 127) << (shift * 7)
            if not byte & 128:
                break
        else:
            raise ValueError("unterminated varint")
        state = palette[value]
        counts[state] += 1
        if state != "minecraft:air" and include_cells:
            cells.append({"pos": [i % dims[0], i // (dims[0] * dims[2]), (i // dims[0]) % dims[2]], "state": state})
    if at != len(encoded):
        raise ValueError("extra block bytes")
    meta = s.get("Metadata", {})
    legacy = [meta.get(k, 0) for k in ("WEOffsetX", "WEOffsetY", "WEOffsetZ")]
    keys=("WEOffsetX", "WEOffsetY", "WEOffsetZ")
    # Schema::read rejects partial legacy metadata instead of filling defaults.
    if version==2 and any(k in meta for k in keys):
        offset=[meta[k] for k in keys]
    else:
        offset=s.get("Offset", [0,0,0])
    if len(offset)!=3 or any(not -(1<<31)<v<(1<<31) for v in offset):
        raise ValueError("invalid/overflowing displacement")
    entities = blocks.get("BlockEntities", [])
    seen=set()
    for e in entities:
        position=tuple(e["Pos"])
        if len(position)!=3 or any(not 0<=v<dim for v,dim in zip(position,dims)) or position in seen:
            raise ValueError("invalid/duplicate block entity position")
        seen.add(position)
    signs = []
    for e in entities:
        if "sign" not in e.get("Id", e.get("id", "")):
            continue
        d = e.get("Data", e)
        sides = {}
        for side in ("front_text", "back_text"):
            messages = d.get(side, {}).get("messages", [])
            sides[side] = {"original": messages, "readable": list(map(text, messages))}
        legacy_rows = [d[k] for k in ("Text1", "Text2", "Text3", "Text4") if k in d]
        sides["legacy"] = {"original": legacy_rows, "readable": list(map(text, legacy_rows))}
        signs.append({"pos": e["Pos"], "text": sides})
    return {"schema_version": 1, "path": path.relative_to(ROOT).as_posix(), "sha256": hashlib.sha256(binary).hexdigest(),
            "bytes": len(binary), "nbt_root_name": name, "sponge_version": version, "data_version": s["DataVersion"],
            "dimensions": dims, "decoded_cells": sum(counts.values()), "encoded_bytes": at,
            "coordinate_convention": "selection-local from saved minimum; x fastest, z next, y slowest",
            "sponge_offset": s.get("Offset"), "legacy_we_offset": legacy, "loader_offset": [-v for v in offset],
            "palette": [{"id": i, "state": palette[i], "count": counts[palette[i]]} for i in sorted(palette)],
            "block_entities": entities, "signs": sorted(signs, key=lambda x: x["pos"]), "nonair_cells": cells}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--write", action="store_true")
    parser.add_argument("--check", action="store_true")
    parser.add_argument("--pack-dir", type=Path, default=PACK)
    args = parser.parse_args()
    source = args.pack_dir.resolve()
    target = source / "inspection"
    target.mkdir(exist_ok=True)
    for path in sorted(source.glob("*.schem")):
        info = inspect(path, include_cells=path.stem not in ("PM1_SORT", "Q2CK_LyCore5_for_sorting"))
        output = target / (path.stem.lower() + ".json")
        content = json.dumps(info, indent=2, ensure_ascii=False) + "\n"
        if args.check and output.read_text(encoding="utf-8") != content:
            raise SystemExit(f"inspection mismatch: {path}")
        if args.write:
            output.write_text(content, encoding="utf-8", newline="\n")
        labels = [" / ".join(row for side in sign["text"].values() for row in side["readable"] if row) for sign in info["signs"]]
        print(path.name, info["dimensions"], info["sha256"], labels)


if __name__ == "__main__":
    main()
