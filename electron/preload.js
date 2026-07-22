const { contextBridge, ipcRenderer } = require('electron');

const listeners = {
  'build:output': new Set(),
  'build:complete': new Set(),
  'flash:output': new Set(),
  'flash:complete': new Set(),
  'serial:data': new Set(),
  'serial:error': new Set(),
  'window:maximized-change': new Set(),
  'menu:new-project': new Set(),
  'menu:open-project': new Set(),
  'menu:save': new Set(),
  'menu:save-all': new Set(),
  'menu:settings': new Set(),
  'menu:find': new Set(),
  'menu:toggle-explorer': new Set(),
  'menu:toggle-agents': new Set(),
  'menu:build': new Set(),
  'menu:flash': new Set(),
}

function on(channel, cb) {
  const handler = (_e, ...args) => cb(...args)
  listeners[channel]?.add(handler)
  ipcRenderer.on(channel, handler)
  return () => {
    listeners[channel]?.delete(handler)
    ipcRenderer.removeListener(channel, handler)
  }
}

contextBridge.exposeInMainWorld('electronAPI', {
  // Window
  minimize: () => ipcRenderer.send('window:minimize'),
  maximize: () => ipcRenderer.send('window:maximize'),
  close: () => ipcRenderer.send('window:close'),
  isMaximized: () => ipcRenderer.invoke('window:isMaximized'),
  onMaximizedChange: (cb) => on('window:maximized-change', cb),

  // Toolchain
  detectToolchains: () => ipcRenderer.invoke('toolchain:detect'),

  // Project
  createProject: (rootDir, name, type) => ipcRenderer.invoke('project:create', rootDir, name, type),
  openProject: () => ipcRenderer.invoke('project:open'),
  listProjectFiles: (dir) => ipcRenderer.invoke('project:list-files', dir),
  readProjectFile: (p) => ipcRenderer.invoke('project:read-file', p),
  writeProjectFile: (p, c) => ipcRenderer.invoke('project:write-file', p, c),
  getProjectTemplates: () => ipcRenderer.invoke('project:get-templates'),
  createProjectFile: (dir, name) => ipcRenderer.invoke('project:create-file', dir, name),
  deleteProjectFile: (p) => ipcRenderer.invoke('project:delete-file', p),
  renameProjectFile: (oldP, newP) => ipcRenderer.invoke('project:rename-file', oldP, newP),
  searchInFiles: (dir, query) => ipcRenderer.invoke('project:search-files', dir, query),

  // Build
  buildProject: (dir, type) => ipcRenderer.invoke('project:build', dir, type),
  cancelBuild: () => ipcRenderer.invoke('project:cancel-build'),
  onBuildOutput: (cb) => on('build:output', cb),
  onBuildComplete: (cb) => on('build:complete', cb),

  // Flash
  flashProject: (dir, type, config) => ipcRenderer.invoke('project:flash', dir, type, config),
  onFlashOutput: (cb) => on('flash:output', cb),
  onFlashComplete: (cb) => on('flash:complete', cb),

  // Serial
  listSerialPorts: () => ipcRenderer.invoke('serial:list-ports'),
  connectSerial: (port, baud) => ipcRenderer.invoke('serial:connect', port, baud),
  sendSerial: (data) => ipcRenderer.invoke('serial:send', data),
  disconnectSerial: () => ipcRenderer.invoke('serial:disconnect'),
  onSerialData: (cb) => on('serial:data', cb),
  onSerialError: (cb) => on('serial:error', cb),

  // Menu events
  onMenuNewProject: (cb) => on('menu:new-project', cb),
  onMenuOpenProject: (cb) => on('menu:open-project', cb),
  onMenuSave: (cb) => on('menu:save', cb),
  onMenuSaveAll: (cb) => on('menu:save-all', cb),
  onMenuSettings: (cb) => on('menu:settings', cb),
  onMenuFind: (cb) => on('menu:find', cb),
  onMenuToggleExplorer: (cb) => on('menu:toggle-explorer', cb),
  onMenuToggleAgents: (cb) => on('menu:toggle-agents', cb),
  onMenuBuild: (cb) => on('menu:build', cb),
  onMenuFlash: (cb) => on('menu:flash', cb),
});
