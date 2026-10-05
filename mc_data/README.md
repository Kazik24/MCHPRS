# Minecraft data inputs

Active target: **Java 1.21.5 / protocol 770 / DataVersion 4325**.

The checked-in Minecraft block, item, entity, sound and protocol datasets come from [PrismarineJS/minecraft-data](https://github.com/PrismarineJS/minecraft-data/tree/23303d2350177f2084f94cd38e5d7c8e54dfc8d3), revision `23303d2350177f2084f94cd38e5d7c8e54dfc8d3`. Its package declares the MIT license. Legacy block/item inputs are from `data/pc/1.18`; target inputs are from `data/pc/1.21.5`. Legacy item IDs start at 1; mappings use actual IDs and reserve ID 0 for air.

`registry_data.json` contains selected client registry content extracted from the exact Mojang 1.21.5 server artifact; `builtin_ids.json` contains selected generated registry reports. `registry_tags.json` resolves vanilla tag references against those exact builtin and dynamic IDs; `tags.bin` carries 556 tags in configuration. These are Minecraft data, not code copied from another server implementation. Mojang's server JAR is not included. `sources.json` records the artifact URL, SHA-1, version and codec reference revision.

Prismarine's shared `loginPacket` dataset points at 1.21.3, so it is deliberately not used to construct 1.21.5 registries. MCProtocolLib revision `290d84cbafd8d64b5cc0143bd4af9d5b4e65b675` was consulted for two component-schema corrections; no Java sources were copied into the implementation.

Rust lookup tables and protocol constants are expanded at compile time by `mchprs_proc_macros::minecraft_blocks!()` and `mchprs_proc_macros::minecraft_protocol!()`. The macros read the pinned JSON with `include_str!`, so Cargo tracks input changes; Python is not needed to build the Rust tables. The existing `generated` modules and their public statics/constants remain the API, with no JSON parsing at runtime. Macro tests fingerprint every table against the previous Python-generated output.

`registries.bin` and `tags.bin` are rebuilt by running `python tools/generate_mc_data.py` from the repository root. The script only writes those binary assets. Configuration registry ordering is fixed by the checked-in input order. MCHPRS overrides the overworld dimension to preserve min Y 0, height/logical height 256 and the bright building environment.
