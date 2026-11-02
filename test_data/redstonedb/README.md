# RedstoneDB fixtures

These schematics come from [RedstoneDB](https://redstonedb.com/). The manifest records each build's source page, metadata, local file, and SHA-256 hash. `definitions.json` lists the fixtures used by the comparison test; input and output positions still need manual mapping.

Download a build by its RedstoneDB ID from the repository root:

```sh
uv run --python 3.12 scripts/scrape-redstonedb.py --id BUILD_ID
```

Repeat `--id` to select several builds. Add each downloaded build to `definitions.json` to include it in the comparison.

Run the interpreter/compiler comparison with:

```sh
cargo test -p mchprs_core redstonedb_levers_match_interpreter --lib -- --nocapture
```

The test exercises lever transitions and reports failing fixtures together. Output ports are not yet mapped, so agreement between the engines does not independently establish circuit correctness.
