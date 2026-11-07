/**
 * Interactive debug session: OpenOCD (or any GDB server) + GDB/MI.
 *
 * One session at a time. The renderer drives it through a fixed set of
 * operations; every argument is validated here, so renderer content cannot
 * inject GDB/MI or shell commands. Flashing happens only when the caller
 * passes loadFirmware: true (the UI asks the user first).
 */
const path = require('path')
const fs = require('fs')
const net = require('net')
const { spawn } = require('child_process')
const { EventEmitter } = require('events')
const { GdbMiSession, miQuote, killTree } = require('./gdbmi')

const NAME_RE = /^[a-zA-Z0-9._-]+$/
const HOST_RE = /^(?:localhost|\d{1,3}(?:\.\d{1,3}){3}|\[[0-9a-fA-F:]+\])$/
const OPENOCD_READY_RE = /Listening on port (\d+) for gdb connections/i

class DebugError extends Error {
  constructor(code, message, detail) {
    super(message)
    this.code = code
    if (detail) this.detail = detail
  }
}

function toError(err) {
  return { code: err.code || 'debug_error', message: err.message, ...(err.detail ? { detail: err.detail } : {}) }
}

function parsePort(v, fallback) {
  const n = v == null || v === '' ? fallback : Number(v)
  if (!Number.isInteger(n) || n < 1 || n > 65535) throw new DebugError('invalid_port', `Invalid TCP port: ${v}`)
  return n
}

function parseRemote(remote) {
  if (typeof remote !== 'string') throw new DebugError('invalid_remote', 'GDB server address required (host:port)')
  const m = /^(.*):(\d+)$/.exec(remote.trim())
  if (!m) throw new DebugError('invalid_remote', `GDB server address must be host:port, got "${remote}"`)
  const host = m[1] || '127.0.0.1'
  if (!HOST_RE.test(host)) throw new DebugError('invalid_remote', `Unsupported GDB server host "${host}"`)
  return { host, port: parsePort(m[2]) }
}

/** Absolute-path or project-relative source file → path GDB can match. */
function normalizeSourceFile(projectDir, file) {
  if (typeof file !== 'string' || !file || /[\r\n"]/.test(file)) {
    throw new DebugError('invalid_breakpoint', 'Breakpoint file is invalid')
  }
  const abs = path.isAbsolute(file) ? path.resolve(file) : path.resolve(projectDir, file)
  const rel = path.relative(path.resolve(projectDir), abs)
  if (rel.startsWith('..') || path.isAbsolute(rel)) {
    throw new DebugError('invalid_breakpoint', 'Breakpoints are limited to files inside the project')
  }
  return abs
}

function bpKey(file, line) {
  return `${path.resolve(file)}:${line}`
}

function frameFromMi(f) {
  if (!f || typeof f !== 'object') return null
  return {
    level: f.level != null ? Number(f.level) : 0,
    addr: f.addr || null,
    func: f.func || null,
    file: f.file || null,
    fullname: f.fullname || null,
    line: f.line != null ? Number(f.line) : null,
  }
}

function waitPortFree(port, host = '127.0.0.1') {
  return new Promise((resolve) => {
    const srv = net.createServer()
    srv.once('error', () => resolve(false))
    srv.listen(port, host, () => srv.close(() => resolve(true)))
  })
}

class DebugManager extends EventEmitter {
  /**
   * @param {object} deps
   * @param {(name:string)=>string} deps.resolveTool
   * @param {()=>object} deps.toolEnv
   * @param {()=>string|null} [deps.openocdScripts]
   */
  constructor(deps) {
    super()
    this.deps = deps
    this.session = null
    this.starting = false
    // Breakpoints persist across sessions: key → { file, line, number|null, verified }
    this.breakpoints = new Map()
    this.registerNames = null
  }

  status() {
    const s = this.session
    return {
      active: !!s,
      starting: this.starting,
      state: s ? s.state : 'idle',
      mode: s?.mode || null,
      elf: s?.elf || null,
      frame: s?.frame || null,
      breakpoints: this.listBreakpoints(),
    }
  }

  listBreakpoints() {
    return [...this.breakpoints.values()].map(b => ({ file: b.file, line: b.line, number: b.number, verified: b.verified }))
  }

  _emit(evt) {
    this.emit('event', evt)
  }

  _output(stream, text) {
    if (text) this._emit({ type: 'output', stream, text })
  }

  /**
   * @param {object} o
   * @param {string} o.projectDir
   * @param {string} o.elf
   * @param {'openocd'|'remote'} [o.mode]
   * @param {string} [o.adapter]   OpenOCD interface cfg name
   * @param {string} [o.target]    OpenOCD target cfg name
   * @param {number} [o.gdbPort]
   * @param {string} [o.remote]    host:port for mode=remote
   * @param {boolean} [o.loadFirmware]  flash the ELF (requires prior user confirmation)
   * @param {string} [o.gdbPath]  override (tests)
   * @param {string} [o.openocdPath] override (tests)
   * @param {string[]} [o.openocdPrefixArgs] leading args, e.g. a script for node (tests)
   */
  async start(o) {
    if (this.session || this.starting) {
      throw new DebugError('session_active', 'A debug session is already running. Stop it before starting another.')
    }
    this.starting = true
    let openocd = null
    let gdb = null
    try {
      const projectDir = path.resolve(o.projectDir)
      const elf = o.elf && path.resolve(o.elf)
      if (!elf || !fs.existsSync(elf)) throw new DebugError('no_elf', 'No ELF file to debug. Build the project first.')
      const mode = o.mode === 'remote' ? 'remote' : 'openocd'
      const env = this.deps.toolEnv()

      let host = '127.0.0.1'
      let port
      if (mode === 'openocd') {
        const adapter = o.adapter
        const target = o.target
        if (!NAME_RE.test(adapter || '') || !NAME_RE.test(target || '')) {
          throw new DebugError('invalid_config', 'Invalid OpenOCD interface or target name')
        }
        port = parsePort(o.gdbPort, 3333)
        if (!(await waitPortFree(port))) {
          throw new DebugError('port_busy', `TCP port ${port} is already in use. Another OpenOCD or GDB server may be running.`)
        }
        openocd = await this._startOpenocd({ adapter, target, port, cwd: projectDir, env, openocdPath: o.openocdPath, prefixArgs: o.openocdPrefixArgs, timeoutMs: o.openocdTimeoutMs })
      } else {
        ;({ host, port } = parseRemote(o.remote))
      }

      gdb = new GdbMiSession({
        gdbPath: o.gdbPath || this.deps.resolveTool('arm-none-eabi-gdb'),
        elf,
        cwd: projectDir,
        env,
      })
      const session = {
        mode, elf, projectDir, gdb, openocd, host, port,
        state: 'starting', frame: null, ending: false,
      }
      this.session = session
      this._wireGdb(session)
      if (openocd) this._wireOpenocd(session)

      await gdb.start()
      // GDB's default TCP connect timeout is 15 s; fail faster on a wrong address
      await gdb.command('-gdb-set tcp connect-timeout 5').catch(() => {})
      const remoteKind = mode === 'openocd' ? 'extended-remote' : 'remote'
      this._output('info', `Connecting GDB to ${host}:${port} (${remoteKind})…\n`)
      await gdb.command(`-target-select ${remoteKind} ${host}:${port}`, { timeoutMs: 20000 })

      if (mode === 'openocd') await gdb.console('monitor reset halt', { timeoutMs: 20000 })
      if (o.loadFirmware) {
        this._output('info', `Flashing ${path.basename(elf)}…\n`)
        await gdb.command('-target-download', { timeoutMs: 120000 })
        if (mode === 'openocd') await gdb.console('monitor reset halt', { timeoutMs: 20000 })
        this._output('info', 'Firmware written.\n')
      }

      for (const bp of this.breakpoints.values()) {
        await this._insertBreakpoint(bp)
      }
      this._emitBreakpoints()

      // Report where the CPU is halted
      session.state = 'stopped'
      session.frame = await this._currentFrame()
      this._emit({ type: 'stopped', reason: 'attached', frame: session.frame })
      this._emit({ type: 'state', state: 'stopped' })
      return this.status()
    } catch (err) {
      await this._teardown(gdb, openocd)
      this.session = null
      throw err instanceof DebugError ? err : new DebugError(err.code || 'start_failed', err.message, err.detail)
    } finally {
      this.starting = false
    }
  }

  _startOpenocd({ adapter, target, port, cwd, env, openocdPath, prefixArgs, timeoutMs = 15000 }) {
    // prefixArgs only come from main-process callers (tests run a JS stand-in)
    const args = Array.isArray(prefixArgs) ? [...prefixArgs] : []
    const scripts = this.deps.openocdScripts?.()
    if (scripts) args.push('-s', scripts)
    args.push(
      '-f', `interface/${adapter}.cfg`,
      '-f', `target/${target}.cfg`,
      '-c', `gdb_port ${port}`,
      '-c', 'tcl_port disabled',
      '-c', 'telnet_port disabled',
    )
    const bin = openocdPath || this.deps.resolveTool('openocd')
    this._output('info', `Starting OpenOCD (${adapter} → ${target}, gdb port ${port})…\n`)
    return new Promise((resolve, reject) => {
      let proc
      try {
        proc = spawn(bin, args, { cwd, env, stdio: ['ignore', 'pipe', 'pipe'], shell: false, windowsHide: true })
      } catch (e) {
        reject(new DebugError('openocd_missing', `Cannot start OpenOCD: ${e.message}`))
        return
      }
      const tail = []
      let settled = false
      const onChunk = (stream) => (d) => {
        const text = d.toString()
        this._output(`openocd-${stream}`, text)
        tail.push(...text.split(/\r?\n/).filter(Boolean))
        while (tail.length > 20) tail.shift()
        if (!settled && OPENOCD_READY_RE.test(text)) {
          settled = true
          clearTimeout(timer)
          resolve(proc)
        }
      }
      proc.stdout.on('data', onChunk('stdout'))
      proc.stderr.on('data', onChunk('stderr'))
      const fail = (code, msg) => {
        if (settled) return
        settled = true
        clearTimeout(timer)
        killTree(proc)
        reject(new DebugError(code, msg, tail.slice(-8).join('\n')))
      }
      const timer = setTimeout(() => fail('openocd_timeout', `OpenOCD did not open its GDB port within ${Math.round(timeoutMs / 1000)} s. Check the probe, cable and target power.`), timeoutMs)
      proc.on('error', (e) => fail('openocd_missing', `Cannot start OpenOCD: ${e.message}`))
      proc.on('close', (code) => fail('openocd_failed', `OpenOCD exited with code ${code} before GDB could connect. Check the probe, cable and target power.`))
    })
  }

  _wireOpenocd(session) {
    session.openocd.on('close', (code) => {
      if (this.session !== session || session.ending) return
      this._output('error', `OpenOCD exited (code ${code}). The probe or target may have been disconnected.\n`)
      void this._end(session, 'openocd-exited')
    })
  }

  _wireGdb(session) {
    const g = session.gdb
    g.on('output', ({ stream, text }) => this._output(`gdb-${stream}`, text))
    g.on('state', (s) => {
      if (this.session !== session) return
      if (s === 'running' || s === 'stopped') {
        session.state = s
        this._emit({ type: 'state', state: s })
      }
    })
    g.on('stopped', async (r) => {
      if (this.session !== session) return
      session.frame = frameFromMi(r.frame)
      this._emit({ type: 'stopped', reason: r.reason || 'signal', signal: r['signal-name'] || null, bkptno: r.bkptno ? Number(r.bkptno) : null, frame: session.frame })
    })
    g.on('notify', ({ class: cls, results }) => {
      if (cls === 'breakpoint-modified' && results.bkpt) this._syncBreakpoint(results.bkpt)
    })
    g.on('target-exited', () => {
      if (this.session === session && !session.ending) void this._end(session, 'target-exited')
    })
    g.on('exit', ({ code }) => {
      if (this.session !== session || session.ending) return
      this._output('error', `GDB exited unexpectedly (code ${code}).\n`)
      void this._end(session, 'gdb-exited')
    })
  }

  _syncBreakpoint(b) {
    for (const bp of this.breakpoints.values()) {
      if (bp.number != null && String(bp.number) === String(b.number)) {
        bp.verified = b.pending == null && !!b.addr && b.addr !== '<PENDING>'
      }
    }
    this._emitBreakpoints()
  }

  _emitBreakpoints() {
    this._emit({ type: 'breakpoints', breakpoints: this.listBreakpoints() })
  }

  async _insertBreakpoint(bp) {
    const g = this.session?.gdb
    if (!g) return
    try {
      const r = await g.command(`-break-insert -f ${miQuote(`${bp.file}:${bp.line}`)}`)
      const b = r.bkpt || {}
      bp.number = b.number != null ? Number(b.number) : null
      bp.verified = !!b.addr && b.addr !== '<PENDING>' && b.pending == null
      if (b.line != null) bp.line = Number(b.line)
    } catch (err) {
      bp.number = null
      bp.verified = false
      this._output('error', `Breakpoint ${path.basename(bp.file)}:${bp.line} not set: ${err.message}\n`)
    }
  }

  async _currentFrame() {
    try {
      const r = await this.session.gdb.command('-stack-info-frame')
      return frameFromMi(r.frame)
    } catch {
      return null
    }
  }

  _requireStopped() {
    const s = this.session
    if (!s) throw new DebugError('no_session', 'No debug session is running.')
    if (s.state !== 'stopped') throw new DebugError('target_running', 'The target is running. Pause it first.')
    return s
  }

  /** Breakpoints can be edited with or without a session. */
  async toggleBreakpoint(projectDir, file, line) {
    const abs = normalizeSourceFile(projectDir, file)
    const ln = Number(line)
    if (!Number.isInteger(ln) || ln < 1) throw new DebugError('invalid_breakpoint', 'Breakpoint line must be a positive integer')
    const key = bpKey(abs, ln)
    const existing = this.breakpoints.get(key)
    if (existing) {
      this.breakpoints.delete(key)
      if (this.session && existing.number != null) {
        // GDB accepts break-delete while the target runs
        await this.session.gdb.command(`-break-delete ${existing.number}`).catch((e) => this._output('error', `Breakpoint delete failed: ${e.message}\n`))
      }
    } else {
      const bp = { file: abs, line: ln, number: null, verified: false }
      this.breakpoints.set(key, bp)
      if (this.session) await this._insertBreakpoint(bp)
    }
    this._emitBreakpoints()
    return this.listBreakpoints()
  }

  async clearBreakpoints() {
    const nums = [...this.breakpoints.values()].map(b => b.number).filter(n => n != null)
    this.breakpoints.clear()
    if (this.session && nums.length) await this.session.gdb.command(`-break-delete ${nums.join(' ')}`).catch(() => {})
    this._emitBreakpoints()
    return []
  }

  /**
   * Run one renderer-requested operation. Only these names exist.
   */
  async op(name, args = {}) {
    const s = this.session
    if (!s) throw new DebugError('no_session', 'No debug session is running.')
    const g = s.gdb
    switch (name) {
      case 'continue':
        this._requireStopped()
        await g.command('-exec-continue')
        return { ok: true }
      case 'pause':
        if (s.state !== 'running') return { ok: true }
        await g.command('-exec-interrupt')
        return { ok: true }
      case 'stepOver':
        this._requireStopped()
        await g.command('-exec-next')
        return { ok: true }
      case 'stepInto':
        this._requireStopped()
        await g.command('-exec-step')
        return { ok: true }
      case 'stepOut':
        this._requireStopped()
        await g.command('-exec-finish')
        return { ok: true }
      case 'stepInstruction':
        this._requireStopped()
        await g.command('-exec-next-instruction')
        return { ok: true }
      case 'reset':
        if (s.mode !== 'openocd') throw new DebugError('unsupported', 'Reset is available with OpenOCD only')
        if (s.state === 'running') await g.command('-exec-interrupt').then(() => g.waitStopped(5000)).catch(() => {})
        await g.console('monitor reset halt', { timeoutMs: 20000 })
        s.state = 'stopped'
        s.frame = await this._currentFrame()
        this._emit({ type: 'stopped', reason: 'reset', frame: s.frame })
        this._emit({ type: 'state', state: 'stopped' })
        return { ok: true }
      case 'stack': {
        this._requireStopped()
        const r = await g.command('-stack-list-frames 0 63')
        const frames = (r.stack || []).map(e => frameFromMi(e.frame || e))
        return { frames }
      }
      case 'locals': {
        this._requireStopped()
        const level = Number.isInteger(args.frame) && args.frame >= 0 && args.frame < 64 ? args.frame : 0
        const r = await g.command(`-stack-list-variables --frame ${level} --thread 1 --simple-values`).catch(() =>
          g.command('-stack-list-variables --simple-values'))
        const vars = (r.variables || []).map(v => ({
          name: v.name, type: v.type ?? null, value: v.value ?? null, arg: v.arg === '1',
        }))
        return { variables: vars }
      }
      case 'registers': {
        this._requireStopped()
        if (!this.registerNames || this.registerNames.elf !== s.elf) {
          const n = await g.command('-data-list-register-names')
          this.registerNames = { elf: s.elf, names: n['register-names'] || [] }
        }
        const names = this.registerNames.names
        const r = await g.command('-data-list-register-values --skip-unavailable x')
        const regs = (r['register-values'] || [])
          .map(v => ({ number: Number(v.number), name: names[Number(v.number)] || `r${v.number}`, value: v.value }))
          .filter(v => v.name)
        return { registers: regs }
      }
      case 'memory': {
        this._requireStopped()
        const addr = String(args.address ?? '').trim()
        // Numeric address or a plain C identifier/&identifier; nothing else reaches GDB
        if (!/^(0x[0-9a-fA-F]{1,16}|\d{1,20}|&?[A-Za-z_][A-Za-z0-9_]{0,127})$/.test(addr)) {
          throw new DebugError('invalid_address', 'Memory address must be a number (0x…) or a symbol name')
        }
        const len = Number(args.length ?? 64)
        if (!Number.isInteger(len) || len < 1 || len > 4096) throw new DebugError('invalid_length', 'Memory length must be 1–4096 bytes')
        const r = await g.command(`-data-read-memory-bytes ${miQuote(addr)} ${len}`)
        const blocks = (r.memory || []).map(m => ({ begin: m.begin, end: m.end, contents: m.contents }))
        return { blocks }
      }
      case 'evaluate': {
        this._requireStopped()
        const expr = String(args.expression ?? '')
        if (!expr.trim() || expr.length > 500 || /[\r\n]/.test(expr)) {
          throw new DebugError('invalid_expression', 'Expression must be a single line of up to 500 characters')
        }
        const r = await g.command(`-data-evaluate-expression ${miQuote(expr)}`)
        return { value: r.value ?? '' }
      }
      default:
        throw new DebugError('unknown_op', `Unknown debug operation: ${String(name).slice(0, 40)}`)
    }
  }

  async stop() {
    const s = this.session
    if (!s) return { ok: true }
    await this._end(s, 'user')
    return { ok: true }
  }

  async _end(session, reason) {
    if (session.ending) return
    session.ending = true
    const g = session.gdb
    try {
      if (g && !g.exited) {
        if (session.state === 'running') {
          await g.command('-exec-interrupt', { timeoutMs: 2000 }).then(() => g.waitStopped(2000)).catch(() => {})
        }
        if (reason === 'user') await g.command('-target-detach', { timeoutMs: 3000 }).catch(() => {})
      }
    } finally {
      await this._teardown(g, session.openocd)
      if (this.session === session) this.session = null
      for (const bp of this.breakpoints.values()) { bp.number = null; bp.verified = false }
      this._emit({ type: 'ended', reason })
      this._emit({ type: 'state', state: 'idle' })
      this._emitBreakpoints()
    }
  }

  async _teardown(gdb, openocd) {
    if (gdb) await gdb.terminate().catch(() => {})
    if (openocd && openocd.exitCode == null) {
      const done = new Promise((r) => openocd.once('close', r))
      try {
        if (process.platform === 'win32') killTree(openocd)
        else openocd.kill('SIGTERM')
      } catch {}
      const t = setTimeout(() => killTree(openocd), 3000)
      await done
      clearTimeout(t)
    }
  }

  /** Synchronous last-resort cleanup for app quit. */
  killAll() {
    const s = this.session
    if (!s) return
    s.ending = true
    try { if (s.gdb?.proc) killTree(s.gdb.proc) } catch {}
    try { if (s.openocd) killTree(s.openocd) } catch {}
    this.session = null
  }
}

module.exports = { DebugManager, DebugError, toError, parseRemote, normalizeSourceFile }
