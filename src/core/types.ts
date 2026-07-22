export interface ThemeColors {
  bg: string
  bgPanel: string
  bgHover: string
  bgActive: string
  border: string
  text: string
  textSecondary: string
  accent: string
  accentHover: string
  accentGlow: string
  sidebarBg: string
  toolbarBg: string
  statusBarBg: string
  editorBg: string
  buttonBg: string
  buttonText: string
  scrollbarBg: string
  scrollbarThumb: string
  tabActiveBg: string
  tabInactiveBg: string
  outputBg: string
  hlKeyword: string
  hlType: string
  hlString: string
  hlNumber: string
  hlComment: string
}

export interface Theme {
  name: string
  type: 'dark' | 'light'
  colors: ThemeColors
}

export interface FileNode {
  id: string
  name: string
  type: 'file' | 'directory'
  children?: FileNode[]
  language?: string
}

export interface AgentMessage {
  role: 'user' | 'assistant'
  content: string
  timestamp: number
}

export interface EditorTabData {
  id: string
  name: string
  language: string
  content: string
  dirty: boolean
  cursorLine: number
  cursorCol: number
}

export interface BuildMessage {
  type: 'info' | 'warn' | 'error' | 'success'
  text: string
  timestamp: number
  source?: 'build' | 'flash' | 'serial'
}

export interface EditorSettings {
  fontSize: number
  tabSize: number
  fontFamily: string
  wordWrap: boolean
  minimap: boolean
  lineNumbers: boolean
  cursorBlinkRate: number
  smoothScroll: boolean
  bracketMatch: boolean
  language: string
  theme: string
}

export interface ProjectConfig {
  dir: string
  name: string
  type: string
}

export interface DropdownItem {
  label: string
  action: () => void
  shortcut?: string
  disabled?: boolean
  separator?: boolean
}

export interface MemoryUsage {
  flashUsed: number
  flashTotal: number
  ramUsed: number
  ramTotal: number
  stackUsed?: number
  heapUsed?: number
}
