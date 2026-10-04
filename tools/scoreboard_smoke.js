// Track objective lifetimes as a Minecraft client does, rather than only decoding packets.
const mc = require('minecraft-protocol');
const nbt = require('prismarine-nbt');
const assert = require('assert/strict');
const port = Number(process.argv[2] || 25585);
const clients = [];
let stopping = false;
const delay = ms => new Promise(resolve => setTimeout(resolve, ms));
async function until(predicate, description) {
  for (let i = 0; i < 100; i++) {
    if (predicate()) return;
    await delay(50);
  }
  throw Error('Timeout: ' + description);
}
async function connect(username) {
  const client = mc.createClient({host:'127.0.0.1',port,username,auth:'offline',version:'1.21.5'});
  clients.push(client);
  client.objectives = new Set(); client.creates = 0; client.removals = 0;
  client.positions = 0; client.messages = [];
  client.on('packet', (packet, meta) => {
    try {
      if (meta.name === 'position') {
        client.positions++;
        client.write('teleport_confirm', {teleportId:packet.teleportId});
      }
      if (meta.name === 'system_chat') client.messages.push(JSON.stringify(nbt.simplify(packet.content)));
      if (meta.name === 'scoreboard_objective') {
        if (packet.action === 0) {
          assert(!client.objectives.has(packet.name), `${username}: duplicate objective ${packet.name}`);
          client.objectives.add(packet.name); client.creates++;
        } else if (packet.action === 1) {
          assert(client.objectives.delete(packet.name), `${username}: missing objective removal`);
          client.removals++;
        } else assert(client.objectives.has(packet.name), `${username}: update before creation`);
      }
      if (meta.name === 'scoreboard_score') assert(client.objectives.has(packet.scoreName), 'Score before objective creation');
    } catch (error) { console.error(error); process.exit(1); }
  });
  client.on('error', error => { if (!stopping) { console.error(error); process.exit(1); } });
  client.on('kick_disconnect', packet => { if (!stopping) { console.error(packet); process.exit(1); } });
  await until(() => client.objectives.has('redpiler_status'), 'join ' + username);
  return client;
}
function command(client, value) { client.write('chat_command', {command:value}); }
async function transfer(client, value) {
  const creates = client.creates; const removals = client.removals;
  command(client, value);
  await until(() => client.creates === creates + 1, value);
  assert.equal(client.removals, removals + 1, 'Old objective removed before plot reentry');
  assert.equal(client.objectives.size, 1);
}
(async () => {
  const player = await connect('ScoreTraveler'); const target = await connect('ScoreTarget');
  await transfer(player, 'tp ScoreTarget');
  await transfer(player, 'tp ScoreTarget');
  await transfer(target, 'tp 300 30 130');
  await transfer(player, 'tp ScoreTarget');
  await transfer(player, 'tp 130 30 130');
  await transfer(player, 'tp MissingScorePlayer');
  command(player, 'rtps 100');
  await until(() => player.messages.some(message => message.includes("circuit's new tick pace is set")), '100 TPS');
  for (let i = 0; i < 5; i++) await transfer(player, 'tp ScoreTraveler');
  const creates = player.creates; const positions = player.positions;
  command(player, 'tp 140 30 140');
  await until(() => player.positions > positions, 'coordinate teleport within plot');
  await delay(100); assert.equal(player.creates, creates);
  player.end(); await delay(150);
  const reconnected = await connect('ScoreTraveler');
  assert.equal(reconnected.creates, 1, 'Fresh connection gets one objective');
  stopping = true; command(reconnected, 'stop'); await delay(300);
  for (const client of clients) client.end();
  console.log('PASS: same/cross-plot teleports, missing target, self teleport at 100 TPS, reconnect and objective lifetimes');
  process.exit(0);
})().catch(error => { console.error(error); process.exit(1); });
setTimeout(() => { console.error('Scoreboard smoke timed out'); process.exit(1); }, 25000).unref();
