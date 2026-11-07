/**
 * GDB/MI (machine interface) client: parser + one controlled GDB process.
 *
 * Commands carry numeric tokens; each resolves with its ^done/^running/
 * ^connected result or rejects with the ^error message. Async records
 * (*stopped, *running, =thread-group-exited, …) are emitted as events.
 * No shell is involved: GDB is spawned with an explicit argument vector.
 */
const { spawn } = require('child_process')
const { EventEmitter } = require('events')

// ── MI output parser ────────────────────────────────────────────────────────

function parseCString(s, i) {
  // s[i] === '"'
  let out = ''
  i++
  while (i < s.length) {
    const c = s[i]
    if (c === '"') return [out, i + 1]
    if (c === '\\') {
      const n = s[i + 1]
      const map = { n: '\n', t: '\t', r: '\r', '"': '"', '\\': '\\', a: '\x07', b: '\b', f: '\f', v: '\v', e: '\x1b' }
      if (n in map) { out += map[n]; i += 2; continue }
      if (/[0-7]/.test(n)) {
        // GDB escapes non-printable bytes as octal; collect raw bytes so UTF-8 survives
        const bytes = []
        while (s[i] === '\\' && /[0-7]/.test(s[i + 1] || '')) {
          let j = i + 1
          let oct = ''
          while (j < s.length && oct.length < 3 && /[0-7]/.test(s[j])) oct += s[j++]
          bytes.push(parseInt(oct, 8))
          i = j
        }
        out += Buffer.from(bytes).toString('utf8')
        continue
      }
      out += n ?? ''
      i += 2
      continue
    }
    out += c
    i++
  }
  throw new Error('Unterminated MI string')
}

function parseValue(s, i) {
  const c = s[i]
  if (c === '"') return parseCString(s, i)
  if (c === '{') {
    const obj = {}
    i++
    if (s[i] === '}') return [obj, i + 1]
    for (;;) {
      const [k, v, j] = parseResult(s, i)
      obj[k] = v
      i = j
      if (s[i] === ',') { i++; continue }
      if (s[i] === '}') return [obj, i + 1]
      throw new Error(`Bad MI tuple at ${i}`)
    }
  }
  if (c === '[') {
    const arr = []
    i++
    if (s[i] === ']') return [arr, i + 1]
    for (;;) {
      if (s[i] === '"' || s[i] === '{' || s[i] === '[') {
        const [v, j] = parseValue(s, i)
        arr.push(v)
        i = j
      } else {
        // list of results: name=value — keep both
        const [k, v, j] = parseResult(s, i)
        arr.push({ [k]: v })
        i = j
      }
      if (s[i] === ',') { i++; continue }
      if (s[i] === ']') return [arr, i + 1]
      throw new Error(`Bad MI list at ${i}`)
    }
  }
  throw new Error(`Bad MI value at ${i}`)
}

function parseResult(s, i) {
  const eq = s.indexOf('=', i)
  if (eq < 0) throw new Error(`Bad MI result at ${i}`)
  const key = s.slice(i, eq)
  const [v, j] = parseValue(s, eq + 1)
  return [key, v, j]
}

function parseResults(s, i) {
  const out = {}
  while (i < s.length && s[i] === ',') {
    const [k, v, j] = parseResult(s, i + 1)
    // repeated keys (e.g. bkpt=…,bkpt=…) become arrays
    if (k in out) out[k] = [].concat(out[k], [v])
    else out[k] = v
    i = j
  }
  return out
}

/**
 * Parse one line of GDB/MI output.
 * → { kind: 'result'|'exec'|'status'|'notify'|'console'|'target'|'log'|'prompt'|'raw',
 *     token, class, results, text }
 */
function parseMiLine(line) {
  const l = line.replace(/\r$/, '')
  if (l === '(gdb)' || l === '(gdb) ') return { kind: 'prompt' }
  const m = /^(\d*)([\^*+=~@&])(.*)$/.exec(l)
  if (!m) return { kind: 'raw', text: l }
  const token = m[1] ? Number(m[1]) : null
  const sigil = m[2]
  const rest = m[3]
  if (sigil === '~' || sigil === '@' || sigil === '&') {
    let text = rest
    try { if (rest.startsWith('"')) text = parseCString(rest, 0)[0] } catch {}
    return { kind: sigil === '~' ? 'console' : sigil === '@' ? 'target' : 'log', token, text }
  }
  const comma = rest.indexOf(',')
  const cls = comma < 0 ? rest : rest.slice(0, comma)
  let results = {}
  if (comma >= 0) {
    try { results = parseResults(rest, comma) } catch (e) { results = { _parseError: e.message, _raw: rest } }
  }
  const kind = { '^': 'result', '*': 'exec', '+': 'status', '=': 'notify' }[sigil]
  return { kind, token, class: cls, results }
}

/** Quote an argument for an MI command line (C string rules). */
function miQuote(s) {
  return '"' + String(s).replace(/\\/g, '\\\\').replace(/"/g, '\\"').replace(/\n/g, '\\n') + '"'
}

// ── Session ─────────────────────────────────────────────────────────────────

class GdbMiSession extends EventEmitter {
  /**
   * @param {object} o
   * @param {string} o.gdbPath   executable
   * @param {string} o.elf       program to debug
   * @param {string} [o.cwd]
   * @param {object} [o.env]
   * @param {number} [o.commandTimeoutMs]
   * @param {Function} [o.spawn]  injectable for tests
   */
  constructor(o) {
    super()
    this.gdbPath = o.gdbPath
    this.elf = o.elf
    this.cwd = o.cwd
    this.env = o.env
    this.commandTimeoutMs = o.commandTimeoutMs ?? 15000
    this._spawn = o.spawn || spawn
    this.proc = null
    this.nextToken = 1
    this.pending = new Map()
    this.buffer = ''
    this.state = 'idle' // idle → starting → stopped|running → exited
    this.exited = false
  }

  start() {
    if (this.proc) throw new Error('GDB already started')
    this.state = 'starting'
    const args = ['--interpreter=mi3', '-q', '-nx', this.elf]
    const proc = this._spawn(this.gdbPath, args, {
      cwd: this.cwd,
      env: this.env,
      stdio: ['pipe', 'pipe', 'pipe'],
      shell: false,
      windowsHide: true,
    })
    this.proc = proc
    proc.stdout.on('data', (d) => this._onData(d))
    proc.stderr.on('data', (d) => this.emit('output', { stream: 'stderr', text: d.toString() }))
    proc.on('error', (err) => this._onExit(null, err))
    proc.on('close', (code, signal) => this._onExit(code, null, signal))
    // Confirm GDB is alive and speaks MI before anything else
    return this.command('-gdb-set mi-async on').catch(() => this.command('-gdb-set target-async on'))
      .then(() => this.command('-gdb-set pagination off'))
      .then(() => this.command('-gdb-set confirm off'))
      .then(() => { if (this.state === 'starting') this.state = 'idle' })
  }

  _onData(chunk) {
    this.buffer += chunk.toString('utf8')
    let nl
    while ((nl = this.buffer.indexOf('\n')) >= 0) {
      const line = this.buffer.slice(0, nl)
      this.buffer = this.buffer.slice(nl + 1)
      if (line.trim()) this._onLine(line)
    }
  }

  _onLine(line) {
    const rec = parseMiLine(line)
    if (rec.kind === 'prompt') return
    if (rec.kind === 'console' || rec.kind === 'target' || rec.kind === 'log') {
      this.emit('output', { stream: rec.kind, text: rec.text })
      return
    }
    if (rec.kind === 'raw') {
      this.emit('output', { stream: 'raw', text: rec.text + '\n' })
      return
    }
    if (rec.kind === 'result') {
      const p = rec.token != null ? this.pending.get(rec.token) : null
      if (rec.class === 'running') this._setState('running')
      if (p) {
        this.pending.delete(rec.token)
        clearTimeout(p.timer)
        if (rec.class === 'error') {
          const err = new Error(rec.results.msg || 'GDB error')
          err.gdb = rec.results
          p.reject(err)
        } else p.resolve(rec.results)
      }
      if (rec.class === 'exit') this.state = 'exiting'
      return
    }
    if (rec.kind === 'exec') {
      if (rec.class === 'running') this._setState('running')
      if (rec.class === 'stopped') {
        this._setState('stopped')
        const r = rec.results
        if (r.reason === 'exited' || r.reason === 'exited-normally' || r.reason === 'exited-signalled') {
          this.emit('target-exited', r)
        }
        this.emit('stopped', r)
      }
      return
    }
    if (rec.kind === 'notify') {
      this.emit('notify', { class: rec.class, results: rec.results })
      if (rec.class === 'thread-group-exited') this.emit('target-exited', rec.results)
    }
  }

  _setState(s) {
    if (this.state === s) return
    this.state = s
    this.emit('state', s)
  }

  _onExit(code, err, signal) {
    if (this.exited) return
    this.exited = true
    this.state = 'exited'
    for (const [, p] of this.pending) {
      clearTimeout(p.timer)
      p.reject(new Error(err ? `GDB failed: ${err.message}` : `GDB exited (code ${code ?? signal})`))
    }
    this.pending.clear()
    this.proc = null
    this.emit('exit', { code, signal, error: err?.message })
  }

  /** Send one MI command; resolves with its result record payload. */
  command(cmd, { timeoutMs } = {}) {
    if (!this.proc || this.exited) return Promise.reject(new Error('GDB is not running'))
    if (/[\r\n]/.test(cmd)) return Promise.reject(new Error('MI command must be a single line'))
    const token = this.nextToken++
    return new Promise((resolve, reject) => {
      const timer = setTimeout(() => {
        if (!this.pending.has(token)) return
        this.pending.delete(token)
        reject(new Error(`GDB did not answer "${cmd.split(' ')[0]}" within ${timeoutMs ?? this.commandTimeoutMs} ms`))
      }, timeoutMs ?? this.commandTimeoutMs)
      this.pending.set(token, { resolve, reject, timer })
      try {
        this.proc.stdin.write(`${token}${cmd}\n`)
      } catch (e) {
        clearTimeout(timer)
        this.pending.delete(token)
        reject(e)
      }
    })
  }

  /** CLI command through MI (e.g. "monitor reset halt"). */
  console(cli, opts) {
    return this.command(`-interpreter-exec console ${miQuote(cli)}`, opts)
  }

  /** Wait for the next *stopped record (or reject on exit/timeout). */
  waitStopped(timeoutMs = 15000) {
    if (this.exited) return Promise.reject(new Error('GDB is not running'))
    return new Promise((resolve, reject) => {
      const onStop = (r) => { cleanup(); resolve(r) }
      const onExit = () => { cleanup(); reject(new Error('GDB exited while waiting for the target to stop')) }
      const timer = setTimeout(() => { cleanup(); reject(new Error(`Target did not stop within ${timeoutMs} ms`)) }, timeoutMs)
      const cleanup = () => {
        clearTimeout(timer)
        this.off('stopped', onStop)
        this.off('exit', onExit)
      }
      this.on('stopped', onStop)
      this.on('exit', onExit)
    })
  }

  async terminate({ graceMs = 2000 } = {}) {
    if (!this.proc || this.exited) return
    const proc = this.proc
    const done = new Promise((r) => proc.once('close', r))
    try {
      // -gdb-exit answers ^exit and quits; detach first is the caller's choice
      proc.stdin.write(`${this.nextToken++}-gdb-exit\n`)
    } catch {}
    const timer = setTimeout(() => killTree(proc), graceMs)
    await done
    clearTimeout(timer)
  }
}

function killTree(proc) {
  if (!proc || proc.exitCode != null) return
  try {
    if (process.platform === 'win32') {
      spawn('taskkill', ['/pid', String(proc.pid), '/f', '/t'], { windowsHide: true })
    } else {
      proc.kill('SIGKILL')
    }
  } catch {}
}

module.exports = { parseMiLine, parseCString, miQuote, GdbMiSession, killTree }
