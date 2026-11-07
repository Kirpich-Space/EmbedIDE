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
  kind?: string
  vid?: number | null
  pid?: number | null
  serialNumber?: string | null
}

interface SerialConnectConfig {
  port: string
  baud: number
  dataBits?: 5 | 6 | 7 | 8
  parity?: 'none' | 'odd' | 'even'
  stopBits?: 1 | 2
  flowControl?: 'none' | 'software' | 'hardware'
  autoReconnect?: boolean
}

interface SerialStatus {
  state: 'open' | 'closed' | 'lost' | 'reconnecting'
  port?: string
  baud?: number
  message?: string
  reconnected?: boolean
}

interface DebugErrorInfo { code: string; message: string; detail?: string }
type DebugResult<T> = { ok: true; result: T } | { ok: false; error: DebugErrorInfo }

interface DebugFrame {
  level: number
  addr: string | null
  func: string | null
  file: string | null
  fullname: string | null
  line: number | null
}

interface DebugBreakpoint { file: string; line: number; number: number | null; verified: boolean }

interface DebugStatusInfo {
  active: boolean
  starting: boolean
  state: 'idle' | 'starting' | 'running' | 'stopped'
  mode: 'openocd' | 'remote' | null
  elf: string | null
  frame: DebugFrame | null
  breakpoints: DebugBreakpoint[]
}

type DebugEvent =
  | { type: 'state'; state: 'idle' | 'running' | 'stopped' }
  | { type: 'stopped'; reason: string; signal?: string | null; bkptno?: number | null; frame: DebugFrame | null }
  | { type: 'output'; stream: string; text: string }
  | { type: 'breakpoints'; breakpoints: DebugBreakpoint[] }
  | { type: 'ended'; reason: string }

interface DebugStartOptions {
  boardId?: string
  mode?: 'openocd' | 'remote'
  adapter?: string
  target?: string
  gdbPort?: number
  remote?: string
  loadFirmware?: boolean
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

  detectToolchains: (opts?: { force?: boolean }) => Promise<ToolchainInfo>
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

  listSerialPorts: () => Promise<{ ok: boolean; ports: SerialPort[]; error?: string }>
  connectSerial: (config: SerialConnectConfig) => Promise<{connected: boolean, port?: string, baud?: number, error?: string}>
  sendSerial: (data: Uint8Array) => Promise<{ ok: boolean; error?: string }>
  disconnectSerial: () => Promise<boolean>
  onSerialData: (cb: (data: Uint8Array) => void) => () => void
  onSerialError: (cb: (data: string) => void) => () => void
  onSerialStatus: (cb: (status: SerialStatus) => void) => () => void

  debugStatus: () => Promise<DebugStatusInfo>
  debugStart: (dir: string, type: string, opts: DebugStartOptions) => Promise<DebugResult<DebugStatusInfo>>
  debugOp: (name: 'continue' | 'pause' | 'stepOver' | 'stepInto' | 'stepOut' | 'stepInstruction' | 'reset' | 'stack' | 'locals' | 'registers' | 'memory' | 'evaluate', args?: Record<string, unknown>) => Promise<DebugResult<any>>
  debugToggleBreakpoint: (dir: string, file: string, line: number) => Promise<DebugResult<DebugBreakpoint[]>>
  debugClearBreakpoints: () => Promise<DebugResult<DebugBreakpoint[]>>
  debugStop: () => Promise<DebugResult<{ ok: true }>>
  onDebugEvent: (cb: (evt: DebugEvent) => void) => () => void

  onCloseRequested: (cb: () => void) => () => void
  respondClose: (ok: boolean) => void

  getDefaultProjectsDir: () => Promise<string>
  loadSettings: () => Promise<Record<string, unknown>>
  saveSettings: (settings: Record<string, unknown>) => Promise<boolean>
  secretStorageStatus: () => Promise<{ encrypted: boolean; backend: string | null }>
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
