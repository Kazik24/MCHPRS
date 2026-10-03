"""Run independently decoded clients in an isolated world, then restart it."""
import os,pathlib,subprocess,tempfile,time,shutil
ROOT=pathlib.Path(__file__).resolve().parents[1]
with tempfile.TemporaryDirectory(prefix='mchprs-1.21.5-smoke-') as directory:
    run=pathlib.Path(directory)
    (run/'Config.toml').write_text('bind_address = "127.0.0.1:25580"\nview_distance = 2\nauto_redpiler = false\n')
    (run/'schems/rf').mkdir(parents=True)
    shutil.copyfile(ROOT/'test_data/ADDER_GWIEZDNY_TEST.schem',run/'schems/rf/ADDER_GWIEZDNY_TEST.schem')
    (run/'schems/BadInput.schem').write_bytes(b'not gzip NBT')
    env=os.environ.copy()
    modules=ROOT/'tools/node_modules'
    if not modules.exists():modules=pathlib.Path(tempfile.gettempdir())/'mchprs-protocol-smoke/node_modules'
    env['NODE_PATH']=str(modules)
    for phase in [[],['--restart']]:
        with (run/'server.log').open('w') as log:
            server=subprocess.Popen([str(ROOT/('target/debug/mchprs.exe' if os.name=='nt' else 'target/debug/mchprs'))],cwd=run,stdout=log,stderr=subprocess.STDOUT,creationflags=getattr(subprocess,'CREATE_NO_WINDOW',0))
            try:
                time.sleep(2)
                result=subprocess.run(['node',str(ROOT/'tools/protocol_smoke.js'),'25580',*phase],env=env,timeout=30)
                if result.returncode==0:
                    server.wait(timeout=10)
                    assert server.returncode==0, f"Server exited with {server.returncode}"
            finally:
                if server.poll() is None:
                    server.terminate();server.wait(timeout=10)
        print((run/'server.log').read_text(errors='replace')[-8000:])
        if result.returncode:raise SystemExit(result.returncode)
    print('PASS: both server processes shut down cleanly; temporary world removed.')
