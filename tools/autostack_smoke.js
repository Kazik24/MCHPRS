// Run after cargo build and npm ci --prefix tools --ignore-scripts.
// Starts an isolated server and verifies /autostack with real protocol packets.
const assert = require('assert/strict');
const fs = require('fs');
const os = require('os');
const path = require('path');
const { spawn } = require('child_process');
const mc = require('minecraft-protocol');
const data = require('minecraft-data')('1.21.5');
const Chunk = require('prismarine-chunk')('1.21.5');
const nbt = require('prismarine-nbt');
const { Vec3 } = require('vec3');
const port = 25584;
const delay = ms => new Promise(resolve => setTimeout(resolve, ms));
async function until(predicate, description) {
  for (let i = 0; i < 200; i++) {
    if (predicate()) return;
    await delay(50);
  }
  throw Error('Timeout: ' + description);
}
const directory = fs.mkdtempSync(path.join(os.tmpdir(), 'mchprs-autostack-'));
fs.writeFileSync(path.join(directory, 'Config.toml'), `bind_address = "127.0.0.1:${port}"\nview_distance = 2\nauto_redpiler = false\n`);
const binary = path.resolve(__dirname, '../target/debug', process.platform === 'win32' ? 'mchprs.exe' : 'mchprs');
const server = spawn(binary, [], { cwd: directory, windowsHide: true });
let log = '', client, failure, sequence = 500;
server.stdout.on('data', bytes => { log += bytes; });
server.stderr.on('data', bytes => { log += bytes; });
server.on('error', error => { failure = error; });
async function connect() {
  const c = mc.createClient({ host: '127.0.0.1', port, username: 'AutoStackSmoke', auth: 'offline', version: '1.21.5' });
  client = c;
  c.messages = []; c.blocks = new Map(); c.chunks = new Map(); c.acks = new Set(); c.positions = 0; c.slotChanges = 0;
  c.on('error', error => { failure = error; });
  c.on('packet', (packet, meta) => {
    try {
      if (meta.name === 'position') { c.positions++; c.write('teleport_confirm', { teleportId: packet.teleportId }); }
      if (meta.name === 'system_chat') c.messages.push(JSON.stringify(nbt.simplify(packet.content)));
      if (meta.name === 'declare_commands') c.tree = packet;
      if (meta.name === 'set_slot') c.slotChanges++;
      if (meta.name === 'acknowledge_player_digging') c.acks.add(packet.sequenceId);
      if (meta.name === 'map_chunk') {
        const chunk = new Chunk({ minY: 0, worldHeight: 256 }); chunk.load(packet.chunkData);
        for (const key of c.blocks.keys()) {
          const [x,,z] = key.split(',').map(Number);
          if (Math.floor(x / 16) === packet.x && Math.floor(z / 16) === packet.z) c.blocks.delete(key);
        }
        c.chunks.set(`${packet.x},${packet.z}`, chunk);
      }
      if (meta.name === 'block_change') c.blocks.set(`${packet.location.x},${packet.location.y},${packet.location.z}`, packet.type);
      if (meta.name === 'multi_block_change') for (const record of packet.records) {
        c.blocks.set(`${packet.chunkCoordinates.x * 16 + ((record >>> 8) & 15)},${packet.chunkCoordinates.y * 16 + (record & 15)},${packet.chunkCoordinates.z * 16 + ((record >>> 4) & 15)}`, record >>> 12);
      }
    } catch (error) { failure = error; }
  });
  await until(() => c.chunks.size && c.tree, 'join');
  return c;
}
function state(x, y = 30, z = 80) {
  return client.blocks.get(`${x},${y},${z}`) ?? client.chunks.get(`${Math.floor(x / 16)},${Math.floor(z / 16)}`)?.getBlockStateId(new Vec3(x & 15, y, z & 15));
}
function send(command) { client.write('chat_command', { command }); }
async function command(command, message) {
  const start = client.messages.length; send(command);
  try {
    await until(() => client.messages.slice(start).some(text => text.includes(message)), command + ': ' + message);
  } catch (error) { throw Error(error.message + ': ' + JSON.stringify(client.messages.slice(start))); }
}
function move(x, y = 30, z = 80) { client.write('position', { x, y, z, flags: { onGround: false, hasHorizontalCollision: false } }); }
async function select(first, second) {
  move(first); await command('/pos1', 'First paw set');
  move(second); await command('/pos2', 'Second paw set');
  move(75, 30, 75);
}
async function place(x) {
  move(x, 30, 78);
  const id = sequence++;
  client.write('block_place', { hand: 0, location: { x, y: 29, z: 80 }, direction: 1, cursorX: 0.5, cursorY: 1, cursorZ: 0.5, insideBlock: false, worldBorderHit: false, sequence: id });
  await until(() => client.acks.has(id), 'placement acknowledgement');
  await delay(100);
}
async function destroy(x) {
  move(x, 30, 78);
  const id = sequence++;
  client.write('block_dig', { status: 0, location: { x, y: 30, z: 80 }, face: 1, sequence: id });
  await until(() => client.acks.has(id), 'break acknowledgement');
  await until(() => state(x) === 0, 'source removed');
}
(async () => {
  await delay(1500);
  if (failure) throw failure;
  await connect();
  assert(client.tree.nodes.some(node => node.extraNodeData?.name === 'autostack'));
  await command('p claim', 'your den');
  send('rtps 0'); await delay(100);
  await select(80, 95); await command('/set air', 'Operation complete');
  await select(80, 82);
  await command('autostack east 2 4', 'Enabled auto stack');
  const previous = client.slotChanges;
  client.write('set_creative_slot', { slot: 36, item: { itemCount: 1, itemId: data.itemsByName.stone.id, addedComponentCount: 0, removedComponentCount: 0, components: [], removeComponents: [] } });
  await until(() => client.slotChanges > previous, 'creative slot');
  client.write('held_item_slot', { slotId: 0 });
  await select(90, 90); // Selection changes cannot retarget the active session.
  await place(80);
  await until(() => [80, 84, 88].every(x => state(x) === data.blocksByName.stone.minStateId), 'automatic copies');
  await place(83); assert.equal(state(87), 0, 'outside source selection');
  await place(90); assert.equal(state(94), 0, 'changed selection is not the source');
  await command('autostack east 2 0', 'nonzero spacing');
  await command('autostack east 100 4', 'inside');
  await place(81);
  await until(() => [85, 89].every(x => state(x) === data.blocksByName.stone.minStateId), 'failed reconfiguration preserves session');
  await destroy(80); await until(() => state(84) === 0 && state(88) === 0, 'automatic removals');
  await command('autostack off', 'Auto stack disabled');
  await place(80); assert.equal(state(84), 0, 'off stops copies');
  await select(80, 82); await command('autostack east 2 4', 'Enabled auto stack');
  let positions = client.positions; send('tp 300 30 80');
  await until(() => client.positions > positions, 'leave plot');
  positions = client.positions; send('tp 75 30 75');
  await until(() => client.positions > positions, 'return to plot');
  await until(() => client.chunks.has('5,5'), 'source chunk after return');
  await place(82); assert.equal(state(86), 0, 'leaving clears session');
  await command('autostack east 2 4', 'Enabled auto stack');
  client.end(); await delay(500); await connect(); move(75, 30, 75);
  await destroy(81); assert.equal(state(85), data.blocksByName.stone.minStateId, 'disconnect clears session');
  if (failure) throw failure;
  console.log('PASS: autostack registration, placements, removals, fixed selection, bounds, off, plot leave and reconnect.');
  send('stop'); await until(() => server.exitCode !== null, 'clean server shutdown');
  assert.equal(server.exitCode, 0);
})().catch(error => { console.error(error, log.slice(-4000)); process.exitCode = 1; }).finally(async () => {
  if (client) client.end();
  if (server.exitCode === null && server.signalCode === null) {
    server.kill(); await until(() => server.exitCode !== null || server.signalCode !== null, 'server termination');
  }
  fs.rmSync(directory, { recursive: true, force: true });
});
