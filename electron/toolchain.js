const { execFile, execSync, spawn, spawnSync } = require('child_process');
const { promisify } = require('util');
const path = require('path');
const fs = require('fs');
const { getBoardOrDefault, DEFAULT_BOARD_ID } = require('./boards');
const { readProjectMeta } = require('./project');
const { resolveTool, getToolchainEnv, isBundled, getBundledStatus, prependBundledToolchainToPath, findOpenocdScripts, getBundledRoot } = require('./bundledToolchain');
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

async function detectToolchains() {
  prependBundledToolchainToPath();
  const result = {
    rust: false,
    armGcc: false,
    openocd: false,
    make: false,
    python: false,
    zig: false,
    bundled: getBundledStatus(),
  };

  const check = async (cmd, key, args = ['--version']) => {
    try {
      const { stdout } = await execTool(cmd, args, { timeout: 5000 });
      result[key] = true;
      result[key + 'Version'] = String(stdout).split('\n')[0].trim();
      result[key + 'Bundled'] = isBundled(cmd);
    } catch {}
  };

  await Promise.all([
    check('make', 'make'),
    check('python', 'python'),
    check('rustc', 'rust'),
    check('arm-none-eabi-gcc', 'armGcc'),
    check('openocd', 'openocd'),
    check('zig', 'zig', ['version']),
  ]);

  if (result.rust) {
    try {
      const { stdout } = await execTool('rustup', ['target', 'list', '--installed'], {
        timeout: 8000,
        maxBuffer: 256 * 1024,
      });
      result.rustEmbeddedTargets = String(stdout)
        .split('\n')
        .map(l => l.trim())
        .filter(l => l.includes('thumb') || l.includes('cortex'));
    } catch {}
  }

  return result;
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
  // Fallback: first matching extension in project root
  const exts = {
    'script-python': ['.py'],
    python: ['.py'],
    'script-bash': ['.sh'],
    shell: ['.sh'],
    'script-js': ['.js', '.mjs'],
  };
  try {
    for (const name of fs.readdirSync(projectDir)) {
      const lower = name.toLowerCase();
      if ((exts[projectType] || []).some(e => lower.endsWith(e))) {
        return path.join(projectDir, name);
      }
    }
  } catch {}
  return null;
}

function scriptCommand(projectType, entry) {
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

let currentRunProc = null;
let currentOpenocdProc = null;

function buildProject(projectDir, projectType, onOutput) {
  return new Promise(async (resolve, reject) => {
    let cmd, args;
    buildWasCancelled = false;

    if (isScriptProject(projectType)) {
      try {
        const result = await runScript(projectDir, projectType, null, onOutput);
        resolve(result);
      } catch (err) {
        reject(err);
      }
      return;
    }

    const tc = await detectToolchains();

    if (isRustProject(projectType)) {
      if (!tc.rust) {
        reject(new Error('Rust toolchain not found. Install rustup: https://rustup.rs'));
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
        reject(new Error('make not found. Install build-essential / Xcode CLT / mingw make.'));
        return;
      }
      if (projectType.includes('zig') || projectType === 'zig') {
        if (!tc.zig) {
          reject(new Error('Zig not found. Install from https://ziglang.org/download/'));
          return;
        }
      } else if (!tc.armGcc) {
        reject(new Error('arm-none-eabi-gcc not found. Install the GNU Arm Embedded Toolchain.'));
        return;
      }
      cmd = 'make';
      args = ['-j4'];
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
      reject(new Error('No script entry found (main.py / main.sh / main.js).'));
      return;
    }
    const spec = scriptCommand(projectType, entry);
    if (!spec) {
      reject(new Error(`Unsupported script type: ${projectType}`));
      return;
    }

    onOutput?.({ type: 'stdout', text: `Running ${path.basename(entry)}...\n` });

    try {
      if (currentRunProc) {
        try { currentRunProc.kill('SIGTERM'); } catch {}
        currentRunProc = null;
      }
      const proc = spawnTool(spec.cmd, spec.args, {
        cwd: projectDir,
        stdio: ['ignore', 'pipe', 'pipe'],
      });
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
        if (code === 0) resolve({ code, stdout, stderr });
        else reject(new Error(`Script exited with code ${code}`));
      });
      proc.on('error', (err) => {
        currentRunProc = null;
        reject(new Error(`${spec.cmd} not found or failed to start: ${err.message}`));
      });
    } catch (err) {
      currentRunProc = null;
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

  const tc = await detectToolchains();
  if (!tc.openocd) {
    throw new Error('OpenOCD not found. Install OpenOCD to debug on target.');
  }
  if (!tc.armGcc) {
    throw new Error('arm-none-eabi-gdb not found (GNU Arm Embedded Toolchain required).');
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

  const tc = await detectToolchains();
  if (!tc.openocd) {
    const installGuide = process.platform === 'win32'
      ? 'Download from https://github.com/openocd-org/openocd/releases'
      : process.platform === 'darwin'
        ? 'Install with: brew install openocd'
        : 'Install with: sudo apt install openocd';
    throw new Error(`OpenOCD not found. ${installGuide}`);
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
