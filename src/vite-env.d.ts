interface ToolchainInfo {
  rust: boolean
  rustVersion?: string
  rustEmbeddedTargets?: string[]
  armGcc: boolean
  armGccVersion?: string
  armGccBundled?: boolean
  armGdb?: boolean
  armGdbVersion?: string
  armGdbBundled?: boolean
  openocd: boolean
  openocdVersion?: string
  openocdBundled?: boolean
  make: boolean
  makeVersion?: string
  makeBundled?: boolean
  python: boolean
  pythonVersion?: string
  pythonBundled?: boolean
  zig?: boolean
  zigVersion?: string
  zigBundled?: boolean
  bundled?: {
    root: string | null
    platform: string
    bundled: boolean
    tools: Record<string, boolean>
  }
}

interface SerialPort {
  device: string
  description: string
}

interface ProjectTemplate {
  id: string
  name: string
  ext: string
  category?: 'firmware' | 'driver' | 'os' | 'systems' | 'script'
  needsBoard?: boolean
  lang?: string
}

interface BoardInfo {
  id: string
  name: string
  family: string
  mcu: string
  flashKb: number
  ramKb: number
  cpu: string
}

interface BoardDetail extends BoardInfo {
  fpu?: string
  floatAbi?: string
  flashOrigin?: string
  ramOrigin?: string
  cDefine?: string
  openocdTarget?: string
  defaultAdapter?: string
  rustTarget?: string
  peripherals?: string[]
}

interface OpenedProject {
  dir: string
  name: string
  type: string
  boardId?: string
  boardName?: string
  flashKb?: number
  ramKb?: number
  peripherals?: string[]
}

interface ProjectMeta {
  name: string
  type: string
  boardId: string
  boardName?: string
  flashKb?: number
  ramKb?: number
  peripherals?: string[]
  openocdTarget?: string
  defaultAdapter?: string
  version?: number
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
  needsToolchainInstall: () => Promise<boolean>
  toolchainInstallStatus: () => Promise<{
    running: boolean
    progress: null | {
      phase?: string
      package?: string
      percent?: number
      message?: string
      received?: number
      total?: number
      root?: string
      auto?: boolean
    }
    status: unknown
  }>
  installToolchain: (opts?: { includeRust?: boolean; force?: boolean }) => Promise<{
    ok: boolean
    root?: string
    skipped?: boolean
    error?: string
  }>
  onToolchainInstallProgress: (cb: (data: {
    phase?: string
    package?: string
    percent?: number
    message?: string
    received?: number
    total?: number
    root?: string
    auto?: boolean
  }) => void) => () => void
  onToolchainInstallComplete: (cb: (data: {
    ok: boolean
    root?: string
    skipped?: boolean
    error?: string
  }) => void) => () => void

  createProject: (rootDir: string, name: string, type: string, boardId?: string) => Promise<string>
  openProject: () => Promise<OpenedProject | null>
  listProjectFiles: (dir: string) => Promise<{id: string, name: string, type: 'file' | 'directory', language?: string}[]>
  readProjectFile: (projectDir: string, path: string) => Promise<string>
  writeProjectFile: (projectDir: string, path: string, content: string) => Promise<boolean>
  getProjectTemplates: () => Promise<ProjectTemplate[]>
  getProjectMeta: (dir: string) => Promise<ProjectMeta | null>
  listBoards: () => Promise<BoardInfo[]>
  getBoard: (boardId: string) => Promise<BoardDetail | null>

  createProjectFile: (dir: string, name: string) => Promise<boolean>
  deleteProjectFile: (projectDir: string, path: string) => Promise<boolean>
  renameProjectFile: (projectDir: string, oldPath: string, newPath: string) => Promise<boolean>
  searchInFiles: (dir: string, query: string) => Promise<{file: string, line: number, text: string}[]>

  buildProject: (dir: string, type: string) => Promise<{success: boolean, output: BuildOutput[]}>
  cancelBuild: () => Promise<boolean>
  onBuildOutput: (cb: (data: BuildOutput) => void) => () => void
  onBuildComplete: (cb: (data: {code: number | null, error?: string, cancelled?: boolean}) => void) => () => void

  flashProject: (dir: string, type: string, config: { adapter?: string, target?: string, boardId?: string, elfPath?: string }) => Promise<{success: boolean, output: BuildOutput[]}>
  onFlashOutput: (cb: (data: BuildOutput) => void) => () => void
  onFlashComplete: (cb: (data: {code: number | null, error?: string}) => void) => () => void

  runScript: (dir: string, type: string, filePath?: string | null) => Promise<{success: boolean, output: BuildOutput[], error?: string}>
  startDebug: (dir: string, type: string, config?: { adapter?: string, target?: string, boardId?: string, elfPath?: string, filePath?: string }) => Promise<{success: boolean, output: BuildOutput[], error?: string}>
  cancelDebug: () => Promise<boolean>
  onRunOutput: (cb: (data: BuildOutput) => void) => () => void
  onRunComplete: (cb: (data: {code: number | null, error?: string}) => void) => () => void

  listSerialPorts: () => Promise<SerialPort[]>
  connectSerial: (port: string, baud: number) => Promise<{connected: boolean, error?: string}>
  sendSerial: (data: string) => Promise<boolean>
  disconnectSerial: () => Promise<boolean>
  onSerialData: (cb: (data: string) => void) => () => void
  onSerialError: (cb: (data: string) => void) => () => void

  getDefaultProjectsDir: () => Promise<string>
  loadSettings: () => Promise<Record<string, unknown>>
  saveSettings: (settings: Record<string, unknown>) => Promise<boolean>
  openExternal: (url: string) => Promise<boolean>

  aiCliStatus: (providerId: string) => Promise<{
    providerId: string
    supported: boolean
    found: boolean
    loggedIn: boolean
    bin: string | null
    path: string | null
    version: string | null
    loginHint: string
    installHint: string
  }>
  aiCliChat: (providerId: string, payload: {
    messages: { role: string; content: string }[]
    model?: string
    cwd?: string
  }) => Promise<{ ok: boolean; text?: string; error?: string }>
  aiCliCancel: () => Promise<boolean>

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
  onMenuDebug: (cb: () => void) => () => void
}

interface Window {
  electronAPI?: ElectronAPI
}
