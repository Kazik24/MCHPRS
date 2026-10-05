"""Rebuild registry/tag binaries from pinned Minecraft inputs. Python 3.

Rust tables are expanded by mchprs_proc_macros during cargo builds.
"""

import json
import pathlib
import struct

ROOT = pathlib.Path(__file__).resolve().parents[1]
DATA = ROOT / "mc_data/1.21.5"


def load(name):
    return json.loads((DATA / (name + ".json")).read_text())


def vi(i):
    out = bytearray()
    while i > 127:
        out.append((i & 127) | 128)
        i >>= 7
    out.append(i)
    return bytes(out)


def string(s):
    b = s.encode()
    return vi(len(b)) + b


tags = {
    "byte": 1,
    "short": 2,
    "int": 3,
    "long": 4,
    "float": 5,
    "double": 6,
    "byteArray": 7,
    "string": 8,
    "list": 9,
    "compound": 10,
    "intArray": 11,
    "longArray": 12,
}


def payload(tag):
    t, v = tag["type"], tag["value"]
    if t == "compound":
        return (
            b"".join(
                bytes([tags[x["type"]]])
                + struct.pack(">H", len(k.encode()))
                + k.encode()
                + payload(x)
                for k, x in v.items()
            )
            + b"\0"
        )
    if t == "string":
        return struct.pack(">H", len(v.encode())) + v.encode()
    if t == "list":
        return (
            bytes([tags[v["type"]]])
            + struct.pack(">i", len(v["value"]))
            + b"".join(payload({"type": v["type"], "value": x}) for x in v["value"])
        )
    if t == "long":
        if isinstance(v, list):
            v = (v[0] << 32) | (v[1] & 0xFFFFFFFF)
        return struct.pack(">Q", v & 0xFFFFFFFFFFFFFFFF)
    if t.endswith("Array"):
        inner = {"byteArray": "byte", "intArray": "int", "longArray": "long"}[t]
        return struct.pack(">i", len(v)) + b"".join(
            payload({"type": inner, "value": x}) for x in v
        )
    return struct.pack(
        ">" + {"byte": "b", "short": "h", "int": "i", "float": "f", "double": "d"}[t], v
    )


def typed(value):
    if isinstance(value, bool):
        return {"type": "byte", "value": int(value)}
    if isinstance(value, int):
        return {"type": "int", "value": value}
    if isinstance(value, float):
        return {"type": "double", "value": value}
    if isinstance(value, str):
        return {"type": "string", "value": value}
    if isinstance(value, list):
        values = [typed(v) for v in value]
        kind = values[0]["type"] if values else "compound"
        if any(v["type"] != kind for v in values):
            raise ValueError("mixed NBT registry list")
        return {
            "type": "list",
            "value": {"type": kind, "value": [v["value"] for v in values]},
        }
    return {"type": "compound", "value": {k: typed(v) for k, v in value.items()}}


registries = {
    name: {
        "entries": [
            {"key": "minecraft:" + key, "value": typed(value)}
            for key, value in entries.items()
        ]
    }
    for name, entries in load("registry_data").items()
}
# Preserve the baseline geometry and bright redstone-building environment.
dimension = next(
    e["value"]
    for e in registries["minecraft:dimension_type"]["entries"]
    if e["key"] == "minecraft:overworld"
)
dimension["value"].update(
    {
        "min_y": {"type": "int", "value": 0},
        "height": {"type": "int", "value": 256},
        "logical_height": {"type": "int", "value": 256},
        "ambient_light": {"type": "float", "value": 1.0},
        "fixed_time": {"type": "long", "value": 6000},
    }
)
registries["minecraft:dimension_type"]["entries"] = [
    {"key": "mchprs:dimension", "value": dimension}
]
plains_id = next(
    i
    for i, e in enumerate(registries["minecraft:worldgen/biome"]["entries"])
    if e["key"] == "minecraft:plains"
)
out = bytearray()
for name, registry in registries.items():
    packet = vi(7) + string(name) + vi(len(registry["entries"]))
    for entry in registry["entries"]:
        packet += string(entry["key"]) + b"\1" + bytes([10]) + payload(entry["value"])
    out += struct.pack(">I", len(packet)) + packet
(DATA / "registries.bin").write_bytes(out)
tag_data = load("registry_tags")
tag_packet = vi(len(tag_data))
for registry, registry_tags in tag_data.items():
    tag_packet += string(registry) + vi(len(registry_tags))
    for name, entries in registry_tags.items():
        tag_packet += string(name) + vi(len(entries)) + b"".join(vi(i) for i in entries)
(DATA / "tags.bin").write_bytes(tag_packet)
print(f"Generated {len(registries)} registries; plains={plains_id}")
