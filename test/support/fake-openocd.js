#!/usr/bin/env node
/**
 * Stand-in for OpenOCD in tests (no probe, no hardware). It accepts the same
 * arguments EmbedIDE passes, serves the simulated target from rsp-stub.js on
 * `gdb_port N`, and prints OpenOCD's readiness line. Behaviour switches:
 *
 *   FAKE_OPENOCD_ELF=path          preload firmware (a board that already holds it)
 *   FAKE_OPENOCD_FAIL=1            print a probe error and exit 1 (no probe)
 *   FAKE_OPENOCD_DIE_AFTER_MS=n    exit n ms after GDB connects (unplugged probe)
 *   FAKE_OPENOCD_SILENT=1          never print the readiness line (hung adapter)
 */
const { startStub } = require('./rsp-stub')

const args = process.argv.slice(2)
let port = 3333
for (let i = 0; i < args.length; i++) {
  if (args[i] === '-c') {
    const m = /^gdb_port (\d+)$/.exec(args[i + 1] || '')
    if (m) port = Number(m[1])
  }
}

process.stderr.write('xPack Open On-Chip Debugger 0.12.0 (fake for tests)\n')
if (process.env.FAKE_OPENOCD_FAIL === '1') {
  process.stderr.write('Error: open failed\nError: No ST-LINK detected\n')
  process.exit(1)
}

startStub({ elf: process.env.FAKE_OPENOCD_ELF || undefined, port }).then((stub) => {
  if (process.env.FAKE_OPENOCD_SILENT !== '1') {
    process.stderr.write(`Info : Listening on port ${stub.port} for gdb connections\n`)
  }
  const dieAfter = Number(process.env.FAKE_OPENOCD_DIE_AFTER_MS || 0)
  if (dieAfter) {
    stub.server.on('connection', () => setTimeout(() => {
      process.stderr.write('Error: libusb_bulk_write error: LIBUSB_ERROR_NO_DEVICE\n')
      process.exit(2)
    }, dieAfter))
  }
}).catch((e) => {
  process.stderr.write(`Error: ${e.message}\n`)
  process.exit(1)
})

process.on('SIGTERM', () => process.exit(0))
