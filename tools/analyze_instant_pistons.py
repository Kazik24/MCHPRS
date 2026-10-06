"""Index frozen traces; compare independent port projections; verify artifacts.

py tools/analyze_instant_pistons.py --ingest <capture-directory> (new files only)
py tools/analyze_instant_pistons.py --write
py tools/analyze_instant_pistons.py --check
Period witnesses compare full physical state at completed boundaries, with
motion identities renamed in sequence and last_tick relative to current tick.
This does not certify arbitrary timing, construction or consumer equivalence.
"""
import argparse
import copy
import gzip
import hashlib
import json
from pathlib import Path
import shutil
from inspect_instant_pistons import ROOT, PACK


def load(p):
    return json.loads(gzip.decompress(p.read_bytes()) if p.suffix == ".gz" else p.read_bytes())


def short(v):
    if isinstance(v, list):
        return list(map(short,v))
    return {"name":v["name"],"properties":v["properties"]}


def projection(t,m):
    if t["engine"] != "Java":
        return [{k:short(v) for k,v in p.items()} for p in t["projections"]]
    positions={tuple(p):str(i) for i,p in enumerate(t["positions_local"])}
    def state(p,s):
        token=s["cells"][positions[tuple(p)]]
        name,_,props=token.partition("[")
        return dict(name=name,properties=dict(pair.split("=") for pair in props.rstrip("]").split(",") if pair))
    def walk(p,s):
        return state(p,s) if isinstance(p[0],int) else [walk(x,s) for x in p]
    response=[s for s in t["samples"] if s["tick"]>t["start_tick"]]
    first=[s for s in t["samples"] if s["tick"]==t["start_tick"]][-1]
    return [{k:walk(v,s) for k,v in m["ports"]["observations"].items()} for s in [first]+response]


def matches(actual,expected):
    if isinstance(expected,list):
        return isinstance(actual,list) and len(actual)==len(expected) and all(matches(a,e) for a,e in zip(actual,expected))
    if isinstance(expected,dict):
        return isinstance(actual,dict) and all(k in actual and matches(actual[k],v) for k,v in expected.items())
    return actual==expected


def period(t):
    if t["engine"]=="Java":
        return None
    states=[];cells={}
    for s in t["samples"]:
        cells.update({i:v for i,v in s["changes"]})
        if s["tick"]<=t["start_tick"] or s["phase"]!="BetweenTicks" or s["label"]!="step":
            continue
        motions=copy.deepcopy(s["piston_state"]["motions"])
        for i,motion in enumerate(motions):
            motion["identity"]=i
            motion["last_tick"]-=s["tick"]
        key=dict(cells=sorted(cells.items()),scheduled=s["scheduled"],events=s["piston_state"]["events"],motions=motions)
        states.append((s["tick"],hashlib.sha256(json.dumps(key,sort_keys=True).encode()).hexdigest()))
    for start in range(len(states)):
        for p in range(1,(len(states)-start)//3+1):
            a=[h for _,h in states[start:start+p]]
            if a==[h for _,h in states[start+p:start+2*p]]==[h for _,h in states[start+2*p:start+3*p]]:
                return dict(first_tick=states[start][0],period_ticks=p,repeated_complete_cycles=3,
                            projection="all watched blocks/entities/power predicates, scheduled queue, events, ordered motion progress; identities renamed, last_tick relative; BetweenTicks only")
    return None


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--ingest",type=Path)
    parser.add_argument("--exclude",help="fixture id to omit when ingesting a superseded capture directory")
    parser.add_argument("--write",action="store_true")
    parser.add_argument("--check",action="store_true")
    parser.add_argument("--pack-dir",type=Path,default=PACK)
    args=parser.parse_args()
    source=args.pack_dir.resolve()
    traces=source/"traces"
    traces.mkdir(exist_ok=True)
    if args.ingest:
        for p in sorted(args.ingest.glob("*.json.gz")):
            if args.exclude and f"-{args.exclude}-" in p.name:
                continue
            dest=traces/p.name
            if dest.exists():
                # Existing frozen episodes are immutable; duplicate capture is not an overwrite.
                a,b=load(dest),load(p)
                fields=("engine","fixture_sha256","case_id","inputs","rotation","ordered_stimuli","samples")
                if any(a.get(k)!=b.get(k) for k in fields):
                    raise ValueError(f"distinct capture needs a versioned filename: {dest}")
                continue
            shutil.copyfile(p,dest)
        return
    manifests={p.stem:load(p) for p in (source/"fixtures").glob("*.json")}
    index=[]; observed=[]; independent=[]; by_case={}
    for p in sorted(traces.glob("*.json.gz")):
        t=load(p)
        fid=Path(t["fixture"]).stem.lower()
        m=manifests[fid]
        assert t["fixture_sha256"]==m["sha256"],p
        c=next(c for c in m["cases"] if c["id"]==t["case_id"])
        assert t["ordered_stimuli"]==c["actions"],p
        if t["engine"]!="Java":
            assert t["engine_identity"]["revision"],p
            assert t["samples"][0]["label"]=="initial",p
            assert t["samples"][-1]["phase"]=="BetweenTicks",p
        proj=projection(t,m)
        entry=dict(artifact=p.relative_to(ROOT).as_posix(),artifact_sha256=hashlib.sha256(p.read_bytes()).hexdigest(),
                   fixture_id=fid,fixture_sha256=m["sha256"],case_id=t["case_id"],rotation=t["rotation"],engine=t["engine"],termination=t["termination"],period_witness=period(t))
        index.append(entry)
        record=dict(fixture_id=fid,fixture_sha256=m["sha256"],case_id=t["case_id"],rotation=t["rotation"],artifact=entry["artifact"],actions=c["actions"],projections=proj)
        if t["engine"]=="Java":
            record["server_sha1"]=t["server_sha1"];record["server_sha256"]=t["server_sha256"]
            independent.append(record)
        else:
            observed.append(record)
            by_case[fid,t["case_id"],t["rotation"]]=record
        print("indexed",p.name,flush=True)
    comparisons=[]
    for r in independent:
        actual=by_case.get((r["fixture_id"],r["case_id"],r["rotation"]))
        if actual:
            comparisons.append(dict(fixture_id=r["fixture_id"],case_id=r["case_id"],rotation=r["rotation"],
                                    status="match" if matches(actual["projections"],r["projections"]) else "mismatch",
                                    mchprs=actual["artifact"],java=r["artifact"],
                                    scope="declared port projection at aligned response boundaries; preparation methods differ as recorded; not Java callback-order proof"))
    products={"trace-index.json":dict(schema_version=1,artifacts=index,comparisons=comparisons),
              "mchprs-projections.json":dict(schema_version=1,evidence="frozen MCHPRS regression observations, not an independent conformance oracle",cases=observed),
              "java-projections.json":dict(schema_version=1,evidence="independent SHA-pinned Java 1.21.5 port observations",cases=independent)}
    for name,data in products.items():
        content=json.dumps(data,indent=2)+"\n"
        if args.check:
            assert (source/name).read_text()==content,name
        if args.write:
            (source/name).write_text(content,newline="\n")
    print("comparisons",len(comparisons),"mismatches",[x for x in comparisons if x["status"]!="match"])


if __name__=="__main__":
    main()
