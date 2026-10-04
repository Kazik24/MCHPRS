"""Capture existing circuit fixtures on the SHA-pinned Java 1.21.5 server.

Requires Java 21 and tools' npm dependencies. Runs a frozen, offline server in
an isolated temporary directory; never reads or writes the live MCHPRS world.
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
import zipfile

ROOT = Path(__file__).resolve().parents[1]
SHA1 = 'e6ec2f64e6080b9b5d9b471b291c33cc7f509733'
FIXTURES = ['UpdateTesterNonInst.schem', 'UpdateTesterInst.schem',
            'UpdateTesterExtendInst.schem', 'UpdateTesterExtendNonInst.schem',
            'MemCellUnalignedNanoTicks.schem']
DECODE = r"""
const fs=require('fs'), nbt=require('prismarine-nbt');
(async()=>{
const {parsed}=await nbt.parse(fs.readFileSync(process.argv[1]));
const raw=nbt.simplify(parsed), s=raw.Schematic||raw, m=s.Metadata||{}, pal={};
for(const [name,id] of Object.entries((s.Blocks||s).Palette)) pal[id]=name;
const off=s.Version===3 ? (s.Offset||[0,0,0]) : ['X','Y','Z'].map(k=>m['WEOffset'+k]||0);
const bytes=Buffer.from(s.Version===3?s.Blocks.Data:s.BlockData), blocks=[]; let i=0, entry=0;
while(i<bytes.length){let v=0, shift=0, b;do{b=bytes[i++];v|=(b&127)<<shift;shift+=7;}while(b&128);
let state=pal[v];
// Old fork emitted a redundant sticky property; Java derives it from block type.
state=state.replace(/,sticky=(true|false)/g,'').replace(/sticky=(true|false),/g,'');
const x=entry%s.Width, z=Math.floor(entry/s.Width)%s.Length, y=Math.floor(entry/(s.Width*s.Length));
blocks.push({pos:[100+off[0]+x,30+off[1]+y,100+off[2]+z],state});entry++;}
console.log(JSON.stringify(blocks));
})().catch(e=>{console.error(e);process.exit(1)});
"""

class Rcon:
    def __init__(self):
        self.s = socket.create_connection(('127.0.0.1', 25584), timeout=3)
        self.s.settimeout(5)
        self.seq = 0
        self.request('piston-reference', 3)
    def read(self, size):
        data=b''
        while len(data)<size:
            chunk=self.s.recv(size-len(data))
            if not chunk: raise ConnectionError('RCON closed')
            data+=chunk
        return data
    def request(self, command, kind=2):
        self.seq+=1
        payload=struct.pack('<ii',self.seq,kind)+command.encode()+b'\0\0'
        self.s.sendall(struct.pack('<i',len(payload))+payload)
        while True:
            size=struct.unpack('<i',self.read(4))[0]
            answer=self.read(size)
            rid,rkind=struct.unpack('<ii',answer[:8])
            if rid==-1: raise PermissionError('RCON authentication failed')
            if rid==self.seq and (kind!=3 or rkind==2):
                return answer[8:-2].decode()
    def step(self):
        before=self.request('time query gametime')
        self.request('tick step 1')
        deadline=time.monotonic()+5
        while self.request('time query gametime')==before:
            if time.monotonic()>deadline: raise TimeoutError('Java tick step')
            time.sleep(.02)
        # The time query can run before the entity phase of the stepped tick.
        time.sleep(.06)

def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--server-jar',type=Path,required=True)
    parser.add_argument('--java',default='java')
    parser.add_argument('--output',type=Path,default=ROOT/'docs/piston-repair/java-traces.json')
    args=parser.parse_args()
    jar=args.server_jar.resolve()
    assert hashlib.sha1(jar.read_bytes()).hexdigest()==SHA1, 'Unexpected Java server binary'
    env=os.environ.copy()
    env['NODE_PATH']=str(ROOT/'tools/node_modules')
    blocks={name:json.loads(subprocess.check_output(['node','-e',DECODE,str(ROOT/'test_data'/name)],cwd=ROOT,env=env)) for name in FIXTURES}
    with tempfile.TemporaryDirectory(prefix='mchprs-java-piston-') as directory:
        run=Path(directory)
        (run/'eula.txt').write_text('eula=true\n')
        (run/'server.properties').write_text('server-ip=127.0.0.1\nserver-port=25583\nonline-mode=false\nenable-rcon=true\nrcon.port=25584\nrcon.password=piston-reference\nview-distance=2\nsimulation-distance=2\nspawn-protection=0\nlevel-type=minecraft:flat\ngenerator-settings={"layers":[{"block":"minecraft:air","height":1}],"biome":"minecraft:plains"}\n')
        pack=run/'world/datapacks/piston_reference'
        functions=pack/'data/piston_reference/function'
        functions.mkdir(parents=True)
        (pack/'pack.mcmeta').write_text(json.dumps({'pack':{'pack_format':71,'description':'Isolated piston trace fixtures'}}))
        for idx,name in enumerate(FIXTURES):
            commands=['fill 85 20 85 120 43 120 minecraft:air strict']
            commands += ['setblock '+' '.join(map(str,b['pos']))+' '+b['state']+' strict' for b in blocks[name] if b['state']!='minecraft:air']
            (functions/f'fixture_{idx}.mcfunction').write_text('\n'.join(commands)+'\n')
        observations=[]
        for label,pos in [('BASE',[100,30,102]),('HEAD',[102,30,102]),('PUSH',[104,30,102])]:
            p=' '.join(map(str,pos))
            observations.append(f'execute if block {p} minecraft:redstone_wire unless block {p} minecraft:redstone_wire[power=0]')
        observations.append('execute if block 97 30 107 minecraft:sticky_piston[extended=true]')
        (functions/'observe.mcfunction').write_text('\n'.join(observations)+'\n')
        with (run/'server.log').open('w') as log:
            server=subprocess.Popen([args.java,'-Xmx1G','-Xms256M','-jar',str(jar),'nogui'],cwd=run,stdout=log,stderr=subprocess.STDOUT,creationflags=getattr(subprocess,'CREATE_NO_WINDOW',0))
            try:
                deadline=time.monotonic()+90
                while True:
                    if server.poll() is not None: raise RuntimeError((run/'server.log').read_text(errors='replace')[-6000:])
                    if 'Done (' in (run/'server.log').read_text(errors='replace'): break
                    if time.monotonic()>deadline: raise TimeoutError('Java server startup')
                    time.sleep(.25)
                rcon=Rcon()
                print(rcon.request('tick freeze'),flush=True)
                rcon.request('forceload add 80 80 127 127')
                result={'version':'1.21.5','server_sha1':SHA1,'setup':'Strict fixture paste into frozen void world; normal button power/removal; one game tick per observation','fixtures':{}}
                for idx,name in enumerate(FIXTURES):
                    # Clear scheduled work from the prior circuit before placing the next.
                    rcon.request('fill 85 20 85 120 43 120 minecraft:air strict')
                    for _ in range(22): rcon.step()
                    response=rcon.request(f'function piston_reference:fixture_{idx}')
                    if 'Unknown' in response: raise RuntimeError(response+'\n'+(run/'server.log').read_text(errors='replace')[-6000:])
                    memory=name.startswith('MemCell')
                    if memory: stimulus='setblock 100 30 100 minecraft:air'
                    else:
                        button=next(b['state'] for b in blocks[name] if b['pos']==[100,30,100])
                        stimulus='setblock 100 30 100 '+button.replace('powered=false','powered=true')
                    response=rcon.request(stimulus)
                    print(name,'paste:',rcon.request('execute if block 100 30 100 minecraft:stone_button'),'stimulus:',response,flush=True)
                    trace=[]
                    details=[]
                    for _ in range(12):
                        rcon.step()
                        statuses=[rcon.request(c) for c in observations]
                        trace.append('Test passed' in statuses[3] if memory else ['Test passed' in t for t in statuses[:3]])
                        if memory:
                            snapshot=[]
                            for pos in [[97,30,107],[98,30,107],[98,31,107],[98,30,103],[98,31,103],[97,33,103],[100,30,103]]:
                                p=' '.join(map(str,pos))
                                state='other'
                                for block in ['sticky_piston','moving_piston','piston_head','observer','air']:
                                    if 'Test passed' in rcon.request(f'execute if block {p} minecraft:{block}'):
                                        state=block;break
                                if state=='sticky_piston': state+=' extended='+str('Test passed' in rcon.request(f'execute if block {p} minecraft:sticky_piston[extended=true]'))
                                if state=='observer': state+=' powered='+str('Test passed' in rcon.request(f'execute if block {p} minecraft:observer[powered=true]'))
                                if state=='moving_piston': state+=' '+rcon.request(f'data get block {p}')
                                snapshot.append({'pos':pos,'state':state})
                            details.append(snapshot)
                    result['fixtures'][name]={'sha256':hashlib.sha256((ROOT/'test_data'/name).read_bytes()).hexdigest(),'stimulus':stimulus,'trace':trace, 'details':details}
                    print(name,trace,flush=True)
                args.output.parent.mkdir(parents=True,exist_ok=True)
                args.output.write_text(json.dumps(result,indent=2)+'\n')
                rcon.request('stop')
                rcon.s.close()
                server.wait(timeout=15)
            finally:
                if server.poll() is None:
                    server.terminate();server.wait(timeout=10)
if __name__=='__main__': main()
