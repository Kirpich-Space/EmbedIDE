#!/usr/bin/env node
/**
 * Fetch and unpack EmbedIDE bundled toolchains into vendor/toolchain/<platform>/.
 * Run before packaging: npm run toolchain:fetch
 *
 * Downloads (xPack / official):
 *  - arm-none-eabi-gcc (+ gdb, objcopy, size)
 *  - openocd
 *  - zig
 * Copies host make/python3 when available.
 */
import { createWriteStream, existsSync, mkdirSync, cpSync, chmodSync, writeFileSync, readdirSync, rmSync, statSync, symlinkSync } from 'fs'
import { execFileSync, spawnSync } from 'child_process'
import { dirname, join, basename } from 'path'
import { fileURLToPath } from 'url'
import { pipeline } from 'stream/promises'
import { createGunzip } from 'zlib'
import { tmpdir } from 'os'
import { createHash } from 'crypto'

const __dirname = dirname(fileURLToPath(import.meta.url))
const ROOT = join(__dirname, '..')

const PLATFORM = (() => {
  const arch = process.arch === 'arm64' ? 'arm64' : 'x64'
  if (process.platform === 'win32') return `win-${arch}`
  if (process.platform === 'darwin') return `darwin-${arch}`
  return `linux-${arch}`
})()

const OUT = join(ROOT, 'vendor', 'toolchain', PLATFORM)
const BIN = join(OUT, 'bin')

// Pinned xPack / Zig releases (update periodically)
const PACKAGES = {
  'linux-x64': {
    gcc: {
      url: 'https://github.com/xpack-dev-tools/arm-none-eabi-gcc-xpack/releases/download/v14.2.1-1.1/xpack-arm-none-eabi-gcc-14.2.1-1.1-linux-x64.tar.gz',
      strip: 1,
    },
    openocd: {
      url: 'https://github.com/xpack-dev-tools/openocd-xpack/releases/download/v0.12.0-6/xpack-openocd-0.12.0-6-linux-x64.tar.gz',
      strip: 1,
    },
    zig: {
      url: 'https://ziglang.org/download/0.13.0/zig-linux-x86_64-0.13.0.tar.xz',
      strip: 1,
      renameBin: true,
    },
  },
  'linux-arm64': {
    gcc: {
      url: 'https://github.com/xpack-dev-tools/arm-none-eabi-gcc-xpack/releases/download/v14.2.1-1.1/xpack-arm-none-eabi-gcc-14.2.1-1.1-linux-arm64.tar.gz',
      strip: 1,
    },
    openocd: {
      url: 'https://github.com/xpack-dev-tools/openocd-xpack/releases/download/v0.12.0-6/xpack-openocd-0.12.0-6-linux-arm64.tar.gz',
      strip: 1,
    },
    zig: {
      url: 'https://ziglang.org/download/0.13.0/zig-linux-aarch64-0.13.0.tar.xz',
      strip: 1,
      renameBin: true,
    },
  },
  'win-x64': {
    gcc: {
      url: 'https://github.com/xpack-dev-tools/arm-none-eabi-gcc-xpack/releases/download/v14.2.1-1.1/xpack-arm-none-eabi-gcc-14.2.1-1.1-win32-x64.zip',
      strip: 1,
    },
    openocd: {
      url: 'https://github.com/xpack-dev-tools/openocd-xpack/releases/download/v0.12.0-6/xpack-openocd-0.12.0-6-win32-x64.zip',
      strip: 1,
    },
    zig: {
      url: 'https://ziglang.org/download/0.13.0/zig-windows-x86_64-0.13.0.zip',
      strip: 1,
      renameBin: true,
    },
  },
  'darwin-x64': {
    gcc: {
      url: 'https://github.com/xpack-dev-tools/arm-none-eabi-gcc-xpack/releases/download/v14.2.1-1.1/xpack-arm-none-eabi-gcc-14.2.1-1.1-darwin-x64.tar.gz',
      strip: 1,
    },
    openocd: {
      url: 'https://github.com/xpack-dev-tools/openocd-xpack/releases/download/v0.12.0-6/xpack-openocd-0.12.0-6-darwin-x64.tar.gz',
      strip: 1,
    },
    zig: {
      url: 'https://ziglang.org/download/0.13.0/zig-macos-x86_64-0.13.0.tar.xz',
      strip: 1,
      renameBin: true,
    },
  },
  'darwin-arm64': {
    gcc: {
      url: 'https://github.com/xpack-dev-tools/arm-none-eabi-gcc-xpack/releases/download/v14.2.1-1.1/xpack-arm-none-eabi-gcc-14.2.1-1.1-darwin-arm64.tar.gz',
      strip: 1,
    },
    openocd: {
      url: 'https://github.com/xpack-dev-tools/openocd-xpack/releases/download/v0.12.0-6/xpack-openocd-0.12.0-6-darwin-arm64.tar.gz',
      strip: 1,
    },
    zig: {
      url: 'https://ziglang.org/download/0.13.0/zig-macos-aarch64-0.13.0.tar.xz',
      strip: 1,
      renameBin: true,
    },
  },
}

function which(cmd) {
  const r = spawnSync(process.platform === 'win32' ? 'where' : 'which', [cmd], { encoding: 'utf8' })
  if (r.status !== 0) return null
  return String(r.stdout).split(/\r?\n/).map(s => s.trim()).filter(Boolean)[0] || null
}

async function download(url, dest) {
  console.log('  ↓', url)
  mkdirSync(dirname(dest), { recursive: true })
  const res = await fetch(url, { redirect: 'follow' })
  if (!res.ok) throw new Error(`HTTP ${res.status} for ${url}`)
  const total = Number(res.headers.get('content-length') || 0)
  let got = 0
  const file = createWriteStream(dest)
  const reader = res.body.getReader()
  while (true) {
    const { done, value } = await reader.read()
    if (done) break
    got += value.length
    file.write(Buffer.from(value))
    if (total) {
      const pct = ((got / total) * 100).toFixed(0)
      process.stdout.write(`\r  ${pct}% (${(got / 1e6).toFixed(1)} MB)   `)
    }
  }
  await new Promise((resolve, reject) => file.end(err => err ? reject(err) : resolve()))
  process.stdout.write('\n')
  return dest
}

function extractArchive(archive, destDir, strip = 1) {
  mkdirSync(destDir, { recursive: true })
  if (archive.endsWith('.zip')) {
    execFileSync('unzip', ['-q', '-o', archive, '-d', destDir], { stdio: 'inherit' })
    // flatten one level if strip
    if (strip > 0) {
      const kids = readdirSync(destDir).filter(n => !n.startsWith('.'))
      if (kids.length === 1) {
        const inner = join(destDir, kids[0])
        if (statSync(inner).isDirectory()) {
          const tmp = join(destDir, '.__flatten__')
          cpSync(inner, tmp, { recursive: true })
          rmSync(inner, { recursive: true, force: true })
          for (const n of readdirSync(tmp)) {
            cpSync(join(tmp, n), join(destDir, n), { recursive: true })
          }
          rmSync(tmp, { recursive: true, force: true })
        }
      }
    }
    return
  }
  // tar.gz / tar.xz
  const args = ['-xaf', archive, '-C', destDir]
  if (strip > 0) args.push(`--strip-components=${strip}`)
  execFileSync('tar', args, { stdio: 'inherit' })
}

function linkBinsFrom(dir) {
  const binSrc = join(dir, 'bin')
  if (!existsSync(binSrc)) return
  mkdirSync(BIN, { recursive: true })
  for (const name of readdirSync(binSrc)) {
    const src = join(binSrc, name)
    const dst = join(BIN, name)
    try {
      if (existsSync(dst)) rmSync(dst, { force: true })
      if (process.platform === 'win32') {
        cpSync(src, dst)
      } else {
        try {
          symlinkSync(src, dst)
        } catch {
          cpSync(src, dst)
        }
        try { chmodSync(dst, 0o755) } catch {}
      }
    } catch (e) {
      console.warn('  skip', name, e.message)
    }
  }
}

function copyHostTool(cmd, destName) {
  const p = which(cmd)
  if (!p) {
    console.warn(`  ! ${cmd} not on host PATH — skip`)
    return false
  }
  mkdirSync(BIN, { recursive: true })
  const dest = join(BIN, destName || basename(p))
  cpSync(p, dest)
  try { chmodSync(dest, 0o755) } catch {}
  console.log(`  ✓ copied ${cmd} → bin/${basename(dest)}`)
  return true
}

async function fetchOne(name, spec) {
  const cache = join(ROOT, 'vendor', 'cache')
  mkdirSync(cache, { recursive: true })
  const file = join(cache, basename(spec.url))
  if (!existsSync(file) || statSync(file).size < 1000) {
    await download(spec.url, file)
  } else {
    console.log('  = cache hit', basename(file))
  }
  const unpack = join(OUT, name)
  if (existsSync(unpack)) rmSync(unpack, { recursive: true, force: true })
  mkdirSync(unpack, { recursive: true })
  console.log('  ✦ extract', name)
  extractArchive(file, unpack, spec.strip ?? 1)
  // xPack layout: unpack/bin already; zig: unpack/zig binary at root
  if (spec.renameBin) {
    mkdirSync(join(unpack, 'bin'), { recursive: true })
    const zigName = process.platform === 'win32' ? 'zig.exe' : 'zig'
    const candidates = [join(unpack, zigName), join(unpack, 'zig', zigName)]
    for (const c of candidates) {
      if (existsSync(c)) {
        cpSync(c, join(unpack, 'bin', zigName))
        try { chmodSync(join(unpack, 'bin', zigName), 0o755) } catch {}
        break
      }
    }
  }
  linkBinsFrom(unpack)
}

async function main() {
  const pkgs = PACKAGES[PLATFORM]
  if (!pkgs) {
    console.error('No package set for', PLATFORM)
    process.exit(1)
  }
  console.log('EmbedIDE toolchain fetch →', OUT)
  mkdirSync(OUT, { recursive: true })
  mkdirSync(BIN, { recursive: true })

  for (const [name, spec] of Object.entries(pkgs)) {
    console.log(`\n[${name}]`)
    await fetchOne(name, spec)
  }

  console.log('\n[host tools]')
  if (!copyHostTool('make', process.platform === 'win32' ? 'make.exe' : 'make')) {
    if (process.platform !== 'win32') {
      console.log('  ↓ building portable GNU make from source…')
      try {
        const cache = join(ROOT, 'vendor', 'cache')
        mkdirSync(cache, { recursive: true })
        const srcTar = join(cache, 'make-4.4.1.tar.gz')
        if (!existsSync(srcTar) || statSync(srcTar).size < 1000) {
          await download('https://ftp.gnu.org/gnu/make/make-4.4.1.tar.gz', srcTar)
        }
        const buildDir = join(cache, 'make-4.4.1-build')
        if (existsSync(buildDir)) rmSync(buildDir, { recursive: true, force: true })
        mkdirSync(buildDir, { recursive: true })
        execFileSync('tar', ['-xzf', srcTar, '-C', buildDir, '--strip-components=1'], { stdio: 'inherit' })
        execFileSync('sh', ['-c', './configure && sh build.sh && ./make -j$(nproc 2>/dev/null || echo 2)'], {
          cwd: buildDir,
          stdio: 'inherit',
          env: process.env,
        })
        const built = join(buildDir, 'make')
        const dest = join(BIN, 'make')
        cpSync(built, dest)
        chmodSync(dest, 0o755)
        console.log('  ✓ portable make 4.4.1')
      } catch (e) {
        console.warn('  ! could not build portable make:', e.message)
      }
    } else {
      console.warn('  ! Install make (e.g. via chocolatey/scoop) for Windows script builds')
    }
  }
  copyHostTool(process.platform === 'win32' ? 'python' : 'python3', process.platform === 'win32' ? 'python.exe' : 'python3')

  // Optional: install rust into vendor if rustup available (offline later)
  if (process.env.EMBEDIDE_FETCH_RUST === '1') {
    console.log('\n[rust] EMBEDIDE_FETCH_RUST=1 — installing portable rustup toolchain…')
    const rustRoot = join(OUT, 'rust')
    mkdirSync(rustRoot, { recursive: true })
    const rustupHome = join(rustRoot, 'rustup')
    const cargoHome = join(rustRoot, 'cargo')
    const env = {
      ...process.env,
      RUSTUP_HOME: rustupHome,
      CARGO_HOME: cargoHome,
    }
    execFileSync('bash', ['-lc', `curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --no-modify-path --default-toolchain stable -t thumbv6m-none-eabi -t thumbv7m-none-eabi -t thumbv7em-none-eabi -t thumbv7em-none-eabihf`], {
      env,
      stdio: 'inherit',
    })
    // expose cargo/rustc in bin via symlink
    for (const tool of ['cargo', 'rustc', 'rustup']) {
      const src = join(cargoHome, 'bin', tool)
      if (existsSync(src)) {
        const dst = join(BIN, tool)
        try { rmSync(dst, { force: true }) } catch {}
        cpSync(src, dst)
        try { chmodSync(dst, 0o755) } catch {}
      }
    }
  }

  const manifest = {
    platform: PLATFORM,
    fetchedAt: new Date().toISOString(),
    packages: Object.fromEntries(Object.entries(pkgs).map(([k, v]) => [k, v.url])),
    rust: process.env.EMBEDIDE_FETCH_RUST === '1',
  }
  writeFileSync(join(OUT, 'manifest.json'), JSON.stringify(manifest, null, 2) + '\n')
  console.log('\nDone. Manifest written. Bin dir:', BIN)
  console.log('Tools:', readdirSync(BIN).slice(0, 30).join(', '), '…')
}

main().catch(err => {
  console.error(err)
  process.exit(1)
})
