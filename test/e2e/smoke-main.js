/**
 * GUI smoke test: drives the real EmbedIDE window (electron/main.js + the
 * built renderer in dist/) with real keyboard/mouse input events.
 *
 *   npm run build && xvfb-run -a npx electron test/e2e/smoke-main.js
 *
 * Isolation: appData and documents point to a fresh temp directory, so the
 * user's settings and projects are never touched. The debug target is the
 * SIMULATED Cortex-M from test/support/rsp-stub.js reached as a GDB server;
 * the serial device is a pseudo-terminal (POSIX). Nothing is flashed.
 * Prints one JSON line per check and exits 1 if any check fails.
 */
const path = require('path')
const fs = require('fs')
const os = require('os')
const { spawn } = require('child_process')
const readline = require('readline')
const { app, dialog, BrowserWindow } = require('electron')

const ROOT = fs.mkdtempSync(path.join(os.tmpdir(), 'embedide e2e '))
app.setPath('appData', path.join(ROOT, 'appdata'))
app.setPath('documents', path.join(ROOT, 'documents'))
const SHOTS = process.env.EMBEDIDE_E2E_SHOTS || path.join(ROOT, 'shots')
fs.mkdirSync(SHOTS, { recursive: true })

const results = []
const consoleErrors = []
function check(name, ok, detail = '') {
  results.push({ name, ok: !!ok, detail })
  process.stdout.write(JSON.stringify({ check: name, ok: !!ok, detail: String(detail).slice(0, 300) }) + '\n')
}

require('../../electron/main.js')

const sleep = (ms) => new Promise((r) => setTimeout(r, ms))

let win
async function js(code) {
  return win.webContents.executeJavaScript(code, true)
}
async function waitFor(code, timeoutMs = 15000, label = code) {
  const end = Date.now() + timeoutMs
  for (;;) {
    let v
    try { v = await js(code) } catch (e) { v = undefined }
    if (v) return v
    if (Date.now() > end) throw new Error(`timeout: ${label}`)
    await sleep(100)
  }
}
async function key(keyCode, modifiers = []) {
  win.webContents.sendInputEvent({ type: 'keyDown', keyCode, modifiers })
  win.webContents.sendInputEvent({ type: 'keyUp', keyCode, modifiers })
  await sleep(80)
}
async function clickSel(sel) {
  const ok = await js(`(() => { const el = document.querySelector(${JSON.stringify(sel)}); if (!el) return false; el.click(); return true })()`)
  if (!ok) throw new Error(`no element ${sel}`)
  await sleep(120)
}
async function clickText(sel, text) {
  const ok = await js(`(() => { const el = [...document.querySelectorAll(${JSON.stringify(sel)})].find(e => e.textContent.trim().includes(${JSON.stringify(text)})); if (!el) return false; el.click(); return true })()`)
  if (!ok) throw new Error(`no ${sel} with text "${text}"`)
  await sleep(150)
}
async function setValue(sel, value) {
  const ok = await js(`(() => {
    const el = document.querySelector(${JSON.stringify(sel)}); if (!el) return false;
    const proto = el instanceof HTMLSelectElement ? HTMLSelectElement.prototype : HTMLInputElement.prototype;
    Object.getOwnPropertyDescriptor(proto, 'value').set.call(el, ${JSON.stringify(value)});
    el.dispatchEvent(new Event(el instanceof HTMLSelectElement ? 'change' : 'input', { bubbles: true }));
    return true })()`)
  if (!ok) throw new Error(`no input ${sel}`)
  await sleep(100)
}
async function realClick(x, y) {
  win.webContents.sendInputEvent({ type: 'mouseDown', x: Math.round(x), y: Math.round(y), button: 'left', clickCount: 1 })
  win.webContents.sendInputEvent({ type: 'mouseUp', x: Math.round(x), y: Math.round(y), button: 'left', clickCount: 1 })
  await sleep(150)
}
async function shot(name) {
  try {
    const img = await win.webContents.capturePage()
    fs.writeFileSync(path.join(SHOTS, `${name}.png`), img.toPNG())
  } catch {}
}
async function outputText() {
  return js(`[...document.querySelectorAll('.output-body *')].map(e => e.textContent).join('\\n')`)
}
async function replaceEditorText(text) {
  await js(`document.querySelector('.cm-content').focus()`)
  await key('A', ['control'])
  win.webContents.insertText(text)
  await sleep(250)
}

const PROGRAM = `#include <stdint.h>

void SystemInit(void) {}

int main(void) {
    volatile int a = 1;
    volatile int b = 2;
    a = 3;
    b = 4;
    for (;;) {
        __asm volatile ("nop");
    }
}
`
const BP_LINE = 9 // b = 4;

class PtyDevice {
  constructor() {
    this.p = spawn('python3', [path.join(__dirname, '..', 'support', 'pty-device.py')], { stdio: ['pipe', 'pipe', 'inherit'] })
    this.q = []
    this.w = []
    readline.createInterface({ input: this.p.stdout }).on('line', (l) => { const f = this.w.shift(); f ? f(l) : this.q.push(l) })
  }
  cmd(c) { return new Promise((r) => { this.w.push(r); this.p.stdin.write(c + '\n') }) }
  quit() { try { this.p.stdin.write('quit\n') } catch {} }
}

async function run() {
  win = BrowserWindow.getAllWindows()[0]
  win.webContents.on('console-message', (_e, level, message) => { if (level >= 3) consoleErrors.push(message) })
  await waitFor(`!!document.querySelector('.editor-empty')`, 20000, 'renderer ready')
  check('app starts and renders the empty editor', true)

  // ── create project through the dialog ──
  await key('N', ['control'])
  await waitFor(`!!document.querySelector('.project-dialog .project-input')`, 5000, 'project dialog')
  await setValue('.project-dialog .project-input', 'smoke')
  await clickSel('.project-btn-create')
  await waitFor(`[...document.querySelectorAll('.file-item')].some(e => e.textContent.includes('main.c'))`, 10000, 'file tree')
  const projDir = path.join(ROOT, 'documents', 'EmbedIDE-Projects', 'smoke')
  check('project created in the projects folder', fs.existsSync(path.join(projDir, 'embedide.json')), projDir)

  // ── open, edit, save ──
  await clickText('.file-item', 'main.c')
  await waitFor(`!!document.querySelector('.cm-content')`, 5000, 'editor')
  await replaceEditorText(PROGRAM)
  check('editing marks the tab dirty', await waitFor(`!!document.querySelector('.tab-dirty')`, 3000))
  await key('S', ['control'])
  await waitFor(`!document.querySelector('.tab-dirty')`, 5000, 'saved')
  const onDisk = fs.readFileSync(path.join(projDir, 'src', 'main.c'), 'utf8')
  check('Ctrl+S writes exactly the editor content', onDisk === PROGRAM, onDisk.slice(0, 80))
  await shot('01-edited')

  // ── build error is shown in the editor ──
  await replaceEditorText(PROGRAM.replace('b = 4;', 'b = ;'))
  await key('S', ['control'])
  await key('B', ['control'])
  await waitFor(`!!document.querySelector('.cm-lintRange-error, .cm-lint-marker-error')`, 60000, 'diagnostic in editor')
  const errOut = await outputText()
  check('compile error appears in Output and as an editor diagnostic', /error/i.test(errOut), errOut.split('\n').find(l => /error/.test(l)))
  await shot('02-build-error')

  // ── successful build ──
  await replaceEditorText(PROGRAM)
  await key('S', ['control'])
  await clickText('.app-bottom-tab', 'Output').catch(() => {})
  await key('B', ['control'])
  const elf = path.join(projDir, 'build', 'smoke.elf')
  let built = false
  for (const end = Date.now() + 90000; Date.now() < end && !built; await sleep(200)) built = fs.existsSync(elf)
  await waitFor(`[...document.querySelectorAll('.output-body *')].some(e => /FLASH: \\d+ bytes/.test(e.textContent))`, 60000, 'size report')
  check('Ctrl+B builds an ARM ELF with the bundled toolchain', built && fs.readFileSync(elf).readUInt16LE(18) === 40, elf)

  // ── breakpoint by clicking the gutter ──
  await clickText('.app-bottom-tab', 'Output').catch(() => {})
  const rect = await js(`(() => {
    const lines = document.querySelectorAll('.cm-debug-gutter .cm-gutterElement');
    const content = [...document.querySelectorAll('.cm-line')];
    const target = content[${BP_LINE - 1}]; if (!target) return null;
    const g = document.querySelector('.cm-debug-gutter').getBoundingClientRect();
    const t = target.getBoundingClientRect();
    return { x: g.left + g.width / 2, y: t.top + t.height / 2 } })()`)
  await realClick(rect.x, rect.y)
  await waitFor(`!!document.querySelector('.cm-bp')`, 3000, 'breakpoint marker')
  const st = await js('window.electronAPI.debugStatus()')
  check('gutter click sets a breakpoint on that line', st.breakpoints.length === 1 && st.breakpoints[0].line === BP_LINE, JSON.stringify(st.breakpoints))

  // ── debug session against a GDB server (simulated target) ──
  const { startStub } = require('../support/rsp-stub')
  const stub = await startStub({ elf })
  await clickText('.app-bottom-tab', 'Debug')
  await setValue('.debug-config select', 'remote')
  await setValue('.debug-config input', `127.0.0.1:${stub.port}`)
  await key('F5')
  await waitFor(`!!document.querySelector('.debug-state-stopped')`, 60000, 'session halted')
  check('F5 builds, starts GDB and halts at reset', true, await js(`document.querySelector('.debug-where')?.textContent`))
  await clickText('.debug-btn', 'Continue')
  await waitFor(`(document.querySelector('.debug-where')?.textContent || '').includes(':${BP_LINE}')`, 15000, 'breakpoint hit')
  await waitFor(`!!document.querySelector('.cm-exec-line')`, 3000, 'execution line highlight')
  check('continue stops at the breakpoint and highlights the line', true)
  const varsText = await waitFor(`(() => { const t = [...document.querySelectorAll('.debug-table tr')].map(r => r.textContent).join(' | '); return t.includes('a') ? t : '' })()`, 5000, 'locals')
  check('variables panel shows locals at the breakpoint', /a.*3/.test(varsText), varsText)
  const regsText = await js(`[...document.querySelectorAll('.debug-section')].find(s => s.textContent.includes('Registers'))?.textContent || ''`)
  check('registers panel shows pc/sp', /pc/.test(regsText) && /sp/.test(regsText))
  await key('F10')
  await waitFor(`!(document.querySelector('.debug-where')?.textContent || '').includes(':${BP_LINE}')`, 10000, 'stepped')
  check('F10 steps over to the next line', true, await js(`document.querySelector('.debug-where')?.textContent`))
  await setValue('.debug-section .debug-eval input', '0x08000000')
  await clickText('.debug-btn', 'Read')
  const mem = await waitFor(`document.querySelector('.debug-memory')?.textContent`, 5000, 'memory')
  check('memory view reads the vector table', /^0x08000000\s+00 00 02 20/.test(mem), mem.split('\n')[0])
  await shot('03-debug-halted')
  await clickText('.debug-btn', 'Stop')
  await waitFor(`!!document.querySelector('.debug-btn-start')`, 10000, 'session stopped')
  check('Stop ends the session', (await js('window.electronAPI.debugStatus()')).active === false)
  await stub.close()

  // ── flash asks first; cancel writes nothing ──
  const before = await outputText()
  await key('B', ['control', 'shift'])
  await waitFor(`!!document.querySelector('.confirm-dialog')`, 3000, 'flash confirm')
  const msg = await js(`document.querySelector('.confirm-dialog').textContent`)
  check('flash requires confirmation naming the board', /STM32F407VG/.test(msg), msg.slice(0, 160))
  await clickSel('.confirm-dialog .project-btn-cancel')
  await sleep(500)
  const after = await outputText()
  check('cancelling the confirmation does not start flashing', !/Starting flash/i.test(after.slice(before.length)))

  // ── serial monitor on a pseudo-terminal ──
  if (process.platform !== 'win32') {
    const dev = new PtyDevice()
    const link = path.join(ROOT, 'ttyE2E')
    await dev.cmd(`new ${link}`)
    await clickText('.app-bottom-tab', 'Serial')
    await setValue('.serial-port-input', link)
    await clickText('.serial-btn', 'Connect')
    await waitFor(`!!document.querySelector('.serial-status-open')`, 8000, 'serial open')
    await dev.cmd(`send ${Buffer.from('hello Привет\r\n').toString('hex')}`)
    await waitFor(`[...document.querySelectorAll('.serial-line')].some(e => e.textContent === 'hello Привет')`, 5000, 'serial rx')
    check('serial monitor shows received UTF-8 text', true)
    await setValue('.serial-input', 'ping')
    await js(`document.querySelector('.serial-input').focus()`)
    await key('Return')
    await sleep(400)
    const got = Buffer.from((await dev.cmd('read')).slice(4), 'hex').toString()
    check('serial send transmits text + LF', got === 'ping\n', JSON.stringify(got))
    await dev.cmd('vanish')
    await waitFor(`!!document.querySelector('.serial-status-lost, .serial-status-reconnecting')`, 8000, 'port lost')
    check('unplugged port is reported', true)
    await dev.cmd(`new ${link}`)
    await waitFor(`!!document.querySelector('.serial-status-open')`, 10000, 'reconnected')
    check('serial reconnects when the port returns', true)
    await shot('04-serial')
    await clickText('.serial-btn', 'Disconnect')
    dev.quit()
  }

  // ── reopen the project ──
  dialog.showOpenDialog = async () => ({ canceled: false, filePaths: [projDir] })
  await key('O', ['control'])
  await waitFor(`[...document.querySelectorAll('.file-item')].some(e => e.textContent.includes('main.c'))`, 10000, 'reopened tree')
  await clickText('.file-item', 'main.c')
  await sleep(500)
  const reopened = await js(`document.querySelector('.cm-content')?.innerText || ''`)
  check('reopened project shows the saved source', reopened.includes('b = 4;'))

  // ── IPC boundary: folders that were not opened are refused ──
  const refused = await js(`window.electronAPI.listProjectFiles('/etc').then(() => false, () => true)`)
  check('renderer cannot list folders that are not open projects', refused)

  check('no renderer console errors', consoleErrors.length === 0, consoleErrors.slice(0, 3).join(' | '))

  // ── closing with unsaved changes asks; Cancel keeps the window ──
  await js(`document.querySelector('.cm-content').focus()`)
  win.webContents.insertText('// unsaved\n')
  await waitFor(`!!document.querySelector('.tab-dirty')`, 3000)
  win.close()
  await waitFor(`[...document.querySelectorAll('.dirty-dialog .settings-title')].some(e => /Unsaved/.test(e.textContent))`, 5000, 'close prompt')
  await clickSel('.dirty-dialog .project-btn-cancel')
  await sleep(500)
  check('close with unsaved changes asks and Cancel keeps the window', !win.isDestroyed())
  await shot('05-close-cancelled')
}

app.whenReady().then(async () => {
  // main.js creates the window in its own whenReady handler
  await sleep(300)
  let code = 0
  try {
    await run()
  } catch (err) {
    check('e2e run completed', false, err.message)
    await shot('99-failure')
  }
  const failed = results.filter((r) => !r.ok)
  code = failed.length ? 1 : 0
  process.stdout.write(JSON.stringify({ summary: { pass: results.length - failed.length, fail: failed.length, shots: SHOTS } }) + '\n')
  // Discard on purpose: the test changes are throwaway
  try { BrowserWindow.getAllWindows().forEach((w) => w.destroy()) } catch {}
  if (!process.env.EMBEDIDE_E2E_SHOTS && !code) fs.rmSync(ROOT, { recursive: true, force: true })
  app.exit(code)
})
