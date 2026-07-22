interface ToolchainInfo {
  rust: boolean
  rustVersion?: string
  rustEmbeddedTargets?: string[]
  armGcc: boolean
  armGccVersion?: string
  openocd: boolean
  openocdVersion?: string
  make: boolean
  python: boolean
}

interface SerialPort {
  device: string
  description: string
}

interface ProjectTemplate {
  id: string
  name: string
  ext: string
}

interface BuildOutput {
  type: 'stdout' | 'stderr'
  text: string
}

interface ElectronAPI {
  minimize: () => void
  maximize: () => void
  close: () => void
  isMaximized: () => Promise<boolean>
  onMaximizedChange: (callback: (isMaximized: boolean) => void) => () => void

  detectToolchains: () => Promise<ToolchainInfo>

  createProject: (rootDir: string, name: string, type: string) => Promise<string>
  openProject: () => Promise<{dir: string, name: string, type: string} | null>
  listProjectFiles: (dir: string) => Promise<{id: string, name: string, type: 'file' | 'directory', language?: string}[]>
  readProjectFile: (path: string) => Promise<string>
  writeProjectFile: (path: string, content: string) => Promise<boolean>
  getProjectTemplates: () => Promise<ProjectTemplate[]>

  createProjectFile: (dir: string, name: string) => Promise<boolean>
  deleteProjectFile: (path: string) => Promise<boolean>
  renameProjectFile: (oldPath: string, newPath: string) => Promise<boolean>
  searchInFiles: (dir: string, query: string) => Promise<{file: string, line: number, text: string}[]>

  buildProject: (dir: string, type: string) => Promise<{success: boolean, output: BuildOutput[]}>
  cancelBuild: () => Promise<boolean>
  onBuildOutput: (cb: (data: BuildOutput) => void) => () => void
  onBuildComplete: (cb: (data: {code: number, error?: string}) => void) => () => void

  flashProject: (dir: string, type: string, config: any) => Promise<{success: boolean, output: BuildOutput[]}>
  onFlashOutput: (cb: (data: BuildOutput) => void) => () => void
  onFlashComplete: (cb: (data: {code: number, error?: string}) => void) => () => void

  listSerialPorts: () => Promise<SerialPort[]>
  connectSerial: (port: string, baud: number) => Promise<{connected: boolean}>
  sendSerial: (data: string) => Promise<boolean>
  disconnectSerial: () => Promise<boolean>
  onSerialData: (cb: (data: string) => void) => () => void
  onSerialError: (cb: (data: string) => void) => () => void

  onMenuNewProject: (cb: () => void) => () => void
  onMenuOpenProject: (cb: () => void) => () => void
  onMenuSave: (cb: () => void) => () => void
  onMenuSaveAll: (cb: () => void) => () => void
  onMenuSettings: (cb: () => void) => () => void
  onMenuFind: (cb: () => void) => () => void
  onMenuToggleExplorer: (cb: () => void) => () => void
  onMenuToggleAgents: (cb: () => void) => () => void
  onMenuBuild: (cb: () => void) => () => void
  onMenuFlash: (cb: () => void) => () => void
}

interface Window {
  electronAPI?: ElectronAPI
}
