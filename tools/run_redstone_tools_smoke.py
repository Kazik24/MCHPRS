"""Exercise RedstoneTools with two clients, then restart the isolated server."""
import os
import pathlib
import subprocess
import tempfile
import time

ROOT = pathlib.Path(__file__).resolve().parents[1]
with tempfile.TemporaryDirectory(prefix="mchprs-tools-smoke-") as directory:
    run = pathlib.Path(directory)
    (run / "Config.toml").write_text(
        'bind_address = "127.0.0.1:25589"\nview_distance = 2\nauto_redpiler = false\n'
    )
    env = os.environ.copy()
    env["NODE_PATH"] = str(ROOT / "tools/node_modules")
    binary = ROOT / ("target/debug/mchprs.exe" if os.name == "nt" else "target/debug/mchprs")
    for phase in [[], ["--restart"]]:
        with (run / "server.log").open("w") as log:
            server = subprocess.Popen(
                [str(binary)],
                cwd=run,
                stdout=log,
                stderr=subprocess.STDOUT,
                creationflags=getattr(subprocess, "CREATE_NO_WINDOW", 0),
            )
            try:
                time.sleep(2)
                result = subprocess.run(
                    ["node", str(ROOT / "tools/redstone_tools_smoke.js"), "25589", *phase],
                    env=env,
                    timeout=90,
                )
                if result.returncode == 0:
                    server.wait(timeout=10)
                    assert server.returncode == 0, f"Server exited with {server.returncode}"
            finally:
                if server.poll() is None:
                    server.terminate()
                    server.wait(timeout=10)
        if result.returncode:
            print((run / "server.log").read_text(errors="replace")[-5000:])
            raise SystemExit(result.returncode)
    print("PASS: both processes stopped cleanly; temporary tools world removed.")
