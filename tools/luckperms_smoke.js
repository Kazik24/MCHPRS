// Only use against the isolated loopback server created by the runner.
const mc = require('minecraft-protocol');
const nbt = require('prismarine-nbt');
const fs = require('fs');
const assert = require('assert/strict');
const accounts = JSON.parse(fs.readFileSync(process.env.MCHPRS_LUCKPERMS_TEST_ACCOUNTS, 'utf8'));
const port = Number(process.argv[2]);
const clients = [];
const delay = ms => new Promise(resolve => setTimeout(resolve, ms));
async function until(predicate, description) {
  for (let i = 0; i < 200; i++) { if (predicate()) return; await delay(50); }
  throw Error('Timeout: ' + description);
}
async function connect(rank) {
  const uuid = accounts[rank].replaceAll('-', '');
  const client = mc.createClient({host: '127.0.0.1', port, auth: 'offline',
    username: 'LP_' + rank, version: '1.21.5',
    fakeHost: 'localhost\0' + '127.0.0.1\0' + uuid});
  clients.push(client); client.messages = []; client.ready = false;
  client.on('packet', (packet, metadata) => {
    if (metadata.name === 'system_chat') client.messages.push(JSON.stringify(nbt.simplify(packet.content)));
    if (metadata.name === 'position') client.write('teleport_confirm', {teleportId: packet.teleportId});
    if (metadata.name === 'map_chunk') client.ready = true;
  });
  client.on('error', error => { client.failure = error; });
  client.on('kick_disconnect', packet => { client.failure = Error(JSON.stringify(packet)); });
  await until(() => client.ready || client.failure, rank + ' login');
  if (client.failure) throw client.failure;
  return client;
}
async function command(client, text, expected) {
  const start = client.messages.length;
  client.write('chat_command', {command: text, timestamp: BigInt(Date.now()), salt: 0n,
    argumentSignatures: [], messageCount: 0, acknowledged: Buffer.alloc(3)});
  try {
    await until(() => client.messages.slice(start).some(message => expected.test(message)), text);
  } catch (error) {
    throw Error(error.message + ': ' + client.messages.slice(start).join('; '));
  }
}
(async () => {
  const ordinary = await connect('default');
  await command(ordinary, 'p info', /No paws on this plot|pawprint on this plot/);
  await command(ordinary, 'stop', /do not have permission|paws don't have permission/);
  await command(ordinary, 'whitelist add LPTest', /do not have permission|paws don't have permission/);
  await command(ordinary, '/copy', /do not have permission|paws don't have permission/);
  await command(ordinary, 'p auto', /Plot .* is your den now/);
  await command(ordinary, 'rhistory on 2', /Recording up to 2 game ticks/);
  await command(ordinary, 'rhistory limit 512', /need plots.admin.rewind.memory permission/);
  await command(ordinary, 'rhistory on 1001', /1000/);
  const builder = await connect('builder');
  await command(builder, 'p auto', /Plot .* is your den now/);
  await command(builder, '/pos1', /First paw/);
  await command(builder, '/pos2', /Second paw/);
  await command(builder, '/copy', /copied/);
  await command(builder, 'stop', /do not have permission|paws don't have permission/);
  const admin = await connect('admin');
  admin.write('chat_command', {command: 'stop', timestamp: BigInt(Date.now()), salt: 0n,
    argumentSignatures: [], messageCount: 0, acknowledged: Buffer.alloc(3)});
  await delay(500);
  console.log('PASS: forwarded UUIDs, default plot pack/rewind, scoped builder WorldEdit, and staff-only administration.');
})().catch(error => {console.error(error); process.exitCode = 1;}).finally(() => {
  for (const client of clients) client.end();
});
