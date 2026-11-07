const { execFile, execSync, spawn, spawnSync } = require('child_process');
const { promisify } = require('util');
const path = require('path');
const fs = require('fs');
const { getBoardOrDefault, DEFAULT_BOARD_ID } = require('./boards');
const { readProjectMeta } = require('./project');
const { resolveTool, getToolchainEnv, isBundled, getBundledStatus, prependBundledToolchainToPath, findOpenocdScripts, getBundledRoot, getBundledBinDirs, invalidateCache, envPathKey, withEnvPath } = require('./bundledToolchain');
const execFileAsync = promisify(execFile);

let currentBuildProc = null;
let buildWasCancelled = false;

const RUST_TARGET_RE = /^[a-zA-Z0-9._-]+$/;

function toolEnv(extra = {}) {
  return getToolchainEnv(extra);
}

function spawnTool(cmd, args, options = {}) {
  const bin = resolveTool(cmd);
  return spawn(bin, args, {
    ...options,
    env: toolEnv(options.env),
    shell: options.shell === true,
  });
}

/** Env without bundled ARM toolchain — for host C/C++/ASM/Rust scripts. */
function hostToolEnv(extra = {}) {
  const bins = new Set(getBundledBinDirs().map(d => path.resolve(d)));
  const sep = process.platform === 'win32' ? ';' : ':';
  const env = { ...process.env, ...extra };
  const pathKey = envPathKey(env);
  const filtered = String(env[pathKey] || '')
    .split(sep)
    .filter(p => p && !bins.has(path.resolve(p)))
    .join(sep);
  return withEnvPath(env, filtered);
}

function spawnHost(cmd, args, options = {}) {
  return spawn(cmd, args, {
    ...options,
    env: hostToolEnv(options.env),
    shell: options.shell === true,
  });
}

async function execHost(cmd, args, options = {}) {
  return execFileAsync(cmd, args, {
    timeout: 4000,
    maxBuffer: 64 * 1024,
    ...options,
    env: hostToolEnv(options.env),
  });
}

function spawnToolSync(cmd, args, options = {}) {
  const bin = resolveTool(cmd);
  return spawnSync(bin, args, {
    ...options,
    env: toolEnv(options.env),
  });
}

async function execTool(cmd, args, options = {}) {
  const bin = resolveTool(cmd);
  return execFileAsync(bin, args, {
    timeout: 4000,
    maxBuffer: 64 * 1024,
    ...options,
    env: toolEnv(options.env),
  });
}

let detectCache = null;
let detectCacheAt = 0;
const DETECT_TTL_MS = 60_000;

async function detectToolchains(opts = {}) {
  const force = !!(opts && opts.force);
  if (!force && detectCache && (Date.now() - detectCacheAt) < DETECT_TTL_MS) {
    return detectCache;
  }

  if (force) {
    invalidateCache();
  } else {
    // Ensure PATH/bins are set once; avoid wiping resolver cache on every poll
    prependBundledToolchainToPath();
  }

  const result = {
    rust: false,
    armGcc: false,
    armGdb: false,
    openocd: false,
    make: false,
    python: false,
    zig: false,
    bundled: getBundledStatus(),
  };

  const check = async (cmd, key, args = ['--version']) => {
    try {
      const { stdout, stderr } = await execTool(cmd, args, { timeout: 2500 });
      result[key] = true;
      const line = String(stdout || stderr || '')
        .split(/\r?\n/)
        .map(l => l.trim())
        .find(Boolean) || '';
      result[key + 'Version'] = line;
      result[key + 'Bundled'] = isBundled(cmd);
    } catch (err) {
      const out = String(err?.stdout || err?.stderr || '');
      if (out.trim()) {
        result[key] = true;
        result[key + 'Version'] = out.split(/\r?\n/).map(l => l.trim()).find(Boolean) || '';
        result[key + 'Bundled'] = isBundled(cmd);
      }
    }
  };

  await Promise.all([
    check('make', 'make'),
    check('python', 'python'),
    check('rustc', 'rust'),
    check('arm-none-eabi-gcc', 'armGcc'),
    check('arm-none-eabi-gdb', 'armGdb'),
    check('openocd', 'openocd'),
    check('zig', 'zig', ['version']),
  ]);

  // rustup target list is slow — only on forced refresh or first success
  if (result.rust && (force || !detectCache?.rustEmbeddedTargets)) {
    try {
      const { stdout } = await execTool('rustup', ['target', 'list', '--installed'], {
        timeout: 4000,
        maxBuffer: 256 * 1024,
      });
      result.rustEmbeddedTargets = String(stdout)
        .split('\n')
        .map(l => l.trim())
        .filter(l => l.includes('thumb') || l.includes('cortex'));
    } catch {}
  } else if (detectCache?.rustEmbeddedTargets) {
    result.rustEmbeddedTargets = detectCache.rustEmbeddedTargets;
  }

  detectCache = result;
  detectCacheAt = Date.now();
  return result;
}

function clearDetectCache() {
  detectCache = null;
  detectCacheAt = 0;
}

function spawnProcess(cmd, args, cwd, onOutput) {
  const proc = spawn(cmd, args, {
    cwd,
    shell: false,
    stdio: ['ignore', 'pipe', 'pipe'],
  });

  let stdout = '';
  let stderr = '';

  proc.stdout.on('data', (data) => {
    const text = data.toString();
    stdout += text;
    onOutput?.({ type: 'stdout', text });
  });

  proc.stderr.on('data', (data) => {
    const text = data.toString();
    stderr += text;
    onOutput?.({ type: 'stderr', text });
  });

  return { proc, getStdout: () => stdout, getStderr: () => stderr };
}

function readCargoPackageName(projectDir) {
  try {
    const toml = fs.readFileSync(path.join(projectDir, 'Cargo.toml'), 'utf8');
    const m = toml.match(/^\s*name\s*=\s*"([^"]+)"/m);
    return m ? m[1] : null;
  } catch {
    return null;
  }
}

function readMakefileTarget(projectDir) {
  try {
    const mk = fs.readFileSync(path.join(projectDir, 'Makefile'), 'utf8');
    const m = mk.match(/^\s*TARGET\s*=\s*(\S+)/m);
    return m ? m[1] : null;
  } catch {
    return null;
  }
}

function resolveBoard(projectDir, config = {}) {
  const meta = readProjectMeta(projectDir);
  const boardId = config.boardId || meta?.boardId || DEFAULT_BOARD_ID;
  return getBoardOrDefault(boardId);
}

/** Parse arm-none-eabi-size / GNU size columnar output → flash/RAM bytes */
function parseSizeOutput(text) {
  const lines = text.split(/\r?\n/);
  for (const line of lines) {
    if (/^\s*text\b/i.test(line)) continue;
    const m = line.trim().match(/^(\d+)\s+(\d+)\s+(\d+)\s+(\d+)/);
    if (m) {
      const textSize = parseInt(m[1], 10);
      const data = parseInt(m[2], 10);
      const bss = parseInt(m[3], 10);
      return {
        flashUsed: textSize + data,
        ramUsed: data + bss,
        text: textSize,
        data,
        bss,
      };
    }
  }
  return null;
}

function reportElfSize(projectDir, projectType, onOutput) {
  try {
    const elf = detectElf(projectDir, projectType);
    if (!elf) return null;
    const result = spawnToolSync('arm-none-eabi-size', [elf], {
      encoding: 'utf8',
      timeout: 15000,
    });
    const out = (result.stdout || '') + (result.stderr || '');
    if (out) onOutput?.({ type: 'stdout', text: out });
    return parseSizeOutput(out);
  } catch {
    return null;
  }
}

function isRustProject(projectType) {
  return projectType === 'rust' || projectType === 'driver-rust' || projectType === 'os-rust';
}

function isMakeProject(projectType) {
  return (
    projectType === 'c' || projectType === 'cpp' || projectType === 'asm' || projectType === 'zig' ||
    projectType === 'driver-c' || projectType === 'driver-cpp' || projectType === 'driver-asm' || projectType === 'driver-zig' ||
    projectType === 'os-c' || projectType === 'os-cpp' || projectType === 'os-asm' || projectType === 'os-zig' ||
    // legacy single-language ids
    projectType === 'driver' || projectType === 'os'
  );
}

function isScriptProject(projectType) {
  return (
    projectType === 'script-c' ||
    projectType === 'script-cpp' ||
    projectType === 'script-rust' ||
    projectType === 'script-asm' ||
    // legacy
    projectType === 'script-python' ||
    projectType === 'script-bash' ||
    projectType === 'script-js' ||
    projectType === 'python' ||
    projectType === 'shell'
  );
}

function scriptEntry(projectDir, projectType, filePath) {
  if (filePath && fs.existsSync(filePath)) return filePath;
  try {
    const meta = readProjectMeta(projectDir);
    if (meta?.entry) {
      const p = path.join(projectDir, meta.entry);
      if (fs.existsSync(p)) return p;
    }
  } catch {}
  const candidates = {
    'script-c': ['main.c', 'src/main.c'],
    'script-cpp': ['main.cpp', 'src/main.cpp'],
    'script-rust': ['main.rs', 'src/main.rs'],
    'script-asm': ['main.S', 'main.s', 'src/main.S', 'src/main.s'],
    'script-python': ['main.py', 'src/main.py'],
    python: ['main.py', 'src/main.py'],
    'script-bash': ['main.sh', 'src/main.sh', 'run.sh'],
    shell: ['main.sh', 'src/main.sh', 'run.sh'],
    'script-js': ['main.js', 'src/main.js', 'index.js'],
  };
  for (const rel of candidates[projectType] || []) {
    const p = path.join(projectDir, rel);
    if (fs.existsSync(p)) return p;
  }
  const exts = {
    'script-c': ['.c'],
    'script-cpp': ['.cpp', '.cc', '.cxx'],
    'script-rust': ['.rs'],
    'script-asm': ['.s', '.S'],
    'script-python': ['.py'],
    python: ['.py'],
    'script-bash': ['.sh'],
    shell: ['.sh'],
    'script-js': ['.js', '.mjs'],
  };
  try {
    for (const name of fs.readdirSync(projectDir)) {
      const lower = name.toLowerCase();
      if ((exts[projectType] || []).some(e => lower.endsWith(e.toLowerCase()))) {
        return path.join(projectDir, name);
      }
    }
  } catch {}
  return null;
}

async function resolveHostCompiler(kind) {
  const lists = {
    c: ['gcc', 'cc', 'clang'],
    cpp: ['g++', 'c++', 'clang++'],
    asm: ['gcc', 'cc', 'clang'],
    rust: ['rustc'],
  };
  for (const cmd of lists[kind] || []) {
    try {
      await execHost(cmd, ['--version'], { timeout: 4000 });
      return { cmd, via: 'host' };
    } catch {}
  }
  // Bundled Zig can compile C/C++/ASM without a system gcc
  if (kind === 'c' || kind === 'cpp' || kind === 'asm') {
    try {
      const zig = resolveTool('zig');
      await execTool('zig', ['version'], { timeout: 4000 });
      return { cmd: zig, via: 'zig' };
    } catch {}
  }
  // Bundled rustc for single-file rust scripts
  if (kind === 'rust') {
    try {
      const rustc = resolveTool('rustc');
      if (rustc && rustc !== 'rustc') {
        await execTool('rustc', ['--version'], { timeout: 4000 });
        return { cmd: rustc, via: 'bundled' };
      }
    } catch {}
  }
  return null;
}

function scriptBinaryPath(projectDir, entry) {
  const base = path.basename(entry, path.extname(entry));
  const outDir = path.join(projectDir, 'build');
  const exe = process.platform === 'win32' ? `${base}.exe` : base;
  return { outDir, outPath: path.join(outDir, exe) };
}

function scriptCompileSpec(projectType, entry, outPath, compiler) {
  if (compiler?.via === 'zig') {
    if (projectType === 'script-c' || projectType === 'script-asm') {
      return { cmd: compiler.cmd, args: ['cc', '-O2', '-g', entry, '-o', outPath], host: false };
    }
    if (projectType === 'script-cpp') {
      return { cmd: compiler.cmd, args: ['c++', '-O2', '-g', entry, '-o', outPath], host: false };
    }
  }
  if (projectType === 'script-c') {
    return { cmd: compiler.cmd, args: ['-O2', '-g', '-Wall', entry, '-o', outPath], host: true };
  }
  if (projectType === 'script-cpp') {
    return { cmd: compiler.cmd, args: ['-O2', '-g', '-Wall', entry, '-o', outPath], host: true };
  }
  if (projectType === 'script-asm') {
    return { cmd: compiler.cmd, args: ['-O2', '-g', entry, '-o', outPath], host: true };
  }
  if (projectType === 'script-rust') {
    return { cmd: compiler.cmd, args: ['-O', '-g', entry, '-o', outPath], host: compiler.via === 'host' };
  }
  return null;
}

function legacyScriptCommand(projectType, entry) {
  if (projectType === 'script-python' || projectType === 'python') {
    return { cmd: 'python', args: [entry] };
  }
  if (projectType === 'script-bash' || projectType === 'shell') {
    return { cmd: 'bash', args: [entry] };
  }
  if (projectType === 'script-js') {
    return { cmd: 'node', args: [entry] };
  }
  return null;
}

function runProcess(cmd, args, cwd, onOutput, { host = false } = {}) {
  return new Promise((resolve, reject) => {
    try {
      if (currentRunProc) {
        try { currentRunProc.kill('SIGTERM'); } catch {}
        currentRunProc = null;
      }
      const proc = host
        ? spawnHost(cmd, args, { cwd, stdio: ['ignore', 'pipe', 'pipe'] })
        : spawnTool(cmd, args, { cwd, stdio: ['ignore', 'pipe', 'pipe'] });
      currentRunProc = proc;
      let stdout = '';
      let stderr = '';
      proc.stdout.on('data', (data) => {
        const text = data.toString();
        stdout += text;
        onOutput?.({ type: 'stdout', text });
      });
      proc.stderr.on('data', (data) => {
        const text = data.toString();
        stderr += text;
        onOutput?.({ type: 'stderr', text });
      });
      proc.on('close', (code) => {
        currentRunProc = null;
        resolve({ code: code ?? 1, stdout, stderr });
      });
      proc.on('error', (err) => {
        currentRunProc = null;
        reject(new Error(`${cmd} not found or failed to start: ${err.message}`));
      });
    } catch (err) {
      currentRunProc = null;
      reject(err);
    }
  });
}

let currentRunProc = null;

/**
 * A path can be handed to Make as a variable only if the recipe shell will
 * not split or reinterpret it: templates expand $(CC) / $(PREFIX)gcc
 * unquoted. Spaces are common (C:\Program Files, user names, "My Projects").
 */
function isMakeSafePath(p) {
  return typeof p === 'string' && p.length > 0 && !/[\s"'`$#%;&|<>()*?\\[\]{}!]/.test(p.replace(/\\/g, '/'));
}

/**
 * Make variable overrides that pin the bundled tools by absolute path.
 * Unsafe paths are omitted: the tools are still found, because the build
 * runs with the bundled bin directory first on PATH (getToolchainEnv).
 * Returns { overrides, skipped } so the caller can explain the fallback.
 */
function makeToolchainOverrides(resolve = resolveTool) {
  const overrides = [];
  const skipped = [];
  const add = (name, value) => {
    if (!value || !(path.isAbsolute(value) || path.win32.isAbsolute(value))) return;
    // A recipe shell (sh.exe under Windows make) treats "\" as an escape;
    // forward slashes work for make, sh and CreateProcess alike.
    const v = /^[a-zA-Z]:\\/.test(value) ? value.replace(/\\/g, '/') : value;
    if (isMakeSafePath(v)) overrides.push(`${name}=${v}`);
    else skipped.push(name);
  };
  const gcc = resolve('arm-none-eabi-gcc');
  // PREFIX=…/arm-none-eabi- → $(PREFIX)gcc resolves to the absolute gcc inside recipes
  if (gcc && path.isAbsolute(gcc) && /arm-none-eabi-gcc(\.exe)?$/i.test(gcc)) {
    add('PREFIX', gcc.replace(/gcc(\.exe)?$/i, ''));
  }
  add('CC', gcc);
  add('CXX', resolve('arm-none-eabi-g++'));
  add('OBJCOPY', resolve('arm-none-eabi-objcopy'));
  add('SIZE', resolve('arm-none-eabi-size'));
  add('ZIG', resolve('zig'));
  add('OPENOCD', resolve('openocd'));
  return { overrides, skipped };
}

function missingBundledHint(tool) {
  return `${tool} not found. Reinstall EmbedIDE (deb/pacman/setup downloads compilers during install), or use Settings → Toolchain → Download / repair.`;
}

function buildProject(projectDir, projectType, onOutput) {
  return new Promise(async (resolve, reject) => {
    let cmd, args;
    buildWasCancelled = false;

    prependBundledToolchainToPath();
    invalidateCache();

    if (isScriptProject(projectType)) {
      try {
        const result = await runScript(projectDir, projectType, null, onOutput);
        resolve(result);
      } catch (err) {
        reject(err);
      }
      return;
    }

    const tc = await detectToolchains({ force: true });

    if (isRustProject(projectType)) {
      if (!tc.rust) {
        reject(new Error(missingBundledHint('rustc/cargo')));
        return;
      }
      cmd = 'cargo';
      args = ['build', '--release'];

      try {
        const config = path.join(projectDir, '.cargo', 'config.toml');
        if (fs.existsSync(config)) {
          const cfg = fs.readFileSync(config, 'utf8');
          const match = cfg.match(/target\s*=\s*"([^"]+)"/);
          if (match) {
            const target = match[1];
            if (!RUST_TARGET_RE.test(target)) {
              reject(new Error(`Invalid rust target: ${target}`));
              return;
            }
            const installed = spawnToolSync('rustup', ['target', 'list', '--installed'], { encoding: 'utf8', stdio: ['ignore', 'pipe', 'ignore'] }).stdout || '';
            const installedTargets = installed.split(/\r?\n/).map(l => l.trim()).filter(Boolean);
            if (!installedTargets.includes(target)) {
              onOutput?.({ type: 'stdout', text: `Installing target ${target}...\n` });
              const add = spawnToolSync('rustup', ['target', 'add', target], { stdio: 'ignore', timeout: 120000 });
              if (add.status !== 0) {
                onOutput?.({ type: 'stderr', text: `Failed to install rustup target ${target}\n` });
              }
            }
          }
        }
      } catch (err) {
        if (err.message?.includes('Invalid rust target')) {
          reject(err);
          return;
        }
      }
    } else if (isMakeProject(projectType)) {
      if (!tc.make) {
        reject(new Error(missingBundledHint('make')));
        return;
      }
      if (projectType.includes('zig') || projectType === 'zig') {
        if (!tc.zig) {
          reject(new Error(missingBundledHint('zig')));
          return;
        }
      } else if (!tc.armGcc) {
        reject(new Error(missingBundledHint('arm-none-eabi-gcc')));
        return;
      }
      cmd = 'make';
      const { overrides, skipped } = makeToolchainOverrides();
      args = ['-j4', ...overrides];
      onOutput?.({
        type: 'stdout',
        text: `Using bundled toolchain${tc.bundled?.root ? `: ${tc.bundled.root}` : ''}\n`,
      });
      if (skipped.length) {
        onOutput?.({
          type: 'stdout',
          text: `Toolchain path contains spaces or shell characters; ${skipped.join(', ')} resolved through PATH instead.\n`,
        });
      }
    } else {
      reject(new Error(`Unknown project type: ${projectType}`));
      return;
    }

    try {
      const proc = spawnTool(cmd, args, {
        cwd: projectDir,
        stdio: ['ignore', 'pipe', 'pipe'],
      });
      currentBuildProc = proc;

      let stdout = '';
      let stderr = '';

      proc.stdout.on('data', (data) => {
        const text = data.toString();
        stdout += text;
        onOutput?.({ type: 'stdout', text });
      });

      proc.stderr.on('data', (data) => {
        const text = data.toString();
        stderr += text;
        onOutput?.({ type: 'stderr', text });
      });

      proc.on('close', (code) => {
        currentBuildProc = null;
        if (buildWasCancelled) {
          buildWasCancelled = false;
          resolve({ code: code ?? null, cancelled: true, stdout, stderr });
          return;
        }
        if (code === 0) {
          const fromLog = parseSizeOutput(stdout + '\n' + stderr);
          const fromElf = reportElfSize(projectDir, projectType, onOutput);
          const size = fromElf || fromLog;
          if (size) {
            onOutput?.({
              type: 'stdout',
              text: `FLASH: ${size.flashUsed} bytes\nRAM: ${size.ramUsed} bytes\n`,
            });
          }
          resolve({ code, stdout, stderr });
        } else {
          reject(new Error(`Build failed with code ${code}`));
        }
      });

      proc.on('error', (err) => {
        currentBuildProc = null;
        reject(err);
      });
    } catch (err) {
      currentBuildProc = null;
      reject(err);
    }
  });
}

function runScript(projectDir, projectType, filePath, onOutput) {
  return new Promise(async (resolve, reject) => {
    const entry = scriptEntry(projectDir, projectType, filePath);
    if (!entry) {
      reject(new Error('No script entry found (.c / .cpp / .rs / .S).'));
      return;
    }

    const legacy = legacyScriptCommand(projectType, entry);
    if (legacy) {
      onOutput?.({ type: 'stdout', text: `Running ${path.basename(entry)}...\n` });
      try {
        const result = await runProcess(legacy.cmd, legacy.args, projectDir, onOutput);
        if (result.code === 0) resolve(result);
        else reject(new Error(`Script exited with code ${result.code}`));
      } catch (err) {
        reject(err);
      }
      return;
    }

    const { outDir, outPath } = scriptBinaryPath(projectDir, entry);
    const kind =
      projectType === 'script-cpp' ? 'cpp'
      : projectType === 'script-rust' ? 'rust'
      : projectType === 'script-asm' ? 'asm'
      : 'c';

    try {
      fs.mkdirSync(outDir, { recursive: true });
      prependBundledToolchainToPath();
      const compiler = await resolveHostCompiler(kind);
      if (!compiler) {
        const hint =
          kind === 'rust'
            ? missingBundledHint('rustc')
            : 'Need gcc/clang on the host, or download Zig via Settings → Toolchain.';
        reject(new Error(`Host compiler not found for ${projectType}. ${hint}`));
        return;
      }

      const compile = scriptCompileSpec(projectType, entry, outPath, compiler);
      if (!compile) {
        reject(new Error(`Unsupported script type: ${projectType}`));
        return;
      }

      onOutput?.({ type: 'stdout', text: `Compiling ${path.basename(entry)} with ${compiler.via === 'zig' ? 'zig' : path.basename(compiler.cmd)}...\n` });
      const built = await runProcess(compile.cmd, compile.args, projectDir, onOutput, { host: !!compile.host });
      if (built.code !== 0) {
        reject(new Error(`Compile failed with code ${built.code}`));
        return;
      }

      onOutput?.({ type: 'stdout', text: `Running ${path.basename(outPath)}...\n` });
      const result = await runProcess(outPath, [], projectDir, onOutput, { host: true });
      if (result.code === 0) resolve(result);
      else reject(new Error(`Script exited with code ${result.code}`));
    } catch (err) {
      reject(err);
    }
  });
}

function cancelRun() {
  if (currentRunProc) {
    try {
      if (process.platform === 'win32') {
        spawn('taskkill', ['/pid', String(currentRunProc.pid), '/f', '/t']);
      } else {
        currentRunProc.kill('SIGTERM');
      }
    } catch {}
    currentRunProc = null;
    return true;
  }
  return false;
}

/**
 * "Debug" for host script projects runs the script. Firmware is debugged
 * interactively by DebugManager (electron/debugSession.js).
 */
async function startDebugSession(projectDir, projectType, config = {}, onOutput) {
  if (isScriptProject(projectType)) {
    return runScript(projectDir, projectType, config.filePath || null, onOutput);
  }
  throw new Error('Firmware projects use the interactive debugger (Run → Start Debugging).');
}

function cancelDebugSession() {
  return cancelRun();
}

function cancelBuild() {
  cancelRun();
  if (currentBuildProc) {
    buildWasCancelled = true;
    try {
      if (process.platform === 'win32') {
        spawn('taskkill', ['/pid', String(currentBuildProc.pid), '/f', '/t'])
      } else {
        currentBuildProc.kill('SIGTERM')
      }
    } catch {}
    currentBuildProc = null;
    return true;
  }
  return false;
}

function detectElf(projectDir, projectType, elfPath) {
  if (elfPath && fs.existsSync(elfPath)) return elfPath;

  const baseName = path.basename(projectDir);
  const cargoName = readCargoPackageName(projectDir) || baseName;
  const makeTarget = readMakefileTarget(projectDir) || baseName;
  const board = resolveBoard(projectDir);
  const rustTarget = board.rustTarget;

  const candidates = [];

  if (isRustProject(projectType)) {
    candidates.push(
      path.join(projectDir, 'target', rustTarget, 'release', cargoName),
      path.join(projectDir, 'target', rustTarget, 'release', cargoName + '.elf'),
      path.join(projectDir, 'target', 'thumbv7em-none-eabihf', 'release', cargoName),
      path.join(projectDir, 'target', 'release', cargoName),
    );
  } else if (projectType === 'zig' || projectType === 'driver-zig' || projectType === 'os-zig') {
    candidates.push(
      path.join(projectDir, 'build', makeTarget + '.elf'),
      path.join(projectDir, 'build', baseName + '.elf'),
      path.join(projectDir, makeTarget),
      path.join(projectDir, makeTarget + '.elf'),
    );
  } else {
    candidates.push(
      path.join(projectDir, 'build', makeTarget + '.elf'),
      path.join(projectDir, 'build', baseName + '.elf'),
    );
  }

  // Fallback scan
  candidates.push(
    path.join(projectDir, 'target', rustTarget, 'release', baseName),
    path.join(projectDir, 'build', path.basename(projectDir) + '.elf'),
  );

  for (const p of candidates) {
    if (fs.existsSync(p)) return p;
  }
  return null;
}

async function flashBoard(projectDir, projectType, config = {}, onOutput) {
  const board = resolveBoard(projectDir, config);
  const adapter = config.adapter || board.defaultAdapter || 'stlink';
  const target = config.target || board.openocdTarget;

  if (!/^[a-zA-Z0-9._-]+$/.test(adapter) || !/^[a-zA-Z0-9._-]+$/.test(target)) {
    throw new Error('Invalid OpenOCD adapter or target name');
  }

  const elf = detectElf(projectDir, projectType, config.elfPath);
  if (!elf) {
    throw new Error('No ELF file found. Build the project first.');
  }

  prependBundledToolchainToPath();
  const tc = await detectToolchains({ force: true });
  if (!tc.openocd) {
    throw new Error(missingBundledHint('OpenOCD'));
  }

  // OpenOCD expects forward slashes; quote path for spaces
  const elfArg = elf.replace(/\\/g, '/');
  const openocdArgs = [];
  const scripts = findOpenocdScripts(getBundledRoot());
  if (scripts) {
    openocdArgs.push('-s', scripts);
  }
  openocdArgs.push(
    '-f', `interface/${adapter}.cfg`,
    '-f', `target/${target}.cfg`,
    '-c', `program "${elfArg}" verify reset exit`,
  );

  onOutput?.({ type: 'stdout', text: `Flashing ${elf} via ${adapter} → ${board.mcu} (${target})...\n` });

  return new Promise((resolve, reject) => {
    const proc = spawnTool('openocd', openocdArgs, {
      cwd: projectDir,
      stdio: ['ignore', 'pipe', 'pipe'],
    });

    proc.stdout.on('data', (data) => onOutput?.({ type: 'stdout', text: data.toString() }));
    proc.stderr.on('data', (data) => onOutput?.({ type: 'stderr', text: data.toString() }));

    proc.on('close', (code) => {
      if (code === 0) resolve({ code, boardId: board.id });
      else reject(new Error(`Flash failed with code ${code}`));
    });

    proc.on('error', (err) => reject(err));
  });
}

module.exports = {
  detectToolchains,
  clearDetectCache,
  buildProject,
  flashBoard,
  cancelBuild,
  detectElf,
  resolveBoard,
  parseSizeOutput,
  runScript,
  startDebugSession,
  cancelDebugSession,
  cancelRun,
  isScriptProject,
  getBundledStatus,
  makeToolchainOverrides,
  isMakeSafePath,
};
