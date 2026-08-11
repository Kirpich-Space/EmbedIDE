import type { EditorSettings } from './types'

export const DEFAULT_EDITOR_SETTINGS: EditorSettings = {
  fontSize: 15,
  tabSize: 4,
  fontFamily: "'JetBrains Mono', monospace",
  wordWrap: false,
  minimap: false,
  lineNumbers: true,
  cursorBlinkRate: 1200,
  smoothScroll: true,
  bracketMatch: true,
  language: 'en',
  theme: 'dark',
  aiEnabled: false,
  aiProvider: 'ollama',
  aiMode: 'local',
  aiEndpoint: 'http://127.0.0.1:11434',
  aiModel: 'llama3.2',
  aiKey: '',
  aiAuthMode: 'api',

  lineHeight: 1.45,
  insertSpaces: true,
  highlightActiveLine: true,
  foldGutter: true,
  autoComplete: true,
  showWhitespace: false,
  caretWidth: 2,

  customAccent: '',
  uiScale: 100,
  compactUi: false,
  reduceMotion: false,
  showStatusBar: true,
  glassEffects: true,

  autoSave: false,
  autoSaveDelayMs: 1500,
  confirmDelete: true,
  defaultBaud: 115200,
  aiAutoApplyFiles: false,
}

/** Merge persisted settings with defaults (forward-compatible). */
export function normalizeEditorSettings(raw: Partial<EditorSettings> | null | undefined): EditorSettings {
  const merged: EditorSettings = { ...DEFAULT_EDITOR_SETTINGS, ...(raw || {}) }
  if (!merged.aiProvider) {
    merged.aiProvider = merged.aiMode === 'cloud' ? 'openai' : 'ollama'
  }
  if (typeof merged.fontFamily === 'string' && merged.fontFamily.includes('IBM Plex Mono')) {
    merged.fontFamily = DEFAULT_EDITOR_SETTINGS.fontFamily
  }
  merged.fontSize = clamp(merged.fontSize, 10, 28)
  merged.tabSize = clamp(merged.tabSize, 1, 8)
  merged.lineHeight = clamp(merged.lineHeight, 1.1, 2.2)
  merged.caretWidth = clamp(merged.caretWidth, 1, 4)
  merged.uiScale = clamp(merged.uiScale, 85, 125)
  merged.autoSaveDelayMs = clamp(merged.autoSaveDelayMs, 500, 10000)
  merged.defaultBaud = clamp(merged.defaultBaud, 300, 3000000)
  if (merged.customAccent && !/^#[0-9A-Fa-f]{6}$/.test(merged.customAccent)) {
    merged.customAccent = ''
  }
  if (merged.aiAuthMode !== 'subscription') merged.aiAuthMode = 'api'
  return merged
}

function clamp(n: number, min: number, max: number) {
  if (typeof n !== 'number' || Number.isNaN(n)) return min
  return Math.min(max, Math.max(min, n))
}

export const ACCENT_PRESETS: { id: string; label: string; value: string }[] = [
  { id: 'theme', label: 'Theme', value: '' },
  { id: 'orange', label: 'Orange', value: '#FF6B00' },
  { id: 'cyan', label: 'Cyan', value: '#00C2FF' },
  { id: 'green', label: 'Green', value: '#3DDC97' },
  { id: 'violet', label: 'Violet', value: '#A78BFA' },
  { id: 'rose', label: 'Rose', value: '#FB7185' },
  { id: 'amber', label: 'Amber', value: '#F59E0B' },
  { id: 'blue', label: 'Blue', value: '#3B82F6' },
]

/** Apply accent override on top of the current theme CSS variables. */
export function applyAccentOverride(hex: string) {
  const root = document.documentElement
  if (!hex) return
  const hover = lighten(hex, 0.14)
  const glow = hexToRgba(hex, 0.4)
  root.style.setProperty('--accent', hex)
  root.style.setProperty('--accent-hover', hover)
  root.style.setProperty('--accent-glow', glow)
  root.style.setProperty('--button-bg', hex)
}

export function applyUiChrome(settings: Pick<EditorSettings, 'uiScale' | 'compactUi' | 'reduceMotion' | 'glassEffects'>) {
  const root = document.documentElement
  root.style.setProperty('--ui-scale', `${settings.uiScale / 100}`)
  root.classList.toggle('ui-compact', !!settings.compactUi)
  root.classList.toggle('ui-reduce-motion', !!settings.reduceMotion)
  root.classList.toggle('ui-no-glass', !settings.glassEffects)
}

function hexToRgba(hex: string, alpha: number) {
  const h = hex.replace('#', '')
  const r = parseInt(h.slice(0, 2), 16)
  const g = parseInt(h.slice(2, 4), 16)
  const b = parseInt(h.slice(4, 6), 16)
  return `rgba(${r}, ${g}, ${b}, ${alpha})`
}

function lighten(hex: string, amount: number) {
  const h = hex.replace('#', '')
  const ch = (i: number) => {
    const v = parseInt(h.slice(i, i + 2), 16)
    return Math.min(255, Math.round(v + (255 - v) * amount))
  }
  const to = (n: number) => n.toString(16).padStart(2, '0')
  return `#${to(ch(0))}${to(ch(2))}${to(ch(4))}`
}
