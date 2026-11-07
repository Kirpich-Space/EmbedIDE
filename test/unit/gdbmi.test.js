const test = require('node:test')
const assert = require('node:assert/strict')
const { EventEmitter } = require('events')
const { PassThrough } = require('stream')
const { parseMiLine, miQuote, GdbMiSession } = require('../../electron/gdbmi')
const { DebugManager, parseRemote, normalizeSourceFile } = require('../../electron/debugSession')

test('result record with token and nested tuple', () => {
  const r = parseMiLine('12^done,bkpt={number="1",type="breakpoint",addr="0x08000076",file="main.c",line="17"}')
  assert.equal(r.kind, 'result')
  assert.equal(r.token, 12)
  assert.equal(r.class, 'done')
  assert.equal(r.results.bkpt.line, '17')
})

test('stopped record with frame and args list', () => {
  const r = parseMiLine('*stopped,reason="breakpoint-hit",bkptno="1",frame={addr="0x1",func="main",args=[{name="x",value="2"}],file="m.c",line="9"},thread-id="1"')
  assert.equal(r.kind, 'exec')
  assert.equal(r.class, 'stopped')
  assert.equal(r.results.frame.args[0].name, 'x')
  assert.equal(r.results['thread-id'], '1')
})

test('lists of results and repeated keys', () => {
  const r = parseMiLine('^done,stack=[frame={level="0",func="a"},frame={level="1",func="b"}]')
  assert.deepEqual(r.results.stack.map(e => e.frame.func), ['a', 'b'])
  const names = parseMiLine('^done,register-names=["r0","r1","",""]')
  assert.deepEqual(names.results['register-names'], ['r0', 'r1', '', ''])
})

test('console stream decodes C escapes and octal UTF-8', () => {
  const r = parseMiLine('~"Hello\\t\\"q\\"\\n\\320\\277\\321\\200\\320\\270"')
  assert.equal(r.kind, 'console')
  assert.equal(r.text, 'Hello\t"q"\nпри')
})

test('error record keeps the message', () => {
  const r = parseMiLine('5^error,msg="No symbol \\"x\\" in current context."')
  assert.equal(r.class, 'error')
  assert.equal(r.results.msg, 'No symbol "x" in current context.')
})

test('prompt and garbage lines do not throw', () => {
  assert.equal(parseMiLine('(gdb) ').kind, 'prompt')
  assert.equal(parseMiLine('random text').kind, 'raw')
  assert.equal(parseMiLine('^done,bad={').results._parseError !== undefined, true)
})

test('miQuote escapes quotes, backslashes and newlines', () => {
  assert.equal(miQuote('C:\\a "b"\n'), '"C:\\\\a \\"b\\"\\n"')
})

function fakeGdb(handler) {
  return () => {
    const proc = new EventEmitter()
    proc.stdin = new PassThrough()
    proc.stdout = new PassThrough()
    proc.stderr = new PassThrough()
    proc.exitCode = null
    proc.kill = () => { proc.exitCode = 0; proc.emit('close', 0) }
    proc.stdin.on('data', (d) => {
      for (const line of d.toString().split('\n').filter(Boolean)) {
        const m = /^(\d+)(.*)$/.exec(line)
        handler(m[1], m[2], proc)
      }
    })
    return proc
  }
}

test('commands resolve by token and errors reject with the GDB message', async () => {
  const s = new GdbMiSession({
    gdbPath: 'gdb', elf: 'x.elf',
    spawn: fakeGdb((tok, cmd, p) => {
      if (cmd.startsWith('-data-evaluate')) p.stdout.write(`${tok}^error,msg="No symbol"\n`)
      else p.stdout.write(`${tok}^done\n`)
    }),
  })
  await s.start()
  await assert.rejects(s.command('-data-evaluate-expression "q"'), /No symbol/)
  await s.command('-exec-continue')
})

test('pending commands reject when GDB dies and the exit is reported', async () => {
  let proc
  const s = new GdbMiSession({
    gdbPath: 'gdb', elf: 'x.elf',
    spawn: fakeGdb((tok, cmd, p) => { proc = p; if (!cmd.startsWith('-target-select')) p.stdout.write(`${tok}^done\n`) }),
  })
  await s.start()
  const exited = new Promise((r) => s.once('exit', r))
  const pending = s.command('-target-select remote :1')
  proc.emit('close', 139)
  await assert.rejects(pending, /GDB exited/)
  assert.equal((await exited).code, 139)
  await assert.rejects(s.command('-exec-next'), /not running/)
})

test('command timeout rejects instead of hanging', async () => {
  const s = new GdbMiSession({
    gdbPath: 'gdb', elf: 'x.elf', commandTimeoutMs: 50,
    spawn: fakeGdb((tok, cmd, p) => { if (!cmd.startsWith('-target-select')) p.stdout.write(`${tok}^done\n`) }),
  })
  await s.start()
  await assert.rejects(s.command('-target-select remote :1'), /did not answer/)
})

test('multi-line MI commands are refused (no injection)', async () => {
  const s = new GdbMiSession({ gdbPath: 'gdb', elf: 'x.elf', spawn: fakeGdb((t, c, p) => p.stdout.write(`${t}^done\n`)) })
  await s.start()
  await assert.rejects(s.command('-exec-next\n-gdb-exit'), /single line/)
})

test('remote address and breakpoint paths are validated', () => {
  assert.deepEqual(parseRemote('127.0.0.1:3333'), { host: '127.0.0.1', port: 3333 })
  assert.deepEqual(parseRemote(':3333'), { host: '127.0.0.1', port: 3333 })
  assert.throws(() => parseRemote('evil;host:1'), /Unsupported/)
  assert.throws(() => parseRemote('127.0.0.1:70000'), /Invalid TCP port/)
  assert.throws(() => normalizeSourceFile('/p', '/etc/passwd'), /inside the project/)
  assert.throws(() => normalizeSourceFile('/p', 'a".c'), /invalid/)
  assert.equal(normalizeSourceFile('/p', 'src/main.c'), require('path').resolve('/p/src/main.c'))
})

test('operations require a session and unknown ops are rejected', async () => {
  const dm = new DebugManager({ resolveTool: (n) => n, toolEnv: () => process.env })
  await assert.rejects(dm.op('continue'), (e) => e.code === 'no_session')
  dm.session = { state: 'stopped', gdb: { command: async () => ({}) } }
  await assert.rejects(dm.op('shell', {}), (e) => e.code === 'unknown_op')
  await assert.rejects(dm.op('memory', { address: '0x0" -gdb-exit', length: 4 }), (e) => e.code === 'invalid_address')
  await assert.rejects(dm.op('memory', { address: '0x0', length: 1e6 }), (e) => e.code === 'invalid_length')
  await assert.rejects(dm.op('evaluate', { expression: 'a\nb' }), (e) => e.code === 'invalid_expression')
  dm.session.state = 'running'
  await assert.rejects(dm.op('stepOver'), (e) => e.code === 'target_running')
})

test('breakpoints can be toggled before any session exists', async () => {
  const dm = new DebugManager({ resolveTool: (n) => n, toolEnv: () => process.env })
  const p = require('path').resolve('/proj')
  let list = await dm.toggleBreakpoint(p, 'src/main.c', 10)
  assert.equal(list.length, 1)
  assert.equal(list[0].verified, false)
  list = await dm.toggleBreakpoint(p, require('path').join(p, 'src/main.c'), 10)
  assert.equal(list.length, 0)
  await assert.rejects(dm.toggleBreakpoint(p, 'src/main.c', 0), (e) => e.code === 'invalid_breakpoint')
})
