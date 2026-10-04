// Dependencies and runner instructions: tools/README.md.
const mc = require('minecraft-protocol');
const data = require('minecraft-data')('1.21.5');
const Chunk = require('prismarine-chunk')('1.21.5');
const { Vec3 } = require('vec3');
const assert = require('assert/strict');
const nbt = require('prismarine-nbt');
const port = Number(process.argv[2] || 25580);
const restart = process.argv.includes('--restart');
const clients = []; let failed = false; let stopping = false;
const delay = ms => new Promise(r => setTimeout(r, ms));
async function until(predicate, description) {
  for (let i = 0; i < 100; i++) { if (predicate()) return; await delay(50); }
  throw Error('Timeout: ' + description);
}
function connect(name) {
  const client = mc.createClient({host:'127.0.0.1',port,username:name,auth:'offline',version:'1.21.5'});
  clients.push(client); client.seen = {}; client.chunks = new Map(); client.slots = new Map();
  client.tags = []; client.messages = []; client.blocks = new Map(); client.actions = []; client.acks = []; client.entities = [];
  client.on('packet', (p, meta) => {
    try {
      client.seen[meta.name] = (client.seen[meta.name] || 0) + 1;
      if (meta.name === 'tags') client.tags=p.tags;
      if (meta.name === 'system_chat') client.messages.push(JSON.stringify(nbt.simplify(p.content)));
      if (meta.name === 'position') client.write('teleport_confirm', {teleportId:p.teleportId});
      if (meta.name === 'map_chunk') {
        const chunk = new Chunk({minY:0,worldHeight:256}); chunk.load(p.chunkData);
        client.chunks.set(`${p.x},${p.z}`, chunk);
        // Force palette reads through the full vertical range, including empty sections.
        for (let y=0;y<256;y++) assert(data.blocksByStateId[chunk.getBlockStateId(new Vec3(2,y,2))]);
      }
      if (meta.name === 'window_items') p.items.forEach((v,i)=>client.slots.set(i,v));
      if (meta.name === 'set_slot') client.slots.set(p.slot,p.item);
      if (meta.name === 'block_change') client.blocks.set(`${p.location.x},${p.location.y},${p.location.z}`,p.type);
      if (meta.name === 'multi_block_change') {
        for (const record of p.records) {
          const x=p.chunkCoordinates.x*16+((record>>>8)&15);
          const z=p.chunkCoordinates.z*16+((record>>>4)&15);
          const y=p.chunkCoordinates.y*16+(record&15);
          client.blocks.set(`${x},${y},${z}`,record>>>12);
        }
      }
      if (meta.name === 'block_action') client.actions.push(p);
      if (meta.name === 'tile_entity_data') client.entities.push(p);
      if (meta.name === 'acknowledge_player_digging') client.acks.push(p.sequenceId);
    } catch (e) { failed = true; console.error(meta.name,e); }
  });
  client.on('error',e=>{if(!stopping){failed=true;console.error(name,e);}});
  client.on('kick_disconnect',p=>{if(!stopping){failed=true;console.error('KICK',name,p);}});
  return until(()=>client.chunks.size>0,'join '+name).then(()=>client);
}
function state(client,x,y,z) {
  const key=`${x},${y},${z}`;
  return client.blocks.get(key) ?? client.chunks.get(`${Math.floor(x/16)},${Math.floor(z/16)}`)?.getBlockStateId(new Vec3(x&15,y,z&15));
}
function item(name, components=[]) {
  return {itemCount:1,itemId:data.itemsByName[name].id,addedComponentCount:components.length,removedComponentCount:0,components,removeComponents:[]};
}
async function creative(client,slot,value) {
  const previous=client.seen.set_slot||0;
  client.write('set_creative_slot',{slot,item:value});
  await until(()=>client.seen.set_slot>previous,'creative inventory echo');
  return client.slots.get(slot);
}
async function place(client,x,y,z,sequence) {
  client.write('block_place',{hand:0,location:{x,y:y-1,z},direction:1,cursorX:0.5,cursorY:1,cursorZ:0.5,insideBlock:false,worldBorderHit:false,sequence});
  await until(()=>client.acks.includes(sequence),'prediction acknowledgement '+sequence);
  await delay(200);
}
function command(client,command) { client.write('chat_command',{command}); }
(async()=>{
  const a=await connect('PortSmokeOne'); const b=await connect('PortSmokeTwo');
  await delay(800);
  for (const c of [a,b]) for (const name of ['login','position','map_chunk','declare_commands','window_items','player_info','spawn_entity']) assert(c.seen[name],`Missing ${name}`);
  for (const c of [a,b]) {
    const enchantment=c.tags.find(t=>t.tagType==='minecraft:enchantment');
    assert(enchantment,'Enchantment tags must bind references in configuration registries');
    for(const set of ['armor','boots','bow','crossbow','damage','mining','riptide']) assert(enchantment.tags.some(t=>t.tagName==='minecraft:exclusive_set/'+set && t.entries.length>0),'Missing exclusive set '+set);
  }
  const x=130,z=130; let floor=0;
  const chunk=a.chunks.get('8,8'); assert(chunk,'spawn chunk');
  for(let y=0;y<256;y++) if(data.blocksByStateId[chunk.getBlockStateId(new Vec3(2,y,3))].name!=='air') floor=y;
  const y=floor+1;
  if(!restart) {
    command(a,'rtps 20'); await delay(100);
    // block_state patch: {facing: east}. Each creative component carries its own length.
    const facing=Buffer.from([1,6,...Buffer.from('facing'),4,...Buffer.from('east')]);
    const echoed=await creative(a,36,item('piston',[{type:'block_state',data:facing}]));
    assert.equal(echoed.itemId,data.itemsByName.piston.id);
    assert.equal(echoed.components[0].type,'block_state');
    a.write('held_item_slot',{slotId:0});
    await place(a,x,y,z,101);
    await until(()=>data.blocksByStateId[state(a,x,y,z)]?.name==='piston','piston placement');
    await creative(a,36,item('redstone_block'));
    await place(a,x-1,y,z,102);
    await until(()=>a.actions.some(p=>p.blockId===data.blocksByName.piston.id),'piston block action');
    await until(()=>data.blocksByStateId[state(a,x+1,y,z)]?.name==='piston_head','extended piston');
    // 1.21.5 unit component and integer component, retained in saves and equipment.
    await creative(a,37,item('stone',[{type:'unbreakable',data:Buffer.alloc(0)},{type:'max_stack_size',data:Buffer.from([16])}]));
    // Exercise actual WorldEdit commands on the supplied Sponge v3 fixture.
    a.write('position',{x:100,y:30,z:100,flags:{onGround:false,hasHorizontalCollision:false}});
    async function we(cmd,text) {
      const start=a.messages.length; command(a,cmd);
      try { await until(()=>a.messages.slice(start).some(m=>m.includes(text)),cmd+' success'); }
      catch(error) { throw Error(error.message+': '+JSON.stringify(a.messages.slice(start))); }
    }
    await we('version','MCHPRS 0.4.1 (Minecraft 1.21.5, protocol 770)');
    await we('wsr','World send rate:');
    await we('wsr 0','successfully set');
    await we('wsr','World send rate: 0 Hz');
    await we('wsr 20','successfully set');
    await we('rtps 200000','successfully set');
    await we('rtps 20','successfully set');
    await we('p visit PortSmokeOne 0','Plot index starts at 1');
    await we('p sel','Second position');
    async function select(first,second) {
      for (const [name,pos] of [['/pos1',first],['/pos2',second]]) {
        a.write('position',{x:pos[0],y:pos[1],z:pos[2],flags:{onGround:false,hasHorizontalCollision:false}});
        await we(name,'position set');
      }
    }
    await select([140,40,120],[144,40,120]);
    await we('/set air','Operation completed');
    await select([140,40,120],[142,40,120]);
    await we('/set stone','Operation completed');
    await select([141,40,120],[141,40,120]);
    await we('/set gold_block','Operation completed');
    await select([140,40,120],[142,40,120]);
    const cells=()=>[140,141,142,143,144].map(x=>data.blocksByStateId[state(a,x,40,120)].name);
    await until(()=>JSON.stringify(cells())===JSON.stringify(['stone','gold_block','stone','air','air']),'rstack initial cells');
    await we('/rstack 2 1 east','stacked successfully');
    await until(()=>JSON.stringify(cells())===JSON.stringify(['stone','stone','stone','gold_block','stone']),'overlapping rstack');
    command(a,'/undo');
    await until(()=>JSON.stringify(cells())===JSON.stringify(['stone','gold_block','stone','air','air']),'overlapping rstack undo');
    command(a,'/redo');
    await until(()=>JSON.stringify(cells())===JSON.stringify(['stone','stone','stone','gold_block','stone']),'overlapping rstack redo');
    a.write('position',{x:100,y:30,z:100,flags:{onGround:false,hasHorizontalCollision:false}});
    await we('/load rf/ADDER_GWIEZDNY_TEST.schem','loaded to your clipboard');
    await we('/paste -as','clipboard was pasted');
    await until(()=>a.entities.some(p=>p.location.x===120 && p.location.y===29 && p.location.z===92 && p.action===7),'O2 sign at paste displacement');
    const signState=state(a,120,29,92); assert(data.blocksByStateId[signState].name.includes('sign'));
    command(a,'/undo'); await until(()=>state(a,120,29,92)===0,'WorldEdit undo');
    command(a,'/redo'); await until(()=>state(a,120,29,92)===signState,'WorldEdit redo');
    await we('/help rs','Like //stack');
    await we('/load BadInput.schem','error loading the schematic');
    // A failed import must retain the successfully loaded clipboard.
    await we('/save SmokeRoundtrip.schem','saved sucessfuly');
    await we('/load SmokeRoundtrip.schem','loaded to your clipboard');
    await we('/paste','clipboard was pasted');
    // This exact production schematic used to panic while encoding mixed sign rows.
    await we('/load rf/sign_mixed_text_v2.schem','loaded to your clipboard');
    const entityStart=a.entities.length;
    await we('/paste','clipboard was pasted');
    await until(()=>a.entities.slice(entityStart).filter(p=>p.action===7).length===2,'mixed sign row packets');
    function render(component) {
      if(typeof component==='string')return component;
      if(Array.isArray(component))return component.map(render).join('');
      if(Object.hasOwn(component,''))return render(component['']);
      return (component.text||'')+(component.extra||[]).map(render).join('');
    }
    const signs=a.entities.slice(entityStart).filter(p=>p.action===7).map(p=>nbt.simplify(p.nbtData));
    assert(signs.some(s=>JSON.stringify(s.front_text.messages.map(render))===JSON.stringify(['Addery ','by','Lord225',''])));
    assert(signs.some(s=>JSON.stringify(s.front_text.messages.map(render))===JSON.stringify(['Dodawanie','Update','',''])));
    assert(signs.every(s=>s.back_text.messages.every(m=>render(m)==='')));
    const mixedPosition=a.entities[entityStart].location;
    command(a,'/undo');
    await until(()=>state(a,mixedPosition.x,mixedPosition.y,mixedPosition.z)===0,'mixed sign undo');
    const redoStart=a.entities.length;
    command(a,'/redo');
    await until(()=>a.entities.slice(redoStart).filter(p=>p.action===7).length===2,'mixed sign redo packets');
    await we('/save MixedSignsRoundtrip.schem','saved sucessfuly');
    await we('/load MixedSignsRoundtrip.schem','loaded to your clipboard');
    await we('/paste','clipboard was pasted');
    a.end(); await delay(600);
    const again=await connect('PortSmokeOne');
    assert.equal(again.slots.get(37).components[1].data,16);
    command(again,'rtps');
    console.log('PASS: configuration, two players, deep chunks, commands, components, placement, piston events, acknowledgements, Sponge v3 and production mixed-sign v2 paste/undo/redo/save/reload and reconnect.');
    stopping=true; command(again,'stop'); await delay(800);
  } else {
    assert.equal(a.slots.get(37).components[1].data,16);
    const pstate=state(a,x,y,z);
    assert.equal(data.blocksByStateId[pstate].name,'piston');
    assert.equal(data.blocksByStateId[state(a,x+1,y,z)].name,'piston_head');
    console.log('PASS: process restart retained structured item components and extended piston/head states.');
    stopping=true;command(a,'stop');await delay(800);
  }
  for(const c of clients)c.end();
  process.exit(failed?1:0);
})().catch(e=>{console.error(e);for(const c of clients)c.end();process.exit(1);});
setTimeout(()=>{console.error('TIMEOUT');process.exit(1);},25000);
