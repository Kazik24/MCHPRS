"""Independent Java 1.21.5 pack captures in a fresh frozen void world.

py tools/capture_instant_pistons_java.py --server-jar <pinned.jar> --output-dir <new-dir>
Each case is placed in a new chunk region; stale work cannot hit the next case.
Java preparation uses normal setblock, explicitly distinct from MCHPRS raw+notify.
No normal placement is substituted for strict-import counterexamples.
"""
import argparse
import gzip
import hashlib
import json
from pathlib import Path
import re
import subprocess
import tempfile
import time
from capture_piston_oscillator import Rcon, SERVER_SHA1
from inspect_instant_pistons import ROOT, PACK, inspect


def snbt(v):
    if isinstance(v, dict):
        return "{" + ",".join(json.dumps(k) + ":" + snbt(x) for k, x in v.items()) + "}"
    if isinstance(v, list):
        return "[" + ",".join(map(snbt, v)) + "]"
    return json.dumps(v, ensure_ascii=True)


def flat_ports(v):
    if isinstance(v, dict):
        return sum((flat_ports(x) for x in v.values()), [])
    if isinstance(v, list) and v and isinstance(v[0], int):
        return [v]
    return sum((flat_ports(x) for x in v), [])


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--server-jar", type=Path, required=True)
    parser.add_argument("--output-dir", type=Path, required=True)
    parser.add_argument("--fixture")
    parser.add_argument("--java", default="java")
    args = parser.parse_args()
    if args.output_dir.exists():
        parser.error("refusing existing output directory")
    jar = args.server_jar.resolve()
    if hashlib.sha1(jar.read_bytes()).hexdigest() != SERVER_SHA1:
        parser.error("Java server SHA1 mismatch")
    server_sha256 = hashlib.sha256(jar.read_bytes()).hexdigest()
    manifests = [json.loads(p.read_text()) for p in sorted((PACK/"fixtures").glob("*.json"))]
    episodes = []
    for m in manifests:
        if args.fixture and m["id"] != args.fixture:
            continue
        if m["id"] in ("pm1_sort", "q2ck_lycore5_for_sorting"):
            continue
        for c in m["cases"]:
            if c["id"] in ("held-zero", "rearm-diagnostic", "misaligned-ab"):
                continue  # Full fine traces remain MCHPRS evidence for these diagnostics.
            if m["id"] == "adder_11bits" and c["id"] not in ("saved-idle", "prepared-0-0-0", "prepared-1023-1-0", "prepared-1024-1024-0", "prepared-2047-2047-0", "prepared-1365-682-0", "prepared-682-1365-0"):
                continue
            episodes.append((m, c))
    with tempfile.TemporaryDirectory(prefix="mchprs-instant-pack-") as directory:
        run = Path(directory)
        (run/"eula.txt").write_text("eula=true\n")
        (run/"server.properties").write_text(
            'server-ip=127.0.0.1\nserver-port=25583\nonline-mode=false\nenable-rcon=true\n'
            'rcon.port=25584\nrcon.password=piston-reference\nview-distance=2\nsimulation-distance=2\n'
            'spawn-protection=0\npause-when-empty-seconds=0\nlevel-type=minecraft:flat\n'
            'generator-settings={"layers":[{"block":"minecraft:air","height":1}],"biome":"minecraft:plains"}\n'
        )
        functions = run/"world/datapacks/piston_reference/data/piston_reference/function"
        functions.mkdir(parents=True)
        (functions.parents[2]/"pack.mcmeta").write_text(json.dumps({"pack": {"pack_format": 71, "description": "Versioned instant fixture evidence"}}))
        specs = []
        for index, (m, c) in enumerate(episodes):
            info = inspect(ROOT/m["fixture"])
            if info["sha256"] != m["sha256"]:
                raise ValueError("stale manifest")
            # New, never previously occupied chunk region per independent snapshot.
            origin = [128 + index*64, 40, 128]
            def absolute(p):
                return " ".join(str(a+b) for a, b in zip(origin, p))
            commands = [f"setblock {absolute(b['pos'])} {b['state']} strict" for b in info["nonair_cells"]]
            for e in info["block_entities"]:
                d = e.get("Data", e).copy()
                for k in ("Id", "id", "Pos", "x", "y", "z"):
                    d.pop(k, None)
                commands.append(f"data merge block {absolute(e['Pos'])} {snbt(d)}")
            (functions/f"place_{index}.mcfunction").write_text("\n".join(commands)+"\n")
            relevant = {tuple(b["pos"]) for b in info["nonair_cells"] if any(x in b["state"].split("[")[0] for x in ("piston", "observer", "redstone_wire", "torch", "repeater", "comparator", "lamp"))}
            if m["id"] == "adder_11bits":
                relevant = {p for p in relevant if p[2] >= 37}  # detailed first two stages plus every output
            if m["id"] == "counter_basic":
                relevant = {p for p in relevant if p[2] <= 7 or p == (2,11,18)}
            relevant.update(map(tuple, flat_ports(m["ports"]["observations"])))
            # Observe heads and payload destinations, including initially empty cells.
            delta = {"north": [0,0,-1], "south": [0,0,1], "east": [1,0,0], "west": [-1,0,0], "up": [0,1,0], "down": [0,-1,0]}
            for b in info["nonair_cells"]:
                if b["state"].split("[")[0] in ("minecraft:piston", "minecraft:sticky_piston"):
                    if tuple(b["pos"]) not in relevant:
                        continue
                    facing = re.search(r"facing=(\w+)", b["state"])[1]
                    for n in (1,2):
                        relevant.add(tuple(a+n*v for a,v in zip(b["pos"],delta[facing])))
            positions = sorted(relevant)
            commands = ["data modify storage piston_reference:trace sample set value {}"]
            for i, p in enumerate(positions):
                coord = absolute(p)
                target = f"storage piston_reference:trace sample.g{i//16}.c{i}"
                states = sorted({b["state"].split("[")[0].removeprefix("minecraft:") for b in info["nonair_cells"]} | {"air", "redstone_block", "moving_piston", "piston_head", "stone", "sandstone", "target"})
                states += [f"{b}[extended={v}]" for b in ("piston", "sticky_piston") for v in ("false", "true")]
                states += [f"observer[powered={v}]" for v in ("false", "true")]
                states += [f"redstone_wire[power={v}]" for v in range(16)]
                states += [f"{b}[{prop}={v}]" for b,prop in (("redstone_torch","lit"),("redstone_wall_torch","lit"),("repeater","powered"),("comparator","powered"),("redstone_lamp","lit")) for v in ("false","true")]
                for state in states:
                    commands.append(f"execute if block {coord} minecraft:{state} run data modify {target} set value {json.dumps(state)}")
                for face in ("north", "south", "east", "west"):
                    for side in ("none", "side", "up"):
                        commands.append(f"execute if block {coord} minecraft:redstone_wire[{face}={side}] run data modify {target}_{face} set value {json.dumps(side)}")
                commands.append(f"execute if block {coord} minecraft:moving_piston run data modify {target}_progress set from block {coord} progress")
            (functions/f"observe_{index}.mcfunction").write_text("\n".join(commands)+"\n")
            specs.append((m,c,origin,positions))
        logpath = run/"server.log"
        with logpath.open("w") as log:
            server = subprocess.Popen([args.java,"-Xmx4G","-Xms256M","-jar",str(jar),"nogui"],cwd=run,stdout=log,stderr=subprocess.STDOUT,creationflags=getattr(subprocess,"CREATE_NO_WINDOW",0))
            rcon = None
            try:
                deadline = time.monotonic()+120
                while "Done (" not in logpath.read_text(errors="replace"):
                    if server.poll() is not None:
                        raise RuntimeError(logpath.read_text(errors="replace"))
                    if time.monotonic()>deadline:
                        raise TimeoutError("startup")
                    time.sleep(.25)
                rcon=Rcon()
                rcon.request("tick freeze")
                args.output_dir.mkdir(parents=True)
                for index,(m,c,origin,positions) in enumerate(specs):
                    dims=m["dimensions"]
                    commands=[]
                    def send(cmd):
                        result=rcon.request(cmd)
                        commands.append({"command":cmd,"response":result})
                        return result
                    def absolute(p):
                        return " ".join(str(a+b) for a,b in zip(origin,p))
                    send(f"forceload add {origin[0]-16} 112 {origin[0]+48} 176")
                    for _ in range(4):rcon.step()
                    response=send(f"function piston_reference:place_{index}")
                    if "Running function" not in response:
                        raise RuntimeError(response)
                    samples=[]
                    def sample(label,action,tick):
                        rcon.request(f"function piston_reference:observe_{index}")
                        # RCON splits large responses; each bounded group fits one packet.
                        raw=[rcon.request(f"data get storage piston_reference:trace sample.g{g}") for g in range((len(positions)+15)//16)]
                        joined=" ".join(raw)
                        cells={int(k):v for k,v in re.findall(r'c(\d+): "([^"]+)"',joined)}
                        if len(cells)!=len(positions):
                            missing=[i for i in range(len(positions)) if i not in cells]
                            raise RuntimeError(f"unmapped observed cells {missing}: {joined[:500]}")
                        sides={k:v for k,v in re.findall(r'(c\d+_(?:north|south|east|west)): "([^"]+)"',joined)}
                        progress={k:float(v) for k,v in re.findall(r'(c\d+_progress): ([0-9.]+)f',joined)}
                        samples.append(dict(label=label,action=action,tick=tick,phase="completed boundary" if label=="step" else "frozen command boundary",cells=cells,wire_sides=sides,moving_progress=progress,raw_snbt=raw))
                    sample("initial",0,0)
                    tick=0
                    ai=0
                    for ai,op in enumerate(c["actions"],1):
                        kind=op["op"]
                        if kind in ("wait","wait_ready"):
                            # Explicit Java construction semantics; independent ready interval.
                            count=8 if kind=="wait_ready" else op["ticks"]
                            for _ in range(count):
                                rcon.step();tick+=1;sample("step",ai,tick)
                        elif kind=="notify":
                            # Java normal setblock already delivered notifications at preparation.
                            commands.append({"MCHPRS_only_notification":op["pos"],"Java":"covered by preceding normal setblock"})
                        else:
                            state="air" if kind=="destroy" else op["state"]
                            send(f"setblock {absolute(op['pos'])} minecraft:{state}")
                        sample("after-action",ai,tick)
                    start=tick
                    for _ in range(c["ticks"]):
                        rcon.step();tick+=1;sample("step",ai,tick)
                    result=dict(schema_version=1,engine="Java",engine_version="1.21.5",server_sha1=SERVER_SHA1,server_sha256=server_sha256,
                                fixture=m["fixture"],fixture_sha256=m["sha256"],case_id=c["id"],inputs=c["inputs"],origin=origin,rotation=0,
                                coordinate_convention=m["coordinates"]["convention"],positions_local=positions,
                                setup="isolated frozen void world; strict setblock saved states, data merge saved entities; unique region for every case; 4 chunk-load ticks before import; prepared data normal setblock then 8 measured game ticks",
                                protocol=m["protocol"],ordered_stimuli=c["actions"],commands=commands,samples=samples,start_tick=start,
                                observation_projection="named ports and listed piston/head/payload/observer/dust/torch/consumer cells; dust strength; queued work and internal callback order unavailable in command-only Java capture",
                                limits={"ticks":c["ticks"],"startup_seconds":120},termination="completed bounded response; no claim of settled state",
                                capture_command=subprocess.list2cmdline(["py","tools/capture_instant_pistons_java.py","--server-jar",str(jar),"--output-dir",str(args.output_dir)]+(["--fixture",args.fixture]if args.fixture else [])))
                    p=args.output_dir/f"java-{m['id']}-{c['id']}-r0.json.gz"
                    p.write_bytes(gzip.compress((json.dumps(result,sort_keys=True,separators=(",",":"))+"\n").encode(),mtime=0))
                    print("captured",p.name,flush=True)
                    send(f"forceload remove {origin[0]-16} 112 {origin[0]+48} 176")
                (args.output_dir/"server.log").write_bytes(logpath.read_bytes())
            finally:
                if rcon:
                    try:rcon.request("stop")
                    except (OSError,ConnectionError):server.terminate()
                else:server.terminate()
                try:server.wait(timeout=30)
                except subprocess.TimeoutExpired:server.kill();server.wait(timeout=10)
                if rcon:rcon.socket.close()


if __name__=="__main__":
    main()
