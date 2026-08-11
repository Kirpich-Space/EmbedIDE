/**
 * Runtime toolchain installer — downloads ARM GCC, OpenOCD, Zig (+ optional Rust)
 * into Electron userData so slim packages stay small.
 */
const path = require('path')
const fs = require('fs')
const { spawnSync, execFileSync } = require('child_process')
const https = require('https')
const http = require('http')
const { PACKAGES, platformKey } = require('./toolchainPackages')
const { invalidateCache, getBundledStatus, prependBundledToolchainToPath } = require('./bundledToolchain')

let app = null
try {
  app = require('electron').app
} catch {
  app = null
}

let installInFlight = null
let lastProgress = null

function emitProgress(onProgress, payload) {
  lastProgress = { ...payload, at: Date.now() }
  try { onProgress?.(lastProgress) } catch {}
}

function getInstallRoot() {
  if (app) {
    return path.join(app.getPath('userData'), 'toolchain')
  }
  return path.join(__dirname, '..', 'vendor', 'toolchain', platformKey())
}

function getCacheDir() {
  if (app) {
    return path.join(app.getPath('userData'), 'toolchain-cache')
  }
  return path.join(__dirname, '..', 'vendor', 'cache')
}

function which(cmd) {
  const r = spawnSync(process.platform === 'win32' ? 'where' : 'which', [cmd], { encoding: 'utf8' })
  if (r.status !== 0) return null
  return String(r.stdout).split(/\r?\n/).map(s => s.trim()).filter(Boolean)[0] || null
}

function downloadFile(url, dest, onProgress, label) {
  return new Promise((resolve, reject) => {
    fs.mkdirSync(path.dirname(dest), { recursive: true })
    const tmp = dest + '.partial'
    const file = fs.createWriteStream(tmp)
    let got = 0
    let total = 0

    const get = (u, redirects = 0) => {
      if (redirects > 8) {
        reject(new Error('Too many redirects'))
        return
      }
        const lib = u.startsWith('https') ? https : http
      const req = lib.get(u, { headers: { 'User-Agent': 'EmbedIDE-toolchain-installer' } }, (res) => {
        if (res.statusCode >= 300 && res.statusCode < 400 && res.headers.location) {
          res.resume()
          let next = res.headers.location
          try { next = new URL(next, u).href } catch {}
          get(next, redirects + 1)
          return
        }
        if (res.statusCode !== 200) {
          reject(new Error(`HTTP ${res.statusCode} for ${url}`))
          res.resume()
          return
        }
        total = Number(res.headers['content-length'] || 0)
        res.on('data', (chunk) => {
          got += chunk.length
          emitProgress(onProgress, {
            phase: 'download',
            package: label,
            url,
            received: got,
            total,
            percent: total ? Math.min(99, Math.round((got / total) * 100)) : 0,
            message: total
              ? `Downloading ${label}: ${(got / 1e6).toFixed(1)} / ${(total / 1e6).toFixed(1)} MB`
              : `Downloading ${label}: ${(got / 1e6).toFixed(1)} MB`,
          })
        })
        res.pipe(file)
        file.on('finish', () => {
          file.close(() => {
            try {
              fs.renameSync(tmp, dest)
              resolve(dest)
            } catch (e) {
              reject(e)
            }
          })
        })
      })
      req.on('error', (err) => {
        try { fs.unlinkSync(tmp) } catch {}
        reject(err)
      })
    }
    get(url)
  })
}

function extractArchive(archive, destDir, strip = 1) {
  fs.mkdirSync(destDir, { recursive: true })
  if (archive.endsWith('.zip')) {
    if (process.platform === 'win32') {
      execFileSync('powershell.exe', [
        '-NoProfile', '-Command',
        `Expand-Archive -LiteralPath '${archive.replace(/'/g, "''")}' -DestinationPath '${destDir.replace(/'/g, "''")}' -Force`,
      ], { stdio: 'ignore' })
    } else {
      execFileSync('unzip', ['-q', '-o', archive, '-d', destDir], { stdio: 'ignore' })
    }
    if (strip > 0) {
      const kids = fs.readdirSync(destDir).filter(n => !n.startsWith('.'))
      if (kids.length === 1) {
        const inner = path.join(destDir, kids[0])
        if (fs.statSync(inner).isDirectory()) {
          const tmp = path.join(destDir, '.__flatten__')
          fs.cpSync(inner, tmp, { recursive: true })
          fs.rmSync(inner, { recursive: true, force: true })
          for (const n of fs.readdirSync(tmp)) {
            fs.cpSync(path.join(tmp, n), path.join(destDir, n), { recursive: true })
          }
          fs.rmSync(tmp, { recursive: true, force: true })
        }
      }
    }
    return
  }
  const args = ['-xaf', archive, '-C', destDir]
  if (strip > 0) args.push(`--strip-components=${strip}`)
  execFileSync('tar', args, { stdio: 'ignore' })
}

function linkBinsFrom(binDir, outBin) {
  const binSrc = path.join(binDir, 'bin')
  if (!fs.existsSync(binSrc)) return
  fs.mkdirSync(outBin, { recursive: true })
  for (const name of fs.readdirSync(binSrc)) {
    const src = path.join(binSrc, name)
    const dst = path.join(outBin, name)
    try {
      try { fs.lstatSync(dst); fs.rmSync(dst, { force: true }) } catch {}
      if (process.platform === 'win32') {
        fs.cpSync(src, dst)
      } else {
        fs.symlinkSync(path.relative(outBin, src), dst)
        try { fs.chmodSync(dst, 0o755) } catch {}
      }
    } catch {
      try { fs.cpSync(src, dst) } catch {}
    }
  }
}

function copyHostTool(cmd, destName, outBin) {
  const p = which(cmd)
  if (!p) return false
  fs.mkdirSync(outBin, { recursive: true })
  const dest = path.join(outBin, destName)
  try {
    if (fs.existsSync(dest) && path.resolve(p) === path.resolve(dest)) return true
    if (fs.existsSync(dest)) fs.rmSync(dest, { force: true })
    fs.cpSync(p, dest)
    try { fs.chmodSync(dest, 0o755) } catch {}
    return true
  } catch {
    return false
  }
}

function copyBootstrapMake(outBin) {
  const name = process.platform === 'win32' ? 'make.exe' : 'make'
  const candidates = [
    process.resourcesPath ? path.join(process.resourcesPath, 'bootstrap', name) : null,
    process.resourcesPath ? path.join(process.resourcesPath, 'bootstrap', platformKey(), name) : null,
    path.join(__dirname, '..', 'build', 'bootstrap', name),
    path.join(__dirname, '..', 'build', 'bootstrap', platformKey(), name),
    path.join(__dirname, '..', 'vendor', 'toolchain', platformKey(), 'bin', name),
  ].filter(Boolean)
  for (const src of candidates) {
    if (!fs.existsSync(src)) continue
    try {
      fs.mkdirSync(outBin, { recursive: true })
      const dest = path.join(outBin, name)
      fs.cpSync(src, dest)
      try { fs.chmodSync(dest, 0o755) } catch {}
      // verify it runs
      const check = spawnSync(dest, ['--version'], { encoding: 'utf8', timeout: 5000 })
      if (check.status === 0) return true
    } catch {}
  }
  return false
}

/** Download GNU make and bootstrap-build it (uses zig cc if available). */
async function buildMakeFromSource(outRoot, outBin, onProgress) {
  const cache = getCacheDir()
  fs.mkdirSync(cache, { recursive: true })
  const srcTar = path.join(cache, 'make-4.4.1.tar.gz')
  const buildDir = path.join(cache, 'make-4.4.1-build')
  emitProgress(onProgress, { phase: 'make', package: 'make', percent: 86, message: 'Downloading GNU make 4.4.1…' })
  if (!fs.existsSync(srcTar) || fs.statSync(srcTar).size < 1000) {
    await downloadFile('https://ftp.gnu.org/gnu/make/make-4.4.1.tar.gz', srcTar, onProgress, 'make')
  }
  if (fs.existsSync(buildDir)) fs.rmSync(buildDir, { recursive: true, force: true })
  fs.mkdirSync(buildDir, { recursive: true })
  execFileSync('tar', ['-xzf', srcTar, '-C', buildDir, '--strip-components=1'], { stdio: 'ignore' })

  const zigCandidates = [
    path.join(outBin, process.platform === 'win32' ? 'zig.exe' : 'zig'),
    path.join(outRoot, 'zig', 'bin', process.platform === 'win32' ? 'zig.exe' : 'zig'),
  ]
  let cc = null
  for (const z of zigCandidates) {
    if (fs.existsSync(z)) {
      cc = `${z} cc`
      break
    }
  }
  if (!cc) {
    if (which('gcc')) cc = 'gcc'
    else if (which('clang')) cc = 'clang'
    else if (which('cc')) cc = 'cc'
  }
  if (!cc) throw new Error('No compiler available to build make')

  emitProgress(onProgress, { phase: 'make', package: 'make', percent: 90, message: `Building make with ${cc}…` })
  execFileSync('sh', ['build.sh'], {
    cwd: buildDir,
    stdio: 'ignore',
    env: { ...process.env, CC: cc },
  })
  // Optional full build
  const boot = path.join(buildDir, 'make')
  if (fs.existsSync(boot)) {
    try {
      execFileSync(boot, ['-j2'], { cwd: buildDir, stdio: 'ignore', timeout: 120000 })
    } catch {}
  }
  const built = path.join(buildDir, process.platform === 'win32' ? 'make.exe' : 'make')
  if (!fs.existsSync(built)) throw new Error('make build produced no binary')
  const dest = path.join(outBin, process.platform === 'win32' ? 'make.exe' : 'make')
  fs.cpSync(built, dest)
  try { fs.chmodSync(dest, 0o755) } catch {}
  return true
}

async function installWindowsMake(outBin, onProgress) {
  const cache = getCacheDir()
  fs.mkdirSync(cache, { recursive: true })
  const url = 'https://downloads.sourceforge.net/project/ezwinports/make-4.4.1-without-guile-w32-bin.zip'
  const zip = path.join(cache, 'make-4.4.1-without-guile-w32-bin.zip')
  emitProgress(onProgress, { phase: 'make', package: 'make', percent: 86, message: 'Downloading Windows make…' })
  if (!fs.existsSync(zip) || fs.statSync(zip).size < 1000) {
    await downloadFile(url, zip, onProgress, 'make')
  }
  const unpack = path.join(cache, 'make-win-unpack')
  if (fs.existsSync(unpack)) fs.rmSync(unpack, { recursive: true, force: true })
  extractArchive(zip, unpack, 0)
  // find make.exe
  const walk = (dir, depth = 0) => {
    if (depth > 4) return null
    for (const name of fs.readdirSync(dir)) {
      const full = path.join(dir, name)
      if (name.toLowerCase() === 'make.exe') return full
      try {
        if (fs.statSync(full).isDirectory()) {
          const hit = walk(full, depth + 1)
          if (hit) return hit
        }
      } catch {}
    }
    return null
  }
  const found = walk(unpack)
  if (!found) throw new Error('make.exe not found in Windows make zip')
  fs.mkdirSync(outBin, { recursive: true })
  fs.cpSync(found, path.join(outBin, 'make.exe'))
  return true
}

async function ensureMake(outRoot, outBin, onProgress) {
  const makeName = process.platform === 'win32' ? 'make.exe' : 'make'
  const dest = path.join(outBin, makeName)
  if (fs.existsSync(dest)) {
    const check = spawnSync(dest, ['--version'], { encoding: 'utf8', timeout: 5000 })
    if (check.status === 0) return true
  }
  emitProgress(onProgress, { phase: 'make', package: 'make', percent: 85, message: 'Installing make…' })
  if (copyBootstrapMake(outBin)) return true
  if (copyHostTool('make', makeName, outBin)) {
    const check = spawnSync(dest, ['--version'], { encoding: 'utf8', timeout: 5000 })
    if (check.status === 0) return true
  }
  if (process.platform === 'win32') {
    await installWindowsMake(outBin, onProgress)
  } else {
    await buildMakeFromSource(outRoot, outBin, onProgress)
  }
  const check = spawnSync(path.join(outBin, makeName), ['--version'], { encoding: 'utf8', timeout: 5000 })
  if (check.status !== 0) throw new Error('make installed but --version failed')
  return true
}

async function installRust(outRoot, onProgress) {
  emitProgress(onProgress, {
    phase: 'rust',
    package: 'rust',
    percent: 0,
    message: 'Installing Rust (rustup)…',
  })
  const rustRoot = path.join(outRoot, 'rust')
  fs.mkdirSync(rustRoot, { recursive: true })
  const rustupHome = path.join(rustRoot, 'rustup')
  const cargoHome = path.join(rustRoot, 'cargo')
  const env = {
    ...process.env,
    RUSTUP_HOME: rustupHome,
    CARGO_HOME: cargoHome,
  }
  const script = `curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --no-modify-path --default-toolchain stable -t thumbv6m-none-eabi -t thumbv7m-none-eabi -t thumbv7em-none-eabi -t thumbv7em-none-eabihf -t thumbv8m.main-none-eabi -t thumbv8m.main-none-eabihf`
  if (process.platform === 'win32') {
    // Windows: download rustup-init.exe
    const init = path.join(getCacheDir(), 'rustup-init.exe')
    await downloadFile(
      'https://static.rust-lang.org/rustup/dist/x86_64-pc-windows-msvc/rustup-init.exe',
      init,
      onProgress,
      'rustup-init',
    )
    execFileSync(init, [
      '-y', '--no-modify-path', '--default-toolchain', 'stable',
      '-t', 'thumbv6m-none-eabi', '-t', 'thumbv7m-none-eabi',
      '-t', 'thumbv7em-none-eabi', '-t', 'thumbv7em-none-eabihf', '-t', 'thumbv8m.main-none-eabi', '-t', 'thumbv8m.main-none-eabihf',
    ], { env, stdio: 'ignore' })
  } else {
    execFileSync('bash', ['-lc', script], { env, stdio: 'ignore' })
  }
  const outBin = path.join(outRoot, 'bin')
  fs.mkdirSync(outBin, { recursive: true })
  for (const tool of ['cargo', 'rustc', 'rustup']) {
    const src = path.join(cargoHome, 'bin', process.platform === 'win32' ? `${tool}.exe` : tool)
    if (!fs.existsSync(src)) continue
    const dst = path.join(outBin, process.platform === 'win32' ? `${tool}.exe` : tool)
    try { fs.rmSync(dst, { force: true }) } catch {}
    if (process.platform === 'win32') {
      fs.cpSync(src, dst)
    } else {
      fs.symlinkSync(path.relative(outBin, src), dst)
    }
  }
}

/**
 * @param {{ includeRust?: boolean, force?: boolean, onProgress?: (p: object) => void }} opts
 */
async function installToolchain(opts = {}) {
  if (installInFlight) return installInFlight

  const { includeRust = true, force = false, onProgress } = opts
  const pk = platformKey()
  const pkgs = PACKAGES[pk]
  if (!pkgs) {
    throw new Error(`No toolchain packages for platform ${pk}`)
  }

  const outRoot = getInstallRoot()
  const outBin = path.join(outRoot, 'bin')
  const cache = getCacheDir()

  installInFlight = (async () => {
    try {
      if (!force) {
        invalidateCache()
        const st = getBundledStatus()
        if (st.bundled && st.tools?.['arm-none-eabi-gcc'] && st.tools?.openocd && st.tools?.zig) {
          emitProgress(onProgress, {
            phase: 'done',
            percent: 100,
            message: 'Toolchain already installed',
            root: st.root,
          })
          return { ok: true, skipped: true, root: st.root }
        }
      }

      fs.mkdirSync(outRoot, { recursive: true })
      fs.mkdirSync(outBin, { recursive: true })
      fs.mkdirSync(cache, { recursive: true })

      const entries = Object.entries(pkgs)
      let i = 0
      for (const [name, spec] of entries) {
        i += 1
        emitProgress(onProgress, {
          phase: 'package',
          package: name,
          percent: Math.round(((i - 1) / entries.length) * 80),
          message: `Preparing ${name}…`,
        })
        const file = path.join(cache, path.basename(spec.url))
        if (!fs.existsSync(file) || fs.statSync(file).size < 1000) {
          await downloadFile(spec.url, file, onProgress, name)
        } else {
          emitProgress(onProgress, {
            phase: 'cache',
            package: name,
            percent: Math.round(((i - 0.5) / entries.length) * 80),
            message: `Using cached ${name}`,
          })
        }
        const unpack = path.join(outRoot, name)
        if (fs.existsSync(unpack)) fs.rmSync(unpack, { recursive: true, force: true })
        fs.mkdirSync(unpack, { recursive: true })
        emitProgress(onProgress, {
          phase: 'extract',
          package: name,
          percent: Math.round((i / entries.length) * 80),
          message: `Extracting ${name}…`,
        })
        extractArchive(file, unpack, spec.strip ?? 1)
        if (spec.renameBin) {
          fs.mkdirSync(path.join(unpack, 'bin'), { recursive: true })
          const zigName = process.platform === 'win32' ? 'zig.exe' : 'zig'
          for (const c of [path.join(unpack, zigName), path.join(unpack, 'zig', zigName)]) {
            if (fs.existsSync(c)) {
              fs.cpSync(c, path.join(unpack, 'bin', zigName))
              try { fs.chmodSync(path.join(unpack, 'bin', zigName), 0o755) } catch {}
              break
            }
          }
        }
        linkBinsFrom(unpack, outBin)
      }

      emitProgress(onProgress, { phase: 'host', percent: 85, message: 'Installing make / python…' })
      try {
        await ensureMake(outRoot, outBin, onProgress)
      } catch (e) {
        emitProgress(onProgress, {
          phase: 'make-warn',
          percent: 90,
          message: `make install failed: ${e.message}`,
        })
      }
      copyHostTool(process.platform === 'win32' ? 'python' : 'python3',
        process.platform === 'win32' ? 'python.exe' : 'python3', outBin)

      if (includeRust) {
        try {
          await installRust(outRoot, onProgress)
        } catch (e) {
          emitProgress(onProgress, {
            phase: 'rust-warn',
            percent: 95,
            message: `Rust optional install failed: ${e.message}`,
          })
        }
      }

      const manifest = {
        platform: pk,
        fetchedAt: new Date().toISOString(),
        source: 'runtime-installer',
        packages: Object.fromEntries(entries.map(([k, v]) => [k, v.url])),
        rust: !!includeRust,
      }
      fs.writeFileSync(path.join(outRoot, 'manifest.json'), JSON.stringify(manifest, null, 2) + '\n')

      invalidateCache()
      prependBundledToolchainToPath()
      const st = getBundledStatus()

      emitProgress(onProgress, {
        phase: 'done',
        percent: 100,
        message: 'Toolchain ready',
        root: st.root || outRoot,
      })
      return { ok: true, root: st.root || outRoot, status: st }
    } finally {
      installInFlight = null
    }
  })()

  return installInFlight
}

function getInstallProgress() {
  return lastProgress
}

function isInstallRunning() {
  return !!installInFlight
}

function needsToolchainInstall() {
  invalidateCache()
  const st = getBundledStatus()
  if (!(st.bundled && st.tools?.['arm-none-eabi-gcc'] && st.tools?.openocd)) return true
  // make is required for virtually all firmware templates
  if (!st.tools?.make) return true
  return false
}

module.exports = {
  installToolchain,
  getInstallProgress,
  isInstallRunning,
  needsToolchainInstall,
  getInstallRoot,
  PACKAGES,
  platformKey,
}
