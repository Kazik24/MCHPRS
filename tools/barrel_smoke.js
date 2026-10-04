// Independent protocol-770 barrel inventory regression. See tools/README.md.
const mc = require('minecraft-protocol');
const data = require('minecraft-data')('1.21.5');
const Chunk = require('prismarine-chunk')('1.21.5');
const Block = require('prismarine-block')('1.21.5');
const { Vec3 } = require('vec3');
const nbt = require('prismarine-nbt');
const assert = require('assert/strict');
const port = Number(process.argv[2] || 25586);
const restart = process.argv.includes('--restart');
const clients = [];
let stopping = false;
let failure;
let sequence = 500;
const delay = ms => new Promise(resolve => setTimeout(resolve, ms));
async function until(predicate, description) {
  for (let i = 0; i < 100; i++) {
    if (failure) throw failure;
    if (predicate()) return;
    await delay(30);
  }
  throw Error('Timeout: ' + description);
}
async function connect(name) {
  const c = mc.createClient({host: '127.0.0.1', port, username: name, version: '1.21.5', auth: 'offline'});
  clients.push(c);
  c.chunks = new Map(); c.blocks = new Map(); c.inventory = new Map(); c.menus = new Map();
  c.messages = []; c.acks = []; c.currentMenu = null; c.contentCount = 0; c.slotCount = 0;
  c.on('packet', (p, m) => {
    try {
      if (m.name === 'position') c.write('teleport_confirm', {teleportId: p.teleportId});
      if (m.name === 'system_chat') c.messages.push(JSON.stringify(nbt.simplify(p.content)));
      if (m.name === 'map_chunk') {
        const chunk = new Chunk({minY: 0, worldHeight: 256}); chunk.load(p.chunkData);
        for (const key of c.blocks.keys()) {
          const [x,,z] = key.split(',').map(Number);
          if (Math.floor(x/16) === p.x && Math.floor(z/16) === p.z) c.blocks.delete(key);
        }
        c.chunks.set(`${p.x},${p.z}`, chunk);
      }
      if (m.name === 'block_change') c.blocks.set(`${p.location.x},${p.location.y},${p.location.z}`, p.type);
      if (m.name === 'multi_block_change') for (const record of p.records) {
        c.blocks.set(`${p.chunkCoordinates.x*16+((record>>>8)&15)},${p.chunkCoordinates.y*16+(record&15)},${p.chunkCoordinates.z*16+((record>>>4)&15)}`, record>>>12);
      }
      if (m.name === 'open_window') { c.currentMenu = p.windowId; c.opened = p; }
      if (m.name === 'close_window' && c.currentMenu === p.windowId) c.currentMenu = null;
      if (m.name === 'window_items') {
        c.contentCount++;
        if (p.windowId === 0) p.items.forEach((item, slot) => c.inventory.set(slot, item));
        else c.menus.set(p.windowId, p);
      }
      if (m.name === 'set_slot') {
        c.slotCount++;
        if (p.windowId === 0) c.inventory.set(p.slot, p.item);
      }
      if (m.name === 'acknowledge_player_digging') c.acks.push(p.sequenceId);
    } catch (error) { failure = error; }
  });
  c.on('error', error => { if (!stopping) failure = error; });
  c.on('kick_disconnect', p => { if (!stopping) failure = Error('Unexpected disconnect: '+JSON.stringify(p)); });
  await until(() => c.chunks.has('8,8'), 'join '+name);
  return c;
}
function state(c, x, y, z=130) {
  return c.blocks.get(`${x},${y},${z}`) ?? c.chunks.get(`${Math.floor(x/16)},${Math.floor(z/16)}`)?.getBlockStateId(new Vec3(x&15,y,z&15));
}
const props = (c,x,y,z) => Block.fromStateId(state(c,x,y,z), 0).getProperties();
const count = slot => slot?.itemCount || 0;
const sum = slots => slots.reduce((total, slot) => total + count(slot), 0);
const menu = c => c.menus.get(c.currentMenu);
function move(c,x,y,z=130) { c.write('position',{x,y,z,flags:{onGround:false,hasHorizontalCollision:false}}); }
async function cmd(c, command, text) {
  const start = c.messages.length;
  c.write('chat_command', {command});
  await until(() => c.messages.slice(start).some(m => m.includes(text)), command+' response');
}
function item(name, itemCount=1, components=[]) {
  return {itemCount,itemId:data.itemsByName[name].id,addedComponentCount:components.length,
    removedComponentCount:0,components,removeComponents:[]};
}
async function creative(c, slot, item) {
  const previous = c.slotCount;
  c.write('set_creative_slot', {slot,item});
  await until(() => c.slotCount > previous, 'creative inventory echo');
}
async function use(c,x,y,z=130,place=false) {
  const seq = sequence++;
  c.write('block_place',{hand:0,location:{x,y:place?y-1:y,z},direction:1,
    cursorX:.5,cursorY:1,cursorZ:.5,insideBlock:false,worldBorderHit:false,sequence:seq});
  await until(() => c.acks.includes(seq), 'use acknowledgement');
}
async function open(c,x,y,z=130,type=2,size=27) {
  await use(c,x,y,z);
  await until(() => c.currentMenu !== null && c.menus.has(c.currentMenu), 'container opens');
  assert.equal(c.opened.inventoryType, type, 'correct container menu');
  assert.equal(menu(c).items.length, size+36, 'container + 36 player slots');
}
async function close(c) {
  const id = c.currentMenu;
  c.write('close_window', {windowId:id});
  await until(() => c.currentMenu === null, 'menu close');
}
async function click(c,slot,mouseButton=0,mode=0,changedSlots=[]) {
  const previous = c.contentCount;
  c.write('window_click',{windowId:c.currentMenu,stateId:menu(c).stateId,
    slot,mouseButton,mode,changedSlots,cursorItem:undefined});
  await until(() => c.contentCount > previous, 'authoritative click result');
}
async function setBlock(c,x,y,name,z=130) {
  move(c,x,y,z);
  await cmd(c,'/pos1','First position'); await cmd(c,'/pos2','Second position');
  await cmd(c,'/set '+name,'Operation completed');
}
(async () => {
  const a = await connect('BarrelSmokeOne');
  const b = await connect('BarrelSmokeTwo');
  let floor = 0;
  for (let y=0;y<256;y++) if (data.blocksByStateId[state(a,128,y,128)]?.name !== 'air') floor=y;
  const y = floor+1;
  move(a,128,y,128); move(b,128,y,128);
  a.write('held_item_slot',{slotId:0}); b.write('held_item_slot',{slotId:0});
  if (restart) {
    assert.equal(props(a,130,y).open, false, 'lid closed on shutdown');
    assert.equal(sum([...a.inventory.values()]), 63, 'cursor returned before player save');
    await open(a,130,y);
    assert.equal(sum(menu(a).items.slice(0,27)),32,'edited barrel survived restart');
    assert.equal(count(menu(a).carriedItem),0);
    assert(menu(a).items[2].components.some(c => c.type === 'unbreakable'), 'stored item components survived restart');
    await close(a);
    for (const [x,type,size,facing] of [[146,16,5,'east'],[150,14,3,'west']]) {
      move(a,x,y);
      await open(a,x,y,130,type,size);
      assert.equal(count(menu(a).items[0]),64,'hopper/furnace contents survive restart');
      assert(menu(a).items[0].components.some(c => c.type === 'unbreakable'));
      assert.equal(props(a,x,y).facing,facing);
      await close(a);
    }
  } else {
    await cmd(a,'rtps 0','successfully set');
    a.write('look',{yaw:90,pitch:0,flags:{onGround:false,hasHorizontalCollision:false}});
    await creative(a,36,item('barrel'));
    await creative(a,37,item('redstone',64,[{type:'unbreakable',data:Buffer.alloc(0)}]));
    await use(a,130,y,130,true);
    await until(() => data.blocksByStateId[state(a,130,y)]?.name === 'barrel','barrel placed');
    assert.equal(props(a,130,y).facing,'east','placement follows look direction');
    await creative(a,36,{itemCount:0});
    await open(a,130,y);
    assert.equal(sum(menu(a).items.slice(0,27)),0,'ordinary barrel starts empty');
    assert.equal(count(menu(a).items[55]),64,'player inventory is included');
    await until(() => props(b,130,y).open === true,'second viewer sees lid');
    await click(a,55); assert.equal(count(menu(a).carriedItem),64);
    await click(a,0); assert.equal(count(menu(a).items[0]),64);
    assert(menu(a).items[0].components.some(c => c.type === 'unbreakable'));
    const fake = {itemId:data.itemsByName.diamond.id,itemCount:64,components:[],removeComponents:[]};
    await click(a,-1,0,0,[{location:1,item:fake}]);
    assert.equal(count(menu(a).items[1]),0,'client predictions cannot create items');
    await close(a);
    await setBlock(a,131,y,'comparator[facing=west]');
    await setBlock(a,132,y,'redstone_wire'); move(a,128,y,128);
    // WorldEdit does not run placement updates; a lid change notifies the new circuit.
    await open(a,130,y); await close(a);
    await cmd(a,'radvance 4','Plot has been advanced');
    await until(() => Number(props(a,132,y).power) === 1,'filled barrel updates comparator');
    await open(a,130,y); await open(b,130,y);
    await click(b,0,0,1);
    await until(() => count(menu(a).items[0]) === 0,'other viewer inventory updates');
    await cmd(a,'radvance 4','Plot has been advanced');
    await until(() => Number(props(a,132,y).power) === 0,'removing items updates comparator');
    await click(b,62,0,1);
    await until(() => count(menu(a).items[0]) === 64,'shift transfer back into barrel');
    await close(a);
    await until(() => props(a,130,y).open === true,'lid stays open for remaining viewer');
    await close(b);
    await until(() => props(a,130,y).open === false,'last viewer closes lid');
    // Preserve facing, inventory and components through WorldEdit and schematic files.
    move(a,130,y);
    await cmd(a,'/pos1','First position'); await cmd(a,'/pos2','Second position');
    await cmd(a,'/copy','selection was copied');
    await cmd(a,'/save BarrelRoundtrip.schem','saved sucessfuly');
    await cmd(a,'/load BarrelRoundtrip.schem','loaded to your clipboard');
    move(a,136,y); await cmd(a,'/paste','clipboard was pasted');
    await until(() => data.blocksByStateId[state(a,136,y)]?.name === 'barrel','barrel paste');
    assert.equal(props(a,136,y).facing,'east');
    await open(a,136,y);
    assert.equal(count(menu(a).items[0]),64);
    assert(menu(a).items[0].components.some(c => c.type === 'unbreakable'));
    await close(a);
    // /container and creative block-entity data must still produce filled barrels.
    const previous = a.slotCount;
    a.write('chat_command',{command:'container barrel 15'});
    await until(() => a.slotCount > previous,'filled container item');
    move(a,138,y); await use(a,140,y,130,true);
    await creative(a,36,{itemCount:0}); await open(a,140,y);
    assert.equal(sum(menu(a).items.slice(0,27)),1728);
    await close(a);
    // Non-default hopper/furnace states need real entities, menus and comparator data.
    for (const [name,x,type,size,power] of [
      ['hopper[facing=east,enabled=false]',146,16,5,3],
      ['furnace[facing=west,lit=true]',150,14,3,5],
    ]) {
      const plainName = name.split('[')[0];
      move(a,x-2,y);
      await creative(a,36,item(plainName));
      await use(a,x,y,130,true);
      await until(() => data.blocksByStateId[state(a,x,y)]?.name === plainName,'ordinary '+plainName+' placement');
      await creative(a,36,{itemCount:0});
      await open(a,x,y,130,type,size);
      assert.equal(sum(menu(a).items.slice(0,size)),0,'ordinary placement opens empty '+plainName);
      await close(a);
      await setBlock(a,x,y,name);
      await creative(a,38,item('redstone',64,[{type:'unbreakable',data:Buffer.alloc(0)}]));
      await open(a,x,y,130,type,size);
      assert.equal(sum(menu(a).items.slice(0,size)),0,'empty entity created for '+name);
      await click(a,size+29); await click(a,0);
      if (type === 14) {
        await click(a,0); await click(a,2); await click(a,1);
        assert.equal(count(menu(a).items[1]),0,'invalid furnace fuel rejected');
        assert.equal(count(menu(a).items[2]),0,'output cannot accept carried items');
        await click(a,0);
      }
      await close(a);
      await setBlock(a,x+1,y,'comparator[facing=west]');
      await setBlock(a,x+2,y,'redstone_wire');
      move(a,x,y); await open(a,x,y,130,type,size);
      await click(a,0); await click(a,0); await close(a);
      await cmd(a,'radvance 4','Plot has been advanced');
      await until(() => Number(props(a,x+2,y).power) === power,'container comparator '+name);
      move(a,x,y);
      await cmd(a,'/pos1','First position'); await cmd(a,'/pos2','Second position');
      await cmd(a,'/copy','selection was copied');
      await cmd(a,'/save ContainerRoundtrip.schem','saved sucessfuly');
      await cmd(a,'/load ContainerRoundtrip.schem','loaded to your clipboard');
      move(a,x,y,134); await cmd(a,'/paste','clipboard was pasted');
      await open(a,x,y,134,type,size);
      assert.equal(count(menu(a).items[0]),64,'schematic inventory '+name);
      assert(menu(a).items[0].components.some(c => c.type === 'unbreakable'));
      assert.equal(props(a,x,y,134).facing,type === 16?'east':'west');
      await click(a,0); await click(a,size+29); await close(a);
      await creative(a,38,{itemCount:0});
    }
    // Cake is an interactable comparator source with seven bites, not a container.
    await setBlock(a,156,y,'cake');
    await setBlock(a,157,y,'comparator[facing=west]');
    await setBlock(a,158,y,'redstone_wire');
    move(a,156,y);
    for (let bite=1; bite<=7; bite++) {
      await use(a,156,y);
      if (bite < 7) assert.equal(Number(props(a,156,y).bites),bite);
      else await until(() => data.blocksByStateId[state(a,156,y)]?.name === 'air','last bite removes cake');
      await cmd(a,'radvance 4','Plot has been advanced');
      await until(() => Number(props(a,158,y).power) === 14-2*bite,'cake comparator bite '+bite);
      assert.equal(a.currentMenu,null,'cake never opens an inventory');
    }
    // Split, single placement and drag distribution keep the original 64 items.
    move(a,128,y,128); await open(a,130,y);
    await click(a,0,1); assert.equal(count(menu(a).carriedItem),32);
    await click(a,1,1); await click(a,0);
    assert.equal(count(menu(a).items[0]),63);
    await click(a,0);
    await click(a,-999,0,5); await click(a,0,1,5); await click(a,2,1,5); await click(a,-999,2,5);
    assert.equal(count(menu(a).items[0]),31); assert.equal(count(menu(a).items[2]),31);
    assert.equal(count(menu(a).carriedItem),1); await close(a);
    assert.equal(sum([...a.inventory.values()]),1,'carried item returned on close');
    // Rewind replaces world inventories and closes screens with current cursors safely.
    await cmd(a,'rhistory on 2','up to 2 game ticks');
    await cmd(a,'radvance 1','Plot has been advanced');
    await open(a,130,y); await click(a,0);
    await cmd(a,'rback','rewound by 1 game ticks and paused');
    await until(() => a.currentMenu === null,'rewind closes inventory');
    await cmd(a,'rhistory off','Released approximately');
    await open(a,130,y); assert.equal(sum(menu(a).items.slice(0,27)),63);
    await close(a);
    // Leaving reach and deleting a barrel must close the menu.
    await open(b,130,y); move(b,160,y);
    await until(() => b.currentMenu === null,'distant viewer closes');
    move(b,136,y); await open(b,136,y);
    await setBlock(a,136,y,'air');
    await until(() => b.currentMenu === null,'removed barrel closes');
    move(a,128,y,128); move(b,128,y,128);
    await open(a,130,y);
    a.end(); await until(() => props(b,130,y).open === false,'disconnect closes lid');
    const reconnected = await connect('BarrelSmokeOne'); move(reconnected,128,y,128);
    await open(reconnected,130,y); await click(reconnected,0);
    assert.equal(count(menu(reconnected).carriedItem),31);
    // Stop with an item on the cursor to check save-before-close ordering on restart.
    clients.splice(clients.indexOf(a),1);
  }
  const active = clients.find(c => c.username === 'BarrelSmokeOne');
  stopping = true; active.write('chat_command',{command:'stop'});
  await delay(200); for (const c of clients) c.end();
  console.log('PASS: '+(restart?'barrel/hopper/furnace inventories, item components, closed lids and returned cursor survive restart.':
    'barrel/hopper/furnace menus, transfers, comparator updates, furnace slot rules, cake bites, two viewers, prediction rejection, schematic data, rewind and menu lifecycle.'));
})().catch(error => { console.error(error); for (const c of clients) c.end(); process.exitCode=1; });
