/**
 * Bundled toolchain resolver for EmbedIDE.
 * Prefer tools shipped under vendor/toolchain/<platform>/ (dev)
 * or process.resourcesPath/toolchain (packaged), then fall back to PATH.
 */
const path = require('path')
const fs = require('fs')

let app = null
try {
  app = require('electron').app
} catch {
  app = null
}

const TOOL_NAMES = {
  make: process.platform === 'win32' ? 'make.exe' : 'make',
  python: process.platform === 'win32' ? 'python.exe' : 'python3',
  rustc: process.platform === 'win32' ? 'rustc.exe' : 'rustc',
  cargo: process.platform === 'win32' ? 'cargo.exe' : 'cargo',
  rustup: process.platform === 'win32' ? 'rustup.exe' : 'rustup',
  'arm-none-eabi-gcc': process.platform === 'win32' ? 'arm-none-eabi-gcc.exe' : 'arm-none-eabi-gcc',
  'arm-none-eabi-g++': process.platform === 'win32' ? 'arm-none-eabi-g++.exe' : 'arm-none-eabi-g++',
  'arm-none-eabi-gdb': process.platform === 'win32' ? 'arm-none-eabi-gdb.exe' : 'arm-none-eabi-gdb',
  'arm-none-eabi-objcopy': process.platform === 'win32' ? 'arm-none-eabi-objcopy.exe' : 'arm-none-eabi-objcopy',
  'arm-none-eabi-size': process.platform === 'win32' ? 'arm-none-eabi-size.exe' : 'arm-none-eabi-size',
  openocd: process.platform === 'win32' ? 'openocd.exe' : 'openocd',
  zig: process.platform === 'win32' ? 'zig.exe' : 'zig',
  node: process.platform === 'win32' ? 'node.exe' : 'node',
  bash: process.platform === 'win32' ? 'bash.exe' : 'bash',
}

function platformKey() {
  const arch = process.arch === 'arm64' ? 'arm64' : 'x64'
  if (process.platform === 'win32') return `win-${arch}`
  if (process.platform === 'darwin') return `darwin-${arch}`
  return `linux-${arch}`
}

function candidateRoots() {
  const roots = []
  // Electron app path / project root (dev) — prefer real vendor tree over any stub
  try {
    if (app && !app.isPackaged) {
      roots.push(path.join(app.getAppPath(), 'vendor', 'toolchain', platformKey()))
      roots.push(path.join(process.cwd(), 'vendor', 'toolchain', platformKey()))
      roots.push(path.join(__dirname, '..', 'vendor', 'toolchain', platformKey()))
    } else if (app) {
      // Packaged: resources/toolchain (electron-builder extraResources)
      if (process.resourcesPath) {
        roots.push(path.join(process.resourcesPath, 'toolchain'))
      }
      roots.push(path.join(app.getPath('userData'), 'toolchain'))
    }
  } catch {
    roots.push(path.join(__dirname, '..', 'vendor', 'toolchain', platformKey()))
    roots.push(path.join(process.cwd(), 'vendor', 'toolchain', platformKey()))
  }
  // Always try repo-relative (scripts / tests without app ready)
  roots.push(path.join(__dirname, '..', 'vendor', 'toolchain', platformKey()))
  // Last resort: resourcesPath (may be electron's own resources in dev — usually empty)
  try {
    if (process.resourcesPath) {
      roots.push(path.join(process.resourcesPath, 'toolchain'))
    }
  } catch {}
  return [...new Set(roots.map(r => path.resolve(r)))]
}

function findBundledRoot() {
  for (const root of candidateRoots()) {
    if (!isUsableToolchainRoot(root)) continue
    return root
  }
  return null
}

/** Reject empty stubs (e.g. {"skipped":true}) and dirs without real tools. */
function isUsableToolchainRoot(root) {
  try {
    if (!root || !fs.existsSync(root) || !fs.statSync(root).isDirectory()) return false
    const mp = path.join(root, 'manifest.json')
    if (fs.existsSync(mp)) {
      try {
        const m = JSON.parse(fs.readFileSync(mp, 'utf8'))
        if (m && m.skipped) return false
      } catch {}
    }
    const bin = path.join(root, 'bin')
    const markers = [
      path.join(bin, TOOL_NAMES['arm-none-eabi-gcc']),
      path.join(bin, TOOL_NAMES.openocd),
      path.join(bin, TOOL_NAMES.zig),
      path.join(root, 'gcc', 'bin', TOOL_NAMES['arm-none-eabi-gcc']),
      path.join(root, 'openocd', 'bin', TOOL_NAMES.openocd),
    ]
    if (markers.some(p => fs.existsSync(p))) return true
    // Fallback: any arm-none-eabi-gcc / openocd under root/bin trees
    const bins = collectBinDirs(root)
    for (const d of bins) {
      if (fs.existsSync(path.join(d, TOOL_NAMES['arm-none-eabi-gcc']))) return true
      if (fs.existsSync(path.join(d, TOOL_NAMES.openocd))) return true
    }
    return false
  } catch {
    return false
  }
}

/** Collect all bin directories under a toolchain root (xPack layout, zig, rust). */
function collectBinDirs(root) {
  if (!root || !fs.existsSync(root)) return []
  const bins = []
  const direct = path.join(root, 'bin')
  if (fs.existsSync(direct)) bins.push(direct)

  const walk = (dir, depth) => {
    if (depth > 4) return
    let entries
    try { entries = fs.readdirSync(dir, { withFileTypes: true }) } catch { return }
    for (const e of entries) {
      if (!e.isDirectory()) continue
      const full = path.join(dir, e.name)
      if (e.name === 'bin') {
        bins.push(full)
        continue
      }
      // xPack: .content/bin, arm-none-eabi/bin
      if (e.name === '.content' || e.name.includes('arm-none') || e.name.includes('gcc') || e.name === 'openocd' || e.name === 'zig' || e.name === 'rust') {
        walk(full, depth + 1)
      } else if (depth < 2) {
        walk(full, depth + 1)
      }
    }
  }
  walk(root, 0)
  return [...new Set(bins)]
}

let cachedRoot = undefined
let cachedBins = undefined

function getBundledRoot() {
  if (cachedRoot === undefined) cachedRoot = findBundledRoot()
  return cachedRoot
}

function getBundledBinDirs() {
  if (cachedBins === undefined) cachedBins = collectBinDirs(getBundledRoot())
  return cachedBins
}

function invalidateCache() {
  cachedRoot = undefined
  cachedBins = undefined
}

function resolveInBins(fileName) {
  for (const bin of getBundledBinDirs()) {
    const p = path.join(bin, fileName)
    if (fs.existsSync(p)) return p
  }
  return null
}

/** Resolve a tool to an absolute path (bundled) or leave as bare name (PATH). */
function resolveTool(name) {
  const file = TOOL_NAMES[name] || name
  const bundled = resolveInBins(file)
  if (bundled) return bundled
  // Also try without .exe mapping if caller passed full name
  if (file !== name) {
    const alt = resolveInBins(name)
    if (alt) return alt
  }
  return name
}

function isBundled(name) {
  const file = TOOL_NAMES[name] || name
  return !!resolveInBins(file)
}

function findOpenocdScripts(root) {
  if (!root) return null
  const candidates = [
    path.join(root, 'openocd', 'share', 'openocd', 'scripts'),
    path.join(root, 'share', 'openocd', 'scripts'),
  ]
  for (const c of candidates) {
    if (fs.existsSync(c)) return c
  }
  // shallow search
  try {
    for (const name of fs.readdirSync(root)) {
      const p = path.join(root, name, 'share', 'openocd', 'scripts')
      if (fs.existsSync(p)) return p
    }
  } catch {}
  return null
}

function getToolchainEnv(extra = {}) {
  const bins = getBundledBinDirs()
  const sep = process.platform === 'win32' ? ';' : ':'
  const pathKey = process.platform === 'win32' ? 'Path' : 'PATH'
  const current = process.env[pathKey] || process.env.PATH || ''
  const prepend = bins.length ? bins.join(sep) + sep : ''
  const root = getBundledRoot()
  const env = {
    ...process.env,
    ...extra,
    [pathKey]: prepend + current,
    EMBEDIDE_TOOLCHAIN: root || '',
  }
  const scripts = findOpenocdScripts(root)
  if (scripts) {
    env.OPENOCD_SCRIPTS = scripts
    env.OPENOCD_HOME = path.dirname(path.dirname(scripts))
  }
  // Rust homes if bundled
  if (root) {
    const rustHome = path.join(root, 'rust')
    if (fs.existsSync(rustHome)) {
      env.RUSTUP_HOME = path.join(rustHome, 'rustup')
      env.CARGO_HOME = path.join(rustHome, 'cargo')
      const cargoBin = path.join(env.CARGO_HOME, 'bin')
      if (fs.existsSync(cargoBin)) {
        env[pathKey] = cargoBin + sep + env[pathKey]
      }
    }
  }
  return env
}

function prependBundledToolchainToPath() {
  const env = getToolchainEnv()
  const pathKey = process.platform === 'win32' ? 'Path' : 'PATH'
  process.env[pathKey] = env[pathKey]
  if (env.RUSTUP_HOME) process.env.RUSTUP_HOME = env.RUSTUP_HOME
  if (env.CARGO_HOME) process.env.CARGO_HOME = env.CARGO_HOME
  if (env.EMBEDIDE_TOOLCHAIN) process.env.EMBEDIDE_TOOLCHAIN = env.EMBEDIDE_TOOLCHAIN
}

function readManifest() {
  const root = getBundledRoot()
  if (!root) return null
  const mp = path.join(root, 'manifest.json')
  try {
    if (fs.existsSync(mp)) return JSON.parse(fs.readFileSync(mp, 'utf8'))
  } catch {}
  return null
}

function getBundledStatus() {
  const root = getBundledRoot()
  const tools = [
    'arm-none-eabi-gcc',
    'arm-none-eabi-gdb',
    'openocd',
    'make',
    'zig',
    'python',
    'rustc',
    'cargo',
  ]
  const present = {}
  for (const t of tools) present[t] = isBundled(t)
  return {
    root,
    platform: platformKey(),
    bundled: !!root,
    tools: present,
    manifest: readManifest(),
    binDirs: getBundledBinDirs(),
  }
}

module.exports = {
  platformKey,
  getBundledRoot,
  getBundledBinDirs,
  resolveTool,
  isBundled,
  getToolchainEnv,
  prependBundledToolchainToPath,
  getBundledStatus,
  invalidateCache,
  findOpenocdScripts,
  TOOL_NAMES,
}
