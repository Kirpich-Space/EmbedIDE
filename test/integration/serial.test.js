/**
 * SerialService ↔ `embedide serial-bridge` ↔ OS serial device.
 * The device is a pseudo-terminal played by test/support/pty-device.py, so
 * this exercises real termios serial I/O, not mocks — but not a USB-UART.
 * POSIX only; needs a built bridge (cargo build -p embedide-cli).
 */
const test = require('node:test')
const assert = require('node:assert/strict')
const fs = require('fs')
const os = require('os')
const path = require('path')
const readline = require('readline')
const { spawn } = require('child_process')
const { SerialService, findBridge, validateConfig } = require('../../electron/serial')

const bridge = findBridge()
const skip = process.platform === 'win32' ? 'pseudo-terminals are POSIX-only'
  : !bridge ? 'embedide binary not built (cargo build -p embedide-cli)' : false

class Device {
  constructor() {
    this.p = spawn('python3', [path.join(__dirname, '..', 'support', 'pty-device.py')], { stdio: ['pipe', 'pipe', 'inherit'] })
    this.lines = []
    this.waiting = []
    readline.createInterface({ input: this.p.stdout }).on('line', (l) => {
      const w = this.waiting.shift()
      if (w) w(l); else this.lines.push(l)
    })
  }
  cmd(c) {
    return new Promise((resolve) => {
      if (this.lines.length) { resolve(this.lines.shift()); return }
      this.waiting.push(resolve)
      this.p.stdin.write(c + '\n')
    })
  }
  async read() { return Buffer.from((await this.cmd('read')).slice(4), 'hex') }
  quit() { try { this.p.stdin.write('quit\n') } catch {} }
}

function waitFor(emitter, event, pred = () => true, ms = 8000) {
  return new Promise((resolve, reject) => {
    const t = setTimeout(() => { emitter.off(event, h); reject(new Error(`timeout waiting for ${event}`)) }, ms)
    const h = (v) => { if (pred(v)) { clearTimeout(t); emitter.off(event, h); resolve(v) } }
    emitter.on(event, h)
  })
}

async function until(fn, ms = 3000) {
  const end = Date.now() + ms
  for (;;) {
    const v = await fn()
    if (v) return v
    if (Date.now() > end) return v
    await new Promise((r) => setTimeout(r, 50))
  }
}

test('config validation rejects bad framing before any device access', () => {
  assert.throws(() => validateConfig({ port: '', baud: 9600 }), /Select a serial port/)
  assert.throws(() => validateConfig({ port: 'COM3', baud: 7 }), /Baud/)
  assert.throws(() => validateConfig({ port: 'COM3', baud: 9600, parity: 'mark' }), /Parity/)
  assert.throws(() => validateConfig({ port: 'COM3', baud: 9600, dataBits: 9 }), /Data bits/)
  assert.deepEqual(validateConfig({ port: 'COM3', baud: 9600 }),
    { port: 'COM3', baud: 9600, data_bits: 8, stop_bits: 1, parity: 'none', flow_control: 'none' })
})

test('list, open, binary send/receive, close', { skip }, async () => {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'embedide-serial-'))
  const link = path.join(dir, 'ttyFAKE0')
  const dev = new Device()
  const svc = new SerialService({ bridgePath: bridge })
  try {
    assert.ok(Array.isArray(await svc.list()))
    assert.match(await dev.cmd(`new ${link}`), /^ready /)
    await svc.open({ port: link, baud: 115200 })
    const payload = Buffer.from([0, 1, 2, 0xff, 0x0d, 0x0a, ...Buffer.from('Привет')])
    svc.write(payload)
    assert.deepEqual(await until(async () => { const b = await dev.read(); return b.length >= payload.length && b }), payload)

    const chunks = []
    svc.on('data', (b) => chunks.push(b))
    const inbound = Buffer.from([0x00, 0xfe, 0x4f, 0x4b, 0x0d, 0x0a]) // NUL, invalid UTF-8, "OK\r\n"
    await dev.cmd(`send ${inbound.toString('hex')}`)
    await until(() => Buffer.concat(chunks).length >= inbound.length)
    assert.deepEqual(Buffer.concat(chunks), inbound)

    assert.throws(() => svc.write(Buffer.alloc(5000)), /4 KiB/)
    const closed = waitFor(svc, 'status', (s) => s.state === 'closed')
    await svc.close()
    await closed
    assert.throws(() => svc.write(Buffer.from('x')), /not open/)
  } finally {
    svc.shutdown()
    dev.quit()
    fs.rmSync(dir, { recursive: true, force: true })
  }
})

test('unplug is reported and the same port reconnects when it returns', { skip }, async () => {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'embedide-serial-'))
  const link = path.join(dir, 'ttyFAKE1')
  const dev = new Device()
  const svc = new SerialService({ bridgePath: bridge })
  try {
    await dev.cmd(`new ${link}`)
    await svc.open({ port: link, baud: 9600 })
    const lost = waitFor(svc, 'status', (s) => s.state === 'lost')
    await dev.cmd('vanish')
    const st = await lost
    assert.equal(st.port, link)
    assert.equal(svc.state, 'reconnecting')

    const back = waitFor(svc, 'status', (s) => s.state === 'open' && s.reconnected, 10000)
    await dev.cmd(`new ${link}`)
    await back
    const chunks = []
    svc.on('data', (b) => chunks.push(b))
    await dev.cmd(`send ${Buffer.from('again\n').toString('hex')}`)
    await until(() => Buffer.concat(chunks).length >= 6)
    assert.equal(Buffer.concat(chunks).toString(), 'again\n')
  } finally {
    svc.shutdown()
    dev.quit()
    fs.rmSync(dir, { recursive: true, force: true })
  }
})

test('user disconnect cancels reconnection; autoReconnect=false stays closed', { skip }, async () => {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'embedide-serial-'))
  const link = path.join(dir, 'ttyFAKE2')
  const dev = new Device()
  const svc = new SerialService({ bridgePath: bridge })
  try {
    svc.autoReconnect = false
    await dev.cmd(`new ${link}`)
    await svc.open({ port: link, baud: 9600 })
    const lost = waitFor(svc, 'status', (s) => s.state === 'lost')
    await dev.cmd('vanish')
    await lost
    assert.equal(svc.state, 'closed')

    svc.autoReconnect = true
    await dev.cmd(`new ${link}`)
    await svc.open({ port: link, baud: 9600 })
    await dev.cmd('vanish')
    await waitFor(svc, 'status', (s) => s.state === 'lost')
    await svc.close()
    await dev.cmd(`new ${link}`)
    await new Promise((r) => setTimeout(r, 1500))
    assert.equal(svc.state, 'closed', 'no silent reconnect after the user disconnected')
  } finally {
    svc.shutdown()
    dev.quit()
    fs.rmSync(dir, { recursive: true, force: true })
  }
})

test('opening a missing port fails with the OS reason', { skip }, async () => {
  const svc = new SerialService({ bridgePath: bridge })
  try {
    await assert.rejects(svc.open({ port: '/dev/embedide-missing-port', baud: 115200 }), /embedide-missing-port/)
    assert.equal(svc.state, 'closed')
  } finally {
    svc.shutdown()
  }
})

test('a crashed helper is reported and replaced on the next request', { skip }, async () => {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'embedide-serial-'))
  const link = path.join(dir, 'ttyFAKE3')
  const dev = new Device()
  const svc = new SerialService({ bridgePath: bridge })
  try {
    svc.autoReconnect = false
    await dev.cmd(`new ${link}`)
    await svc.open({ port: link, baud: 9600 })
    const lost = waitFor(svc, 'status', (s) => s.state === 'lost')
    svc.proc.kill('SIGKILL')
    assert.match((await lost).message, /helper stopped/)
    assert.ok(Array.isArray(await svc.list()), 'new helper started')
  } finally {
    svc.shutdown()
    dev.quit()
    fs.rmSync(dir, { recursive: true, force: true })
  }
})
