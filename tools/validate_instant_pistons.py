"""Check pack provenance, complete episode coverage, coordinate maps and document links.

py tools/validate_instant_pistons.py
py tools/validate_instant_pistons.py --recapture-dir <new-MCHPRS-capture-directory>
No artifacts or expectations are changed. Run inspection/analyzer --check separately.
"""
import argparse
import gzip
import hashlib
import json
from pathlib import Path
import re
import subprocess
from urllib.parse import unquote
from inspect_instant_pistons import ROOT, PACK


def load(path):
    data=path.read_bytes()
    return json.loads(gzip.decompress(data) if path.suffix==".gz" else data)


def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def download_manifest():
    path=PACK/"download-manifest.json"
    if path.exists():return load(path)
    # Immutable fallback for offline validation without the original record.
    revision="a46bf74c0381697a2d7b18449f2b3dfdd9aa357a"
    binary=subprocess.check_output(["git","show",revision+":test_data/instant-pistons/download-manifest.json"],cwd=ROOT)
    print("Download manifest absent; verifying against Git revision",revision,"blob SHA-256",hashlib.sha256(binary).hexdigest())
    return json.loads(binary)


def check_links(path):
    content=path.read_text(encoding="utf-8")
    for target in re.findall(r"\[[^\]\n]+\]\(([^)\n]+)\)",content):
        if "://" in target or target.startswith("mailto:"):
            continue
        name,_,anchor=target.partition("#")
        linked=(path.parent/unquote(name)).resolve() if name else path
        assert linked.exists(),(path,target)
        if anchor and linked.suffix==".md":
            headings=re.findall(r"^#+\s+(.+)$",linked.read_text(encoding="utf-8"),re.M)
            slugs={re.sub(r"[^\w\- ]","",h.lower()).replace(" ","-") for h in headings}
            assert anchor in slugs,(path,target,"missing heading")


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--recapture-dir",type=Path)
    args=parser.parse_args()
    downloaded={x["name"]:x for x in download_manifest()["files"]}
    manifests={p.stem:load(p) for p in (PACK/"fixtures").glob("*.json")}
    schematics={p.stem.lower():p for p in PACK.glob("*.schem")}
    assert set(manifests)==set(schematics),"inventory mismatch"
    index=load(PACK/"trace-index.json")
    artifact_keys=set()
    indexed=set()
    for a in index["artifacts"]:
        path=ROOT/a["artifact"]
        assert sha(path)==a["artifact_sha256"],path
        assert a["fixture_sha256"]==manifests[a["fixture_id"]]["sha256"],path
        key=a["engine"],a["fixture_id"],a["case_id"],a["rotation"]
        assert key not in artifact_keys,key
        artifact_keys.add(key);indexed.add(path.resolve())
    assert indexed=={p.resolve() for p in (PACK/"traces").glob("*.json.gz")},"unindexed trace"
    for fid,m in manifests.items():
        binary=schematics[fid]
        assert sha(binary)==m["sha256"]==downloaded[binary.name]["sha256"],fid
        assert binary.stat().st_size==downloaded[binary.name]["bytes"],fid
        info=load(ROOT/m["inspection"])
        assert info["dimensions"]==m["dimensions"] and info["sha256"]==m["sha256"],fid
        assert info["loader_offset"]==m["coordinates"]["loader_offset"],fid
        assert m["coordinates"]["paste_anchor"]==[a+b for a,b in zip(m["coordinates"]["origin"],info["loader_offset"])],fid
        def positions(v):
            if isinstance(v,dict):
                for x in v.values():yield from positions(x)
            elif isinstance(v,list) and v and isinstance(v[0],int):
                yield v
            elif isinstance(v,list):
                for x in v:yield from positions(x)
        for p in positions(m["ports"]):
            assert len(p)==3 and all(0<=v<dim for v,dim in zip(p,m["dimensions"])),(fid,p)
        if m["cases"]:
            assert len(m["label_mappings"])==len(info["signs"]),fid
            assert len({c["id"] for c in m["cases"]})==len(m["cases"]),fid
            for c in m["cases"]:
                for r in m["rotations"]:
                    assert ("MCHPRS interpreter",fid,c["id"],r) in artifact_keys,(fid,c["id"],r)
                for a in c["artifacts"]:
                    assert (ROOT/a).resolve() in indexed,a
            assert isinstance(m["observation_decoder"],dict),fid
            cells={tuple(c["pos"]):c["state"] for c in info["nonair_cells"]}
            for n in m["negation_paths"]:
                assert "piston[" in cells[tuple(n["actor"])],(fid,n)
                assert cells[tuple(n["controlled_wire"])].startswith("minecraft:redstone_wire["),(fid,n)
                assert cells[tuple(n["supply"])]=="minecraft:redstone_block",(fid,n)
                if "consumer" in n:assert "piston[" in cells[tuple(n["consumer"])],(fid,n)
        else:
            assert "defer" in str(m["unknowns"]).lower(),fid
    assert all(c["status"]=="match" for c in index["comparisons"]),"Java projection mismatch"
    for name in ("INSTANT_PISTON_SCHEMATICS.md","INSTANT_PISTON_VALIDATION.md",
                 "INSTANT_PISTON_IMPLEMENTATION_PLAN.md","INSTANT_REDPILLER.md","ANSWERS.md","REDSTONE_MODEL.md"):
        check_links(ROOT/"docs"/name)
    reproduced=0
    if args.recapture_dir:
        # Evidence annotations/provenance may advance; all measured cells, work,
        # callbacks, operation/action indices and projections must match exactly.
        fields=("fixture_sha256","case_id","inputs","rotation","ordered_stimuli",
                "readiness_checks","observation_projection","stepping","start_tick",
                "limits","termination","watch_absolute","samples","tail_callbacks","projections")
        for p in args.recapture_dir.glob("*.json.gz"):
            a,b=load(p),load(PACK/"traces"/p.name)
            measured=fields if a["engine"]!="Java" else ("fixture_sha256","case_id","inputs","rotation","ordered_stimuli",
                       "server_sha1","server_sha256","origin","positions_local","start_tick","limits","termination","samples")
            for field in measured:
                assert a[field]==b[field],(p,field,"recapture differs")
            reproduced+=1
        assert reproduced,"empty recapture directory"
    print(f"Verified {len(schematics)} binaries/manifests, {len(indexed)} trace hashes, {len(index['comparisons'])} Java comparisons, coordinate/coverage maps and Markdown links; {reproduced} complete operation episodes reproduced.")


if __name__=="__main__":main()
