# Port validation tools

The target is Minecraft Java 1.21.5, protocol 770, Minecraft DataVersion 4325.

`python tools/generate_mc_data.py` rebuilds Rust state/item mappings and configuration registry packets from the checked-in inputs. Rust builds do not download or execute generators.

The source revisions and official artifact SHA-1 are in [`sources.json`](../mc_data/1.21.5/sources.json). To reproduce Mojang registry inputs, download that exact server artifact to a temporary directory, then run the **data generator**, not a Minecraft server:

```text
java -DbundlerMainClass=net.minecraft.data.Main -jar server.jar --reports --output generated
python /path/to/MCHPRS/tools/import_official_registries.py server.jar generated/reports/registries.json
python /path/to/MCHPRS/tools/generate_mc_data.py
```

Use Java 21 and run the first command from the temporary directory: the bundler extracts dependencies there. The importer checks the official SHA-1 and version metadata. No server JAR is checked in.

Independent protocol tests require Python 3 and Node 22:

```text
npm ci --prefix tools --ignore-scripts
cargo build --locked
python tools/run_protocol_smoke.py
python tools/run_plot_load_smoke.py
```

On Windows, `py` can replace `python`. If your npm installation does not resolve `--prefix`, run `npm ci --ignore-scripts` inside `tools`. The runner binds localhost port 25580 and creates a temporary world. It tests two clients, configuration/login, deep chunk palette decoding, inventory components, commands, piston extension, prediction acknowledgements, Sponge v3 load/paste/undo/redo, v2 save/reload, reconnect and process restart. Both processes stop through `/stop`. It never opens the repository's existing world or configuration.

These clients independently decode the wire formats; they do not replace graphical vanilla-client checks of registry codecs, appearance and animations. The known memory-cell simulation regression remains visible in `cargo test --workspace --locked --no-fail-fast`.

The plot-load runner uses localhost port 25582 and a separate temporary world. It checks repeated attempts to enter an unsupported plot, failed spawn/template loading, disconnect reasons, continued access to healthy plots, preservation of rejected files and graceful shutdown across three server processes.

The protocol smoke runner also includes the exact production mixed-sign fixture (`test_data/sign_mixed_text_v2.schem`). It checks both sign sides' message contents, paste, undo/redo and v2 export/reload, covering the reported `HeterogeneousList` crash.
