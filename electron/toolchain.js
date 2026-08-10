const { execSync, spawn, spawnSync } = require('child_process');
const path = require('path');
const fs = require('fs');
const { getBoardOrDefault, DEFAULT_BOARD_ID } = require('./boards');
const { readProjectMeta } = require('./project');

let currentBuildProc = null;
let buildWasCancelled = false;

const RUST_TARGET_RE = /^[a-zA-Z0-9._-]+$/;

function detectToolchains() {
  const result = { rust: false, armGcc: false, openocd: false, make: false, python: false, zig: false };

  const check = (cmd, key, versionFlag = '--version') => {
    try {
      const out = execSync(`${cmd} ${versionFlag}`, { encoding: 'utf8', stdio: ['ignore', 'pipe', 'ignore'] });
      result[key] = true;
      result[key + 'Version'] = out.split('\n')[0].trim();
    } catch {}
  };

  check('make', 'make')
  check('python3', 'python')
  check('rustc', 'rust')
  check('arm-none-eabi-gcc', 'armGcc')
  check('openocd', 'openocd')
  check('zig', 'zig', 'version')

  if (result.rust) {
    try {
      const targets = execSync('rustup target list --installed', { encoding: 'utf8', stdio: ['ignore', 'pipe', 'ignore'] });
      result.rustEmbeddedTargets = targets
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
    const result = spawnSync('arm-none-eabi-size', [elf], {
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

function buildProject(projectDir, projectType, onOutput) {
  return new Promise(async (resolve, reject) => {
    let cmd, args;
    buildWasCancelled = false;

    if (isRustProject(projectType)) {
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
            const installed = execSync('rustup target list --installed', { encoding: 'utf8', stdio: ['ignore', 'pipe', 'ignore'] });
            const installedTargets = installed.split(/\r?\n/).map(l => l.trim()).filter(Boolean);
            if (!installedTargets.includes(target)) {
              onOutput?.({ type: 'stdout', text: `Installing target ${target}...\n` });
              const add = spawnSync('rustup', ['target', 'add', target], { stdio: 'ignore', timeout: 120000 });
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
      cmd = 'make';
      args = ['-j4'];
    } else {
      reject(new Error(`Unknown project type: ${projectType}`));
      return;
    }

    try {
      const proc = spawn(cmd, args, {
        cwd: projectDir,
        shell: false,
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

function cancelBuild() {
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

function flashBoard(projectDir, projectType, config = {}, onOutput) {
  return new Promise((resolve, reject) => {
    const board = resolveBoard(projectDir, config);
    const adapter = config.adapter || board.defaultAdapter || 'stlink';
    const target = config.target || board.openocdTarget;

    if (!/^[a-zA-Z0-9._-]+$/.test(adapter) || !/^[a-zA-Z0-9._-]+$/.test(target)) {
      reject(new Error('Invalid OpenOCD adapter or target name'));
      return;
    }

    const elf = detectElf(projectDir, projectType, config.elfPath);
    if (!elf) {
      reject(new Error('No ELF file found. Build the project first.'));
      return;
    }

    if (!detectToolchains().openocd) {
      const installGuide = process.platform === 'win32'
        ? 'Download from https://github.com/openocd-org/openocd/releases'
        : process.platform === 'darwin'
          ? 'Install with: brew install openocd'
          : 'Install with: sudo apt install openocd'
      reject(new Error(`OpenOCD not found. ${installGuide}`));
      return;
    }

    // OpenOCD expects forward slashes; quote path for spaces
    const elfArg = elf.replace(/\\/g, '/');
    const openocdArgs = [
      '-f', `interface/${adapter}.cfg`,
      '-f', `target/${target}.cfg`,
      '-c', `program "${elfArg}" verify reset exit`,
    ];

    onOutput?.({ type: 'stdout', text: `Flashing ${elf} via ${adapter} → ${board.mcu} (${target})...\n` });

    const proc = spawn('openocd', openocdArgs, {
      cwd: projectDir,
      shell: false,
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

module.exports = { detectToolchains, buildProject, flashBoard, cancelBuild, detectElf, resolveBoard, parseSizeOutput };
