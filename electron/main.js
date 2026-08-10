const { app, BrowserWindow, ipcMain, dialog, Menu, shell } = require('electron');
const path = require('path');
const { detectToolchains, buildProject, flashBoard, cancelBuild } = require('./toolchain');
const { listSerialPorts, connectSerial, disconnectSerial } = require('./serial');
const { createProject, listProjectFiles, readProjectFile, writeProjectFile, createProjectFile, deleteProjectFile, renameProjectFile, searchInFiles, getTemplateList, readProjectMeta } = require('./project');
const { listBoards, getBoard, getBoardOrDefault, DEFAULT_BOARD_ID } = require('./boards');
const fs = require('fs');

app.disableHardwareAcceleration();

let mainWindow;
let serialConnection = null;
const VITE_DEV_SERVER_URL = process.env.VITE_DEV_SERVER_URL;
const SETTINGS_PATH = path.join(app.getPath('userData'), 'settings.json');

/** Resolve targetPath under projectDir; reject traversal / absolute escapes. */
function assertInsideProject(projectDir, targetPath) {
  if (!projectDir || typeof projectDir !== 'string') {
    throw new Error('Project directory required')
  }
  const root = path.resolve(projectDir)
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
  const isWin = process.platform === 'win32'
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
    title: 'EmbedIDE',
    icon: path.join(__dirname, '..', 'build', 'icons', '256.png'),
    webPreferences: {
      preload: path.join(__dirname, 'preload.js'),
      contextIsolation: true,
      nodeIntegration: false,
    },
  });

  if (VITE_DEV_SERVER_URL) {
    mainWindow.loadURL(VITE_DEV_SERVER_URL);
  } else {
    mainWindow.loadFile(path.join(__dirname, '../dist/index.html'));
  }

  mainWindow.once('ready-to-show', () => {
    if (!mainWindow.isVisible()) mainWindow.show();
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
          label: 'Build',
          accelerator: 'CmdOrCtrl+B',
          click: () => mainWindow?.webContents.send('menu:build'),
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
  buildAppMenu()
  createWindow()
});

app.on('window-all-closed', () => {
  if (process.platform !== 'darwin') app.quit();
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
ipcMain.handle('window:isMaximized', () => mainWindow?.isMaximized());

// Toolchain detection
ipcMain.handle('toolchain:detect', () => detectToolchains());

// Project management
ipcMain.handle('project:create', (_e, rootDir, name, type, boardId) => {
  return createProject(rootDir, name, type, boardId || DEFAULT_BOARD_ID);
});

ipcMain.handle('boards:list', () => listBoards());
ipcMain.handle('boards:get', (_e, boardId) => getBoard(boardId));

ipcMain.handle('project:list-files', (_e, projectDir) => {
  return listProjectFiles(projectDir);
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

  const dir = result.filePaths[0];
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
  const meta = readProjectMeta(dir);
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
    const result = await buildProject(projectDir, projectType, onOutput);
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
    const result = await flashBoard(projectDir, projectType, config, onOutput);
    mainWindow?.webContents.send('flash:complete', { code: result.code });
    return { success: true, output };
  } catch (err) {
    const codeMatch = /code (-?\d+)/.exec(err.message || '')
    const code = codeMatch ? Number(codeMatch[1]) : -1
    mainWindow?.webContents.send('flash:complete', { code, error: err.message });
    return { success: false, error: err.message, output };
  }
});

// Serial
ipcMain.handle('serial:list-ports', async () => {
  const ports = await listSerialPorts();
  return ports;
});

ipcMain.handle('serial:connect', async (_e, port, baud) => {
  disconnectSerial();
  try {
    serialConnection = await connectSerial(
      port,
      baud,
      (data) => mainWindow?.webContents.send('serial:data', data),
      (err) => mainWindow?.webContents.send('serial:error', err),
    );
    return { connected: true };
  } catch (err) {
    serialConnection = null;
    return { connected: false, error: err.message || String(err) };
  }
});

ipcMain.handle('serial:send', (_e, data) => {
  if (serialConnection) serialConnection.send(data);
  return true;
});

ipcMain.handle('serial:disconnect', () => {
  disconnectSerial();
  serialConnection = null;
  return true;
});

// App info
ipcMain.handle('app:get-default-projects-dir', () => {
  const docs = app.getPath('documents');
  return path.join(docs, 'EmbedIDE-Projects');
});

// Settings persistence
ipcMain.handle('shell:open-external', async (_e, url) => {
  if (typeof url !== 'string' || !/^https?:\/\//i.test(url)) {
    throw new Error('Only http(s) URLs are allowed')
  }
  await shell.openExternal(url)
  return true
})

ipcMain.handle('settings:load', () => {
  try {
    if (fs.existsSync(SETTINGS_PATH)) {
      return JSON.parse(fs.readFileSync(SETTINGS_PATH, 'utf8'));
    }
  } catch {}
  return {};
});

ipcMain.handle('settings:save', async (_e, settings) => {
  try {
    await fs.promises.mkdir(path.dirname(SETTINGS_PATH), { recursive: true });
    // Compact JSON — fewer bytes, faster writes on every settings change
    await fs.promises.writeFile(SETTINGS_PATH, JSON.stringify(settings), 'utf8');
    return true;
  } catch { return false; }
});
