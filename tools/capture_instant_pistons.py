"""Capture test-only MCHPRS operation traces and exact source provenance.

py tools/capture_instant_pistons.py --output-dir <new-directory> [--fixture not_1]
No frozen reference is overwritten. Output is deterministic gzip JSON.
"""
import argparse
import gzip
import hashlib
import json
import os
from pathlib import Path
import subprocess
from inspect_instant_pistons import ROOT, PACK

SOURCES = ["Cargo.lock", "crates/core/src/redstone/mod.rs", "crates/core/src/redstone/piston.rs",
           "crates/core/src/redstone/wire/mod.rs", "crates/core/src/redstone/wire/turbo.rs",
           "crates/core/src/redstone/wire/turbo_cache.rs", "crates/core/src/plot/mod.rs",
           "crates/core/src/plot/worldedit/mod.rs", "crates/core/src/plot/worldedit/schematic.rs",
           "crates/core/src/interaction.rs", "crates/core/src/redpiler/backend/queue.rs",
           "crates/core/src/world/mod.rs", "crates/core/src/world/wire_cache.rs",
           "crates/world/src/lib.rs", "crates/blocks/src/block_entities.rs", "crates/blocks/src/blocks/mod.rs",
           "crates/core/src/redstone/instant_piston_tests.rs", "tools/inspect_instant_pistons.py",
           "tools/capture_instant_pistons.py", "tools/instant_piston_protocols.py",
           "crates/core/src/redstone/instant_piston_tests/io.rs", "tools/instant_piston_io_protocols.py"]


def identity():
    return dict(schema_version=1, engine="MCHPRS interpreter 0.1.0", revision=subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=ROOT, text=True).strip(),
                working_tree=subprocess.check_output(["git", "status", "--porcelain=v1"], cwd=ROOT, text=True).splitlines(),
                sha256={p: hashlib.sha256((ROOT/p).read_bytes()).hexdigest() for p in SOURCES},
                runtime_changes="Recorder and fixture actions are test-only; no interpreter rule changes by characterization. Shared working-tree changes are listed separately.",
                independent_reference="MCHPRS traces are regression evidence, not independent Minecraft oracles")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output-dir", type=Path, required=True)
    parser.add_argument("--fixture")
    parser.add_argument("--pack-dir", type=Path, default=PACK)
    args = parser.parse_args()
    if args.output_dir.exists():
        parser.error("use a new output directory")
    baseline = identity()
    source = args.pack_dir.resolve()
    (source/"source-baseline.json").write_text(json.dumps(baseline, indent=2)+"\n", newline="\n")
    env = os.environ.copy()
    env["INSTANT_CAPTURE_DIR"] = str(args.output_dir.resolve())
    env["INSTANT_PACK_DIR"] = str(source)
    if args.fixture:
        env["INSTANT_FIXTURE"] = args.fixture
    command = ["cargo", "test", "-p", "mchprs_core", "--lib", "redstone::instant_piston_tests::capture_pack", "--", "--ignored", "--exact", "--test-threads=1", "--nocapture"]
    subprocess.run(command, env=env, cwd=ROOT, check=True)
    for p in sorted(args.output_dir.glob("*.json")):
        data = json.loads(p.read_bytes())
        data["capture_command"] = subprocess.list2cmdline(["py", "tools/capture_instant_pistons.py", "--output-dir", str(args.output_dir), "--pack-dir", str(args.pack_dir)] + (["--fixture", args.fixture] if args.fixture else []))
        binary = (json.dumps(data, sort_keys=True, separators=(",", ":"))+"\n").encode()
        p.with_suffix(".json.gz").write_bytes(gzip.compress(binary, mtime=0))
        p.unlink()  # exact generated file only; no recursive deletion


if __name__ == "__main__":
    main()
