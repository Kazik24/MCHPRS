"""Create PM1's corrected copy; preserve every NBT byte except two block entries.

Run: py tools/fix_pm1_sort.py
"""
import gzip
import hashlib
import struct
from pathlib import Path

from inspect_instant_pistons import NBT, inspect

ROOT = Path(__file__).resolve().parents[1]
SOURCE = ROOT / "test_data/PM1_SORT.schem"
TARGET = ROOT / "test_data/PM1_SORT_FIXED.schem"
REMOVED = {
    (198, 34, 32): "minecraft:moving_piston[facing=down,type=normal]",
    (198, 35, 32): "minecraft:moving_piston[facing=down,type=sticky]",
}


class Reader(NBT):
    def value(self, tag):
        start = self.at
        value = super().value(tag)
        if tag == 7:
            self.arrays.append((start + 4, self.at))
        return value


def main():
    source = SOURCE.read_bytes()
    assert hashlib.sha256(source).hexdigest() == "e338028d50a4400056e25037d1f43d37c08baed079edede6298f78b0a6341654"
    original = inspect(SOURCE)
    assert {tuple(c["pos"]): c["state"] for c in original["nonair_cells"] if tuple(c["pos"]) in REMOVED} == REMOVED
    assert not any(tuple(e["Pos"]) in REMOVED for e in original["block_entities"])
    data = bytearray(gzip.decompress(source))
    reader = Reader(data)
    reader.arrays = []
    assert reader.number("B") == 10
    reader.string()
    root = reader.value(10)
    assert root["Version"] == 2 and len(reader.arrays) == 1
    start, end = reader.arrays[0]
    assert bytes(data[start:end]) == bytes(root["BlockData"])
    air = root["Palette"]["minecraft:air"]
    assert air < 128
    width, length = root["Width"], root["Length"]
    indices = {x + width * (z + length * y) for x, y, z in REMOVED}
    at = start
    spans = []
    for index in range(width * length * root["Height"]):
        entry = at
        while data[at] & 128:
            at += 1
        at += 1
        if index in indices:
            spans.append((entry, at))
    assert at == end
    for first, last in reversed(spans):
        data[first:last] = bytes([air])
    removed_bytes = sum(last - first - 1 for first, last in spans)
    data[start - 4:start] = struct.pack(">i", end - start - removed_bytes)
    original_data = gzip.decompress(source)
    assert data[:start - 4] == original_data[:start - 4]
    assert data[end - removed_bytes:] == original_data[end:]
    TARGET.write_bytes(gzip.compress(data, mtime=0))
    fixed = inspect(TARGET)
    assert fixed["block_entities"] == original["block_entities"]
    assert fixed["nonair_cells"] == [c for c in original["nonair_cells"] if tuple(c["pos"]) not in REMOVED]
    print(TARGET.relative_to(ROOT), fixed["sha256"])


if __name__ == "__main__":
    main()
