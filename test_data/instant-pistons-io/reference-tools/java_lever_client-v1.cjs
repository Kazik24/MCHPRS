// Isolated offline reference client: actual UseItemOn packets, never chat.
// JSON lines on stdin/stdout are the capture tool's local control protocol.
const mc = require('./node_modules/minecraft-protocol')
const readline = require('node:readline')
const client = mc.createClient({
  host: '127.0.0.1', port: 25583, username: 'InstantReference',
  version: '1.21.5', auth: 'offline'
})
let pending = null
let sequence = 0
function emit (value) { process.stdout.write(JSON.stringify(value) + '\n') }
client.once('login', () => emit({ event: 'ready', version: require('./node_modules/minecraft-protocol/package.json').version }))
client.on('position', packet => {
  client.write('teleport_confirm', { teleportId: packet.teleportId })
  if (!pending) return
  const command = pending
  pending = null
  const interaction = {
    hand: 0, location: { x: command.pos[0], y: command.pos[1], z: command.pos[2] },
    direction: 1, cursorX: 0.5, cursorY: 0.5, cursorZ: 0.5,
    insideBlock: false, worldBorderHit: false, sequence: ++sequence
  }
  client.write('block_place', interaction)
  emit({ event: 'sent', id: command.id, interaction, teleportId: packet.teleportId })
})
client.on('error', error => { emit({ event: 'error', message: error.message }); process.exit(1) })
client.on('end', reason => { emit({ event: 'end', reason }); process.exit(0) })
readline.createInterface({ input: process.stdin }).on('line', line => {
  const command = JSON.parse(line)
  if (command.op === 'prepare') {
    if (pending) throw new Error('overlapping interactions')
    pending = command
    emit({ event: 'prepared', id: command.id })
  } else if (command.op === 'stop') {
    client.end('reference complete')
    setTimeout(() => process.exit(0), 100)
  } else throw new Error('unknown command')
})
