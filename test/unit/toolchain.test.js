const test = require('node:test')
const assert = require('node:assert/strict')
const fs = require('fs')
const os = require('os')
const path = require('path')

const { makeToolchainOverrides, isMakeSafePath } = require('../../electron/toolchain')
const { envPathKey, withEnvPath, getToolchainEnv } = require('../../electron/bundledToolchain')
const { createProject } = require('../../electron/project')

test('paths with spaces are never passed to make as variables', () => {
  const root = '/home/user/My Projects/EmbedIDE/vendor/toolchain/linux-x64/bin'
  const resolve = (name) => `${root}/${name}`
  const { overrides, skipped } = makeToolchainOverrides(resolve)
  assert.deepEqual(overrides, [])
  assert.deepEqual(skipped.sort(), ['CC', 'CXX', 'OBJCOPY', 'OPENOCD', 'PREFIX', 'SIZE', 'ZIG'])
})

test('safe absolute paths are pinned, including the gcc PREFIX', () => {
  const resolve = (name) => `/opt/embedide/bin/${name}`
  const { overrides, skipped } = makeToolchainOverrides(resolve)
  assert.deepEqual(skipped, [])
  assert.ok(overrides.includes('PREFIX=/opt/embedide/bin/arm-none-eabi-'))
  assert.ok(overrides.includes('CC=/opt/embedide/bin/arm-none-eabi-gcc'))
})

test('Windows drive paths are passed with forward slashes', () => {
  const { overrides } = makeToolchainOverrides((n) => `C:\\EmbedIDE\\toolchain\\bin\\${n}.exe`)
  assert.ok(overrides.includes('CC=C:/EmbedIDE/toolchain/bin/arm-none-eabi-gcc.exe'))
  assert.ok(overrides.every((o) => !o.includes('\\')))
  const spaced = makeToolchainOverrides((n) => `C:\\Program Files\\EmbedIDE\\bin\\${n}.exe`)
  assert.deepEqual(spaced.overrides, [])
})

test('bare tool names (PATH lookup) produce no overrides', () => {
  const { overrides, skipped } = makeToolchainOverrides((n) => n)
  assert.deepEqual(overrides, [])
  assert.deepEqual(skipped, [])
})

test('isMakeSafePath rejects shell and make metacharacters', () => {
  for (const bad of ['C:\\Program Files\\x', '/a b/c', '/a$b', '/a#b', '/a;b', '/a(b)', "/a'b", '/a%b']) {
    assert.equal(isMakeSafePath(bad), false, bad)
  }
  for (const good of ['/opt/tc/bin/gcc', 'C:\\EmbedIDE\\bin\\gcc.exe', '/home/u/.local/x-y_z.1/gcc']) {
    assert.equal(isMakeSafePath(good), true, good)
  }
})

test('Windows env keeps exactly one PATH entry whatever its spelling', () => {
  const env = { PATH: 'C:\\a', ComSpec: 'cmd' }
  assert.equal(envPathKey(env, 'win32'), 'PATH')
  const out = withEnvPath({ ...env, Path: 'C:\\dup' }, 'C:\\tc;C:\\a', 'win32')
  const keys = Object.keys(out).filter((k) => k.toUpperCase() === 'PATH')
  assert.equal(keys.length, 1)
  assert.equal(out[keys[0]], 'C:\\tc;C:\\a')
  assert.equal(envPathKey({}, 'win32'), 'Path')
  assert.equal(envPathKey({ Path: 'x' }, 'linux'), 'PATH')
})

test('repeated toolchain env construction does not grow PATH', () => {
  const before = process.env.PATH
  try {
    const first = getToolchainEnv()
    process.env.PATH = first.PATH
    const second = getToolchainEnv()
    assert.equal(second.PATH, first.PATH)
  } finally {
    process.env.PATH = before
  }
})

function generated(type, boardId, rel) {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'embedide-tpl-'))
  try {
    const dir = createProject(root, 'tpl', type, boardId)
    return fs.readFileSync(path.join(dir, rel), 'utf8')
  } finally {
    fs.rmSync(root, { recursive: true, force: true })
  }
}

test('Zig templates keep container doc comments at the top of the file', () => {
  for (const type of ['zig', 'driver-zig', 'os-zig']) {
    const src = generated(type, 'stm32f407vg', 'src/main.zig')
    const lines = src.split('\n')
    const firstCode = lines.findIndex((l) => l.trim() && !l.startsWith('//!'))
    const lateDoc = lines.slice(firstCode).find((l) => l.startsWith('//!'))
    assert.equal(lateDoc, undefined, `${type}: //! after code`)
    assert.doesNotMatch(src, /@intFromPtr\(&_estack\)/, `${type}: comptime int of extern symbol`)
  }
})

test('kernel task table fits small-RAM parts', () => {
  const small = generated('os-c', 'stm32f030r8', 'include/kernel.h')
  assert.match(small, /#define KERNEL_MAX_TASKS 4\b/)
  assert.match(small, /#define KERNEL_STACK_WORDS 128\b/)
  const big = generated('os-c', 'stm32f407vg', 'include/kernel.h')
  assert.match(big, /#define KERNEL_MAX_TASKS 8\b/)
})

test('startup assembly avoids .cpu names the bundled assembler rejects', () => {
  const m85 = generated('c', 'ra8d1', 'src/startup.S')
  assert.doesNotMatch(m85, /^\.cpu cortex-m85/m)
  const m4 = generated('c', 'stm32f407vg', 'src/startup.S')
  assert.match(m4, /^\.cpu cortex-m4$/m)
})
