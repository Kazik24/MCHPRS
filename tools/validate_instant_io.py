"""Verify the separately versioned I/O pack, including classified differences.

py tools/validate_instant_io.py [--recapture-dir <new capture directory>]
    Known XOR projection differences are retained, not converted to passing oracles.
"""
import argparse
from pathlib import Path
from inspect_instant_pistons import ROOT, inspect
from validate_instant_pistons import check_links, load, sha
from analyze_instant_pistons import projection, matches

PACK=ROOT/"test_data/instant-pistons-io"


def positions(v):
    if isinstance(v,dict):
        for x in v.values():yield from positions(x)
    elif isinstance(v,list) and v and isinstance(v[0],int):yield v
    elif isinstance(v,list):
        for x in v:yield from positions(x)


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--recapture-dir",type=Path)
    args=parser.parse_args()
    downloads=load(PACK/"download-manifest.json")
    downloaded={x["name"]:x for x in downloads["files"]}
    manifests={p.stem:load(p) for p in (PACK/"fixtures").glob("*.json")}
    schematics={p.stem.lower():p for p in PACK.glob("*.schem")}
    assert set(manifests)==set(schematics) and len(manifests)==22
    index=load(PACK/"trace-index.json")
    indexed=set();keys=set();artifacts={}
    for a in index["artifacts"]:
        path=ROOT/a["artifact"]
        assert sha(path)==a["artifact_sha256"],path
        key=a["engine"],a["fixture_id"],a["case_id"],a["rotation"]
        assert key not in keys,key
        keys.add(key);indexed.add(path.resolve());artifacts[a["artifact"]]=a
        assert a["fixture_sha256"]==manifests[a["fixture_id"]]["sha256"]
    assert indexed=={p.resolve() for p in (PACK/"traces").glob("*.json.gz")}
    for fid,m in manifests.items():
        p=schematics[fid];info=inspect(p)
        assert sha(p)==m["sha256"]==downloaded[p.name]["sha256"]
        assert p.stat().st_size==downloaded[p.name]["bytes"]
        assert info==load(ROOT/m["inspection"]),(fid,"inspection not reproduced")
        assert info["decoded_cells"]==m["dimensions"][0]*m["dimensions"][1]*m["dimensions"][2]
        assert info["dimensions"]==m["dimensions"]
        co=m["coordinates"]
        assert co["loader_offset"]==info["loader_offset"]
        assert co["paste_anchor"]==[a+b for a,b in zip(co["origin"],co["loader_offset"])]
        cells={tuple(c["pos"]):c["state"] for c in info["nonair_cells"]}
        for p in positions(m["ports"]["inputs"]):
            if fid=="bud_instantpistonupdate" and p==[1,3,-1]:
                assert any(a["op"]=="set_notified" and a["pos"]==p for c in m["cases"] for a in c["actions"])
                continue
            assert cells[tuple(p)].startswith("minecraft:lever"),(fid,p)
        for p in positions(m["ports"]["observations"]):
            if p==[1,3,-1]:continue
            assert len(p)==3 and all(0<=v<d for v,d in zip(p,m["dimensions"])),(fid,p)
        assert len(m["evidence"]["label_mappings"])==len(info["signs"])
        assert isinstance(m["evidence"]["observation_decoder"],dict)
        assert len({c["id"] for c in m["cases"]})==len(m["cases"])
        for c in m["cases"]:
            for r in m["rotations"]:
                assert ("MCHPRS interpreter",fid,c["id"],r) in keys,(fid,c["id"],r)
        for c in m["evidence"]["cases"]:
            assert c["artifacts"]
            assert all(p in artifacts for p in c["artifacts"])
    mismatches=[c for c in index["comparisons"] if c["status"]!="match"]
    assert {(c["fixture_id"],c["case_id"]) for c in mismatches}=={
        ("xor_simple","events-11-12"),("xor_simple","events-11-21")}
    diagnostics=load(PACK/"diagnostics/xor-origin-comparisons.json")
    assert len(diagnostics["comparisons"])==2
    for c in diagnostics["comparisons"]:
        a,b=load(ROOT/c["mchprs"]),load(ROOT/c["java"])
        assert sha(ROOT/c["java"])==c["java_artifact_sha256"]
        assert a["coordinates"]["origin"]==b["origin"]==[40,30,40]
        assert a["fixture_sha256"]==b["fixture_sha256"]==manifests["xor_simple"]["sha256"]
        assert a["ordered_stimuli"]==b["ordered_stimuli"]
        status="match" if matches(projection(a,manifests["xor_simple"]),projection(b,manifests["xor_simple"])) else "mismatch"
        assert status==c["status"]==("match" if c["case_id"].endswith("12") else "mismatch")
    for name in ("INSTANT_PISTON_IO_SCHEMATICS.md","INSTANT_PISTON_IO_VALIDATION.md",
                 "INSTANT_PISTON_REDPILER_MODEL.md","INSTANT_PISTON_IMPLEMENTATION_PLAN.md"):
        check_links(ROOT/"docs"/name)
    check_links(ROOT/"tools/README.md")
    reproduced=0
    if args.recapture_dir:
        fields=("fixture_sha256","case_id","inputs","rotation","ordered_stimuli","readiness_checks",
                "observation_projection","stepping","start_tick","limits","termination","watch_absolute",
                "samples","tail_callbacks","projections","completed_boundaries")
        for p in args.recapture_dir.glob("*.json.gz"):
            a,b=load(p),load(PACK/"traces"/p.name)
            for field in fields:assert a[field]==b[field],(p,field)
            reproduced+=1
        assert reproduced
    print(f"Verified 22 exact binaries/manifests, {len(indexed)} trace hashes, {len(index['comparisons'])} Java comparisons (2 classified differences), aligned-origin diagnostics (1 match, 1 mismatch), links and coordinates; {reproduced} complete operation episodes reproduced.")


if __name__=="__main__":main()
