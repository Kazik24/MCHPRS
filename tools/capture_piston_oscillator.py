"""Capture EDGECASE_PISTION.schem on the official SHA-pinned Java 1.21.5 server.

Requires Java 21, Python 3, and prismarine-nbt in tools/node_modules. Runs an
isolated frozen void world; never opens the live MCHPRS world. Example:
  py tools/capture_piston_oscillator.py --server-jar <server.jar> --output <new.json>
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import socket
import struct
import subprocess
import tempfile
import time

ROOT = Path(__file__).resolve().parents[1]
SERVER_SHA1 = "e6ec2f64e6080b9b5d9b471b291c33cc7f509733"
DECODE = r"""
const fs = require('fs'), nbt = require('prismarine-nbt');
(async () => {
  const {parsed} = await nbt.parse(fs.readFileSync(process.argv[1]));
  const raw = nbt.simplify(parsed), s = raw.Schematic || raw;
  const palette = Object.fromEntries(Object.entries((s.Blocks || s).Palette).map(([name, id]) => [id, name]));
  const data = Buffer.from(s.Version === 3 ? s.Blocks.Data : s.BlockData);
  const blocks = [];
  let i = 0, entry = 0;
  while (i < data.length) {
    let id = 0, shift = 0, b;
    do { b = data[i++]; id |= (b & 127) << shift; shift += 7; } while (b & 128);
    const x = entry % s.Width, z = Math.floor(entry / s.Width) % s.Length;
    const y = Math.floor(entry / (s.Width * s.Length));
    blocks.push({pos: [40 + x, 30 + y, 40 + z], state: palette[id]});
    entry++;
  }
  if (s.Width !== 1 || s.Height !== 4 || s.Length !== 9 || entry !== 36) throw Error('Unexpected fixture dimensions');
  console.log(JSON.stringify(blocks));
})().catch(e => { console.error(e); process.exit(1); });
"""


class Rcon:
    def __init__(self):
        self.socket = socket.create_connection(("127.0.0.1", 25584), timeout=5)
        self.seq = 0
        self.request("piston-reference", 3)

    def read(self, size):
        data = b""
        while len(data) < size:
            chunk = self.socket.recv(size - len(data))
            if not chunk:
                raise ConnectionError("RCON closed")
            data += chunk
        return data

    def request(self, command, kind=2):
        self.seq += 1
        payload = struct.pack("<ii", self.seq, kind) + command.encode() + b"\0\0"
        self.socket.sendall(struct.pack("<i", len(payload)) + payload)
        while True:
            answer = self.read(struct.unpack("<i", self.read(4))[0])
            rid, rkind = struct.unpack("<ii", answer[:8])
            if rid == -1:
                raise PermissionError("RCON authentication failed")
            if rid == self.seq and (kind != 3 or rkind == 2):
                return answer[8:-2].decode()

    def step(self):
        before = self.request("time query gametime")
        self.request("tick step 1")
        deadline = time.monotonic() + 5
        while self.request("time query gametime") == before:
            if time.monotonic() > deadline:
                raise TimeoutError("Java tick step")
            time.sleep(.02)
        # A time query can finish before the moving-entity phase of the tick.
        time.sleep(.06)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--server-jar", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--java", default="java")
    args = parser.parse_args()
    if args.output.exists():
        parser.error("Refusing to overwrite an existing reference")
    jar = args.server_jar.resolve()
    if hashlib.sha1(jar.read_bytes()).hexdigest() != SERVER_SHA1:
        parser.error("Unexpected Java server binary")
    schematic = ROOT / "test_data/EDGECASE_PISTION.schem"
    env = os.environ.copy()
    env["NODE_PATH"] = str(ROOT / "tools/node_modules")
    blocks = json.loads(subprocess.check_output(["node", "-e", DECODE, str(schematic)], env=env))
    with tempfile.TemporaryDirectory(prefix="mchprs-piston-oscillator-") as directory:
        run = Path(directory)
        (run / "eula.txt").write_text("eula=true\n")
        (run / "server.properties").write_text(
            'server-ip=127.0.0.1\nserver-port=25583\nonline-mode=false\n'
            'enable-rcon=true\nrcon.port=25584\nrcon.password=piston-reference\n'
            'view-distance=2\nsimulation-distance=2\nspawn-protection=0\n'
            'level-type=minecraft:flat\n'
            'generator-settings={"layers":[{"block":"minecraft:air","height":1}],"biome":"minecraft:plains"}\n'
        )
        pack = run / "world/datapacks/piston_reference"
        functions = pack / "data/piston_reference/function"
        functions.mkdir(parents=True)
        (pack / "pack.mcmeta").write_text(json.dumps({"pack": {"pack_format": 71, "description": "Piston edge-case reference"}}))
        commands = ["fill 38 28 38 42 35 50 minecraft:air strict"]
        commands += ["setblock " + " ".join(map(str, b["pos"])) + " " + b["state"] + " strict" for b in blocks if b["state"] != "minecraft:air"]
        (functions / "fixture.mcfunction").write_text("\n".join(commands) + "\n")
        with (run / "server.log").open("w") as log:
            server = subprocess.Popen([args.java, "-Xmx1G", "-Xms256M", "-jar", str(jar), "nogui"], cwd=run, stdout=log, stderr=subprocess.STDOUT, creationflags=getattr(subprocess, "CREATE_NO_WINDOW", 0))
            rcon = None
            try:
                deadline = time.monotonic() + 90
                while "Done (" not in (run / "server.log").read_text(errors="replace"):
                    if server.poll() is not None:
                        raise RuntimeError((run / "server.log").read_text(errors="replace"))
                    if time.monotonic() > deadline:
                        raise TimeoutError("Java startup")
                    time.sleep(.25)
                rcon = Rcon()
                rcon.request("tick freeze")
                rcon.request("forceload add 32 32 63 63")
                for _ in range(4):
                    rcon.step()
                response = rcon.request("function piston_reference:fixture")
                if "Running function" not in response:
                    raise RuntimeError(response)
                for _ in range(8):
                    rcon.step()
                rcon.request("setblock 40 31 40 minecraft:air")

                def test(pos, state):
                    return "Test passed" in rcon.request("execute if block " + " ".join(map(str, pos)) + " minecraft:" + state)

                trace = []
                for tick in range(33):
                    sample = [next(name for name in ["sticky_piston[extended=true]", "sticky_piston[extended=false]", "moving_piston"] if test([40, 31, z], name)) for z in [42, 46]]
                    sample.extend(test([40, 32, z], "observer[powered=true]") for z in [42, 46])
                    for z in [41, 45]:
                        pos = [40, 31, z]
                        power = next(i for i in range(16) if test(pos, f"redstone_wire[power={i}]"))
                        sides = [next(v for v in ["none", "side", "up"] if test(pos, f"redstone_wire[{side}={v}]")) for side in ["north", "south", "east", "west"]]
                        sample.append([power, *sides])
                    trace.append(sample)
                    print(tick, sample, flush=True)
                    rcon.step()
                result = {"version": "1.21.5", "server_sha1": SERVER_SHA1, "schematic_sha256": hashlib.sha256(schematic.read_bytes()).hexdigest(), "origin": [40, 30, 40], "stimulus": "setblock 40 31 40 minecraft:air", "setup": "Strict paste in frozen void world; settle eight game ticks; remove external source; sample at game-tick boundaries, including tick zero.", "columns": ["first_piston", "second_piston", "first_observer_powered", "second_observer_powered", "first_wire [power,north,south,east,west]", "second_wire [power,north,south,east,west]"], "trace": trace}
                args.output.parent.mkdir(parents=True, exist_ok=True)
                args.output.write_text(json.dumps(result, indent=2) + "\n")
            finally:
                try:
                    if rcon is not None:
                        rcon.request("stop")
                    else:
                        server.terminate()
                except (OSError, ConnectionError):
                    server.terminate()
                try:
                    server.wait(timeout=30)
                except subprocess.TimeoutExpired:
                    server.kill()
                    server.wait(timeout=10)
                if rcon is not None:
                    rcon.socket.close()


if __name__ == "__main__":
    main()
