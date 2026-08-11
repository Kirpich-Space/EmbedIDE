const { execFile, execSync, spawn, spawnSync } = require('child_process');
const { promisify } = require('util');
const path = require('path');
const fs = require('fs');
const { getBoardOrDefault, DEFAULT_BOARD_ID } = require('./boards');
const { readProjectMeta } = require('./project');
const { resolveTool, getToolchainEnv, isBundled, getBundledStatus, prependBundledToolchainToPath, findOpenocdScripts, getBundledRoot, getBundledBinDirs, invalidateCache } = require('./bundledToolchain');
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
  const pathKey = process.platform === 'win32' ? 'Path' : 'PATH';
  const sep = process.platform === 'win32' ? ';' : ':';
  const raw = process.env[pathKey] || process.env.PATH || '';
  const filtered = raw
    .split(sep)
    .filter(p => p && !bins.has(path.resolve(p)))
    .join(sep);
  return { ...process.env, ...extra, [pathKey]: filtered };
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
let currentOpenocdProc = null;

function makeToolchainOverrides() {
  const overrides = [];
  const gcc = resolveTool('arm-none-eabi-gcc');
  const gxx = resolveTool('arm-none-eabi-g++');
  const objcopy = resolveTool('arm-none-eabi-objcopy');
  const size = resolveTool('arm-none-eabi-size');
  const zig = resolveTool('zig');
  const openocd = resolveTool('openocd');

  // PREFIX=…/arm-none-eabi- → $(PREFIX)gcc resolves to absolute gcc even inside Make recipes
  if (gcc && path.isAbsolute(gcc) && /arm-none-eabi-gcc(\.exe)?$/i.test(gcc)) {
    const prefix = gcc.replace(/gcc(\.exe)?$/i, '');
    overrides.push(`PREFIX=${prefix}`);
  }
  if (gcc && path.isAbsolute(gcc)) overrides.push(`CC=${gcc}`);
  if (gxx && path.isAbsolute(gxx)) overrides.push(`CXX=${gxx}`);
  if (objcopy && path.isAbsolute(objcopy)) overrides.push(`OBJCOPY=${objcopy}`);
  if (size && path.isAbsolute(size)) overrides.push(`SIZE=${size}`);
  if (zig && path.isAbsolute(zig)) overrides.push(`ZIG=${zig}`);
  if (openocd && path.isAbsolute(openocd)) {
    overrides.push(`OPENOCD=${openocd}`);
    // Many Makefiles hardcode `openocd` — also put a Make variable some templates use
  }
  return overrides;
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
      args = ['-j4', ...makeToolchainOverrides()];
      onOutput?.({
        type: 'stdout',
        text: `Using bundled toolchain${tc.bundled?.root ? `: ${tc.bundled.root}` : ''}\n`,
      });
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

function sleep(ms) {
  return new Promise(r => setTimeout(r, ms));
}

async function startDebugSession(projectDir, projectType, config = {}, onOutput) {
  if (isScriptProject(projectType)) {
    return runScript(projectDir, projectType, config.filePath || null, onOutput);
  }

  prependBundledToolchainToPath();
  const tc = await detectToolchains({ force: true });
  if (!tc.openocd) {
    throw new Error(missingBundledHint('OpenOCD'));
  }
  if (!tc.armGdb && !tc.armGcc) {
    throw new Error(missingBundledHint('arm-none-eabi-gdb'));
  }

  const board = resolveBoard(projectDir, config);
  const adapter = config.adapter || board.defaultAdapter || 'stlink';
  const target = config.target || board.openocdTarget;
  if (!/^[a-zA-Z0-9._-]+$/.test(adapter) || !/^[a-zA-Z0-9._-]+$/.test(target)) {
    throw new Error('Invalid OpenOCD adapter or target name');
  }

  let elf = detectElf(projectDir, projectType, config.elfPath);
  if (!elf) {
    onOutput?.({ type: 'stdout', text: 'No ELF found — compiling before debug...\n' });
    await buildProject(projectDir, projectType, onOutput);
    elf = detectElf(projectDir, projectType, config.elfPath);
  }
  if (!elf) {
    throw new Error('No ELF file found after build. Check compile errors above.');
  }

  cancelDebugSession();

  onOutput?.({ type: 'stdout', text: `Starting OpenOCD (${adapter} → ${target})...\n` });

  const ocdArgs = [];
  const scripts = findOpenocdScripts(getBundledRoot());
  if (scripts) ocdArgs.push('-s', scripts);
  ocdArgs.push(
    '-f', `interface/${adapter}.cfg`,
    '-f', `target/${target}.cfg`,
    '-c', 'gdb_port 3333',
    '-c', 'tcl_port disabled',
    '-c', 'telnet_port disabled',
  );

  const openocd = spawnTool('openocd', ocdArgs, {
    cwd: projectDir,
    stdio: ['ignore', 'pipe', 'pipe'],
  });
  currentOpenocdProc = openocd;

  openocd.stdout.on('data', (d) => onOutput?.({ type: 'stdout', text: d.toString() }));
  openocd.stderr.on('data', (d) => onOutput?.({ type: 'stderr', text: d.toString() }));

  // Wait until OpenOCD listens or fails
  let ready = false;
  const onReadyChunk = (buf) => {
    const s = buf.toString();
    if (/Listening on port 3333|gdb.?port|started by/i.test(s)) ready = true;
  };
  openocd.stdout.on('data', onReadyChunk);
  openocd.stderr.on('data', onReadyChunk);
  for (let i = 0; i < 20 && openocd.exitCode == null && !ready; i++) {
    await sleep(150);
  }
  openocd.stdout.off?.('data', onReadyChunk);
  openocd.stderr.off?.('data', onReadyChunk);

  if (openocd.exitCode != null) {
    currentOpenocdProc = null;
    throw new Error('OpenOCD exited before GDB could connect. Check probe/cable and board power.');
  }

  onOutput?.({ type: 'stdout', text: `Connecting GDB → ${elf}\n` });

  return new Promise((resolve, reject) => {
    const gdb = spawnTool('arm-none-eabi-gdb', [
      '-q', elf,
      '-batch',
      '-ex', 'set pagination off',
      '-ex', 'set confirm off',
      '-ex', 'target extended-remote :3333',
      '-ex', 'monitor reset halt',
      '-ex', 'load',
      '-ex', 'info registers',
      '-ex', 'bt',
      '-ex', 'detach',
    ], {
      cwd: projectDir,
      stdio: ['ignore', 'pipe', 'pipe'],
    });
    currentRunProc = gdb;

    gdb.stdout.on('data', (d) => onOutput?.({ type: 'stdout', text: d.toString() }));
    gdb.stderr.on('data', (d) => onOutput?.({ type: 'stderr', text: d.toString() }));

    gdb.on('close', (code) => {
      currentRunProc = null;
      cancelDebugSession();
      if (code === 0 || code === null) {
        onOutput?.({
          type: 'stdout',
          text: 'Debug OK: firmware loaded, CPU halted, registers + backtrace dumped.\n',
        });
        resolve({ code: code ?? 0 });
      } else {
        reject(new Error(`GDB exited with code ${code}`));
      }
    });
    gdb.on('error', (err) => {
      currentRunProc = null;
      cancelDebugSession();
      reject(err);
    });
  });
}

function cancelDebugSession() {
  cancelRun();
  if (currentOpenocdProc) {
    try {
      if (process.platform === 'win32') {
        spawn('taskkill', ['/pid', String(currentOpenocdProc.pid), '/f', '/t']);
      } else {
        currentOpenocdProc.kill('SIGTERM');
      }
    } catch {}
    currentOpenocdProc = null;
    return true;
  }
  return false;
}

function cancelBuild() {
  cancelDebugSession();
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
};
