"""Capture compiler diagnostics and bounded interpreter episodes for author builds.

py tools/capture_piston_research.py --fixture rilax_memory_bank_bud --output E:/rilax-research.json
Use --case to select one episode. Outputs are new files; frozen traces never change.
The ordinary server contains no research recorder or interpreter-backed Redpiler.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import subprocess

ROOT = Path(__file__).resolve().parents[1]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--fixture", choices=["fpu_legal", "rilax_memory_bank_bud"], required=True)
    parser.add_argument("--case")
    parser.add_argument("--probe-logic", action="store_true", help="FPU response extraction only, bypassing executable admission")
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--target-dir", type=Path)
    args = parser.parse_args()
    if args.probe_logic and args.fixture != "fpu_legal":
        parser.error("--probe-logic applies to FPU only")
    output = args.output.resolve()
    baseline = output.with_suffix(".source.json")
    if output.exists() or baseline.exists():
        parser.error("choose a new output filename")
    output.parent.mkdir(parents=True, exist_ok=True)
    sources = ["Cargo.lock", "crates/core/src/interaction.rs"]
    for directory in ["crates/core/src/redpiler", "crates/core/src/redstone", "crates/core/src/world", "crates/world/src"]:
        sources.extend(p.relative_to(ROOT).as_posix() for p in (ROOT/directory).rglob("*.rs"))
    for name in ["crates/core/src/plot/mod.rs", "crates/core/src/plot/worldedit/schematic.rs", "crates/core/src/plot/worldedit/mod.rs"]:
        sources.append(name)
    identity = dict(revision=subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=ROOT, text=True).strip(),
                    working_tree=subprocess.check_output(["git", "status", "--porcelain=v1"], cwd=ROOT, text=True).splitlines(),
                    sha256={p: hashlib.sha256((ROOT/p).read_bytes()).hexdigest() for p in sorted(set(sources))})
    baseline.write_text(json.dumps(identity, indent=2)+"\n", encoding="utf-8", newline="\n")
    env = os.environ.copy()
    env["MCHPRS_PISTON_RESEARCH_OUTPUT"] = str(output)
    env["MCHPRS_PISTON_RESEARCH_FIXTURE"] = args.fixture
    if args.case:
        env["MCHPRS_PISTON_RESEARCH_CASE"] = args.case
    else:
        env.pop("MCHPRS_PISTON_RESEARCH_CASE", None)
    if args.target_dir:
        env["CARGO_TARGET_DIR"] = str(args.target_dir.resolve())
    test = "capture_fpu_response_extraction" if args.probe_logic else "capture_author_research_fixtures"
    command = ["cargo", "test", "-p", "mchprs_core", "--lib", "--locked",
               "redpiler::analysis::tests::research::"+test, "--",
               "--ignored", "--exact", "--test-threads=1", "--nocapture"]
    subprocess.run(command, cwd=ROOT, env=env, check=True)
    print(output)


if __name__ == "__main__":
    main()
