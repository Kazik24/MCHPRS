// Used only by run_plot_load_smoke.py with its isolated world.
const mc = require('minecraft-protocol');
const assert = require('assert/strict');
const port = Number(process.argv[2]);
const first = process.argv.includes('--first');
const clients = [];
const delay = ms => new Promise(resolve => setTimeout(resolve, ms));
async function until(predicate, description) {
  for (let i = 0; i < 100; i++) {
    if (predicate()) return;
    await delay(50);
  }
  throw Error('Timeout: ' + description);
}
function connect(username) {
  const client = mc.createClient({host: '127.0.0.1', port, username, auth: 'offline', version: '1.21.5'});
  clients.push(client);
  client.chunks = new Set();
  client.on('position', packet => {
    client.x = packet.x;
    client.write('teleport_confirm', {teleportId: packet.teleportId});
  });
  client.on('map_chunk', packet => client.chunks.add(`${packet.x},${packet.z}`));
  client.on('kick_disconnect', packet => { client.kick = packet; });
  client.on('end', () => { client.ended = true; });
  client.on('error', error => { client.failure = error; });
  return client;
}
function command(client, command) { client.write('chat_command', {command}); }
(async () => {
  const keeper = connect('LoadKeeper');
  await until(() => keeper.chunks.size > 0, 'healthy plot login');
  if (first) {
    // Retry the same failed plot to check that its running entry was removed.
    for (const username of ['LoadProbeOne', 'LoadProbeTwo']) {
      const probe = connect(username);
      await until(() => probe.chunks.size > 0, 'probe login');
      command(probe, 'plot tp 1 0');
      await until(() => probe.ended, 'failed plot disconnect');
      assert(probe.kick, 'Client should receive a reason before disconnect');
      assert(JSON.stringify(probe.kick).includes('Could not load plot 1,0'));
      assert(!keeper.ended && !keeper.failure, 'Healthy plot must remain available');
    }
    command(keeper, 'plot tp 2 0');
    await until(() => keeper.x >= 512 && keeper.chunks.has('40,8'), 'healthy neighboring plot');
  } else {
    assert(keeper.x >= 512, 'Keeper should reconnect to its saved healthy plot');
  }
  command(keeper, 'stop');
  await delay(400);
  for (const client of clients) client.end();
  console.log('PASS: failed plot refused, healthy plot available, graceful shutdown requested.');
  process.exit(0);
})().catch(error => {
  console.error(error);
  for (const client of clients) client.end();
  process.exit(1);
});
setTimeout(() => { console.error('TIMEOUT'); process.exit(1); }, 20000);
