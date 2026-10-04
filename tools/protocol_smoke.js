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
  client.tags = []; client.messages = []; client.blocks = new Map(); client.actions = []; client.acks = []; client.entities = []; client.movingUpdates = 0;
  client.on('packet', (p, meta) => {
    try {
      client.seen[meta.name] = (client.seen[meta.name] || 0) + 1;
      if (meta.name === 'tags') client.tags=p.tags;
      if (meta.name === 'declare_commands') client.commandTree=p;
      if (meta.name === 'system_chat') client.messages.push(JSON.stringify(nbt.simplify(p.content)));
      if (meta.name === 'position') client.write('teleport_confirm', {teleportId:p.teleportId});
      if (meta.name === 'map_chunk') {
        const chunk = new Chunk({minY:0,worldHeight:256}); chunk.load(p.chunkData);
        // An authoritative chunk replaces previously received incremental changes.
        for (const key of client.blocks.keys()) {
          const [x,,z] = key.split(',').map(Number);
          if (Math.floor(x/16)===p.x && Math.floor(z/16)===p.z) client.blocks.delete(key);
        }
        client.chunks.set(`${p.x},${p.z}`, chunk);
        client.chunkEntities=(client.chunkEntities||[]).concat(p.blockEntities.map(entity=>nbt.simplify(entity.nbtData)));
        // Force palette reads through the full vertical range, including empty sections.
        for (let y=0;y<256;y++) assert(data.blocksByStateId[chunk.getBlockStateId(new Vec3(2,y,2))]);
      }
      if (meta.name === 'window_items') p.items.forEach((v,i)=>client.slots.set(i,v));
      if (meta.name === 'set_slot') client.slots.set(p.slot,p.item);
      if (meta.name === 'held_item_slot') client.heldSlot=p.slot;
      if (meta.name === 'block_change') client.blocks.set(`${p.location.x},${p.location.y},${p.location.z}`,p.type);
      if (meta.name === 'multi_block_change') {
        for (const record of p.records) {
          const x=p.chunkCoordinates.x*16+((record>>>8)&15);
          const z=p.chunkCoordinates.z*16+((record>>>4)&15);
          const y=p.chunkCoordinates.y*16+(record&15);
          client.blocks.set(`${x},${y},${z}`,record>>>12);
          if (data.blocksByStateId[record>>>12]?.name === 'moving_piston') client.movingUpdates++;
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
    // Help is available before claiming a plot and does not change the simulation.
    const help=a.commandTree.nodes.find(node=>node.extraNodeData?.name==='help');
    assert(help,'General help appears in the command tree');
    await we('help','MCHPRS quick start');
    for(const [topic,text] of [['plots','/p auto'],['rtps','Tick control'],['we','WorldEdit'],['schematics','Schematics'],['pistons','/piston_anim off'],['rewind','Tick rewind'],['chat','/tellraw @a'],['redpiler','/rp compile'],['TICK_REWIND','/rhistory on'],['rback','/rback 10']]) {
      await we('help '+topic,text);
    }
    await we('help missing','Unknown help topic');
    await we('help we extra','Usage: /help [topic]');
    await we('/help paste','Help for /paste');
    const secondPosition=b.seen.position;
    command(b,'tp 300 30 100');
    await until(()=>b.seen.position>secondPosition,'second client changes plot');
    await we('say global message with  spaces','[PortSmokeOne] global message with  spaces');
    await until(()=>b.messages.some(m=>m.includes('global message with  spaces')),'say reaches another plot');
    await we('tellraw @a[distance=..1] ["",{"text":"Formatted broadcast","bold":true,"color":"red","hoverEvent":{"action":"show_text","contents":"ignored"}}]','Formatted broadcast');
    await until(()=>b.messages.some(m=>m.includes('Formatted broadcast')),'tellraw ignores selector filters across plots');
    const formatted=JSON.parse(a.messages.find(m=>m.includes('Formatted broadcast')));
    assert.equal(formatted.extra[0].color,'red'); assert.equal(formatted.extra[0].bold,1);
    assert(!JSON.stringify(formatted).includes('hoverEvent'));
    await we('tellraw @s "Self message"','Self message');
    await delay(100); assert(!b.messages.some(m=>m.includes('Self message')));
    command(a,'tellraw PortSmokeTwo {"text":"Named message","extra":[{"text":"child","italic":false}],"clickEvent":{"action":"run_command","value":"/stop"}}');
    await until(()=>b.messages.some(m=>m.includes('Named message')),'named tellraw');
    assert(!a.messages.some(m=>m.includes('Named message')));
    await we('tellraw @a {bad','Usage: /tellraw');
    await we('say still connected','still connected');
    command(b,'tp 128 128 128');
    await we('wsr','World send rate:');
    await we('wsr 0','successfully set');
    await we('wsr','World send rate: 0 Hz');
    await we('wsr 20','successfully set');
    await we('rtps 200000','successfully set');
    await we('rtps 20','successfully set');
    await we('rtps 101','successfully set');
    await we('wsr','effective 10 Hz');
    const actionsBefore=a.actions.length, movingBefore=a.movingUpdates;
    await creative(a,36,item('piston',[{type:'block_state',data:facing}]));
    await place(a,136,y,134,103);
    await creative(a,36,item('redstone_block'));
    await place(a,135,y,134,104);
    await until(()=>data.blocksByStateId[state(a,137,y,134)]?.name==='piston_head','fast static piston head');
    assert.equal(a.actions.length,actionsBefore,'high TPS sends no piston animation actions');
    assert.equal(a.movingUpdates,movingBefore,'high TPS sends no moving piston states');
    await we('piston_anim on','on (effective on)');
    await we('wsr','effective 20 Hz');
    await creative(a,36,item('piston',[{type:'block_state',data:facing}]));
    await place(a,150,y,134,107);
    await creative(a,36,item('redstone_block'));
    await place(a,149,y,134,108);
    await until(()=>a.actions.length>actionsBefore,'animation command overrides high TPS');
    await we('rtps 20','successfully set');
    await we('bisdon_anim off','off (effective off)');
    await we('wsr','effective 10 Hz');
    const offActions=a.actions.length;
    await creative(a,36,item('piston',[{type:'block_state',data:facing}]));
    await place(a,155,y,134,109);
    await creative(a,36,item('redstone_block'));
    await place(a,154,y,134,110);
    await until(()=>data.blocksByStateId[state(a,156,y,134)]?.name==='piston_head','forced static piston head');
    assert.equal(a.actions.length,offActions,'off command suppresses low TPS animations');
    await we('piston_anim auto','auto (effective on)');
    await we('wsr','effective 20 Hz');
    // Rewind capacity, piston round trip, disabled sends, two viewers and wraparound.
    const commandNodes=a.commandTree.nodes;
    assert(commandNodes.some(n=>n.extraNodeData?.name==='rhistory') && commandNodes.some(n=>n.extraNodeData?.name==='rback'),'rewind commands declared');
    for (const node of commandNodes) {
      for (const child of node.children) assert(child>=0 && child<commandNodes.length,'command child index');
    }
    await we('rtps 0','successfully set');
    await we('rhistory on','History enabled: up to');
    assert(a.messages.some(m=>m.includes('Estimated memory at full capacity:')),'enable memory projection');
    console.log('History memory estimate:',a.messages.at(-1));
    // Normal commands work without LuckPerms, but the cap bypass requires a grant.
    await we('rhistory on 1000','up to 1000 game ticks');
    await we('rhistory on 1001','requires plots.admin.rewind.unlimited');
    await we('rhistory status','Available: 0/1000 game ticks');
    await we('rhistory on 4','up to 4 game ticks');
    await we('rhistory on 0','capacity must be between');
    await we('rhistory status','Available: 0/4 game ticks');
    await we('radvance pico 1','Disable tick history');
    await creative(a,36,item('piston',[{type:'block_state',data:facing}]));
    await place(a,140,y,142,113);
    await creative(a,36,item('glass'));
    await place(a,141,y,142,114);
    await creative(a,36,item('redstone_block'));
    await place(a,139,y,142,115);
    await we('radvance 4','Plot has been advanced');
    await until(()=>data.blocksByStateId[state(a,141,y,142)]?.name==='piston_head','rewind fixture extended');
    await until(()=>data.blocksByStateId[state(b,141,y,142)]?.name==='piston_head','second viewer sees fixture');
    await we('rback 1001','requires plots.admin.rewind.unlimited');
    await we('rhistory status','Available: 4/4 game ticks');
    await we('rback 5','Only 4 game ticks');
    await we('wsr 0','successfully set');
    const beforeRefresh=[a.seen.map_chunk,b.seen.map_chunk];
    await we('rback 4','rewound by 4 game ticks and paused');
    await until(()=>a.seen.map_chunk>beforeRefresh[0] && b.seen.map_chunk>beforeRefresh[1],'rewind refreshes both viewers with wsr zero');
    for(const c of [a,b]) {
      assert.equal(data.blocksByStateId[state(c,141,y,142)]?.name,'glass','restore original payload');
      assert.equal(data.blocksByStateId[state(c,142,y,142)]?.name,'air','remove moved payload');
    }
    await we('rtps','(0)');
    await we('rhistory','Available: 0/4 game ticks');
    await we('radvance 4','Plot has been advanced');
    await we('wsr 20','successfully set');
    await until(()=>data.blocksByStateId[state(a,141,y,142)]?.name==='piston_head','re-advance reproduces piston');
    await we('radvance 3','Plot has been advanced');
    await we('rback','rewound by 1 game ticks and paused');
    await we('rback 3','rewound by 3 game ticks and paused');
    await we('rhistory','Available: 0/4 game ticks');
    await we('radvance 1','Plot has been advanced');
    await creative(a,36,item('gold_block'));
    assert(a.chunks.has('8,8'),'edited block is in a visible chunk');
    await place(a,143,y,142,116);
    await until(()=>data.blocksByStateId[state(a,143,y,142)]?.name==='gold_block','post-snapshot edit');
    await we('rback','rewound by 1 game ticks and paused');
    assert.equal(data.blocksByStateId[state(a,143,y,142)]?.name,'air','rewind undoes later edit');
    await we('rhistory off','Released approximately');
    await we('rhistory','Tick history: off');
    await we('rback','Tick history is disabled');
    // Automatic whole ticks use the same capture path as manual advancement.
    await we('rhistory on 4','up to 4 game ticks');
    await we('rtps 20','successfully set');
    await delay(400);
    await we('rtps 0','successfully set');
    await we('rhistory','Available: 4/4 game ticks');
    await we('rback 2','rewound by 2 game ticks and paused');
    await we('rhistory off','Released approximately');
    await we('rtps 20','successfully set');
    // Creative command-block placement, editor update and a single redstone activation.
    a.write('position',{x:142,y,z:136,flags:{onGround:false,hasHorizontalCollision:false}});
    a.write('held_item_slot',{slotId:0});
    await creative(a,36,item('command_block'));
    await place(a,142,y,138,105);
    await until(()=>data.blocksByStateId[state(a,142,y,138)]?.name==='command_block','command block placement');
    a.write('update_command_block',{location:{x:142,y,z:138},command:'tellraw @a[distance=..1] {"text":"Command smoke","bold":true,"color":"gold","clickEvent":{"action":"run_command","value":"/stop"}}',mode:2,flags:1});
    await until(()=>a.entities.some(p=>p.location.x===142 && p.location.y===y && p.location.z===138 && nbt.simplify(p.nbtData).Command?.includes('Command smoke')),'command editor data update');
    await creative(a,36,item('redstone_block'));
    await place(a,141,y,138,106);
    await until(()=>a.messages.some(m=>m.includes('Command smoke')) && b.messages.some(m=>m.includes('Command smoke')),'powered command block broadcasts');
    const commandsBefore=a.messages.filter(m=>m.includes('Command smoke')).length;
    await delay(200);
    assert.equal(a.messages.filter(m=>m.includes('Command smoke')).length,commandsBefore,'powered impulse runs once');
    a.write('update_command_block',{location:{x:142,y,z:138},command:'scoreboard players add x y 1',mode:2,flags:1});
    await until(()=>a.entities.some(p=>p.location.x===142 && nbt.simplify(p.nbtData).Command==='scoreboard players add x y 1'),'unsupported command retained');
    a.write('update_command_block',{location:{x:142,y,z:138},command:'say saved command',mode:2,flags:1});
    await until(()=>a.entities.some(p=>p.location.x===142 && nbt.simplify(p.nbtData).Command==='say saved command'),'saved command data');
    await select([142,y,138],[142,y,138]);
    await we('/copy','selection was copied');
    await we('/save CommandRoundtrip.schem','saved sucessfuly');
    await we('/load CommandRoundtrip.schem','loaded to your clipboard');
    a.write('position',{x:148,y,z:138,flags:{onGround:false,hasHorizontalCollision:false}});
    await we('/paste','clipboard was pasted');
    await until(()=>a.entities.some(p=>p.location.x===148 && p.location.y===y && p.location.z===138 && nbt.simplify(p.nbtData).Command==='say saved command'),'schematic retains command text');
    command(a,'/undo'); await until(()=>state(a,148,y,138)===0,'command schematic undo');
    command(a,'/redo'); await until(()=>data.blocksByStateId[state(a,148,y,138)]?.name==='command_block','command schematic redo');
    await creative(a,36,item('redstone_block'));
    await place(a,147,y,138,111);
    await until(()=>a.messages.some(m=>m.includes('[@] saved command')),'imported command runs on power');
    a.write('position',{x:130,y,z:129,flags:{onGround:false,hasHorizontalCollision:false}});
    a.write('pick_item_from_block',{position:{x:130,y,z:130},includeData:false});
    await until(()=>a.heldSlot===2 && a.slots.get(38)?.itemId===data.itemsByName.piston.id,'middle click creates/selects piston');
    const slotChanges=a.seen.set_slot;
    a.write('pick_item_from_block',{position:{x:130,y,z:130},includeData:false});
    await delay(150);
    assert.equal(a.seen.set_slot,slotChanges,'repeated pick reuses existing stack');
    a.write('pick_item_from_block',{position:{x:129,y,z:130},includeData:false});
    await until(()=>a.heldSlot===0,'middle click selects existing redstone hotbar stack');
    a.write('position',{x:100,y:30,z:100,flags:{onGround:false,hasHorizontalCollision:false}});
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
    a.write('position',{x:120,y:28,z:91,flags:{onGround:false,hasHorizontalCollision:false}});
    a.write('pick_item_from_block',{position:{x:120,y:29,z:92},includeData:true});
    await until(()=>a.heldSlot===3 && a.slots.get(39)?.components?.some(c=>c.type==='block_entity_data'),'Ctrl-middle click retains sign text');
    a.write('position',{x:100,y:30,z:100,flags:{onGround:false,hasHorizontalCollision:false}});
    command(a,'/undo'); await until(()=>state(a,120,29,92)===0,'WorldEdit undo');
    command(a,'/redo'); await until(()=>state(a,120,29,92)===signState,'WorldEdit redo');
    await we('/help rs','Like //stack');
    await we('/load BadInput.schem','Could not load schematic:');
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
    await we('piston_anim off','off (effective off)');
    // Keep recording enabled across shutdown: present state persists, history does not.
    await we('rtps 0','successfully set');
    await we('rhistory on 4','up to 4 game ticks');
    await we('radvance 2','Plot has been advanced');
    await we('rback','rewound by 1 game ticks and paused');
    a.end(); await delay(600);
    const again=await connect('PortSmokeOne');
    assert.equal(again.slots.get(37).components[1].data,16);
    command(again,'rtps');
    command(again,'piston_anim');
    await until(()=>again.messages.some(m=>m.includes('Piston animation: off')),'animation choice survives reconnect');
    console.log('PASS: configuration, two players, deep chunks, version/rate/selection commands, combined flags, overlapping undo/redo, components, placement, piston events, acknowledgements, Sponge v3 and production mixed-sign v2 paste/undo/redo/save/reload and reconnect.');
    stopping=true; command(again,'stop'); await delay(800);
  } else {
    assert.equal(a.slots.get(37).components[1].data,16);
    const pstate=state(a,x,y,z);
    assert.equal(data.blocksByStateId[pstate].name,'piston');
    assert.equal(data.blocksByStateId[state(a,x+1,y,z)].name,'piston_head');
    assert.equal(data.blocksByStateId[state(a,142,y,138)].name,'command_block');
    assert(a.chunkEntities.some(e=>e.Command==='say saved command'),'restart retained command text');
    command(a,'bisdon_anim');
    await until(()=>a.messages.some(m=>m.includes('Piston animation: off')),'animation choice survives process restart');
    command(a,'rhistory');
    await until(()=>a.messages.some(m=>m.includes('Tick history: off') && m.includes('Available: 0/0')),'restart clears history');
    assert.equal(data.blocksByStateId[state(a,141,y,142)].name,'piston_head','rewound present persisted');
    // A compile boundary must discard interpreter history, even with no compiled ticks.
    command(b,'tp 300 30 100');
    await delay(250);
    const commandB=async (cmd,text)=>{
      const start=b.messages.length; command(b,cmd);
      await until(()=>b.messages.slice(start).some(m=>m.includes(text)),cmd+' on second plot');
    };
    await commandB('rtps 0','successfully set');
    await commandB('rhistory on 2','up to 2 game ticks');
    await commandB('radvance 1','Plot has been advanced');
    await commandB('rp compile','Tick history disabled because compiled execution is starting');
    await commandB('rhistory on','only available during interpreted execution');
    await commandB('rback','only available during interpreted execution');
    command(b,'rp reset');
    await commandB('rhistory','Tick history: off');
    console.log('PASS: process restart retained structured item components and extended piston/head states.');
    stopping=true;command(a,'stop');await delay(800);
  }
  for(const c of clients)c.end();
  process.exit(failed?1:0);
})().catch(e=>{console.error(e);for(const c of clients)c.end();process.exit(1);});
setTimeout(()=>{console.error('TIMEOUT');process.exit(1);},40000);
