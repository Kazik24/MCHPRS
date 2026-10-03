# 1.21.5 registry-loading disconnect

Reported by the user from `disconnect-2026-10-03_23.35.37-client.txt`, Minecraft 1.21.5, during configuration registry loading.

The client rejected unbound tags in `minecraft:enchantment`: `exclusive_set/armor`, `boots`, `bow`, `crossbow`, `damage`, `mining`, and `riptide`.

Cause: the server transmitted enchantment definitions referencing these tags and then sent an empty configuration Update Tags packet. The independent protocol client decoded the packet format but did not initially verify registry-reference binding.

Fix: extract tag definitions from the verified Mojang 1.21.5 artifact, recursively expand tag references, resolve IDs against the exact builtin registry report and transmitted dynamic registry ordering, and send all 556 applicable vanilla tags before configuration finishes. No substitution with 1.21.1 or 1.21.3 registry data.

The independent integration test now requires all seven named enchantment exclusion sets with nonempty entries for both clients. The generator rejects unresolved required references and tag cycles. The official armor set resolves to protection, blast protection, fire protection and projectile protection in the transmitted enchantment order.

Validation: debug and release builds completed; the two-client configuration/Sponge/piston/restart integration test passed using locked Node dependencies and the new tag assertions. Generator outputs reproduce byte for byte.

The updated binaries must replace/rebuild the server process being tested. Graphical reconnect confirmation remains pending; the original report establishes this server-side defect and does not implicate the client's mods.
