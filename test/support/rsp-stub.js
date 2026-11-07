/**
 * Minimal GDB Remote Serial Protocol server for tests — a SIMULATED Cortex-M.
 *
 * It lets the real arm-none-eabi-gdb talk to "a target" without hardware:
 * memory comes from the ELF's PT_LOAD segments, registers follow the
 * org.gnu.gdb.arm.m-profile layout, "monitor reset halt" loads SP/PC from
 * the vector table, and a tiny Thumb interpreter executes the handful of
 * instructions that gcc -O0 emits for straight-line code (movs, str/ldr via
 * r7, push/pop, bl/bx lr, b, add/sub sp). Everything else is a no-op that
 * only advances PC. This proves the IDE ↔ GDB/MI ↔ GDB ↔ RSP plumbing;
 * it is not evidence of behaviour on real silicon.
 */
const net = require('net')
const fs = require('fs')

const REG_NAMES = ['r0', 'r1', 'r2', 'r3', 'r4', 'r5', 'r6', 'r7', 'r8', 'r9', 'r10', 'r11', 'r12', 'sp', 'lr', 'pc', 'xpsr']
const SP = 13, LR = 14, PC = 15, XPSR = 16

const TARGET_XML = `<?xml version="1.0"?>
<!DOCTYPE target SYSTEM "gdb-target.dtd">
<target version="1.0">
<architecture>arm</architecture>
<feature name="org.gnu.gdb.arm.m-profile">
${REG_NAMES.map((n, i) => `<reg name="${n}" bitsize="32" regnum="${i}"${n === 'sp' ? ' type="data_ptr"' : n === 'pc' || n === 'lr' ? ' type="code_ptr"' : ''}/>`).join('\n')}
</feature>
</target>`

class Memory {
  constructor() { this.pages = new Map() }
  page(addr) {
    const k = Math.floor(addr / 4096)
    let p = this.pages.get(k)
    if (!p) { p = Buffer.alloc(4096); this.pages.set(k, p) }
    return [p, addr % 4096]
  }
  read8(a) { const [p, o] = this.page(a >>> 0); return p[o] }
  write8(a, v) { const [p, o] = this.page(a >>> 0); p[o] = v & 0xff }
  read(a, n) { const b = Buffer.alloc(n); for (let i = 0; i < n; i++) b[i] = this.read8(a + i); return b }
  write(a, buf) { for (let i = 0; i < buf.length; i++) this.write8(a + i, buf[i]) }
  read16(a) { return this.read8(a) | (this.read8(a + 1) << 8) }
  read32(a) { return (this.read8(a) | (this.read8(a + 1) << 8) | (this.read8(a + 2) << 16) | (this.read8(a + 3) << 24)) >>> 0 }
  write32(a, v) { for (let i = 0; i < 4; i++) this.write8(a + i, (v >>> (8 * i)) & 0xff) }
}

function loadElf(mem, file) {
  const b = fs.readFileSync(file)
  const phoff = b.readUInt32LE(28)
  const phentsize = b.readUInt16LE(42)
  const phnum = b.readUInt16LE(44)
  for (let i = 0; i < phnum; i++) {
    const o = phoff + i * phentsize
    if (b.readUInt32LE(o) !== 1) continue // PT_LOAD
    const offset = b.readUInt32LE(o + 4)
    const paddr = b.readUInt32LE(o + 12)
    const filesz = b.readUInt32LE(o + 16)
    mem.write(paddr, b.subarray(offset, offset + filesz))
  }
}

const sext = (v, bits) => (v << (32 - bits)) >> (32 - bits)

class Cpu {
  constructor(mem, flashBase) {
    this.mem = mem
    this.flashBase = flashBase
    this.r = new Array(17).fill(0)
    this.reset()
  }
  reset() {
    this.r.fill(0)
    this.r[SP] = this.mem.read32(this.flashBase)
    this.r[PC] = this.mem.read32(this.flashBase + 4) & ~1
    this.r[XPSR] = 0x01000000
    this.r[LR] = 0xffffffff
  }
  push(v) { this.r[SP] = (this.r[SP] - 4) >>> 0; this.mem.write32(this.r[SP], v >>> 0) }
  pop() { const v = this.mem.read32(this.r[SP]); this.r[SP] = (this.r[SP] + 4) >>> 0; return v }
  /** Execute one instruction (subset); returns nothing. */
  step() {
    const r = this.r
    const pc = r[PC]
    const hw = this.mem.read16(pc)
    const top5 = hw >>> 11
    if (top5 === 0b11101 || top5 === 0b11110 || top5 === 0b11111) {
      const hw2 = this.mem.read16(pc + 2)
      // BL T1
      if ((hw & 0xf800) === 0xf000 && (hw2 & 0xd000) === 0xd000) {
        const s = (hw >> 10) & 1
        const j1 = (hw2 >> 13) & 1, j2 = (hw2 >> 11) & 1
        const i1 = ~(j1 ^ s) & 1, i2 = ~(j2 ^ s) & 1
        const imm = (s << 24) | (i1 << 23) | (i2 << 22) | ((hw & 0x3ff) << 12) | ((hw2 & 0x7ff) << 1)
        r[LR] = (pc + 4) | 1
        r[PC] = (pc + 4 + sext(imm, 25)) >>> 0
        return
      }
      r[PC] = pc + 4
      return
    }
    let next = pc + 2
    if ((hw & 0xf800) === 0x2000) r[(hw >> 8) & 7] = hw & 0xff // MOVS Rd,#imm8
    else if ((hw & 0xf800) === 0x6000 || (hw & 0xf800) === 0x6800) { // STR/LDR Rt,[Rn,#imm5*4]
      const imm = ((hw >> 6) & 0x1f) * 4, rn = (hw >> 3) & 7, rt = hw & 7
      const addr = (r[rn] + imm) >>> 0
      if (hw & 0x0800) r[rt] = this.mem.read32(addr)
      else this.mem.write32(addr, r[rt])
    } else if ((hw & 0xf000) === 0x9000) { // STR/LDR Rt,[SP,#imm8*4]
      const addr = (r[SP] + (hw & 0xff) * 4) >>> 0, rt = (hw >> 8) & 7
      if (hw & 0x0800) r[rt] = this.mem.read32(addr)
      else this.mem.write32(addr, r[rt])
    } else if ((hw & 0xf800) === 0x4800) { // LDR Rt,[PC,#imm8*4] (literal pool)
      r[(hw >> 8) & 7] = this.mem.read32((((pc + 4) & ~3) + (hw & 0xff) * 4) >>> 0)
    } else if ((hw & 0xff80) === 0xb080) r[SP] = (r[SP] - (hw & 0x7f) * 4) >>> 0 // SUB SP
    else if ((hw & 0xff80) === 0xb000) r[SP] = (r[SP] + (hw & 0x7f) * 4) >>> 0 // ADD SP
    else if ((hw & 0xff00) === 0xaf00) r[7] = (r[SP] + (hw & 0xff) * 4) >>> 0 // ADD r7,SP,#imm
    else if ((hw & 0xff00) === 0x4600 || (hw & 0xff00) === 0x4400) { // MOV / ADD (register, high regs)
      const rd = (hw & 7) | ((hw >> 4) & 8), rm = (hw >> 3) & 0xf
      if (rd !== PC) r[rd] = ((hw & 0x0200) ? r[rm] : (r[rd] + r[rm])) >>> 0
    } else if ((hw & 0xf800) === 0x3000) { // ADDS Rdn,#imm8
      const rd = (hw >> 8) & 7
      r[rd] = (r[rd] + (hw & 0xff)) >>> 0
    } else if ((hw & 0xfe00) === 0xb400) { // PUSH {list[,lr]}
      const list = hw & 0xff
      if (hw & 0x100) this.push(r[LR])
      for (let i = 7; i >= 0; i--) if (list & (1 << i)) this.push(r[i])
    } else if ((hw & 0xfe00) === 0xbc00) { // POP {list[,pc]}
      const list = hw & 0xff
      for (let i = 0; i <= 7; i++) if (list & (1 << i)) r[i] = this.pop()
      if (hw & 0x100) next = this.pop() & ~1
    } else if (hw === 0x4770) next = r[LR] & ~1 // BX LR
    else if ((hw & 0xf800) === 0xe000) next = (pc + 4 + sext((hw & 0x7ff) << 1, 12)) >>> 0 // B T2
    r[PC] = next >>> 0
  }
}

function checksum(s) {
  let c = 0
  for (const ch of Buffer.from(s, 'latin1')) c = (c + ch) & 0xff
  return c.toString(16).padStart(2, '0')
}

const hex32 = (v) => {
  const b = Buffer.alloc(4); b.writeUInt32LE(v >>> 0); return b.toString('hex')
}

/**
 * @param {object} o
 * @param {string} [o.elf]        preload (simulates a board that already holds firmware)
 * @param {number} [o.port]       0 = ephemeral
 * @param {number} [o.flashBase]
 * @returns {Promise<{port:number, close:Function, stats:object}>}
 */
function startStub(o = {}) {
  const mem = new Memory()
  const flashBase = o.flashBase ?? 0x08000000
  if (o.elf) loadElf(mem, o.elf)
  const cpu = new Cpu(mem, flashBase)
  const stats = { connections: 0, writes: 0, resets: 0, packets: [] }
  const breakpoints = new Set()

  const server = net.createServer((sock) => {
    stats.connections++
    let buf = ''
    let running = false
    let runTimer = null
    const send = (data) => { if (!sock.destroyed) sock.write(`$${data}#${checksum(data)}`) }
    const stopReply = (sig) => `T${sig}thread:1;`

    const runChunk = () => {
      runTimer = null
      if (!running) return
      for (let i = 0; i < 20000; i++) {
        cpu.step()
        if (breakpoints.has(cpu.r[PC])) {
          running = false
          send(stopReply('05') + 'swbreak:;')
          return
        }
      }
      runTimer = setImmediate(runChunk)
    }

    const handle = (pkt) => {
      stats.packets.push(pkt.slice(0, 24))
      if (pkt.startsWith('qSupported')) return send('PacketSize=4000;qXfer:features:read+;vContSupported+;swbreak+;hwbreak+')
      if (pkt.startsWith('qXfer:features:read:target.xml:')) {
        const [off, len] = pkt.split(':').pop().split(',').map(h => parseInt(h, 16))
        const chunk = TARGET_XML.slice(off, off + len)
        return send((off + len >= TARGET_XML.length ? 'l' : 'm') + chunk)
      }
      if (pkt === '?') return send(stopReply('05'))
      if (pkt === 'g') return send(cpu.r.map(hex32).join(''))
      if (pkt[0] === 'G') {
        const h = pkt.slice(1)
        for (let i = 0; i < 17 && i * 8 < h.length; i++) cpu.r[i] = Buffer.from(h.slice(i * 8, i * 8 + 8), 'hex').readUInt32LE()
        return send('OK')
      }
      if (pkt[0] === 'p') {
        const n = parseInt(pkt.slice(1), 16)
        return send(n < 17 ? hex32(cpu.r[n]) : 'E01')
      }
      if (pkt[0] === 'P') {
        const [n, v] = pkt.slice(1).split('=')
        const i = parseInt(n, 16)
        if (i >= 17) return send('E01')
        cpu.r[i] = Buffer.from(v, 'hex').readUInt32LE()
        return send('OK')
      }
      if (pkt[0] === 'm') {
        const [a, l] = pkt.slice(1).split(',').map(h => parseInt(h, 16))
        return send(mem.read(a, Math.min(l, 4096)).toString('hex'))
      }
      if (pkt[0] === 'M') {
        const [head, data] = pkt.slice(1).split(':')
        const [a] = head.split(',').map(h => parseInt(h, 16))
        mem.write(a, Buffer.from(data, 'hex'))
        stats.writes++
        return send('OK')
      }
      if (pkt[0] === 'X') return send('') // force hex M writes
      if (/^[Zz][01],/.test(pkt)) {
        const addr = parseInt(pkt.split(',')[1], 16)
        if (pkt[0] === 'Z') breakpoints.add(addr); else breakpoints.delete(addr)
        return send('OK')
      }
      if (pkt === 'vCont?') return send('vCont;c;C;s;S')
      if (pkt === 'c' || /^vCont;c/.test(pkt) || /^vCont;C/.test(pkt)) {
        running = true
        // step off a breakpoint we are sitting on
        cpu.step()
        if (breakpoints.has(cpu.r[PC])) { running = false; return send(stopReply('05') + 'swbreak:;') }
        runTimer = setImmediate(runChunk)
        return // reply comes when the target stops
      }
      if (pkt === 's' || /^vCont;s/.test(pkt)) {
        cpu.step()
        return send(stopReply('05'))
      }
      if (pkt.startsWith('qRcmd,')) {
        const cmd = Buffer.from(pkt.slice(6), 'hex').toString()
        if (/^reset/.test(cmd)) { cpu.reset(); stats.resets++ }
        return send('OK')
      }
      if (pkt === 'qfThreadInfo') return send('m1')
      if (pkt === 'qsThreadInfo') return send('l')
      if (pkt === 'qC') return send('QC1')
      if (pkt === 'qAttached') return send('1')
      if (pkt[0] === 'H' || pkt[0] === 'T') return send('OK')
      if (pkt.startsWith('qSymbol')) return send('OK')
      if (pkt === 'D' || pkt.startsWith('D;')) { send('OK'); setTimeout(() => sock.end(), 10); return }
      if (pkt === 'k') { sock.end(); return }
      return send('') // unsupported
    }

    sock.on('data', (d) => {
      buf += d.toString('latin1')
      for (;;) {
        if (buf[0] === '+' || buf[0] === '-') { buf = buf.slice(1); continue }
        if (buf[0] === '\x03') {
          buf = buf.slice(1)
          if (running) {
            running = false
            if (runTimer) clearImmediate(runTimer)
            send(stopReply('02'))
          }
          continue
        }
        const start = buf.indexOf('$')
        if (start < 0) { buf = ''; break }
        const end = buf.indexOf('#', start)
        if (end < 0 || buf.length < end + 3) break
        const pkt = buf.slice(start + 1, end)
        buf = buf.slice(end + 3)
        sock.write('+')
        handle(pkt)
      }
    })
    sock.on('close', () => { running = false; if (runTimer) clearImmediate(runTimer) })
    sock.on('error', () => {})
  })

  return new Promise((resolve, reject) => {
    server.once('error', reject)
    server.listen(o.port ?? 0, '127.0.0.1', () => {
      resolve({
        port: server.address().port,
        stats,
        cpu,
        close: () => new Promise((r) => server.close(() => r())),
        server,
      })
    })
  })
}

module.exports = { startStub, REG_NAMES }
