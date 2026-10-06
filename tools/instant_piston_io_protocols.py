"""Reviewed ports and bounded lever-input episodes for the independent I/O revision.

py tools/instant_piston_io_protocols.py --write
No historical protocol, schematic or reference is changed.
"""
import argparse
import itertools
import json
from pathlib import Path
from inspect_instant_pistons import ROOT, inspect

PACK = ROOT/"test_data/instant-pistons-io"


def lever(pos, powered):
    return dict(op="lever", pos=pos, powered=bool(powered))


def wait(ticks=8, ready=False):
    return dict(op="wait_ready" if ready else "wait", ticks=ticks)


def case(name, actions=(), inputs=None, ticks=24, **kwargs):
    return dict(id=name, actions=list(actions), inputs=inputs, ticks=ticks, **kwargs)


def generate(path):
    info=inspect(path)
    name=path.stem
    fid=name.lower()
    cells={tuple(c["pos"]):c["state"] for c in info["nonair_cells"]}
    bases=[list(p) for p,s in cells.items() if s.split("[")[0] in ("minecraft:piston","minecraft:sticky_piston")]
    inputs={}; outputs={}; cases=[case("saved-idle")]; unknowns=[]
    roles={}; derived=[]
    classification="instant with ordinary-node input and consumer boundary"
    purpose="Versioned lever/repeater circuit; measure the actual consumer response."
    if name.startswith("INSTANT_"):
        control=next(list(p) for p,s in cells.items() if s.startswith("minecraft:lever"))
        sink=next(list(p) for p,s in cells.items() if s.startswith("minecraft:repeater"))
        base=bases[0]
        inputs={"trigger":control}
        outputs={"control":control,"input_torch":[control[0],control[1],2],
                 "base":base,"repeater":sink,"raw_output":[sink[0],sink[1],sink[2]-1],
                 "output_wire":[sink[0],sink[1],sink[2]+1]}
        if name=="INSTANT_CHAIN":outputs["downstream_base"]=bases[1]
        cases += [case("activate",[lever(control,True)],[1]),
                  case("held-active",[lever(control,True),wait(12),lever(control,True)],[1])]
        if name in ("INSTANT_TORCH","INSTANT_DOWN_TORCH_RESET"):
            cases += [case("reuse-after-quiescence",[lever(control,True),wait(32,True),lever(control,False),wait(32,True),lever(control,True)],[1,1])]
        unknowns += ["Quiet/periodic readiness is assessed from physical work; no universal reuse interval."]
    elif name in ("OR_1","AND_1","AND_2","AND_3","NOT_1","XOR_Simple","OR_Interpreter_illigal"):
        controls=sorted(list(p) for p,s in cells.items() if s.startswith("minecraft:lever"))
        # Actual signs swap the NOT/XOR labels compared with the symmetric examples.
        if name in ("NOT_1","XOR_Simple"):controls.reverse()
        inputs={"IN1":controls[0],"IN2":controls[1]}
        lamp=next(list(p) for p,s in cells.items() if s.startswith("minecraft:redstone_lamp"))
        sink=next(list(p) for p,s in cells.items() if s.startswith("minecraft:repeater"))
        outputs={"IN1":controls[0],"IN2":controls[1],"lamp":lamp,"repeater":sink,
                 "consumer_input":[sink[0],sink[1],sink[2]+1],"output_stage":bases[0]}
        for a,b,order in [(0,0,"12"),(1,0,"12"),(0,1,"12"),(1,1,"12"),(1,1,"21")]:
            vector={"1":a,"2":b}
            actions=[lever(inputs["IN"+key],True) for key in order if vector[key]]
            cases.append(case(f"events-{a}{b}-{order}",actions,[a,b]))
        if name=="NOT_1":roles={"IN1":"inhibit/right base", "IN2":"ordinary trigger/left base"}
        if name=="AND_3":roles={"IN1":"local power plus base update", "IN2":"remote QC power"}
        if name=="OR_Interpreter_illigal":classification="author-excluded reset counterexample; diagnostics only"
        purpose="Measure the full new output instant, repeater and lamp, including internal reset effects."
        unknowns += ["First-wave logical projection and complete consumer waveform must be measured separately."]
    elif name.startswith("ADDER_"):
        width=1 if name=="ADDER_1BIT" else 11
        if width==1:
            inputs={"A":[1,6,3],"B":[1,6,1],"Cin":[8,3,6],"trigger":[0,5,5]}
            outputs={"sum_payload":[18,2,1],"sum_repeater":[19,2,1],"carry_repeater":[11,1,0]}
            vectors=list(itertools.product(range(2),repeat=3))
        else:
            inputs={"A":[[4,5,43-4*i] for i in range(11)],"B":[[4,5,41-4*i] for i in range(11)],"trigger":[2,5,45]}
            outputs={"sum_payload":[[20,2,41-4*i] for i in range(11)],"sum_repeater":[[21,2,41-4*i] for i in range(11)]}
            vectors=[(a,b,0) for a,b in [(0,0),(1,0),(0,1),(31,1),(1023,1),(1024,1024),(2047,0),(0,2047),(2047,1),(2047,2047),(0x555,0x2aa),(0x2aa,0x555)]]
            vectors += [(1<<i,0,0) for i in range(11)]+[(0,1<<i,0) for i in range(11)]
            seed=0x1a57
            for _ in range(8):
                seed=(1664525*seed+1013904223)&0xffffffff;a=seed&2047
                seed=(1664525*seed+1013904223)&0xffffffff
                vectors.append((a,seed&2047,0))
        for a,b,ci in dict.fromkeys(vectors):
            actions=[]
            for key,value in [("A",a),("B",b)]+([("Cin",ci)] if width==1 else []):
                bank=[inputs[key]] if width==1 else inputs[key]
                actions.extend(lever(p,not ((value>>i)&1)) for i,p in enumerate(bank))
            actions += [wait(32,True),lever(inputs["trigger"],False)]
            cases.append(case(f"prepared-{a}-{b}-{ci}",actions,[a,b,ci],expectation=dict(sum=(a+b+ci)&((1<<width)-1),carry=(a+b+ci)>>width)))
        roles={"data":"lever powered means zero; unpowered means one", "trigger":"saved powered; falling lever power launches wave", "bit_order":"LSB largest Z, spacing -4"}
        purpose=f"Prepared {width}-bit arithmetic with real lever inputs and distinct repeater outputs."
        unknowns += ["External repeated-calculation protocol not established by fresh cases."]
    elif name=="COUNTER_BASIC":
        inputs={"trigger":[7,8,0]}
        outputs={"control":[7,8,0],"memory":[[5,11,4+2*i] for i in range(16)],
                 "output_memory":[[17,3,4+2*i] for i in range(16)],
                 "repeater":[[18,1,4+2*i] for i in range(16)],"generator":[2,11,19]}
        cases += [case("release",[lever(inputs["trigger"],True)],[1],ticks=96)]
        classification="stateful free-running counter with explicit consumer bank"
        purpose="Compare stored count with the newly attached output-memory/repeater bank."
        roles={"trigger":"off-to-on lever, torch negation", "memory":"retracted=one, extended=zero; LSB increasing Z"}
        unknowns += ["Clear, restart, high carry and full wrap are outside this bounded protocol."]
    else:
        classification="BUD memory with independent data/update paths"
        observer=name=="BUD_InstantMemoryCellObserverUpdate"
        piston=name=="BUD_PistonUpdate"
        if observer:
            inputs={"data":[1,6,3],"update":[1,4,0]}
            outputs={"memory":[0,3,8],"repeater":[0,1,9],"QC_data_wire":[0,6,8],
                     "update_wire":[0,3,7],"data_instant":[0,6,4],"update_piston":[1,3,5],"update_observer":[1,3,6]}
        elif piston:
            inputs={"data":[0,7,1],"update":[1,3,0]}
            outputs={"memory":[0,3,4],"repeater":[0,1,5],"QC_data_wire":[0,6,4],"update_piston":[1,3,3]}
        else:
            inputs={"data":[0,7,0],"update":[1,4,0] if name=="BUD_NonInstantInputs" else [1,3,-1]}
            outputs={"memory":[0,3,3],"repeater":[0,1,4],"QC_data_wire":[0,6,3]}
            if name=="BUD_NonInstantInputs":outputs["update_wire"]=[0,3,2]
            else:
                outputs["update_piston"]=[1,3,2]
                derived=[dict(op="set_notified",pos=[1,3,-1],state="lever",properties={"face":"wall","facing":"north","powered":"false"})]
                outputs["derived_update_control"]=[1,3,-1]
                unknowns.append("Update lever is absent in this downloaded selection; source adapter is a derived diagnostic, not original acceptance.")
        data_one=not observer
        update_active=not observer
        prepare=derived+[wait(32,True)] if derived else []
        cases += [case("data-only",prepare+[lever(inputs["data"],data_one)],[1,0])]
        for d in (0,1):
            actions=prepare+[lever(inputs["data"],data_one if d else not data_one),wait(32,True),lever(inputs["update"],update_active)]
            cases.append(case(f"sample-{d}",actions,[d,1],ticks=36))
        if name in ("BUD_NonInstantInputs","BUD_PistonUpdate"):
            actions=[lever(inputs["data"],True),wait(32,True),lever(inputs["update"],True),wait(32,True),lever(inputs["data"],False),wait(32,True),lever(inputs["update"],False)]
            cases.append(case("store-hold-resample",actions,[1,0],ticks=24))
            cases.append(case("update-before-data",[lever(inputs["update"],True),wait(32,True),lever(inputs["data"],True)],[1,0]))
        roles={"data":"logical stored one uses lever "+str(data_one).lower(),
               "update":"sampling activation uses lever "+str(update_active).lower(),
               "memory":"stationary retracted=one, extended=zero; moving invalid"}
        purpose="Demonstrate sampling, retained state and the actual output consumer, not a combinational truth table."
        unknowns += ["Instant-update generators require their own recurring-response protocol; unrestricted data changes during reset are not assumed."]
    origin=[40,30,40]
    result=dict(schema_version=1,id=fid,fixture=path.relative_to(ROOT).as_posix(),sha256=info["sha256"],dimensions=info["dimensions"],
                inspection=(PACK/"inspection"/(fid+".json")).relative_to(ROOT).as_posix(),classification=classification,purpose=purpose,
                coordinates=dict(convention="selection-local relative to saved minimum",origin=origin,loader_offset=info["loader_offset"],
                                 paste_anchor=[a+b for a,b in zip(origin,info["loader_offset"])],orientation="saved; declared horizontal rotations transform geometry, states and lever attachments"),
                ports=dict(inputs=inputs,observations=outputs,roles=roles),
                setup="Strict paste preserves all saved states/entities, empty interpreter work. Lever actions follow both interaction::on_use notification paths. Derived additions are explicitly listed in cases.",
                protocol="Fresh imported snapshots for independent cases. Prepared data remains stable through the response. External changes during instant reset are undefined; internal reset/clock work is retained. BUD store-hold-resample uses measured quiescent endpoints.",
                cases=cases,rotations=[0,90,180,270] if name in ("INSTANT_OBSERVER","BUD_NonInstantInputs","BUD_PistonUpdate","NOT_1") else [0],
                mapping_status="Reviewed sign/geometry candidates; controlled episode evidence is recorded separately.",
                expected_status="Arithmetic expectations require the declared polarity/width decoder; other functional claims remain measured projections.",
                unknowns=unknowns)
    dest=PACK/"fixtures"/(fid+".json")
    if dest.exists():
        existing=json.loads(dest.read_text())
        if "evidence" in existing:result["evidence"]=existing["evidence"]
    return dest,result


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--write",action="store_true")
    parser.add_argument("--check",action="store_true")
    args=parser.parse_args()
    (PACK/"fixtures").mkdir(exist_ok=True)
    for path in sorted(PACK.glob("*.schem")):
        dest,data=generate(path)
        content=json.dumps(data,indent=2)+"\n"
        if args.check:assert dest.read_text()==content,dest
        if args.write:dest.write_text(content,encoding="utf-8",newline="\n")
        print(data["id"],len(data["cases"]),"episodes")


if __name__=="__main__":main()
