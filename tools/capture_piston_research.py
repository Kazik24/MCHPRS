"""Capture a new native BubbleSort reference without replacing frozen reports."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import subprocess

ROOT = Path(__file__).resolve().parents[1]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--target-dir", type=Path)
    args = parser.parse_args()
    output = args.output.resolve()
    baseline = output.with_suffix(".source.json")
    if output.exists() or baseline.exists():
        parser.error("choose new output filenames")
    output.parent.mkdir(parents=True, exist_ok=True)
    sources = [
        "Cargo.lock",
        "crates/core/benches/support/cpus.rs",
        "crates/core/tests/cpu_references.rs",
        "crates/core/src/interaction.rs",
        "crates/core/src/redstone/mod.rs",
    ]
    identity = {
        "revision": subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=ROOT, text=True).strip(),
        "working_tree": subprocess.check_output(["git", "status", "--porcelain=v1"], cwd=ROOT, text=True).splitlines(),
        "sha256": {path: hashlib.sha256((ROOT / path).read_bytes()).hexdigest() for path in sources},
    }
    baseline.write_text(json.dumps(identity, indent=2) + "\n", encoding="utf-8", newline="\n")
    env = os.environ.copy()
    env["MCHPRS_PISTON_RESEARCH_OUTPUT"] = str(output)
    command = ["cargo", "test", "-p", "mchprs_core", "--test", "cpu_references", "--locked", "capture_bubblesort_reference", "--", "--ignored", "--exact", "--test-threads=1", "--nocapture"]
    if args.target_dir:
        env["CARGO_TARGET_DIR"] = str(args.target_dir.resolve())
    subprocess.run(command, cwd=ROOT, env=env, check=True)
    print(output)


if __name__ == "__main__":
    main()
