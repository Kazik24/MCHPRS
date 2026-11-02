"""Replay RedstoneDB fixture 8111 on an official vanilla 1.21.5 server."""
import hashlib
import argparse
import json
import queue
import socket
import struct
import subprocess
import sys
import threading
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parents[4]
sys.path.insert(0, str(ROOT))
from tools.inspect_instant_pistons import inspect

FIXTURE_ID = "8111b000090d8f0b3e1370448bcb64a3"
SCHEM = ROOT / "test_data/redstonedb/schematics/8111b000090d8f0b3e1370448bcb64a3-8_bit_Carry_Cancel_Adder.schem"
ACTIONS = Path(__file__).parent / "mair" / f"{FIXTURE_ID}.actions.json"
SERVER_SHA1 = "e6ec2f64e6080b9b5d9b471b291c33cc7f509733"
ORIGIN = (8, 8, 8)
PORT, RCON_PORT, PASSWORD = 25583, 25584, "carry-cancel-reference"


def snbt(value, key=None):
    if isinstance(value, bool):
        return str(value).lower()
    if isinstance(value, int):
        return f"{value}b" if key == "Slot" else str(value)
    if isinstance(value, float):
        return repr(value)
    if isinstance(value, str):
        return json.dumps(value, ensure_ascii=True)
    if isinstance(value, list):
        return "[" + ",".join(snbt(item) for item in value) + "]"
    if isinstance(value, dict):
        return "{" + ",".join(json.dumps(k) + ":" + snbt(v, k) for k, v in value.items()) + "}"
    raise TypeError(type(value))


class Rcon:
    def __init__(self):
        self.sock = socket.create_connection(("127.0.0.1", RCON_PORT), timeout=30)
        self.sock.settimeout(30)
        self.ident = 1
        if self.packet(3, PASSWORD)[0] == -1:
            raise RuntimeError("RCON authentication failed")

    def packet(self, kind, text):
        ident = self.ident
        self.ident += 1
        payload = struct.pack("<ii", ident, kind) + text.encode("utf-8") + b"\0\0"
        self.sock.sendall(struct.pack("<i", len(payload)) + payload)
        size = struct.unpack("<i", self.read(4))[0]
        body = self.read(size)
        response_id, response_kind = struct.unpack("<ii", body[:8])
        if response_id != ident:
            raise RuntimeError(f"unexpected RCON response id {response_id}, expected {ident}")
        return response_id, body[8:-2].decode("utf-8", errors="replace")

    def read(self, size):
        chunks = bytearray()
        while len(chunks) < size:
            chunk = self.sock.recv(size - len(chunks))
            if not chunk:
                raise ConnectionError("RCON disconnected")
            chunks.extend(chunk)
        return bytes(chunks)

    def command(self, text):
        return self.packet(2, text)[1]

    def step_one(self):
        before = int(self.command("time query gametime").rsplit(" ", 1)[1])
        self.command("tick step 1")
        deadline = time.monotonic() + 3
        while time.monotonic() < deadline:
            after = int(self.command("time query gametime").rsplit(" ", 1)[1])
            if after == before + 1:
                return after
            if after > before + 1:
                raise RuntimeError(f"tick step advanced game time from {before} to {after}")
            time.sleep(0.005)
        raise TimeoutError(f"tick step did not advance game time from {before}")

    def close(self):
        self.sock.close()


class Player:
    def __init__(self, server_log):
        self.proc = subprocess.Popen(
            ["C:/Program Files/nodejs/node.exe", str(ROOT / "tools/java_lever_client.cjs")],
            cwd=ROOT / "tools", stdin=subprocess.PIPE, stdout=subprocess.PIPE,
            stderr=server_log, text=True, creationflags=getattr(subprocess, "CREATE_NO_WINDOW", 0))
        self.responses = queue.Queue()
        threading.Thread(target=self._read, daemon=True).start()
        ready = self.receive("ready")
        self.identity = {"method": "vanilla UseItemOn packet from offline player", "protocol": ready}
        self.sequence = 0

    def _read(self):
        for line in self.proc.stdout:
            try:
                self.responses.put(json.loads(line))
            except ValueError:
                self.responses.put({"event": "error", "message": line})

    def receive(self, expected):
        response = self.responses.get(timeout=30)
        if response.get("event") != expected:
            raise RuntimeError(f"expected player event {expected}, got {response}")
        return response

    def click(self, pos, rcon):
        self.sequence += 1
        self.proc.stdin.write(json.dumps({"op": "prepare", "id": self.sequence, "pos": pos}) + "\n")
        self.proc.stdin.flush()
        self.receive("prepared")
        x, y, z = pos
        rcon.command(f"tp InstantReference {x + 0.5} {y + 1.0} {z + 1.5}")
        return self.receive("sent")

    def close(self):
        if self.proc.poll() is None:
            self.proc.stdin.write('{"op":"stop"}\n')
            self.proc.stdin.flush()
            self.proc.wait(timeout=10)


def absolute(pos):
    return tuple(a + b for a, b in zip(ORIGIN, pos))


def probe(rcon, pos, states):
    xyz = " ".join(map(str, pos))
    rcon.command('data modify storage carry_cancel:probe result set value "unmatched"')
    for state in states:
        rcon.command(f'execute if block {xyz} minecraft:{state} run data modify storage carry_cancel:probe result set value "{state}"')
    return rcon.command("data get storage carry_cancel:probe result").split(": ", 1)[-1].strip().strip('"')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--server-jar", type=Path, required=True)
    parser.add_argument("--run-dir", type=Path, required=True, help="new isolated world directory")
    args = parser.parse_args()
    jar = args.server_jar.resolve()
    run = args.run_dir.resolve()
    global RUN
    RUN = run
    assert hashlib.sha1(jar.read_bytes()).hexdigest() == SERVER_SHA1
    schematic = inspect(SCHEM)
    assert schematic["sha256"] == "19b2b16bb84371f320e9c7ac5d6fdac38fe6a0e248f1ba0f03eb12f623f9175d"
    actions = json.loads(ACTIONS.read_text(encoding="utf-8"))
    assert len(actions) == 66 and sum(action["wait"] for action in actions) == 831
    assert not RUN.exists(), "refusing an existing server directory"
    RUN.mkdir(parents=True, exist_ok=True)
    (RUN / "eula.txt").write_text("eula=true\n")
    (RUN / "server.properties").write_text(
        f"server-ip=127.0.0.1\nserver-port={PORT}\nonline-mode=false\nenable-rcon=true\n"
        f"rcon.port={RCON_PORT}\nrcon.password={PASSWORD}\nview-distance=2\nsimulation-distance=2\n"
        "spawn-protection=0\npause-when-empty-seconds=0\nlevel-type=minecraft:flat\n"
        'generator-settings={"layers":[{"block":"minecraft:air","height":1}],"biome":"minecraft:plains"}\n')
    function_dir = RUN / "world/datapacks/carry_cancel/data/carry_cancel/function"
    function_dir.mkdir(parents=True, exist_ok=True)
    (function_dir.parents[2] / "pack.mcmeta").write_text(
        json.dumps({"pack": {"pack_format": 71, "description": "Java 1.21.5 carry-cancel replay"}}))
    place = []
    for cell in schematic["nonair_cells"]:
        x, y, z = absolute(cell["pos"])
        place.append(f"setblock {x} {y} {z} {cell['state']}")
    for entity in schematic["block_entities"]:
        data = entity.get("Data", entity).copy()
        for key in ("Id", "id", "Pos", "x", "y", "z"):
            data.pop(key, None)
        x, y, z = absolute(entity["Pos"])
        place.append(f"data merge block {x} {y} {z} {snbt(data)}")
    (function_dir / "place.mcfunction").write_text("\n".join(place) + "\n", encoding="utf-8")

    lever_cells = [c for c in schematic["nonair_cells"] if c["state"].startswith("minecraft:lever")]
    lever_cells.sort(key=lambda c: (c["pos"][1], c["pos"][2], c["pos"][0]))
    lever_states = ["powered=true" in c["state"] for c in lever_cells]
    assert len(lever_cells) == 17
    server_log_path = RUN / "server.log"
    with server_log_path.open("w", encoding="utf-8") as server_log:
        server = subprocess.Popen(["java", "-Xmx2G", "-Xms256M", "-jar", str(jar), "nogui"],
                                  cwd=RUN, stdout=server_log, stderr=subprocess.STDOUT,
                                  creationflags=getattr(subprocess, "CREATE_NO_WINDOW", 0))
        rcon = player = None
        try:
            deadline = time.monotonic() + 120
            while not server_log_path.exists() or "Done (" not in server_log_path.read_text(errors="replace"):
                if server.poll() is not None:
                    raise RuntimeError(server_log_path.read_text(errors="replace"))
                if time.monotonic() > deadline:
                    raise TimeoutError("server startup")
                time.sleep(0.25)
            rcon = Rcon()
            rcon.command("tick freeze")
            rcon.command("forceload add 0 0 15 31")
            rcon.command("reload")
            result = rcon.command("function carry_cancel:place")
            if "Running function" not in result:
                raise RuntimeError(f"schematic placement failed: {result}")
            player = Player(server_log)
            rcon.command("gamemode creative InstantReference")
            start_game_time = int(rcon.command("time query gametime").rsplit(" ", 1)[1])
            samples = []
            def sample(action, tick):
                coords = [absolute(p) for p in ((2, 2, 3), (5, 1, 1), (6, 1, 0))]
                return {"action": action, "tick": tick,
                        "gametime": rcon.command("time query gametime"),
                        "source_comparator": probe(rcon, coords[0], [
                            f"comparator[facing={face},mode={mode},powered={powered}]"
                            for face in ("north", "south", "east", "west")
                            for mode in ("compare", "subtract") for powered in ("true", "false")]),
                        "target_comparator": probe(rcon, coords[1], [
                            f"comparator[facing={face},mode={mode},powered={powered}]"
                            for face in ("north", "south", "east", "west")
                            for mode in ("compare", "subtract") for powered in ("true", "false")]),
                        "source_output_signal": rcon.command(f"data get block {' '.join(map(str,coords[0]))} OutputSignal"),
                        "target_output_signal": rcon.command(f"data get block {' '.join(map(str,coords[1]))} OutputSignal"),
                        "target_wire_power": probe(rcon, coords[2], [f"redstone_wire[power={n}]" for n in range(16)])}
            total_ticks = 0
            for action in actions:
                if action["step"] == 39:
                    assert total_ticks == 694
                    assert int(rcon.command("time query gametime").rsplit(" ", 1)[1]) == start_game_time + total_ticks
                    samples.append(sample(39, -1))
                lever = action["index"]
                pos = absolute(lever_cells[lever]["pos"])
                lever_states[lever] = not lever_states[lever]
                interaction = player.click(pos, rcon)
                expected = str(lever_states[lever]).lower()
                deadline = time.monotonic() + 3
                while True:
                    rcon.command('data modify storage carry_cancel:probe lever set value "no"')
                    rcon.command(f'execute if block {" ".join(map(str,pos))} minecraft:lever[powered={expected}] run data modify storage carry_cancel:probe lever set value "yes"')
                    response = rcon.command("data get storage carry_cancel:probe lever")
                    if '"yes"' in response or time.monotonic() > deadline:
                        break
                    time.sleep(0.01)
                if '"yes"' not in response:
                    raise RuntimeError(f"Java rejected lever action {action['step']}: {interaction}; {response}")
                if action["step"] == 39:
                    samples.append(sample(39, 0))
                    for local_tick in range(1, action["wait"] + 1):
                        rcon.step_one()
                        total_ticks += 1
                        samples.append(sample(39, local_tick))
                else:
                    for _ in range(action["wait"]):
                        rcon.step_one()
                        total_ticks += 1
            assert total_ticks == 831
            assert [sample["tick"] for sample in samples] == [-1, 0, 1, 2, 3, 4, 5]
            assert samples[0]["source_output_signal"].endswith(": 7")
            assert samples[4]["source_output_signal"].endswith(": 0")
            assert samples[-1]["target_wire_power"] == "redstone_wire[power=0]"
            out = {"schema_version": 1, "engine": "Java", "engine_version": "1.21.5",
                   "server_sha1": SERVER_SHA1, "server_sha256": hashlib.sha256(jar.read_bytes()).hexdigest(),
                   "fixture": FIXTURE_ID, "fixture_sha256": schematic["sha256"],
                   "actions_sha256": hashlib.sha256(ACTIONS.read_bytes()).hexdigest(),
                   "origin": ORIGIN, "lever_order": [c["pos"] for c in lever_cells],
                   "game_time_at_start": start_game_time,
                   "setup": "isolated flat void world; Java setblock + data merge of saved schematic states; frozen ticks; vanilla offline-player UseItemOn lever packets",
                   "absolute_tick_before_action_39": 694, "absolute_tick_after_action_39_wait": 699,
                   "samples": samples, "player": player.identity,
                   "capture_command": ["py", str(Path(__file__).relative_to(ROOT)), "--server-jar", str(jar), "--run-dir", str(run)]}
            out_path = Path(__file__).parent / "java-8111-carry-cancel.json"
            out_path.write_text(json.dumps(out, indent=2) + "\n", encoding="utf-8")
            print(json.dumps(out, indent=2))
        finally:
            if player:
                player.close()
            if rcon:
                try:
                    rcon.command("forceload remove 0 0 15 31")
                    rcon.command("stop")
                finally:
                    rcon.close()
            else:
                server.terminate()
            try:
                server.wait(timeout=30)
            except subprocess.TimeoutExpired:
                server.kill()
                server.wait(timeout=10)


if __name__ == "__main__":
    main()
