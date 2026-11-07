/**
 * Debugger integration: the REAL bundled arm-none-eabi-gdb drives a
 * SIMULATED Cortex-M (test/support/rsp-stub.js), directly or behind the
 * OpenOCD stand-in (test/support/fake-openocd.js). Proves the IDE's session
 * lifecycle, GDB/MI handling and failure paths — not real silicon.
 *
 * Skips when the bundled toolchain is missing (npm run toolchain:fetch).
 */
const test = require('node:test')
const assert = require('node:assert/strict')
const fs = require('fs')
const os = require('os')
const net = require('net')
const path = require('path')
const { execFileSync } = require('child_process')

const { resolveTool, getToolchainEnv } = require('../../electron/bundledToolchain')
const { DebugManager } = require('../../electron/debugSession')
const { startStub } = require('../support/rsp-stub')

const gdb = resolveTool('arm-none-eabi-gdb')
const gcc = resolveTool('arm-none-eabi-gcc')
const haveTools = path.isAbsolute(gdb) && path.isAbsolute(gcc) && fs.existsSync(gdb) && fs.existsSync(gcc)
const skip = haveTools ? false : 'bundled arm-none-eabi-gcc/gdb not found (npm run toolchain:fetch)'
const FAKE_OPENOCD = path.join(__dirname, '..', 'support', 'fake-openocd.js')

const SOURCE = `#include <stdint.h>
extern uint32_t _estack;
void Reset_Handler(void);
static void Default_Handler(void) { for (;;) {} }
__attribute__((section(".isr_vector"), used))
const void *const vectors[16] = { &_estack, Reset_Handler, Default_Handler, Default_Handler };

static int add(int x, int y) {
    int s = x + y;
    return s;
}

int main(void) {
    volatile int a = 1;
    volatile int b = 2;
    a = 3;
    b = 4;
    int c = add(a, b);
    a = c;
    for (;;) {}
}

void Reset_Handler(void) { main(); }
`
const LINKER = `MEMORY { FLASH (rx) : ORIGIN = 0x08000000, LENGTH = 1024K
 RAM (xrw) : ORIGIN = 0x20000000, LENGTH = 128K }
_estack = ORIGIN(RAM) + LENGTH(RAM);
SECTIONS { .isr_vector : { KEEP(*(.isr_vector)) } > FLASH
 .text : { *(.text*) } > FLASH .rodata : { *(.rodata*) } > FLASH
 .data : { *(.data*) } > RAM AT > FLASH .bss : { *(.bss*) *(COMMON) } > RAM }
`

let dir
let elf
test.before(() => {
  if (skip) return
  dir = fs.mkdtempSync(path.join(os.tmpdir(), 'embedide dbg '))
  fs.writeFileSync(path.join(dir, 'main.c'), SOURCE)
  fs.writeFileSync(path.join(dir, 'linker.ld'), LINKER)
  elf = path.join(dir, 'fw.elf')
  execFileSync(gcc, ['-mcpu=cortex-m4', '-mthumb', '-O0', '-g3', '-nostdlib', '-T', 'linker.ld', 'main.c', '-o', elf], { cwd: dir })
})
test.after(() => { if (dir) fs.rmSync(dir, { recursive: true, force: true }) })

function manager() {
  return new DebugManager({ resolveTool, toolEnv: () => getToolchainEnv() })
}

function nextEvent(dm, pred, timeoutMs = 10000) {
  return new Promise((resolve, reject) => {
    const t = setTimeout(() => { dm.off('event', h); reject(new Error('event timeout')) }, timeoutMs)
    const h = (e) => { if (pred(e)) { clearTimeout(t); dm.off('event', h); resolve(e) } }
    dm.on('event', h)
  })
}
const stopped = (dm) => nextEvent(dm, (e) => e.type === 'stopped')

function freePort() {
  return new Promise((r) => { const s = net.createServer(); s.listen(0, '127.0.0.1', () => { const p = s.address().port; s.close(() => r(p)) }) })
}

function alive(pid) {
  try { process.kill(pid, 0); return true } catch { return false }
}

test('breakpoints, continue, step over/into/out, locals, registers, memory, evaluate', { skip }, async () => {
  const stub = await startStub({ elf })
  const dm = manager()
  try {
    await dm.toggleBreakpoint(dir, 'main.c', 17) // b = 4;
    const st = await dm.start({ projectDir: dir, elf, mode: 'remote', remote: `127.0.0.1:${stub.port}` })
    assert.equal(st.state, 'stopped')
    assert.equal(st.frame.func, 'Reset_Handler')
    assert.equal(dm.listBreakpoints()[0].verified, true)

    let ev = stopped(dm)
    await dm.op('continue')
    ev = await ev
    assert.equal(ev.reason, 'breakpoint-hit')
    assert.equal(ev.frame.line, 17)
    let vars = (await dm.op('locals')).variables
    assert.equal(vars.find(v => v.name === 'a').value, '3')
    assert.equal(vars.find(v => v.name === 'b').value, '2')

    ev = stopped(dm)
    await dm.op('stepOver')
    ev = await ev
    assert.equal(ev.reason, 'end-stepping-range')
    assert.equal(ev.frame.line, 18)
    vars = (await dm.op('locals')).variables
    assert.equal(vars.find(v => v.name === 'b').value, '4')

    ev = stopped(dm)
    await dm.op('stepInto')
    ev = await ev
    assert.equal(ev.frame.func, 'add')
    const frames = (await dm.op('stack')).frames
    assert.deepEqual(frames.slice(0, 2).map(f => f.func), ['add', 'main'])
    const args = (await dm.op('locals')).variables.filter(v => v.arg)
    assert.deepEqual(args.map(v => `${v.name}=${v.value}`), ['x=3', 'y=4'])

    ev = stopped(dm)
    await dm.op('stepOut')
    ev = await ev
    assert.equal(ev.frame.func, 'main')

    const regs = (await dm.op('registers')).registers
    const byName = Object.fromEntries(regs.map(r => [r.name, r.value]))
    assert.ok(byName.pc && byName.sp && byName.lr && byName.xpsr, 'm-profile core registers present')

    const mem = (await dm.op('memory', { address: '0x08000000', length: 8 })).blocks[0]
    assert.equal(mem.contents.slice(0, 8), '00000220', 'initial SP 0x20020000 from the vector table')
    assert.equal((await dm.op('evaluate', { expression: 'a + b' })).value, '7')
    await assert.rejects(dm.op('evaluate', { expression: 'nope_symbol' }), /No symbol/)

    // remove the breakpoint while stopped, run free, pause
    await dm.toggleBreakpoint(dir, 'main.c', 17)
    assert.equal(dm.listBreakpoints().length, 0)
    const running = nextEvent(dm, (e) => e.type === 'state' && e.state === 'running')
    await dm.op('continue')
    await running
    ev = stopped(dm)
    await dm.op('pause')
    ev = await ev
    assert.equal(ev.reason, 'signal-received')
  } finally {
    const gdbPid = dm.session?.gdb?.proc?.pid
    await dm.stop()
    await stub.close()
    assert.equal(dm.status().active, false)
    if (gdbPid) assert.equal(alive(gdbPid), false, 'GDB process cleaned up')
  }
})

test('OpenOCD mode: attaching does not flash; loadFirmware flashes; reset works', { skip }, async () => {
  const port = await freePort()
  const dm = manager()
  const base = { projectDir: dir, elf, mode: 'openocd', adapter: 'stlink', target: 'stm32f4x', gdbPort: port,
    openocdPath: process.execPath, openocdPrefixArgs: [FAKE_OPENOCD] }
  const env = process.env.FAKE_OPENOCD_ELF
  process.env.FAKE_OPENOCD_ELF = elf
  try {
    const out = []
    dm.on('event', (e) => { if (e.type === 'output') out.push(e.text) })
    await dm.start({ ...base, loadFirmware: false })
    assert.ok(!out.join('').includes('Flashing'), 'no flash without explicit request')
    const ocdPid = dm.session.openocd.pid
    await dm.op('reset')
    assert.equal(dm.status().frame.func, 'Reset_Handler')
    await dm.stop()
    assert.equal(alive(ocdPid), false, 'OpenOCD stopped with the session')

    out.length = 0
    await dm.start({ ...base, loadFirmware: true })
    assert.ok(out.join('').includes('Firmware written'), 'flash only on request')
    await dm.stop()
  } finally {
    if (env === undefined) delete process.env.FAKE_OPENOCD_ELF; else process.env.FAKE_OPENOCD_ELF = env
    await dm.stop()
  }
})

test('a second session is refused while one is active', { skip }, async () => {
  const stub = await startStub({ elf })
  const dm = manager()
  try {
    await dm.start({ projectDir: dir, elf, mode: 'remote', remote: `127.0.0.1:${stub.port}` })
    await assert.rejects(dm.start({ projectDir: dir, elf, mode: 'remote', remote: `127.0.0.1:${stub.port}` }),
      (e) => e.code === 'session_active')
  } finally {
    await dm.stop()
    await stub.close()
  }
})

test('OpenOCD without a probe fails with a structured error and its output', { skip }, async () => {
  const dm = manager()
  process.env.FAKE_OPENOCD_FAIL = '1'
  try {
    await assert.rejects(dm.start({ projectDir: dir, elf, mode: 'openocd', adapter: 'stlink', target: 'stm32f4x',
      gdbPort: await freePort(), openocdPath: process.execPath, openocdPrefixArgs: [FAKE_OPENOCD] }),
    (e) => e.code === 'openocd_failed' && /No ST-LINK/.test(e.detail))
    assert.equal(dm.status().active, false)
  } finally {
    delete process.env.FAKE_OPENOCD_FAIL
  }
})

test('OpenOCD that never becomes ready times out and is killed', { skip }, async () => {
  const dm = manager()
  process.env.FAKE_OPENOCD_SILENT = '1'
  try {
    await assert.rejects(dm.start({ projectDir: dir, elf, mode: 'openocd', adapter: 'stlink', target: 'stm32f4x',
      gdbPort: await freePort(), openocdPath: process.execPath, openocdPrefixArgs: [FAKE_OPENOCD], openocdTimeoutMs: 1500 }),
    (e) => e.code === 'openocd_timeout')
  } finally {
    delete process.env.FAKE_OPENOCD_SILENT
  }
})

test('probe disconnect mid-session ends the session and cleans up', { skip }, async () => {
  const dm = manager()
  process.env.FAKE_OPENOCD_ELF = elf
  process.env.FAKE_OPENOCD_DIE_AFTER_MS = '1500'
  try {
    await dm.start({ projectDir: dir, elf, mode: 'openocd', adapter: 'stlink', target: 'stm32f4x',
      gdbPort: await freePort(), openocdPath: process.execPath, openocdPrefixArgs: [FAKE_OPENOCD] })
    const gdbPid = dm.session.gdb.proc.pid
    const ended = await nextEvent(dm, (e) => e.type === 'ended', 10000)
    assert.ok(['openocd-exited', 'gdb-exited'].includes(ended.reason), ended.reason)
    assert.equal(dm.status().active, false)
    await new Promise((r) => setTimeout(r, 300))
    assert.equal(alive(gdbPid), false, 'GDB cleaned up after disconnect')
    // a new session can start afterwards (no stale state)
    delete process.env.FAKE_OPENOCD_DIE_AFTER_MS
    await dm.start({ projectDir: dir, elf, mode: 'openocd', adapter: 'stlink', target: 'stm32f4x',
      gdbPort: await freePort(), openocdPath: process.execPath, openocdPrefixArgs: [FAKE_OPENOCD] })
    await dm.stop()
  } finally {
    delete process.env.FAKE_OPENOCD_ELF
    delete process.env.FAKE_OPENOCD_DIE_AFTER_MS
    await dm.stop()
  }
})

test('busy GDB port is reported before OpenOCD starts', { skip }, async () => {
  const srv = net.createServer()
  await new Promise((r) => srv.listen(0, '127.0.0.1', r))
  const dm = manager()
  try {
    await assert.rejects(dm.start({ projectDir: dir, elf, mode: 'openocd', adapter: 'stlink', target: 'stm32f4x',
      gdbPort: srv.address().port, openocdPath: process.execPath, openocdPrefixArgs: [FAKE_OPENOCD] }),
    (e) => e.code === 'port_busy')
  } finally {
    await new Promise((r) => srv.close(r))
  }
})

test('GDB crash mid-session is reported and the session is cleared', { skip }, async () => {
  const stub = await startStub({ elf })
  const dm = manager()
  try {
    await dm.start({ projectDir: dir, elf, mode: 'remote', remote: `127.0.0.1:${stub.port}` })
    const ended = nextEvent(dm, (e) => e.type === 'ended')
    dm.session.gdb.proc.kill('SIGKILL')
    assert.equal((await ended).reason, 'gdb-exited')
    assert.equal(dm.status().active, false)
  } finally {
    await dm.stop()
    await stub.close()
  }
})

test('unreachable GDB server fails cleanly', { skip }, async () => {
  const dm = manager()
  await assert.rejects(dm.start({ projectDir: dir, elf, mode: 'remote', remote: `127.0.0.1:${await freePort()}` }),
    (e) => /refused|Connection|timed out/i.test(e.message))
  assert.equal(dm.status().active, false)
})
