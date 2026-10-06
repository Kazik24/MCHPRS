"""Download the author's lever/repeater revision without replacing frozen fixtures.

py tools/download_instant_io.py
Read-only SSH/scp access to the existing schematic source; hashes are verified.
"""
import argparse
from datetime import datetime, timezone
import hashlib
import json
from pathlib import Path
import re
import shutil
import subprocess

ROOT = Path(__file__).resolve().parents[1]
SKIPPED = {"NANOTICK_EXAMPLE.schem", "PM1_SORT.schem",
           "Q2CK_LyCore5_for_sorting.schem", "MCHPRS_REDSTONE_UPDATE_EDGECASE.schem"}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--host", default="urmom")
    parser.add_argument("--source", default="/srv/mchprs/data/schems")
    parser.add_argument("--output-dir", type=Path, default=ROOT/"test_data/instant-pistons-io")
    parser.add_argument("--refresh", action="store_true", help="archive corrected imports before protocols/captures are frozen")
    args = parser.parse_args()
    if args.output_dir.exists() and not args.refresh:
        parser.error("use a new revision directory; frozen binaries are never overwritten")
    if args.refresh and (any((args.output_dir/"fixtures").glob("*.json")) or
                         any((args.output_dir/"traces").glob("*.json*"))):
        parser.error("protocols/captures exist: use a new revision directory")
    prior = json.loads((args.output_dir/"download-manifest.json").read_text()) if args.refresh else {}
    previous = prior.get("previous_revisions", [])
    old = {f["name"]: f for f in prior.get("files", [])}
    for name, f in old.items():
        assert hashlib.sha256((args.output_dir/name).read_bytes()).hexdigest() == f["sha256"], name
    names = {p.name for p in (ROOT/"test_data/instant-pistons").glob("*.schem")} - SKIPPED
    remote = """import hashlib,json,pathlib
from datetime import datetime,timezone
directory=pathlib.Path(SOURCE)
print(json.dumps([dict(name=p.name,bytes=p.stat().st_size,
sha256=hashlib.sha256(p.read_bytes()).hexdigest(),
modified_utc=datetime.fromtimestamp(p.stat().st_mtime,timezone.utc).isoformat())
for p in sorted(directory.glob('*.schem'))]))
""".replace("SOURCE", repr(args.source))
    output = subprocess.check_output(
        ["ssh", "-o", "BatchMode=yes", "-o", "ConnectTimeout=10", args.host, "python3 -"],
        input=remote.encode())
    files = [f for f in json.loads(output) if f["name"] in names or f["name"].startswith("BUD_")]
    assert names <= {f["name"] for f in files}, "an expected replacement is missing"
    assert all(re.fullmatch(r"[A-Za-z0-9_]+\.schem", f["name"]) for f in files)
    args.output_dir.mkdir(parents=True, exist_ok=True)
    changed = [f for f in files if old.get(f["name"], {}).get("sha256") != f["sha256"]]
    incoming = args.output_dir/".incoming"
    incoming.mkdir(exist_ok=True)
    if changed:
        subprocess.run(["scp", "-q", "-o", "BatchMode=yes"] +
                       [f"{args.host}:{args.source}/{f['name']}" for f in changed] +
                       [str(incoming.resolve())], check=True)
    for f in changed:
        binary = (incoming/f["name"]).read_bytes()
        assert len(binary) == f["bytes"]
        assert hashlib.sha256(binary).hexdigest() == f["sha256"], f["name"]
    for f in changed:
        if f["name"] in old:
            archive = args.output_dir/"rejected-imports"/(Path(f["name"]).stem+"-"+old[f["name"]]["sha256"][:12]+".schem")
            archive.parent.mkdir(exist_ok=True)
            assert not archive.exists()
            shutil.copyfile(args.output_dir/f["name"], archive)
            previous.append(dict(**old[f["name"]], path=archive.relative_to(ROOT).as_posix(),
                                 status="superseded import before protocol freeze; author corrected selection"))
        (incoming/f["name"]).replace(args.output_dir/f["name"])
    incoming.rmdir()
    manifest = dict(schema_version=1, source_host=args.host, source_directory=args.source,
                    verified_utc=datetime.now(timezone.utc).isoformat(),
                    files=files, skipped=sorted(SKIPPED),
                    historical_fixtures="test_data/instant-pistons; unchanged",
                    previous_revisions=previous,
                    capture_command="py tools/download_instant_io.py" + (" --refresh" if args.refresh else ""))
    (args.output_dir/"download-manifest.json").write_text(
        json.dumps(manifest, indent=2)+"\n", encoding="utf-8", newline="\n")
    print(f"Verified {len(files)} schematics in {args.output_dir}")


if __name__ == "__main__":
    main()
