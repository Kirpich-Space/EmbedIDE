const { app, BrowserWindow, ipcMain, dialog, Menu } = require('electron');
const path = require('path');
const { detectToolchains, buildProject, flashBoard, cancelBuild } = require('./toolchain');
const { listSerialPorts, connectSerial, disconnectSerial } = require('./serial');
const { createProject, listProjectFiles, readProjectFile, writeProjectFile, createProjectFile, deleteProjectFile, renameProjectFile, searchInFiles, TEMPLATES } = require('./project');
const fs = require('fs');

let mainWindow;
let serialConnection = null;
let contentCache = {};

const VITE_DEV_SERVER_URL = process.env.VITE_DEV_SERVER_URL;

function sanitizePath(userPath, projectDir) {
  const resolved = path.resolve(projectDir, userPath)
  if (!resolved.startsWith(path.resolve(projectDir))) {
    throw new Error('Path traversal denied')
  }
  return resolved
}

function createWindow() {
  mainWindow = new BrowserWindow({
    width: 1600,
    height: 1000,
    minWidth: 1100,
    minHeight: 700,
    frame: false,
    webPreferences: {
      preload: path.join(__dirname, 'preload.js'),
      contextIsolation: true,
      nodeIntegration: false,
    },
    backgroundColor: '#0A0A0A',
    show: false,
    title: 'EmbedIDE',
    icon: path.join(__dirname, '..', 'build', 'icons', '256.png'),
  });

  if (VITE_DEV_SERVER_URL) {
    mainWindow.loadURL(VITE_DEV_SERVER_URL);
  } else {
    mainWindow.loadFile(path.join(__dirname, '../dist/index.html'));
  }

  mainWindow.once('ready-to-show', () => mainWindow.show());

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
              message: 'EmbedIDE v0.1.0',
              detail: 'An embedded development IDE for Rust, C, C++, and Assembly.',
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
ipcMain.handle('project:create', (_e, rootDir, name, type) => {
  return createProject(rootDir, name, type);
});

ipcMain.handle('project:list-files', (_e, projectDir) => {
  return listProjectFiles(projectDir);
});

ipcMain.handle('project:read-file', (_e, filePath) => {
  return readProjectFile(filePath);
});

ipcMain.handle('project:write-file', (_e, filePath, content) => {
  writeProjectFile(filePath, content);
  return true;
});

ipcMain.handle('project:create-file', (_e, dir, name) => {
  const safeDir = sanitizePath('.', dir)
  return createProjectFile(safeDir, name);
});

ipcMain.handle('project:delete-file', (_e, filePath) => {
  return deleteProjectFile(filePath);
});

ipcMain.handle('project:rename-file', (_e, oldPath, newPath) => {
  return renameProjectFile(oldPath, newPath);
});

ipcMain.handle('project:search-files', (_e, dir, query) => {
  return searchInFiles(dir, query);
});

ipcMain.handle('project:open', async () => {
  const result = await dialog.showOpenDialog(mainWindow, {
    properties: ['openDirectory'],
    title: 'Open Embedded Project',
  });
  if (result.canceled || !result.filePaths[0]) return null;

  const dir = result.filePaths[0];
  const name = path.basename(dir);
  let type = 'c';

  const files = fs.readdirSync(dir);
  if (files.includes('Cargo.toml')) type = 'rust';
  else if (files.some(f => f.endsWith('.cpp'))) type = 'cpp';
  else if (files.some(f => f.endsWith('.S') || f.endsWith('.s'))) type = 'asm';

  return { dir, name, type };
});

ipcMain.handle('project:get-templates', () => {
  return Object.entries(TEMPLATES).map(([key, t]) => ({
    id: key,
    name: t.name,
    ext: t.ext,
  }));
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
    mainWindow?.webContents.send('build:complete', { code: result.code });
    return { success: true, output };
  } catch (err) {
    mainWindow?.webContents.send('build:complete', { code: -1, error: err.message });
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
    mainWindow?.webContents.send('flash:complete', { code: -1, error: err.message });
    return { success: false, error: err.message, output };
  }
});

// Serial
ipcMain.handle('serial:list-ports', async () => {
  const ports = await listSerialPorts();
  return ports;
});

ipcMain.handle('serial:connect', (_e, port, baud) => {
  disconnectSerial();
  serialConnection = connectSerial(
    port,
    baud,
    (data) => mainWindow?.webContents.send('serial:data', data),
    (err) => mainWindow?.webContents.send('serial:error', err),
  );
  return { connected: true };
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
