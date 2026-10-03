"""Extract registry inputs from the verified 1.21.5 bundler and its --reports output.

Usage: python tools/import_official_registries.py server.jar reports/registries.json
Run Mojang's data generator in a temporary directory as documented in tools/README.md.
No server JAR is distributed in this repository.
"""
import hashlib
import io
import json
import pathlib
import sys
import zipfile

ROOT = pathlib.Path(__file__).resolve().parents[1]
DATA = ROOT / "mc_data/1.21.5"
source = json.loads((DATA / "sources.json").read_text())["official_server"]
bundler = pathlib.Path(sys.argv[1]).read_bytes()
if hashlib.sha1(bundler).hexdigest() != source["sha1"]:
    raise ValueError("Official server SHA-1 mismatch")
with zipfile.ZipFile(io.BytesIO(bundler)) as archive:
    jar = next(n for n in archive.namelist() if n.startswith("META-INF/versions/1.21.5/") and n.endswith(".jar"))
    inner = archive.read(jar)
registries = [
    "worldgen/biome", "chat_type", "trim_pattern", "trim_material", "wolf_variant",
    "wolf_sound_variant", "pig_variant", "cow_variant", "chicken_variant",
    "frog_variant", "cat_variant", "painting_variant", "dimension_type", "damage_type",
    "banner_pattern", "enchantment", "jukebox_song", "instrument",
]
data = {}
with zipfile.ZipFile(io.BytesIO(inner)) as archive:
    version = json.loads(archive.read("version.json"))
    if version["id"] != "1.21.5" or version["world_version"] != 4325 or version["protocol_version"] != 770:
        raise ValueError("Unexpected Minecraft version")
    for registry in registries:
        prefix = "data/minecraft/" + registry + "/"
        entries = {}
        for name in sorted(n for n in archive.namelist() if n.startswith(prefix) and n.endswith(".json")):
            key = name[len(prefix):-5]
            value = json.loads(archive.read(name))
            if registry == "worldgen/biome":
                # Client network codecs omit generation and mob spawning settings.
                value = {k:v for k,v in value.items() if k in ["temperature","temperature_modifier","downfall","has_precipitation","effects"]}
            entries[key] = value
        if not entries:
            raise ValueError("Missing registry " + registry)
        data["minecraft:" + registry] = entries
reports = json.loads(pathlib.Path(sys.argv[2]).read_text())
builtin = {k:reports[k] for k in [
    "minecraft:block", "minecraft:block_entity_type", "minecraft:data_component_type",
    "minecraft:entity_type", "minecraft:item", "minecraft:sound_event", "minecraft:menu",
    "minecraft:fluid", "minecraft:game_event", "minecraft:point_of_interest_type",
]}
tag_data = {}
with zipfile.ZipFile(io.BytesIO(inner)) as archive:
    for registry in [*builtin, *data]:
        prefix = "data/minecraft/tags/" + registry.split(":",1)[1] + "/"
        tags = {
            "minecraft:" + name[len(prefix):-5]: json.loads(archive.read(name))["values"]
            for name in sorted(archive.namelist()) if name.startswith(prefix) and name.endswith(".json")
        }
        if not tags:
            continue
        ids = ({key:value["protocol_id"] for key,value in builtin[registry]["entries"].items()}
               if registry in builtin else {"minecraft:"+key:i for i,key in enumerate(data[registry])})
        resolved = {}
        visiting = set()
        def resolve(key):
            if key in resolved:
                return resolved[key]
            if key in visiting:
                raise ValueError("Cyclic tag " + key)
            visiting.add(key)
            entries = []
            for value in tags[key]:
                required = True
                if isinstance(value, dict):
                    required = value.get("required",True)
                    value = value["id"]
                if value.startswith("#"):
                    reference = value[1:]
                    if reference not in tags and not required:
                        continue
                    entries.extend(resolve(reference))
                elif value in ids:
                    entries.append(ids[value])
                elif required:
                    raise ValueError(f"Missing {registry} entry {value}")
            visiting.remove(key)
            resolved[key] = list(dict.fromkeys(entries))
            return resolved[key]
        tag_data[registry] = {key:resolve(key) for key in tags}
(DATA / "registry_data.json").write_text(json.dumps(data, indent=2) + "\n")
(DATA / "builtin_ids.json").write_text(json.dumps(builtin, indent=2) + "\n")
(DATA / "registry_tags.json").write_text(json.dumps(tag_data, indent=2) + "\n")
print(f"Imported {len(data)} dynamic registries, {len(builtin)} builtin ID tables, {sum(len(v) for v in tag_data.values())} tags")
