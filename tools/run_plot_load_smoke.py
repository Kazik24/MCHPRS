"""Verify failed plot/template loads and shutdown without opening the real world."""
import os
from pathlib import Path
import subprocess
import tempfile
import time

ROOT = Path(__file__).resolve().parents[1]
BAD_PLOT = b'\x86MCHPRS\0' + (2).to_bytes(4, 'little')
with tempfile.TemporaryDirectory(prefix='mchprs-plot-load-') as directory:
    run = Path(directory)
    plots = run / 'world/plots'
    plots.mkdir(parents=True)
    (plots / 'p1,0').write_bytes(BAD_PLOT)
    (run / 'Config.toml').write_text('bind_address = "127.0.0.1:25582"\nview_distance = 2\nauto_redpiler = false\n')
    env = os.environ.copy()
    env['NODE_PATH'] = str(ROOT / 'tools/node_modules')
    for phase in ['neighbor', 'spawn', 'template']:
        if phase == 'spawn':
            (plots / 'p0,0').write_bytes(BAD_PLOT)
        elif phase == 'template':
            (plots / 'p0,0').unlink()
            (plots / 'pTEMPLATE').write_bytes(BAD_PLOT)
        with (run / 'server.log').open('w') as log:
            server = subprocess.Popen(
                [str(ROOT / ('target/debug/mchprs.exe' if os.name == 'nt' else 'target/debug/mchprs'))],
                cwd=run, stdout=log, stderr=subprocess.STDOUT,
                creationflags=getattr(subprocess, 'CREATE_NO_WINDOW', 0),
            )
            try:
                time.sleep(1)
                args = ['node', str(ROOT / 'tools/plot_load_smoke.js'), '25582']
                if phase == 'neighbor':
                    args.append('--first')
                subprocess.run(args, env=env, timeout=25, check=True)
                server.wait(timeout=10)
                assert server.returncode == 0, server.returncode
            finally:
                if server.poll() is None:
                    server.terminate()
                    server.wait(timeout=10)
        output = (run / 'server.log').read_text(errors='replace')
        print(output[-4000:])
        assert 'panicked' not in output
        assert 'Could not load plot' in output
        assert (plots / 'p1,0').read_bytes() == BAD_PLOT
        if phase == 'spawn':
            assert (plots / 'p0,0').read_bytes() == BAD_PLOT
        elif phase == 'template':
            assert not (plots / 'p0,0').exists()
            assert (plots / 'pTEMPLATE').read_bytes() == BAD_PLOT
    print('PASS: unsupported neighboring/spawn/template saves preserved; all three processes stopped cleanly.')
