import importlib.util, pathlib, subprocess, time, json, hashlib
root=pathlib.Path(r'F:/rustrepos/MCHPRS')
spec=importlib.util.spec_from_file_location('capture',root/'docs/redpiler-regressions/2026-11-02/evidence/java-8111-capture.py'); c=importlib.util.module_from_spec(spec); spec.loader.exec_module(c)
run=pathlib.Path(r'E:/codex-builds/piston-wire-shape-20261102'); run.mkdir(exist_ok=True)
jar=pathlib.Path(r'E:/codex-builds/java-1215-carry-cancel/server.jar')
assert hashlib.sha1(jar.read_bytes()).hexdigest()==c.SERVER_SHA1
(run/'eula.txt').write_text('eula=true\n')
(run/'server.properties').write_text('server-ip=127.0.0.1\nserver-port=25583\nonline-mode=false\nenable-rcon=true\nrcon.port=25584\nrcon.password=carry-cancel-reference\nview-distance=2\nsimulation-distance=2\nspawn-protection=0\npause-when-empty-seconds=0\nlevel-type=minecraft:flat\ngenerator-settings={"layers":[{"block":"minecraft:air","height":1}],"biome":"minecraft:plains"}\n')
with (run/'server.log').open('w') as log:
 p=subprocess.Popen(['java','-Xmx1G','-Xms256M','-jar',str(jar),'nogui'],cwd=run,stdout=log,stderr=subprocess.STDOUT,creationflags=getattr(subprocess,'CREATE_NO_WINDOW',0))
 r=None; player=None
 try:
  for i in range(180):
   if p.poll() is not None: raise RuntimeError('Java server stopped')
   if 'Done (' in (run/'server.log').read_text(errors='replace'): break
   time.sleep(1)
  r=c.Rcon(); r.command('tick freeze'); r.command('forceload add 0 0 31 31'); r.command('fill 9 9 9 22 22 22 air')
  cells=[((15,14,15),'stone'),((15,14,16),'stone'),((15,15,14),'stone'),((14,16,15),'stone'),((12,15,15),'stone'),((13,16,15),'sticky_piston[facing=east,extended=false]'),((12,16,15),'lever[face=floor,facing=north,powered=false]'),((16,15,15),'observer[facing=west,powered=false]'),((16,16,15),'observer[facing=west,powered=false]'),((15,15,15),'redstone_wire[north=up,south=side,east=none,west=none,power=0]'),((15,15,16),'redstone_wire[north=side,south=side,east=none,west=none,power=0]'),((15,16,14),'redstone_wire[north=side,south=side,east=none,west=none,power=0]')]
  for pos,state in cells: r.command('setblock '+' '.join(map(str,pos))+' minecraft:'+state)
  player=c.Player(log)
  def sample(phase,tick):
   return {'phase':phase,'tick':tick,'gametime':r.command('time query gametime'),'wire':{s:c.probe(r,(15,15,15),[f'redstone_wire[{s}={v}]' for v in ['none','side','up']]) for s in ['north','south','east','west']},'power':c.probe(r,(15,15,15),['redstone_wire[power=0]']),'above':c.probe(r,(15,16,15),['air','moving_piston','stone']),'observer':c.probe(r,(16,15,15),['observer[powered=false]','observer[powered=true]']),'cap_observer':c.probe(r,(16,16,15),['observer[powered=false]','observer[powered=true]'])}
  for _ in range(8): r.step_one()
  samples=[sample(-1,-1)]
  for phase in range(2):
   player.click((12,16,15),r)
   expected=str(phase==0).lower()
   for i in range(100):
    if c.probe(r,(12,16,15),[f'lever[powered={expected}]'])!='unmatched': break
    time.sleep(.01)
   else: raise RuntimeError('lever click failed')
   for tick in range(8):
    r.step_one(); samples.append(sample(phase,tick))
  r.command('fill 9 9 9 22 22 22 air')
  lone_cells=[((15,14,15),'stone'),((15,15,15),'lever[face=floor,facing=north,powered=false]'),((16,15,15),'sticky_piston[facing=east,extended=false]'),((17,15,15),'redstone_block'),((18,14,16),'stone'),((18,15,16),'comparator[facing=north,mode=compare,powered=false]'),((18,15,17),'redstone_lamp[lit=false]'),((19,14,15),'stone'),((19,15,15),'redstone_wire[north=side,south=side,east=side,west=side,power=0]')]
  for pos,state in lone_cells: r.command('setblock '+' '.join(map(str,pos))+' minecraft:'+state)
  for _ in range(8): r.step_one()
  def lone_sample(tick):
   return {'tick':tick,'sides':{side:c.probe(r,(19,15,15),[f'redstone_wire[{side}={v}]' for v in ['none','side','up']]) for side in ['north','south','east','west']}}
  lone_samples=[lone_sample(-1)]
  player.click((15,15,15),r)
  for _ in range(100):
   if c.probe(r,(15,15,15),['lever[powered=true]'])!='unmatched': break
   time.sleep(.01)
  else: raise RuntimeError('lone fixture lever click failed')
  for tick in range(5):
   r.step_one(); lone_samples.append(lone_sample(tick))
  for state in samples:
   phase,tick=state['phase'],state['tick']
   expected='side' if phase==0 and tick>=2 else 'up'
   assert state['wire']['north']==f'redstone_wire[north={expected}]',state
   assert state['power']=='redstone_wire[power=0]',state
   if phase>=0:
    powered=(4<=tick<6) if phase==0 else (2<=tick<4)
    assert state['observer']==f'observer[powered={str(powered).lower()}]',state
    assert state['cap_observer']==f'observer[powered={str(2<=tick<4).lower()}]',state
  for state in lone_samples:
   expected='none' if state['tick']>=2 else 'side'
   assert state['sides']['north']==f'redstone_wire[north={expected}]',state
  result={'server_sha1':c.SERVER_SHA1,'server_sha256':hashlib.sha256(jar.read_bytes()).hexdigest(),'version':'1.21.5','cells':cells,'interaction':player.identity,'samples':samples,'original_publication_fixture':{'cells':lone_cells,'samples':lone_samples}}
  (run/'wire-shape.json').write_text(json.dumps(result,indent=2)); print(json.dumps(result,indent=2),flush=True)
 finally:
  if player: player.close()
  if r:
   try:r.command('stop')
   except Exception:pass
   r.close()
  try:p.wait(timeout=30)
  except subprocess.TimeoutExpired:p.terminate()

