# Basic sounds (Minecraft Java 1.21.5)

## Supported actions

- Placing supported blocks: the block's vanilla placement sound, volume and pitch.
- Breaking blocks: the existing vanilla level event 2001 supplies both particles and sound.
- Levers: on/off clicks.
- Stone buttons: press and automatic release, including compiled circuits.
- Comparators: clicks when toggling compare/subtract mode. Vanilla repeater adjustment is silent.
- Chests and barrels: open on the first viewer, close on the last viewer.
- Note blocks: tuning and redstone playback, with corrected instrument registry IDs.

Existing build, interaction and container permission checks govern these actions. Sounds do not grant access to blocks or commands. Cancelled placements and already pressed buttons do not emit additional action sounds.

## Java behavior and limits

The implementation follows the official 1.21.5 Java server's `BlockItem`, `ButtonBlock`, `LeverBlock`, `ComparatorBlock` and `SoundType` behavior. Placement uses `(volume + 1) / 2` and `pitch * 0.8`. Placement and control clicks exclude the actor: their vanilla client already predicts that sound. Break events also exclude the actor. Container and note sounds include the actor.

Sound holder IDs in `sounds.json` are one based; the packet encoder accepts a zero based registry ID and adds one. Named lookup and note instrument IDs account for that offset. Each emitted packet gets a random seed for sound variation.

Sounds are sent to players on the same plot within `16 * max(volume, 1)` blocks. The plot queues at most 256 sounds per update; excess sounds are dropped. Fast rendering suppresses automated circuit sounds, while successful manual placement, control, container and note interactions still produce feedback. Container pitch is currently fixed at 0.95.

This covers existing supported interactions. It does not add new block interactions, footsteps, ambient audio, cross-plot sound propagation or sounds for bulk WorldEdit operations.

## Data provenance

`mc_data/1.21.5/block_sounds.json` contains 1,104 default block sound profiles extracted from the [official Mojang 1.21.5 server artifact](https://piston-data.mojang.com/v1/objects/e6ec2f64e6080b9b5d9b471b291c33cc7f509733/server.jar), verified against SHA-1 `e6ec2f64e6080b9b5d9b471b291c33cc7f509733`. Version metadata and mappings come from the [official manifest](https://piston-meta.mojang.com/v1/packages/5b1e09e69f4f9c650ba11c36d8b27fd0153e4e82/1.21.5.json). `sources.json` records the pinned sources.

`tools/ExportBlockSounds.java` is a development-only reflection exporter for that exact artifact. With Java 21 or later, extract the bundle's `META-INF/versions/1.21.5/server-1.21.5.jar` as `minecraft.jar` and its `META-INF/libraries` jars into `lib`, then run from that scratch directory (use `:` instead of `;` on Linux):

```powershell
javac -proc:none -cp "minecraft.jar;lib/*" -d . F:/rustrepos/MCHPRS/tools/ExportBlockSounds.java
java -cp ".;minecraft.jar;lib/*" mchprs.research.ExportBlockSounds F:/rustrepos/MCHPRS/mc_data/1.21.5/block_sounds.json
```

The exporter bootstraps the registry and reads each block's default state sound type. Its obfuscated reflection names are specific to 1.21.5. Deployment embeds the checked-in JSON during the normal Rust/Docker build; no Java exporter or custom script runs on the server.

## Verification

`cargo check -p mchprs_core` passed. The Compose release build and live service status are checked during deployment. No new automated sound tests were added or run. In-game audio and client prediction still need a listening check with a Minecraft client.
