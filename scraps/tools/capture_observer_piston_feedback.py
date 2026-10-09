"""Capture observer/piston feedback on the official SHA-pinned Java 1.21.5 server.

Requires Java 21 and Python 3. Runs an isolated frozen void world. Example:
  py scraps/tools/capture_observer_piston_feedback.py --server-jar <server.jar> --output <new.json>
"""
import argparse
import hashlib
import json
from pathlib import Path
import subprocess
import tempfile
import time

from capture_piston_oscillator import Rcon, SERVER_SHA1


def capture_case(rcon, facing, dust, hold):
    rcon.request("fill 36 28 38 44 34 42 minecraft:air strict")
    for _ in range(8):
        rcon.step()
    rcon.request("setblock 40 30 40 minecraft:piston[facing=east,extended=false] strict")
    rcon.request(f"setblock 40 31 40 minecraft:observer[facing={facing},powered=false] strict")
    rcon.request("setblock 40 32 40 minecraft:stone strict")
    if dust:
        rcon.request("setblock 39 29 40 minecraft:stone strict")
        rcon.request("setblock 39 30 40 minecraft:redstone_wire")
    for _ in range(8):
        rcon.step()
    source = "38 30 40" if dust else "39 30 40"
    rcon.request(f"setblock {source} minecraft:redstone_block")
    for _ in range(hold):
        rcon.step()
    rcon.request(f"setblock {source} minecraft:air")

    def test(pos, state):
        return "Test passed" in rcon.request(f"execute if block {pos} minecraft:{state}")

    trace = []
    for tick in range(33):
        name = next(n for n in ["piston", "moving_piston", "air"] if test("40 30 40", n))
        extended = str(test("40 30 40", "piston[extended=true]")).lower() if name == "piston" else None
        powered = str(test("40 31 40", "observer[powered=true]")).lower()
        trace.append([name, extended, powered])
        rcon.step()
    return {"facing": facing, "dust": dust, "hold": hold, "trace": trace}


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

    with tempfile.TemporaryDirectory(prefix="mchprs-observer-feedback-") as directory:
        run = Path(directory)
        (run / "eula.txt").write_text("eula=true\n")
        (run / "server.properties").write_text(
            'server-ip=127.0.0.1\nserver-port=25583\nonline-mode=false\n'
            'enable-rcon=true\nrcon.port=25584\nrcon.password=piston-reference\n'
            'view-distance=2\nsimulation-distance=2\nspawn-protection=0\n'
            'pause-when-empty-seconds=0\nlevel-type=minecraft:flat\n'
            'generator-settings={"layers":[{"block":"minecraft:air","height":1}],"biome":"minecraft:plains"}\n'
        )
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
                rcon.request("forceload add 32 32 63 63")
                for _ in range(4):
                    rcon.step()
                cases = []
                for facing in ["down", "up"]:
                    for dust in [False, True]:
                        for hold in [1, 2, 4, 8]:
                            cases.append(capture_case(rcon, facing, dust, hold))
                            print(f"Captured observer={facing}, dust={dust}, hold={hold}", flush=True)
                result = {
                    "version": "1.21.5",
                    "server_sha1": SERVER_SHA1,
                    "setup": "Frozen void world; piston at (40,30,40) facing east, observer at (40,31,40), stone at (40,32,40), all placed strict. Dust cases place dust at (39,30,40) on stone at (39,29,40). Settle eight game ticks; place redstone block at (39,30,40) for direct input or (38,30,40) for dust input; wait hold game ticks; remove source; capture tick zero through tick 32. Observer facing down has its output dot upward.",
                    "columns": ["piston block name", "piston extended (null while moving)", "observer powered"],
                    "cases": cases,
                }
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
