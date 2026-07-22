const { execSync, spawn } = require('child_process');
const path = require('path');
const fs = require('fs');

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
    try { currentBuildProc.kill('SIGTERM') } catch {}
    currentBuildProc = null;
    return true;
  }
  return false;
}

function flashBoard(projectDir, projectType, config, onOutput) {
  return new Promise((resolve, reject) => {
    const { adapter = 'stlink', target = 'stm32f4x', elfPath } = config;

    const detectElf = () => {
      if (elfPath && fs.existsSync(elfPath)) return elfPath;
      const candidates = [
        path.join(projectDir, 'target', 'thumbv7em-none-eabihf', 'release', path.basename(projectDir)),
        path.join(projectDir, 'target', 'release', path.basename(projectDir)),
        path.join(projectDir, 'build', path.basename(projectDir) + '.elf'),
      ];
      for (const c of candidates) {
        for (const ext of ['', '.elf']) {
          const p = c + ext;
          if (fs.existsSync(p)) return p;
        }
      }
      return null;
    };

    const elf = detectElf();
    if (!elf) {
      reject(new Error('No ELF file found. Build the project first.'));
      return;
    }

    if (!detectToolchains().openocd) {
      reject(new Error('OpenOCD not found. Install it: sudo apt install openocd'));
      return;
    }

    const openocdArgs = [
      '-f', `interface/${adapter}.cfg`,
      '-f', `target/${target}.cfg`,
      '-c', `program ${elf} verify reset exit`,
    ];

    onOutput?.({ type: 'stdout', text: `Flashing ${elf} via ${adapter}...\n` });

    const proc = spawn('openocd', openocdArgs, {
      cwd: projectDir,
      shell: false,
      stdio: ['ignore', 'pipe', 'pipe'],
    });

    proc.stdout.on('data', (data) => onOutput?.({ type: 'stdout', text: data.toString() }));
    proc.stderr.on('data', (data) => onOutput?.({ type: 'stderr', text: data.toString() }));

    proc.on('close', (code) => {
      if (code === 0) resolve({ code });
      else reject(new Error(`Flash failed with code ${code}`));
    });

    proc.on('error', (err) => reject(err));
  });
}

module.exports = { detectToolchains, buildProject, flashBoard, cancelBuild };
