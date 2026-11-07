/**
 * Serial monitor service.
 *
 * Talks to `embedide serial-bridge` (crates/embedide-cli), which uses the
 * Rust serialport crate: no Python or pyserial is needed, and data is
 * binary-safe (hex on the wire, bytes to the renderer). One long-lived bridge
 * process serves enumeration and the open port; it is restarted if it dies
 * and stopped when the app quits.
 */
const { spawn } = require('child_process')
const { EventEmitter } = require('events')
const fs = require('fs')
const path = require('path')

const EXE = process.platform === 'win32' ? 'embedide.exe' : 'embedide'

/** Locate the bridge binary: env override, packaged resources, dev builds. */
function bridgeCandidates() {
  const list = []
  if (process.env.EMBEDIDE_SERIAL_BRIDGE) list.push(process.env.EMBEDIDE_SERIAL_BRIDGE)
  try {
    if (process.resourcesPath) list.push(path.join(process.resourcesPath, 'bin', EXE))
  } catch {}
  const repo = path.join(__dirname, '..')
  list.push(path.join(repo, 'target', 'release', EXE), path.join(repo, 'target', 'debug', EXE))
  return list
}

function findBridge() {
  return bridgeCandidates().find((p) => {
    try { return fs.statSync(p).isFile() } catch { return false }
  }) || null
}

const BAUD_MAX = 12_000_000

function validateConfig(c) {
  if (!c || typeof c !== 'object') throw new Error('Serial configuration required')
  const port = String(c.port || '')
  if (!port || port.length > 256 || /[\r\n\0]/.test(port)) throw new Error('Select a serial port')
  const baud = Number(c.baud)
  if (!Number.isInteger(baud) || baud < 50 || baud > BAUD_MAX) throw new Error(`Baud rate must be 50–${BAUD_MAX}`)
  const dataBits = c.dataBits == null ? 8 : Number(c.dataBits)
  if (![5, 6, 7, 8].includes(dataBits)) throw new Error('Data bits must be 5, 6, 7 or 8')
  const stopBits = c.stopBits == null ? 1 : Number(c.stopBits)
  if (![1, 2].includes(stopBits)) throw new Error('Stop bits must be 1 or 2')
  const parity = c.parity || 'none'
  if (!['none', 'odd', 'even'].includes(parity)) throw new Error('Parity must be none, odd or even')
  const flow = c.flowControl || 'none'
  if (!['none', 'software', 'hardware'].includes(flow)) throw new Error('Flow control must be none, software or hardware')
  return { port, baud, data_bits: dataBits, stop_bits: stopBits, parity, flow_control: flow }
}

class SerialService extends EventEmitter {
  constructor({ bridgePath, env } = {}) {
    super()
    this.bridgePath = bridgePath || null
    this.env = env
    this.proc = null
    this.buf = ''
    this.waiters = [] // { events:Set, resolve, reject, timer }
    this.config = null // last opened config (for reconnect)
    this.state = 'closed' // closed | open | reconnecting
    this.autoReconnect = true
    this.reconnectTimer = null
    this.userClosed = false
  }

  _ensure() {
    if (this.proc && this.proc.exitCode == null && !this.proc.killed) return
    const bin = this.bridgePath || findBridge()
    if (!bin) {
      throw new Error('Serial helper (embedide) is missing. Reinstall EmbedIDE, or in a source checkout run: cargo build --release -p embedide-cli')
    }
    const proc = spawn(bin, ['serial-bridge'], { stdio: ['pipe', 'pipe', 'pipe'], env: this.env, shell: false, windowsHide: true })
    this.proc = proc
    this.buf = ''
    proc.stdout.on('data', (d) => this._onData(d))
    proc.stderr.on('data', (d) => this.emit('log', d.toString()))
    proc.on('error', (e) => this._onBridgeExit(e.message))
    proc.on('close', (code) => this._onBridgeExit(`code ${code}`))
  }

  _onBridgeExit(why) {
    if (!this.proc) return
    this.proc = null
    for (const w of this.waiters.splice(0)) {
      clearTimeout(w.timer)
      w.reject(new Error(`Serial helper stopped (${why})`))
    }
    if (this.state === 'open') this._lost(`Serial helper stopped (${why})`)
  }

  _onData(chunk) {
    this.buf += chunk.toString('utf8')
    let nl
    while ((nl = this.buf.indexOf('\n')) >= 0) {
      const line = this.buf.slice(0, nl)
      this.buf = this.buf.slice(nl + 1)
      if (!line.trim()) continue
      let ev
      try { ev = JSON.parse(line) } catch { continue }
      this._onEvent(ev)
    }
  }

  _onEvent(ev) {
    if (ev.event === 'data') {
      this.emit('data', Buffer.from(ev.hex || '', 'hex'))
      return
    }
    if (ev.event === 'dropped') {
      this.emit('status', { state: this.state, message: `${ev.bytes} received bytes dropped (display too slow)` })
      return
    }
    if (ev.event === 'disconnected') {
      this._lost(ev.message || 'Port disconnected')
      return
    }
    const idx = this.waiters.findIndex((w) => w.events.has(ev.event) && (!w.op || ev.event !== 'error' || ev.op === w.op))
    if (idx >= 0) {
      const [w] = this.waiters.splice(idx, 1)
      clearTimeout(w.timer)
      if (ev.event === 'error') w.reject(new Error(ev.message || 'Serial error'))
      else w.resolve(ev)
      return
    }
    if (ev.event === 'error') this.emit('error-message', ev.message || 'Serial error')
  }

  _request(obj, events, { op, timeoutMs = 5000 } = {}) {
    this._ensure()
    return new Promise((resolve, reject) => {
      const timer = setTimeout(() => {
        const i = this.waiters.indexOf(w)
        if (i >= 0) this.waiters.splice(i, 1)
        reject(new Error('Serial helper did not respond'))
      }, timeoutMs)
      const w = { events: new Set(events), op, resolve, reject, timer }
      this.waiters.push(w)
      try {
        this.proc.stdin.write(JSON.stringify(obj) + '\n')
      } catch (e) {
        clearTimeout(timer)
        this.waiters.splice(this.waiters.indexOf(w), 1)
        reject(e)
      }
    })
  }

  async list() {
    const ev = await this._request({ op: 'list' }, ['ports', 'error'], { op: 'list' })
    return (ev.ports || []).map((p) => ({
      device: p.name,
      kind: p.kind,
      vid: p.vid ?? null,
      pid: p.pid ?? null,
      serialNumber: p.serial_number ?? null,
      description: [p.manufacturer, p.product].filter(Boolean).join(' ') ||
        (p.vid != null ? `USB ${p.vid.toString(16).padStart(4, '0')}:${p.pid.toString(16).padStart(4, '0')}` : p.kind),
    }))
  }

  async open(config) {
    const cfg = validateConfig(config)
    this._cancelReconnect()
    this.userClosed = false
    await this._request({ op: 'open', config: cfg }, ['opened', 'error'], { op: 'open' })
    this.config = cfg
    this.state = 'open'
    this.emit('status', { state: 'open', port: cfg.port, baud: cfg.baud })
    return { port: cfg.port, baud: cfg.baud }
  }

  write(bytes) {
    if (this.state !== 'open' || !this.proc) throw new Error('Serial port is not open')
    const buf = Buffer.isBuffer(bytes) ? bytes : Buffer.from(bytes)
    if (buf.length === 0) return
    if (buf.length > 4096) throw new Error('Serial message exceeds 4 KiB')
    this.proc.stdin.write(JSON.stringify({ op: 'write', hex: buf.toString('hex') }) + '\n')
  }

  async close() {
    this.userClosed = true
    this._cancelReconnect()
    const wasOpen = this.state !== 'closed'
    this.state = 'closed'
    if (this.proc && wasOpen) {
      await this._request({ op: 'close' }, ['closed'], { timeoutMs: 3000 }).catch(() => {})
    }
    if (wasOpen) this.emit('status', { state: 'closed' })
  }

  _lost(message) {
    if (this.state === 'closed') return
    this.emit('status', { state: 'lost', message, port: this.config?.port })
    if (this.autoReconnect && !this.userClosed && this.config) {
      this.state = 'reconnecting'
      this._scheduleReconnect(Date.now() + 120000)
    } else {
      this.state = 'closed'
    }
  }

  _scheduleReconnect(deadline) {
    this._cancelReconnect()
    this.reconnectTimer = setTimeout(async () => {
      this.reconnectTimer = null
      if (this.state !== 'reconnecting' || this.userClosed) return
      if (Date.now() > deadline) {
        this.state = 'closed'
        this.emit('status', { state: 'closed', message: 'Port did not come back within 2 minutes' })
        return
      }
      try {
        const ports = await this.list()
        // Device paths (also /dev/serial/by-id links) vanish on unplug and return on replug
        const present = ports.some((p) => p.device === this.config.port) ||
          (path.isAbsolute(this.config.port) && fs.existsSync(this.config.port))
        if (present) {
          await this._request({ op: 'open', config: this.config }, ['opened', 'error'], { op: 'open' })
          if (this.state !== 'reconnecting') return
          this.state = 'open'
          this.emit('status', { state: 'open', port: this.config.port, baud: this.config.baud, reconnected: true })
          return
        }
      } catch {}
      this.emit('status', { state: 'reconnecting', port: this.config.port })
      this._scheduleReconnect(deadline)
    }, 1000)
  }

  _cancelReconnect() {
    if (this.reconnectTimer) clearTimeout(this.reconnectTimer)
    this.reconnectTimer = null
  }

  shutdown() {
    this._cancelReconnect()
    this.state = 'closed'
    const p = this.proc
    this.proc = null
    if (p) {
      try { p.stdin.end() } catch {}
      setTimeout(() => { try { if (p.exitCode == null) p.kill() } catch {} }, 1000).unref?.()
    }
  }
}

module.exports = { SerialService, validateConfig, findBridge, bridgeCandidates }
