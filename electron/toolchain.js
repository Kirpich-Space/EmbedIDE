const { execSync, spawn } = require('child_process');
const path = require('path');
const fs = require('fs');
const { getBoardOrDefault, DEFAULT_BOARD_ID } = require('./boards');
const { readProjectMeta } = require('./project');

let currentBuildProc = null;

function detectToolchains() {
  const result = { rust: false, armGcc: false, openocd: false, make: false, python: false };

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
    shell: true,
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

  return { proc, stdout, stderr };
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

function buildProject(projectDir, projectType, onOutput) {
  return new Promise((resolve, reject) => {
    let cmd, args;

    if (projectType === 'rust') {
      cmd = 'cargo';
      args = ['build', '--release'];

      try {
        const config = path.join(projectDir, '.cargo', 'config.toml');
        if (fs.existsSync(config)) {
          const cfg = fs.readFileSync(config, 'utf8');
          const match = cfg.match(/target\s*=\s*"([^"]+)"/);
          if (match) {
            const target = match[1];
            const installed = execSync('rustup target list --installed', { encoding: 'utf8', stdio: ['ignore', 'pipe', 'ignore'] });
            if (!installed.includes(target)) {
              onOutput?.({ type: 'stdout', text: `Installing target ${target}...\n` });
              execSync(`rustup target add ${target}`, { stdio: 'ignore', timeout: 120000 });
            }
          }
        }
      } catch {}
    } else if (projectType === 'c' || projectType === 'cpp' || projectType === 'asm') {
      cmd = 'make';
      args = ['-j4'];
    } else {
      reject(new Error(`Unknown project type: ${projectType}`));
      return;
    }

    const { proc, stdout, stderr } = spawnProcess(cmd, args, projectDir, onOutput);
    currentBuildProc = proc;

    proc.on('close', (code) => {
      currentBuildProc = null;
      if (code === 0) resolve({ code, stdout, stderr });
      else reject(new Error(`Build failed with code ${code}`));
    });

    proc.on('error', (err) => {
      currentBuildProc = null;
      reject(err);
    });
  });
}

function cancelBuild() {
  if (currentBuildProc) {
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

  if (projectType === 'rust') {
    candidates.push(
      path.join(projectDir, 'target', rustTarget, 'release', cargoName),
      path.join(projectDir, 'target', rustTarget, 'release', cargoName + '.elf'),
      path.join(projectDir, 'target', 'thumbv7em-none-eabihf', 'release', cargoName),
      path.join(projectDir, 'target', 'release', cargoName),
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

    const openocdArgs = [
      '-f', `interface/${adapter}.cfg`,
      '-f', `target/${target}.cfg`,
      '-c', `program ${elf} verify reset exit`,
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

module.exports = { detectToolchains, buildProject, flashBoard, cancelBuild, detectElf, resolveBoard };
