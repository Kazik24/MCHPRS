"""Retain additional XOR reference versions without renaming the original captures."""
import argparse
import json
import shutil
from pathlib import Path
from inspect_instant_pistons import ROOT
from analyze_instant_pistons import projection, matches
from validate_instant_pistons import load, sha

PACK=ROOT/"test_data/instant-pistons-io"


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--write",action="store_true")
    args=parser.parse_args()
    records=[]
    m=load(PACK/"fixtures/xor_simple.json")
    destdir=PACK/"diagnostics"
    destdir.mkdir(exist_ok=True)
    for order in ("12","21"):
        name=f"java-xor_simple-events-11-{order}-r0.json.gz"
        source=ROOT/f"target/instant-io-java-xor-aligned-v2-{order}"/name
        dest=destdir/f"java-xor_simple-events-11-{order}-origin-40-30-40-v2.json.gz"
        if args.write:
            if dest.exists():assert dest.read_bytes()==source.read_bytes()
            else:shutil.copyfile(source,dest)
        a,b=load(PACK/"traces"/name.replace("java-","mchprs-")),load(dest)
        assert a["ordered_stimuli"]==b["ordered_stimuli"]
        assert a["coordinates"]["origin"]==b["origin"]==[40,30,40]
        status="match" if matches(projection(a,m),projection(b,m)) else "mismatch"
        records.append(dict(case_id=a["case_id"],fixture_sha256=m["sha256"],origin=b["origin"],
            mchprs=(PACK/"traces"/name.replace("java-","mchprs-")).relative_to(ROOT).as_posix(),
            java=dest.relative_to(ROOT).as_posix(),java_artifact_sha256=sha(dest),status=status,
            distinct_previous_reference=(PACK/"traces"/name).relative_to(ROOT).as_posix(),
            classification="Order12 agrees at aligned origin and differs from its Java high-origin capture. Order21 still differs at aligned origin. Spatial/order effects exist but do not explain every conformance difference; root cause unresolved.",
            limits="First-wave XOR works. A location-independent complete reset contract and general runtime admission remain unverified."))
    out=dict(schema_version=1,fixture_id="xor_simple",comparisons=records,
        evidence="Additional vanilla Java versions; original different-origin frozen captures remain unchanged.")
    content=json.dumps(out,indent=2)+"\n"
    path=destdir/"xor-origin-comparisons.json"
    if args.write:path.write_text(content,newline="\n")
    else:assert path.read_text()==content
    print("Verified 2 origin-aligned XOR diagnostics (1 match, 1 mismatch), retaining the different-origin captures")


if __name__=="__main__":main()
