const { app, BrowserWindow, ipcMain, dialog, Menu, shell, nativeImage, safeStorage } = require('electron');
const path = require('path');
const { detectToolchains, buildProject, flashBoard, cancelBuild, runScript, startDebugSession, cancelDebugSession, clearDetectCache, detectElf, resolveBoard, isScriptProject } = require('./toolchain');
const { prependBundledToolchainToPath, getBundledStatus, resolveTool, getToolchainEnv, findOpenocdScripts, getBundledRoot } = require('./bundledToolchain');
const { DebugManager, toError } = require('./debugSession');
const { encodeSettingsForDisk, decodeSettingsFromDisk, writeFileAtomic } = require('./settingsStore');
const { installToolchain, needsToolchainInstall, isInstallRunning, getInstallProgress } = require('./toolchainInstaller');
const { SerialService } = require('./serial');
const { createProject, listProjectFiles, readProjectFile, writeProjectFile, createProjectFile, deleteProjectFile, renameProjectFile, searchInFiles, getTemplateList, readProjectMeta } = require('./project');
const { listBoards, getBoard, getBoardOrDefault, DEFAULT_BOARD_ID } = require('./boards');
const { getCliStatus, chatViaCli, cancelCliChat } = require('./aiCli');
const fs = require('fs');

const APP_NAME = 'EmbedIDE';
const APP_ID = 'com.kirpichspace.embedide';
/** Stable config dir — must match installers (Setup.sh / NSIS / ps1). */
const USER_DATA_DIR = 'embed-ide';

// Identity before ready — otherwise taskbar/dock shows "Electron"
try {
  app.setName(APP_NAME);
  if (typeof process.setName === 'function') process.setName(APP_NAME);
} catch {}
try { process.title = APP_NAME; } catch {}
// Keep userData at …/embed-ide so package installers and the app share toolchain/settings
try {
  app.setPath('userData', path.join(app.getPath('appData'), USER_DATA_DIR));
} catch {}
if (process.platform === 'win32') {
  try { app.setAppUserModelId(APP_ID); } catch {}
}

// GPU: keep Windows software path by default (black-screen history). Linux/mac use GPU.
// Force off everywhere: EMBEDIDE_DISABLE_GPU=1 — force on: EMBEDIDE_ENABLE_GPU=1
if (process.env.EMBEDIDE_DISABLE_GPU === '1') {
  app.disableHardwareAcceleration();
} else if (process.platform === 'win32' && process.env.EMBEDIDE_ENABLE_GPU !== '1') {
  app.disableHardwareAcceleration();
}

try {
  prependBundledToolchainToPath();
} catch (e) {
  console.warn('Bundled toolchain PATH setup failed:', e.message);
}

let mainWindow;
let allowWindowClose = false;
let closeRequestTimer = null;
const VITE_DEV_SERVER_URL = process.env.VITE_DEV_SERVER_URL;
const SETTINGS_PATH = path.join(app.getPath('userData'), 'settings.json');

function resolveAppIconPath() {
  const names = ['512.png', '256.png', '128.png', 'icon.png', 'icon.ico'];
  const roots = [];
  try {
    if (process.resourcesPath) roots.push(path.join(process.resourcesPath, 'icons'));
  } catch {}
  roots.push(path.join(__dirname, '..', 'build', 'icons'));
  roots.push(path.join(__dirname, '..', 'build'));
  for (const root of roots) {
    for (const name of names) {
      const p = path.join(root, name);
      if (fs.existsSync(p)) return p;
    }
  }
  return null;
}

function loadAppIcon() {
  const p = resolveAppIconPath();
  if (!p) return null;
  try {
    const img = nativeImage.createFromPath(p);
    return img && !img.isEmpty() ? img : null;
  } catch {
    return null;
  }
}

/**
 * Project roots the user opened (dialog) or created in this session. Every
 * filesystem, build, flash and debug request from the renderer must name one
 * of them, so renderer content cannot read or run things elsewhere.
 */
const openProjectRoots = new Set();

function trustProjectRoot(dir) {
  const root = path.resolve(dir);
  openProjectRoots.add(root);
  return root;
}

function requireProjectRoot(projectDir) {
  if (!projectDir || typeof projectDir !== 'string') {
    throw new Error('Project directory required')
  }
  const root = path.resolve(projectDir)
  if (!openProjectRoots.has(root)) {
    throw new Error('This folder is not an open EmbedIDE project. Open it with File → Open Project.')
  }
  return root
}

/** Resolve targetPath under projectDir; reject traversal / absolute escapes. */
function assertInsideProject(projectDir, targetPath) {
  const root = requireProjectRoot(projectDir)
  const resolved = path.isAbsolute(targetPath)
    ? path.resolve(targetPath)
    : path.resolve(root, targetPath)
  const rel = path.relative(root, resolved)
  if (rel.startsWith('..') || path.isAbsolute(rel)) {
    throw new Error('Path traversal denied')
  }
  return resolved
}

function createWindow() {
  allowWindowClose = false
  const isWin = process.platform === 'win32'
  const icon = loadAppIcon()
  mainWindow = new BrowserWindow({
    width: 1600,
    height: 1000,
    minWidth: 1100,
    minHeight: 700,
    frame: true,
    ...(isWin ? { titleBarStyle: 'hidden' } : { frame: false }),
    show: true,
    backgroundColor: '#000000',
    backgroundThrottling: false,
    title: APP_NAME,
    ...(icon ? { icon } : {}),
    webPreferences: {
      preload: path.join(__dirname, 'preload.js'),
      contextIsolation: true,
      nodeIntegration: false,
    },
  });

  if (icon) {
    try { mainWindow.setIcon(icon); } catch {}
  }

  if (VITE_DEV_SERVER_URL) {
    mainWindow.loadURL(VITE_DEV_SERVER_URL);
  } else {
    mainWindow.loadFile(path.join(__dirname, '../dist/index.html'));
  }

  mainWindow.once('ready-to-show', () => {
    if (!mainWindow.isVisible()) mainWindow.show();
    try { mainWindow.setTitle(APP_NAME); } catch {}
  });

  mainWindow.on('page-title-updated', (e) => {
    e.preventDefault();
    try { mainWindow.setTitle(APP_NAME); } catch {}
  });

  // Ask the renderer before closing so unsaved editor changes are not lost.
  // If the renderer does not answer (hung or crashed), close anyway.
  mainWindow.on('close', (e) => {
    if (allowWindowClose) return;
    e.preventDefault();
    if (closeRequestTimer) return;
    try {
      mainWindow.webContents.send('app:close-requested');
    } catch {
      allowWindowClose = true;
      mainWindow.close();
      return;
    }
    closeRequestTimer = setTimeout(() => {
      closeRequestTimer = null;
      if (mainWindow?.webContents.isCrashed?.()) {
        allowWindowClose = true;
        mainWindow.close();
      }
    }, 3000);
  });

  mainWindow.on('maximize', () => mainWindow.webContents.send('window:maximized-change', true));
  mainWindow.on('unmaximize', () => mainWindow.webContents.send('window:maximized-change', false));
}

function buildAppMenu() {
  const template = [
    {
      label: 'File',
      submenu: [
        {
          label: 'New Project',
          accelerator: 'CmdOrCtrl+N',
          click: () => mainWindow?.webContents.send('menu:new-project'),
        },
        {
          label: 'Open Project',
          accelerator: 'CmdOrCtrl+O',
          click: () => mainWindow?.webContents.send('menu:open-project'),
        },
        { type: 'separator' },
        {
          label: 'Save',
          accelerator: 'CmdOrCtrl+S',
          click: () => mainWindow?.webContents.send('menu:save'),
        },
        {
          label: 'Save All',
          accelerator: 'CmdOrCtrl+Shift+S',
          click: () => mainWindow?.webContents.send('menu:save-all'),
        },
        { type: 'separator' },
        {
          label: 'Settings',
          accelerator: 'CmdOrCtrl+,',
          click: () => mainWindow?.webContents.send('menu:settings'),
        },
        { type: 'separator' },
        { role: 'quit' },
      ],
    },
    {
      label: 'Edit',
      submenu: [
        { role: 'undo' },
        { role: 'redo' },
        { type: 'separator' },
        { role: 'cut' },
        { role: 'copy' },
        { role: 'paste' },
        { role: 'selectAll' },
        { type: 'separator' },
        {
          label: 'Find',
          accelerator: 'CmdOrCtrl+F',
          click: () => mainWindow?.webContents.send('menu:find'),
        },
      ],
    },
    {
      label: 'View',
      submenu: [
        {
          label: 'Toggle Explorer',
          accelerator: 'CmdOrCtrl+Shift+E',
          click: () => mainWindow?.webContents.send('menu:toggle-explorer'),
        },
        {
          label: 'Toggle AI Agents',
          accelerator: 'CmdOrCtrl+Shift+A',
          click: () => mainWindow?.webContents.send('menu:toggle-agents'),
        },
        { type: 'separator' },
        {
          label: 'Build / Compile',
          accelerator: 'CmdOrCtrl+B',
          click: () => mainWindow?.webContents.send('menu:build'),
        },
        {
          label: 'Debug',
          click: () => mainWindow?.webContents.send('menu:debug'),
        },
        {
          label: 'Flash',
          accelerator: 'CmdOrCtrl+Shift+B',
          click: () => mainWindow?.webContents.send('menu:flash'),
        },
        { type: 'separator' },
        { role: 'toggleDevTools' },
        { role: 'reload' },
      ],
    },
    {
      label: 'Run',
      submenu: [
        {
          label: 'Build / Compile',
          accelerator: 'F7',
          click: () => mainWindow?.webContents.send('menu:build'),
        },
        {
          label: 'Start Debugging',
          accelerator: 'F5',
          click: () => mainWindow?.webContents.send('menu:debug'),
        },
        {
          label: 'Flash to Device',
          accelerator: 'CmdOrCtrl+Shift+B',
          click: () => mainWindow?.webContents.send('menu:flash'),
        },
      ],
    },
    {
      label: 'Help',
      submenu: [
        {
          label: 'About EmbedIDE',
          click: () => {
            dialog.showMessageBox(mainWindow, {
              type: 'info',
              title: 'About EmbedIDE',
              message: `EmbedIDE v${app.getVersion()}`,
              detail: 'An embedded development IDE for Rust, C, C++, Assembly, and Zig.',
              ...(loadAppIcon() ? { icon: loadAppIcon() } : {}),
            });
          },
        },
      ],
    },
  ];
  const menu = Menu.buildFromTemplate(template);
  Menu.setApplicationMenu(menu);
}

app.whenReady().then(() => {
  // Dock / panel identity (esp. macOS + some Linux DEs)
  try {
    const icon = loadAppIcon();
    if (icon && process.platform === 'darwin' && app.dock) {
      app.dock.setIcon(icon);
      app.dock.setBadge('');
    }
  } catch {}
  buildAppMenu()
  createWindow()
  // Toolchain is installed by the OS package postinst / setup installer — not on first IDE launch.
});

app.on('window-all-closed', () => {
  if (process.platform !== 'darwin') app.quit();
});

// Never leave OpenOCD, GDB, a build or the serial helper running after exit
app.on('before-quit', () => {
  try { debugManager.killAll(); } catch {}
  try { cancelBuild(); } catch {}
  try { serial.shutdown(); } catch {}
});

app.on('activate', () => {
  if (BrowserWindow.getAllWindows().length === 0) createWindow();
});

// Window controls
ipcMain.on('window:minimize', () => mainWindow?.minimize());
ipcMain.on('window:maximize', () => {
  if (mainWindow?.isMaximized()) mainWindow.unmaximize();
  else mainWindow?.maximize();
});
ipcMain.on('window:close', () => mainWindow?.close());
// Renderer answer to app:close-requested: true = close now, false = stay open
ipcMain.on('app:close-response', (_e, ok) => {
  if (closeRequestTimer) { clearTimeout(closeRequestTimer); closeRequestTimer = null; }
  if (ok === true && mainWindow) {
    allowWindowClose = true;
    mainWindow.close();
  }
});
ipcMain.handle('window:isMaximized', () => mainWindow?.isMaximized());

// Toolchain detection + installer
ipcMain.handle('toolchain:detect', (_e, opts) => detectToolchains(opts || {}));
ipcMain.handle('toolchain:needs-install', () => needsToolchainInstall());
ipcMain.handle('toolchain:install-status', () => ({
  running: isInstallRunning(),
  progress: getInstallProgress(),
  status: getBundledStatus(),
}));
ipcMain.handle('toolchain:install', async (_e, opts = {}) => {
  const send = (p) => {
    try { mainWindow?.webContents.send('toolchain:install-progress', p) } catch {}
  }
  try {
    const result = await installToolchain({
      includeRust: opts?.includeRust !== false,
      force: !!opts?.force,
      onProgress: send,
    })
    clearDetectCache()
    mainWindow?.webContents.send('toolchain:install-complete', { ok: true, ...result })
    return { ok: true, ...result }
  } catch (err) {
    const error = err.message || String(err)
    mainWindow?.webContents.send('toolchain:install-complete', { ok: false, error })
    return { ok: false, error }
  }
});

// Project management
function defaultProjectsDir() {
  return path.join(app.getPath('documents'), 'EmbedIDE-Projects');
}

ipcMain.handle('project:create', (_e, rootDir, name, type, boardId) => {
  // New projects go to the standard projects folder only
  if (typeof rootDir !== 'string' || path.resolve(rootDir) !== path.resolve(defaultProjectsDir())) {
    throw new Error('Projects are created in the EmbedIDE projects folder');
  }
  fs.mkdirSync(defaultProjectsDir(), { recursive: true });
  const dir = createProject(rootDir, name, type, boardId || DEFAULT_BOARD_ID);
  trustProjectRoot(dir);
  return dir;
});

ipcMain.handle('boards:list', () => listBoards());
ipcMain.handle('boards:get', (_e, boardId) => getBoard(boardId));

ipcMain.handle('project:list-files', (_e, projectDir) => {
  return listProjectFiles(requireProjectRoot(projectDir));
});

ipcMain.handle('project:read-file', (_e, projectDir, filePath) => {
  return readProjectFile(assertInsideProject(projectDir, filePath));
});

ipcMain.handle('project:write-file', (_e, projectDir, filePath, content) => {
  writeProjectFile(assertInsideProject(projectDir, filePath), content);
  return true;
});

ipcMain.handle('project:create-file', (_e, dir, name) => {
  if (typeof name !== 'string' || name.includes('..') || name.includes('~') || path.isAbsolute(name)) {
    throw new Error('Invalid name')
  }
  assertInsideProject(dir, name.replace(/\/$/, '') || '.')
  return createProjectFile(dir, name);
});

ipcMain.handle('project:delete-file', (_e, projectDir, filePath) => {
  return deleteProjectFile(assertInsideProject(projectDir, filePath));
});

ipcMain.handle('project:rename-file', (_e, projectDir, oldPath, newPath) => {
  return renameProjectFile(
    assertInsideProject(projectDir, oldPath),
    assertInsideProject(projectDir, newPath),
  );
});

ipcMain.handle('project:search-files', (_e, dir, query) => {
  assertInsideProject(dir, '.')
  return searchInFiles(dir, query);
});

ipcMain.handle('project:open', async () => {
  const result = await dialog.showOpenDialog(mainWindow, {
    properties: ['openDirectory'],
    title: 'Open Embedded Project',
  });
  if (result.canceled || !result.filePaths[0]) return null;

  const dir = trustProjectRoot(result.filePaths[0]);
  const meta = readProjectMeta(dir);
  const name = meta?.name || path.basename(dir);
  let type = meta?.type || 'c';
  const boardId = meta?.boardId || DEFAULT_BOARD_ID;

  if (!meta?.type) {
    const files = fs.readdirSync(dir);
    if (files.includes('Cargo.toml')) type = 'rust';
    else if (files.some(f => f.endsWith('.zig'))) type = 'zig';
    else if (files.some(f => f.endsWith('.cpp'))) type = 'cpp';
    else if (files.some(f => f.endsWith('.S') || f.endsWith('.s'))) type = 'asm';
  }

  const board = getBoardOrDefault(boardId);
  return {
    dir,
    name,
    type,
    boardId: board.id,
    boardName: board.name,
    flashKb: board.flashKb,
    ramKb: board.ramKb,
    peripherals: board.peripherals || [],
  };
});

ipcMain.handle('project:get-templates', () => getTemplateList());

ipcMain.handle('project:get-meta', (_e, dir) => {
  const meta = readProjectMeta(requireProjectRoot(dir));
  if (!meta) return null;
  const board = getBoardOrDefault(meta.boardId);
  return {
    ...meta,
    boardName: board.name,
    flashKb: board.flashKb,
    ramKb: board.ramKb,
    peripherals: board.peripherals || [],
    openocdTarget: board.openocdTarget,
    defaultAdapter: board.defaultAdapter,
  };
});

// Build
ipcMain.handle('project:build', async (_e, projectDir, projectType) => {
  const output = [];
  const onOutput = (data) => {
    output.push(data);
    mainWindow?.webContents.send('build:output', data);
  };

  try {
    const result = await buildProject(requireProjectRoot(projectDir), projectType, onOutput);
    if (result.cancelled) {
      mainWindow?.webContents.send('build:complete', { code: null, cancelled: true });
      return { success: false, cancelled: true, output };
    }
    mainWindow?.webContents.send('build:complete', { code: result.code });
    return { success: true, output };
  } catch (err) {
    const codeMatch = /code (-?\d+)/.exec(err.message || '')
    const code = codeMatch ? Number(codeMatch[1]) : -1
    mainWindow?.webContents.send('build:complete', { code, error: err.message });
    return { success: false, error: err.message, output };
  }
});

ipcMain.handle('project:cancel-build', () => {
  cancelBuild();
  return true;
});

// Flash
ipcMain.handle('project:flash', async (_e, projectDir, projectType, config) => {
  const output = [];
  const onOutput = (data) => {
    output.push(data);
    mainWindow?.webContents.send('flash:output', data);
  };

  try {
    const result = await flashBoard(requireProjectRoot(projectDir), projectType, { boardId: config?.boardId }, onOutput);
    mainWindow?.webContents.send('flash:complete', { code: result.code });
    return { success: true, output };
  } catch (err) {
    const codeMatch = /code (-?\d+)/.exec(err.message || '')
    const code = codeMatch ? Number(codeMatch[1]) : -1
    mainWindow?.webContents.send('flash:complete', { code, error: err.message });
    return { success: false, error: err.message, output };
  }
});

ipcMain.handle('project:run-script', async (_e, projectDir, projectType, filePath) => {
  const output = [];
  const onOutput = (data) => {
    output.push(data);
    mainWindow?.webContents.send('run:output', data);
  };
  try {
    const root = requireProjectRoot(projectDir);
    const entry = filePath ? assertInsideProject(root, filePath) : null;
    const result = await runScript(root, projectType, entry, onOutput);
    mainWindow?.webContents.send('run:complete', { code: result.code });
    return { success: true, output };
  } catch (err) {
    const codeMatch = /code (-?\d+)/.exec(err.message || '')
    const code = codeMatch ? Number(codeMatch[1]) : -1
    mainWindow?.webContents.send('run:complete', { code, error: err.message });
    return { success: false, error: err.message, output };
  }
});

ipcMain.handle('project:debug', async (_e, projectDir, projectType, config) => {
  const output = [];
  const onOutput = (data) => {
    output.push(data);
    mainWindow?.webContents.send('run:output', data);
  };
  try {
    const root = requireProjectRoot(projectDir);
    const entry = config?.filePath ? assertInsideProject(root, config.filePath) : null;
    const result = await startDebugSession(root, projectType, { filePath: entry }, onOutput);
    mainWindow?.webContents.send('run:complete', { code: result.code ?? 0 });
    return { success: true, output };
  } catch (err) {
    const codeMatch = /code (-?\d+)/.exec(err.message || '')
    const code = codeMatch ? Number(codeMatch[1]) : -1
    mainWindow?.webContents.send('run:complete', { code, error: err.message });
    return { success: false, error: err.message, output };
  }
});

ipcMain.handle('project:cancel-debug', () => {
  cancelDebugSession();
  return true;
});

// Interactive firmware debugging (OpenOCD / GDB server + GDB/MI)
const debugManager = new DebugManager({
  resolveTool,
  toolEnv: () => getToolchainEnv(),
  openocdScripts: () => findOpenocdScripts(getBundledRoot()),
});
debugManager.on('event', (evt) => {
  try { mainWindow?.webContents.send('debug:event', evt) } catch {}
});

/** IPC results carry structured errors; thrown errors would lose `code`. */
async function debugCall(fn) {
  try {
    return { ok: true, result: await fn() };
  } catch (err) {
    return { ok: false, error: toError(err) };
  }
}

ipcMain.handle('debug:status', () => debugManager.status());

ipcMain.handle('debug:start', (_e, projectDir, projectType, opts = {}) => debugCall(async () => {
  const root = requireProjectRoot(projectDir);
  if (isScriptProject(String(projectType || ''))) {
    throw Object.assign(new Error('Host scripts run with Build/Run; the firmware debugger needs a firmware project.'), { code: 'unsupported' });
  }
  const board = resolveBoard(root, { boardId: typeof opts.boardId === 'string' ? opts.boardId : undefined });
  const elf = detectElf(root, String(projectType || ''));
  prependBundledToolchainToPath();
  // Only these fields reach the debug manager; tool paths are never taken from the renderer
  return debugManager.start({
    projectDir: root,
    elf,
    mode: opts.mode === 'remote' ? 'remote' : 'openocd',
    adapter: typeof opts.adapter === 'string' && opts.adapter ? opts.adapter : board.defaultAdapter,
    target: typeof opts.target === 'string' && opts.target ? opts.target : board.openocdTarget,
    gdbPort: opts.gdbPort,
    remote: typeof opts.remote === 'string' ? opts.remote : undefined,
    loadFirmware: opts.loadFirmware === true,
  });
}));

ipcMain.handle('debug:op', (_e, name, args) => debugCall(() =>
  debugManager.op(String(name || ''), args && typeof args === 'object' ? args : {})));

ipcMain.handle('debug:toggle-breakpoint', (_e, projectDir, file, line) => debugCall(() =>
  debugManager.toggleBreakpoint(requireProjectRoot(projectDir), file, line)));

ipcMain.handle('debug:clear-breakpoints', () => debugCall(() => debugManager.clearBreakpoints()));

ipcMain.handle('debug:stop', () => debugCall(() => debugManager.stop()));

// Serial (Rust serial-bridge: binary-safe, no Python)
const serial = new SerialService();
serial.on('data', (buf) => {
  try { mainWindow?.webContents.send('serial:data', new Uint8Array(buf)) } catch {}
});
serial.on('status', (st) => {
  try { mainWindow?.webContents.send('serial:status', st) } catch {}
});
serial.on('error-message', (msg) => {
  try { mainWindow?.webContents.send('serial:error', msg) } catch {}
});

ipcMain.handle('serial:list-ports', async () => {
  try {
    return { ok: true, ports: await serial.list() };
  } catch (err) {
    return { ok: false, ports: [], error: err.message || String(err) };
  }
});

ipcMain.handle('serial:connect', async (_e, config) => {
  try {
    if (config && typeof config === 'object' && typeof config.autoReconnect === 'boolean') {
      serial.autoReconnect = config.autoReconnect;
    }
    const r = await serial.open(config);
    return { connected: true, ...r };
  } catch (err) {
    return { connected: false, error: err.message || String(err) };
  }
});

ipcMain.handle('serial:send', (_e, data) => {
  try {
    // Renderer sends bytes (Uint8Array) — text encoding/line endings are its choice
    const bytes = data instanceof Uint8Array ? Buffer.from(data) : Buffer.from(String(data ?? ''), 'utf8');
    serial.write(bytes);
    return { ok: true };
  } catch (err) {
    return { ok: false, error: err.message || String(err) };
  }
});

ipcMain.handle('serial:disconnect', async () => {
  await serial.close();
  return true;
});

// App info
ipcMain.handle('app:get-default-projects-dir', () => defaultProjectsDir());

// Settings persistence
ipcMain.handle('shell:open-external', async (_e, url) => {
  if (typeof url !== 'string' || !/^https?:\/\//i.test(url)) {
    throw new Error('Only http(s) URLs are allowed')
  }
  await shell.openExternal(url)
  return true
})

function secretCodec() {
  let available = false;
  let backend = null;
  try { available = safeStorage.isEncryptionAvailable(); } catch {}
  try { backend = safeStorage.getSelectedStorageBackend?.() || null; } catch {}
  return {
    available,
    backend,
    encrypt: (s) => safeStorage.encryptString(s).toString('base64'),
    decrypt: (b64) => safeStorage.decryptString(Buffer.from(b64, 'base64')),
  };
}

ipcMain.handle('settings:load', () => {
  try {
    let raw = null;
    if (fs.existsSync(SETTINGS_PATH)) {
      raw = JSON.parse(fs.readFileSync(SETTINGS_PATH, 'utf8'));
    } else {
      // Migrate settings if an older build used Electron's default EmbedIDE userData dir
      const legacy = path.join(app.getPath('appData'), 'EmbedIDE', 'settings.json');
      if (fs.existsSync(legacy)) raw = JSON.parse(fs.readFileSync(legacy, 'utf8'));
    }
    if (!raw) return {};
    const { settings, needsRewrite } = decodeSettingsFromDisk(raw, secretCodec());
    // Older builds stored the API key in plain text: re-save it protected
    if (needsRewrite) {
      try { writeFileAtomic(SETTINGS_PATH, JSON.stringify(encodeSettingsForDisk(settings, secretCodec()))); } catch {}
    }
    return settings;
  } catch {}
  return {};
});

ipcMain.handle('settings:save', async (_e, settings) => {
  try {
    if (!settings || typeof settings !== 'object' || Array.isArray(settings)) return false;
    fs.mkdirSync(path.dirname(SETTINGS_PATH), { recursive: true });
    writeFileAtomic(SETTINGS_PATH, JSON.stringify(encodeSettingsForDisk(settings, secretCodec())));
    return true;
  } catch { return false; }
});

ipcMain.handle('settings:secret-storage', () => {
  const c = secretCodec();
  // basic_text means Chromium's fixed-key obfuscation (no keyring on Linux)
  return { encrypted: c.available && c.backend !== 'basic_text', backend: c.backend };
});

ipcMain.handle('ai:cli-status', (_e, providerId) => {
  return getCliStatus(String(providerId || ''))
})

ipcMain.handle('ai:cli-chat', async (_e, providerId, payload) => {
  const messages = Array.isArray(payload?.messages) ? payload.messages : []
  // A model id that starts with "-" would be parsed by the CLI as a flag
  const model = typeof payload?.model === 'string' && /^[A-Za-z0-9][A-Za-z0-9._:/-]{0,99}$/.test(payload.model)
    ? payload.model : undefined
  // Run the CLI only inside an open project (or the temp dir)
  const cwd = typeof payload?.cwd === 'string' && openProjectRoots.has(path.resolve(payload.cwd))
    ? path.resolve(payload.cwd) : undefined
  try {
    const text = await chatViaCli(String(providerId || ''), { messages, model, cwd })
    return { ok: true, text }
  } catch (err) {
    return { ok: false, error: err?.message || String(err) }
  }
})

ipcMain.handle('ai:cli-cancel', () => cancelCliChat())
