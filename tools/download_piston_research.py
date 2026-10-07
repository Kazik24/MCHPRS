"""Download author research fixtures using read-only SSH access.

py tools/download_piston_research.py [--output-dir test_data/piston-research]
Existing binaries must match the remote hashes; revisions require a new directory.
"""
import argparse
from datetime import datetime, timezone
import hashlib
import json
from pathlib import Path
import subprocess

ROOT = Path(__file__).resolve().parents[1]
NAMES = ["FPU_LEGAL.schem", "RILAX_MEMORY_BANK_BUD.schem"]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--host", default="urmom")
    parser.add_argument("--source", default="/srv/mchprs/data/schems")
    parser.add_argument("--output-dir", type=Path, default=ROOT/"test_data/piston-research")
    parser.add_argument("--names", nargs="+", default=NAMES, help="schematic filenames to download")
    args = parser.parse_args()
    if any(Path(name).name != name or not name.endswith(".schem") for name in args.names):
        parser.error("names must be .schem filenames without directories")
    remote = """import hashlib,json,pathlib
from datetime import datetime,timezone
root=pathlib.Path(SOURCE)
print(json.dumps([dict(name=n,bytes=(root/n).stat().st_size,
sha256=hashlib.sha256((root/n).read_bytes()).hexdigest(),
modified_utc=datetime.fromtimestamp((root/n).stat().st_mtime,timezone.utc).isoformat())
for n in NAMES]))
""".replace("SOURCE", repr(args.source)).replace("NAMES", repr(args.names))
    files = json.loads(subprocess.check_output(
        ["ssh", "-o", "BatchMode=yes", "-o", "ConnectTimeout=10", args.host, "python3 -"], input=remote.encode()))
    args.output_dir.mkdir(parents=True, exist_ok=True)
    for file in files:
        destination = args.output_dir/file["name"]
        if destination.exists():
            if hashlib.sha256(destination.read_bytes()).hexdigest() != file["sha256"]:
                parser.error("remote revision differs; use a new output directory")
        else:
            subprocess.run(["scp", "-q", "-o", "BatchMode=yes", args.host+":"+args.source+"/"+file["name"], str(destination.resolve())], check=True)
        binary = destination.read_bytes()
        assert len(binary) == file["bytes"]
        assert hashlib.sha256(binary).hexdigest() == file["sha256"]
    manifest = args.output_dir/"download-manifest.json"
    if not manifest.exists():
        manifest.write_text(json.dumps(dict(schema_version=1, source_host=args.host, source_directory=args.source,
                            verified_utc=datetime.now(timezone.utc).isoformat(), files=files), indent=2)+"\n", encoding="utf-8", newline="\n")
    print("Verified", ", ".join(f["name"] for f in files))


if __name__ == "__main__":
    main()
