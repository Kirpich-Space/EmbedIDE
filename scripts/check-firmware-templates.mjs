#!/usr/bin/env node
/**
 * Build every firmware template through the same createProject/buildProject
 * code the Electron main process uses, then check that the result is an
 * ARM ELF (e_machine 40) with a vector table in flash.
 *
 * Needs the bundled toolchain (npm run toolchain:fetch). Rust templates
 * also need the board's rustup target and network access for crates.io
 * the first time.
 *
 *   node scripts/check-firmware-templates.mjs [--boards a,b] [--types c,cpp] [--json out.json]
 *
 * No hardware is touched: this only compiles and links.
 */
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from 'fs'
import { tmpdir } from 'os'
import { join } from 'path'
import { createRequire } from 'module'

const require = createRequire(import.meta.url)
const { createProject, getTemplateList } = require('../electron/project.js')
const { buildProject, detectElf } = require('../electron/toolchain.js')
const { getBoard } = require('../electron/boards.js')

const args = process.argv.slice(2)
const opt = (name) => {
  const i = args.indexOf(name)
  return i >= 0 ? args[i + 1] : undefined
}
const boards = (opt('--boards') || 'stm32f407vg,stm32f030r8,stm32f103c8,stm32h743zi,nrf52840').split(',')
const firmwareTypes = getTemplateList()
  .map(t => t.id)
  .filter(id => !id.startsWith('script-'))
const types = opt('--types') ? opt('--types').split(',') : firmwareTypes
const jsonOut = opt('--json')

/** ELF32 little-endian section headers → [{ name, addr, offset, size }] */
function sections(b) {
  const shoff = b.readUInt32LE(32)
  const shentsize = b.readUInt16LE(46)
  const shnum = b.readUInt16LE(48)
  const shstrndx = b.readUInt16LE(50)
  const hdr = (i) => {
    const o = shoff + i * shentsize
    return { nameOff: b.readUInt32LE(o), addr: b.readUInt32LE(o + 12), offset: b.readUInt32LE(o + 16), size: b.readUInt32LE(o + 20) }
  }
  const strtab = hdr(shstrndx)
  const out = []
  for (let i = 0; i < shnum; i++) {
    const h = hdr(i)
    const start = strtab.offset + h.nameOff
    const name = b.toString('latin1', start, b.indexOf(0, start))
    out.push({ name, addr: h.addr, offset: h.offset, size: h.size })
  }
  return out
}

const hex = (n) => '0x' + n.toString(16).padStart(8, '0')

/**
 * The image must start with a usable Cortex-M vector table: initial SP
 * inside the board's RAM, reset handler in flash with the Thumb bit set.
 */
function inspectElf(file, board) {
  const b = readFileSync(file)
  if (b.length < 52 || b.readUInt32BE(0) !== 0x7f454c46) return { ok: false, why: 'not an ELF' }
  if (b[4] !== 1 || b[5] !== 1) return { ok: false, why: 'not ELF32 little-endian' }
  const machine = b.readUInt16LE(18)
  if (machine !== 40) return { ok: false, why: `e_machine ${machine}, expected 40 (ARM)` }
  const vt = sections(b).find(s => s.name === '.isr_vector' || s.name === '.vector_table')
  if (!vt || vt.size < 8) return { ok: false, why: 'no .isr_vector/.vector_table section' }
  const sp = b.readUInt32LE(vt.offset)
  const reset = b.readUInt32LE(vt.offset + 4)
  const flashLo = Number(board.flashOrigin ?? 0x08000000)
  const flashHi = flashLo + board.flashKb * 1024
  const ramLo = Number(board.ramOrigin ?? 0x20000000)
  const ramHi = ramLo + board.ramKb * 1024
  if (vt.addr < flashLo || vt.addr >= flashHi) return { ok: false, why: `vector table at ${hex(vt.addr)} outside flash` }
  if (sp <= ramLo || sp > ramHi) return { ok: false, why: `initial SP ${hex(sp)} outside RAM ${hex(ramLo)}..${hex(ramHi)}` }
  if ((reset & 1) !== 1) return { ok: false, why: `reset vector ${hex(reset)} lacks the Thumb bit` }
  if ((reset & ~1) < flashLo || (reset & ~1) >= flashHi) return { ok: false, why: `reset vector ${hex(reset)} outside flash` }
  return { ok: true, sp: hex(sp), reset: hex(reset), bytes: b.length }
}

const root = mkdtempSync(join(tmpdir(), 'embedide-fw-'))
const results = []
for (const boardId of boards) {
  if (!getBoard(boardId)) {
    results.push({ boardId, type: '*', status: 'SKIP', detail: 'unknown board id' })
    continue
  }
  for (const type of types) {
    const name = `t_${type.replace(/-/g, '_')}_${boardId}`
    let dir
    const log = []
    const t0 = Date.now()
    try {
      dir = createProject(root, name, type, boardId)
      await buildProject(dir, type, (o) => log.push(o.text))
      const elf = detectElf(dir, type)
      if (!elf) throw new Error('build succeeded but no ELF was found')
      const info = inspectElf(elf, getBoard(boardId))
      if (!info.ok) throw new Error(info.why)
      // A fresh template should compile cleanly; report compiler warnings as failures
      const warnings = log.join('').split('\n').filter(l => /\bwarning:/.test(l) && !/cargo:warning|warning: unused manifest key/.test(l))
      if (warnings.length) throw new Error(`template compiles with ${warnings.length} warning(s): ${warnings[0].trim()}`)
      results.push({ boardId, type, status: 'PASS', ms: Date.now() - t0, elf: elf.slice(root.length + 1), entry: `sp=${info.sp} reset=${info.reset}` })
    } catch (err) {
      const tail = log.join('').split('\n').filter(Boolean).slice(-6).join('\n')
      results.push({ boardId, type, status: 'FAIL', ms: Date.now() - t0, detail: `${err.message}\n${tail}` })
    }
    const r = results[results.length - 1]
    console.log(`${r.status.padEnd(4)}  ${boardId.padEnd(12)} ${type.padEnd(12)} ${r.status === 'PASS' ? `${r.elf} entry=${r.entry}` : r.detail.split('\n')[0]}`)
  }
}

const count = (s) => results.filter(r => r.status === s).length
console.log(`\n${count('PASS')} PASS, ${count('FAIL')} FAIL, ${count('SKIP')} SKIP`)
for (const r of results.filter(r => r.status === 'FAIL')) {
  console.log(`\n--- ${r.boardId} ${r.type}\n${r.detail}`)
}
if (jsonOut) writeFileSync(jsonOut, JSON.stringify(results, null, 2))
if (!process.env.KEEP) rmSync(root, { recursive: true, force: true })
else console.log(`kept: ${root}`)
process.exit(count('FAIL') ? 1 : 0)
