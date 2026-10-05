# Required development tools

This directory keeps the Minecraft data pipeline and the integration scripts
used by [CI](../.github/workflows/test.yml). The target is Minecraft Java 1.21.5,
protocol 770, DataVersion 4325.

## Minecraft data

`python tools/generate_mc_data.py` rebuilds Rust state/item mappings and
configuration registry packets from checked-in inputs. CI checks that these
outputs match the committed files. Rust builds do not execute generators.

`import_official_registries.py` reproduces the registry inputs. The pinned source
revisions and official artifact SHA-1 are in
[`sources.json`](../mc_data/1.21.5/sources.json). Download that server artifact to
a temporary directory and run Mojang's data generator there with Java 21:

```text
java -DbundlerMainClass=net.minecraft.data.Main -jar server.jar --reports --output generated
python /path/to/MCHPRS/tools/import_official_registries.py server.jar generated/reports/registries.json
python /path/to/MCHPRS/tools/generate_mc_data.py
```

The importer checks the official SHA-1 and version metadata. No server JAR is
checked in.

## CI integration tests

The protocol and plot-load runners require Python 3, Node 22 and a debug build:

```text
npm ci --prefix tools --ignore-scripts
cargo build --locked
python tools/run_protocol_smoke.py
python tools/run_plot_load_smoke.py
node tools/autostack_smoke.js
```

On Windows, `py` can replace `python`. Each Python runner starts the server in a
temporary world and invokes its matching JavaScript client. The protocol runner
uses localhost:25580 to check wire formats, commands, schematics and persistence
across restart. The plot-load runner uses localhost:25582 to check failed plot
and template loads, preservation of rejected files and graceful shutdown.
