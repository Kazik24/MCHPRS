"""Capture ADDER_11BITS.schem on the SHA-pinned Java 1.21.5 server.

Requires Java 21, Python 3, and prismarine-nbt in tools/node_modules. Runs an
isolated frozen void world. Example:
  py scraps/tools/capture_adder.py --server-jar <server.jar> --output-directory <new-dir>
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import subprocess
import tempfile
import time

from capture_piston_oscillator import Rcon, SERVER_SHA1

ROOT = Path(__file__).resolve().parents[2]
ORIGIN = [100, 26, 56]
PORTS = {"a": [3, 5, 43], "b": [3, 5, 41], "out": [20, 2, 41], "tick": [2, 4, 45]}
WIDTH = 11
DECODE = r"""
const fs = require('fs'), nbt = require('prismarine-nbt');
(async () => {
  const {parsed} = await nbt.parse(fs.readFileSync(process.argv[1]));
  const raw = nbt.simplify(parsed), s = raw.Schematic || raw;
  if (s.Version !== 2 || s.Width !== 21 || s.Height !== 7 || s.Length !== 46 || s.BlockEntities.length !== 6) throw Error('Unexpected adder format');
  const palette = Object.fromEntries(Object.entries(s.Palette).map(([name, id]) => [id, name]));
  const data = Buffer.from(s.BlockData), blocks = [];
  let i = 0, entry = 0;
  while (i < data.length) {
    let id = 0, shift = 0, b;
    do { b = data[i++]; id |= (b & 127) << shift; shift += 7; } while (b & 128);
    blocks.push({pos: [entry % s.Width, Math.floor(entry / (s.Width * s.Length)), Math.floor(entry / s.Width) % s.Length], state: palette[id]});
    entry++;
  }
  if (entry !== 21 * 7 * 46) throw Error('Unexpected block count');
  console.log(JSON.stringify(blocks));
})().catch(e => { console.error(e); process.exit(1); });
"""


def absolute(port, bit=0):
    return [ORIGIN[0] + port[0], ORIGIN[1] + port[1], ORIGIN[2] + port[2] - 4 * bit]


def coordinates(pos):
    return " ".join(map(str, pos))


def capture(rcon, inputs):
    response = rcon.request("function piston_reference:fixture")
    if "Running function" not in response:
        raise RuntimeError(response)
    if inputs is not None:
        for key, value in zip(["a", "b"], inputs):
            for bit in range(WIDTH):
                state = "stone" if value & (1 << bit) else "redstone_block"
                rcon.request(f"setblock {coordinates(absolute(PORTS[key], bit))} minecraft:{state}")
        for _ in range(8):
            rcon.step()
    stimulus = f"setblock {coordinates(absolute(PORTS['tick']))} minecraft:air"
    rcon.request(stimulus)
    output = []
    for _ in range(12):
        rcon.step()
        value = 0
        moving = False
        for bit in range(WIDTH):
            pos = coordinates(absolute(PORTS["out"], bit))
            def test(state):
                return "Test passed" in rcon.request(f"execute if block {pos} minecraft:{state}")
            if test("air"):
                value |= 1 << bit
            elif test("moving_piston"):
                moving = True
            elif not test("redstone_block"):
                raise RuntimeError(f"Unexpected output block at {pos}")
        output.append(None if moving else value)
    print(f"Inputs {inputs or [2047, 2047]}: {output}", flush=True)
    return {"stimulus": stimulus, "adder_output": output}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--server-jar", type=Path, required=True)
    parser.add_argument("--output-directory", type=Path, required=True)
    parser.add_argument("--java", default="java")
    args = parser.parse_args()
    names = ["java-adder-traces.json", "java-adder-inputs.json"]
    if any((args.output_directory / name).exists() for name in names):
        parser.error("Refusing to overwrite an existing reference")
    jar = args.server_jar.resolve()
    if hashlib.sha1(jar.read_bytes()).hexdigest() != SERVER_SHA1:
        parser.error("Unexpected Java server binary")
    schematic = ROOT / "test_data/instant-pistons/ADDER_11BITS.schem"
    env = os.environ.copy()
    env["NODE_PATH"] = str(ROOT / "tools/node_modules")
    blocks = json.loads(subprocess.check_output(["node", "-e", DECODE, str(schematic)], env=env))

    with tempfile.TemporaryDirectory(prefix="mchprs-adder-") as directory:
        run = Path(directory)
        (run / "eula.txt").write_text("eula=true\n")
        (run / "server.properties").write_text(
            'server-ip=127.0.0.1\nserver-port=25583\nonline-mode=false\n'
            'enable-rcon=true\nrcon.port=25584\nrcon.password=piston-reference\n'
            'view-distance=2\nsimulation-distance=2\nspawn-protection=0\n'
            'pause-when-empty-seconds=0\nlevel-type=minecraft:flat\n'
            'generator-settings={"layers":[{"block":"minecraft:air","height":1}],"biome":"minecraft:plains"}\n'
        )
        pack = run / "world/datapacks/piston_reference"
        functions = pack / "data/piston_reference/function"
        functions.mkdir(parents=True)
        (pack / "pack.mcmeta").write_text(json.dumps({"pack": {"pack_format": 71, "description": "11-bit adder reference"}}))
        commands = ["fill 98 24 54 122 34 104 minecraft:air strict"]
        commands += [f"setblock {coordinates([o + p for o, p in zip(ORIGIN, b['pos'])])} {b['state']} strict" for b in blocks if b["state"] != "minecraft:air"]
        (functions / "fixture.mcfunction").write_text("\n".join(commands) + "\n")
        with (run / "server.log").open("w") as log:
            server = subprocess.Popen(
                [args.java, "-Xmx1G", "-Xms256M", "-jar", str(jar), "nogui"],
                cwd=run, stdout=log, stderr=subprocess.STDOUT,
                creationflags=getattr(subprocess, "CREATE_NO_WINDOW", 0),
            )
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
                rcon.request("forceload add 96 48 127 111")
                for _ in range(4):
                    rcon.step()
                for name, inputs in zip(names, [None, [0x555, 0x2aa]]):
                    # Drain work from the previous circuit before placing a new one.
                    rcon.request("fill 98 24 54 122 34 104 minecraft:air strict")
                    for _ in range(8):
                        rcon.step()
                    fixture = capture(rcon, inputs)
                    fixture["sha256"] = hashlib.sha256(schematic.read_bytes()).hexdigest()
                    expected = sum(inputs or [2047, 2047]) & ((1 << WIDTH) - 1)
                    if fixture["adder_output"][0] != expected:
                        raise RuntimeError(f"Adder did not compute {expected}: {fixture}")
                    result = {
                        "version": "1.21.5", "server_sha1": SERVER_SHA1,
                        "setup": "Strict fixture paste into frozen void world; changed inputs use normal setblock followed by eight settling game ticks; remove TICK source; observe after each completed game tick.",
                        "origin": ORIGIN, "ports": PORTS, "bit_width": WIDTH,
                        "bit_order": "Least significant bit first; subtract four from Z for each subsequent bit; intended arithmetic result is sum modulo 2048.",
                        "adder_inputs": inputs or [2047, 2047],
                        "fixtures": {"ADDER_11BITS.schem": fixture},
                    }
                    args.output_directory.mkdir(parents=True, exist_ok=True)
                    (args.output_directory / name).write_text(json.dumps(result, indent=2) + "\n")
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
