"""Capture bulb edges on the SHA-pinned Java 1.21.5 server in an isolated world."""
import argparse
import hashlib
import json
from pathlib import Path
import re
import subprocess
import tempfile
import time

from capture_piston_oscillator import Rcon, SERVER_SHA1

VARIANTS = [prefix + "copper_bulb" for prefix in ["", "exposed_", "weathered_", "oxidized_", "waxed_", "waxed_exposed_", "waxed_weathered_", "waxed_oxidized_"]]


def capture(rcon, variant, lit, powered):
    def command(text):
        return rcon.request(text)

    def test(pos, state):
        return "Test passed" in command(f"execute if block {pos} minecraft:{state}")

    def strength(pos):
        response = command(f"data get block {pos} OutputSignal")
        match = re.search(r"(-?\d+)$", response)
        assert match, response
        return int(match[1])

    command("fill 36 28 36 46 34 46 minecraft:air strict")
    for _ in range(4):
        rcon.step()
    command(f"setblock 40 30 40 minecraft:{variant}[lit={str(lit).lower()},powered={str(powered).lower()}] strict")
    for pos in ["40 29 39", "40 29 41", "42 29 40", "41 30 40"]:
        command(f"setblock {pos} minecraft:stone strict")
    command("setblock 40 30 39 minecraft:lever[face=floor,facing=north,powered=false] strict")
    for pos, facing in [("40 30 41", "north"), ("42 30 40", "west")]:
        command(f"setblock {pos} minecraft:comparator[facing={facing},mode=compare,powered=false] strict")
    for pos in ["40 30 42", "43 30 40"]:
        command(f"setblock {pos} minecraft:redstone_lamp[lit=false] strict")
    command("setblock 39 30 40 minecraft:observer[facing=east,powered=false] strict")
    trace = []
    for input_power, ticks in [(True, 0), (True, 1), (True, 1), (True, 2), (False, 0), (False, 2), (False, 2), (True, 0), (False, 0), (True, 0), (True, 2), (True, 2), (False, 0), (False, 4)]:
        command(f"setblock 40 30 39 minecraft:lever[face=floor,facing=north,powered={str(input_power).lower()}]")
        for _ in range(ticks):
            rcon.step()
        state = [test("40 30 40", f"{variant}[lit=true]"), test("40 30 40", f"{variant}[powered=true]"), strength("40 30 41"), strength("42 30 40"), test("39 30 40", "observer[powered=true]")]
        trace.append(dict(input=input_power, ticks=ticks, state=state))
    return dict(variant=variant, lit=lit, powered=powered, trace=trace)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--server-jar", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--java", default="java")
    args = parser.parse_args()
    if args.output.exists():
        parser.error("Refusing to overwrite a reference")
    jar = args.server_jar.resolve()
    if hashlib.sha1(jar.read_bytes()).hexdigest() != SERVER_SHA1:
        parser.error("Server JAR does not match the pinned Java 1.21.5 SHA-1")
    with tempfile.TemporaryDirectory(prefix="mroww-bulbs-") as directory:
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
                rcon.request("gamerule randomTickSpeed 0")
                rcon.request("forceload add 32 32 63 63")
                for _ in range(4):
                    rcon.step()
                cases = []
                for variant in VARIANTS:
                    for lit in [False, True]:
                        for powered in [False, True]:
                            cases.append(capture(rcon, variant, lit, powered))
                    print(f"Captured {variant}", flush=True)
                assert len(cases) == 32
                for case in cases:
                    assert case["trace"][0]["state"][:2] == [case["lit"] ^ (not case["powered"]), True]
                args.output.parent.mkdir(parents=True, exist_ok=True)
                args.output.write_text(json.dumps(dict(version="1.21.5", server_sha1=SERVER_SHA1, setup="Strict initial placement in a frozen void world; lever north, direct comparator south, far comparator east behind stone, observer west watching east. Each sample follows the declared input and game-tick advance.", columns=["lit", "powered", "direct_comparator_strength", "far_comparator_strength", "observer_powered"], cases=cases), indent=2) + "\n")
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
