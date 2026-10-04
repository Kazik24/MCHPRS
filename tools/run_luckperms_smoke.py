"""Exercise SQL permission lookups in a temporary world behind a fake local proxy.

Requires private MCHPRS_LUCKPERMS_TEST_CONFIG and MCHPRS_LUCKPERMS_TEST_ACCOUNTS
files. The latter maps default/builder/admin to existing UUIDs. Never run the JS
client against production: forwarding is only simulated on an isolated server.
"""
import os
import pathlib
import subprocess
import tempfile
import time

ROOT = pathlib.Path(__file__).resolve().parents[1]
config = pathlib.Path(os.environ['MCHPRS_LUCKPERMS_TEST_CONFIG']).read_text()
with tempfile.TemporaryDirectory(prefix='mchprs-luckperms-smoke-') as directory:
    run = pathlib.Path(directory)
    (run / 'Config.toml').write_text(
        'bind_address = "127.0.0.1:25588"\nview_distance = 2\nbungeecord = true\n' + config)
    env = os.environ.copy()
    modules = ROOT / 'tools/node_modules'
    if not modules.exists():
        modules = pathlib.Path(tempfile.gettempdir()) / 'mchprs-protocol-smoke/node_modules'
    env['NODE_PATH'] = str(modules)
    with (run / 'server.log').open('w') as log:
        server = subprocess.Popen(
            [str(ROOT / ('target/debug/mchprs.exe' if os.name == 'nt' else 'target/debug/mchprs'))],
            cwd=run, stdout=log, stderr=subprocess.STDOUT,
            creationflags=getattr(subprocess, 'CREATE_NO_WINDOW', 0))
        try:
            time.sleep(2)
            result = subprocess.run(['node', str(ROOT / 'tools/luckperms_smoke.js'), '25588'],
                                    env=env, timeout=90)
            if result.returncode == 0:
                server.wait(timeout=15)
                assert server.returncode == 0
        finally:
            if server.poll() is None:
                server.terminate()
                server.wait(timeout=10)
    if result.returncode:
        print((run / 'server.log').read_text(errors='replace')[-6000:])
        raise SystemExit(result.returncode)
    print('PASS: isolated server shut down cleanly; test world removed.')
