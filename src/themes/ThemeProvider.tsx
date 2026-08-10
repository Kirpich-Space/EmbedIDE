import { createContext, useContext, useState, useCallback, useEffect, type ReactNode } from 'react'
import type { Theme } from '../core/types'
import { themes, defaultTheme } from './themes'
import { applyAccentOverride } from '../core/defaultSettings'

interface ThemeContextType {
  theme: Theme
  setTheme: (name: string) => void
  themeNames: string[]
}

const ThemeContext = createContext<ThemeContextType | null>(null)

export function applyTheme(theme: Theme) {
  const root = document.documentElement
  const c = theme.colors
  const dark = theme.type === 'dark'
  const set = (key: string, val: string) => root.style.setProperty(key, val)
  set('--bg', c.bg)
  set('--bg-panel', c.bgPanel)
  set('--bg-hover', c.bgHover)
  set('--bg-active', c.bgActive)
  set('--border', c.border)
  set('--text', c.text)
  set('--text-secondary', c.textSecondary)
  set('--accent', c.accent)
  set('--accent-hover', c.accentHover)
  set('--accent-glow', c.accentGlow)
  set('--sidebar-bg', c.sidebarBg)
  set('--toolbar-bg', c.toolbarBg)
  set('--statusbar-bg', c.statusBarBg)
  set('--editor-bg', c.editorBg)
  set('--button-bg', c.buttonBg)
  set('--button-text', c.buttonText)
  set('--scrollbar-bg', c.scrollbarBg)
  set('--scrollbar-thumb', c.scrollbarThumb)
  set('--tab-active-bg', c.tabActiveBg)
  set('--tab-inactive-bg', c.tabInactiveBg)
  set('--output-bg', c.outputBg)
  set('--hl-keyword', c.hlKeyword)
  set('--hl-type', c.hlType)
  set('--hl-string', c.hlString)
  set('--hl-number', c.hlNumber)
  set('--hl-comment', c.hlComment)

  // Islands chrome: soft depth + tactile raise/press
  set('--shadow-sm', dark
    ? '0 1px 2px rgba(0,0,0,0.55)'
    : '0 1px 2px rgba(26,29,35,0.04)')
  set('--shadow-md', dark
    ? '0 8px 24px rgba(0,0,0,0.55)'
    : '0 4px 16px rgba(26,29,35,0.06), 0 1px 2px rgba(26,29,35,0.04)')
  set('--shadow-lg', dark
    ? '0 24px 64px rgba(0,0,0,0.72), 0 0 0 1px rgba(255,255,255,0.06)'
    : '0 16px 48px rgba(26,29,35,0.1), 0 0 0 1px rgba(26,29,35,0.06)')
  set('--btn-raise', dark
    ? '0 1px 0 rgba(255,255,255,0.07), 0 2px 6px rgba(0,0,0,0.55)'
    : '0 1px 0 rgba(255,255,255,0.9), 0 1px 2px rgba(26,29,35,0.08)')
  set('--btn-raise-hover', dark
    ? '0 2px 0 rgba(255,255,255,0.09), 0 8px 18px rgba(0,0,0,0.65)'
    : '0 1px 0 rgba(255,255,255,1), 0 3px 8px rgba(26,29,35,0.1)')
  set('--btn-press', dark
    ? 'inset 0 2px 8px rgba(0,0,0,0.75), inset 0 0 0 1px rgba(255,255,255,0.04)'
    : 'inset 0 1px 3px rgba(26,29,35,0.12)')
  set('--accent-raise', dark
    ? `0 1px 0 rgba(255,255,255,0.18), 0 2px 14px ${c.accentGlow}`
    : `0 1px 0 rgba(255,255,255,0.35), 0 2px 8px ${c.accentGlow}`)
  set('--accent-press', dark
    ? 'inset 0 2px 8px rgba(0,0,0,0.55)'
    : 'inset 0 2px 4px rgba(0,0,0,0.18)')
  // Glass rim: brighter top/left edge (Islands / Zed Dark Islands)
  set('--panel-edge', dark
    ? `1px solid color-mix(in srgb, ${c.border} 70%, transparent)`
    : `1px solid ${c.border}`)
  set('--island-rim', dark
    ? `inset 0 1px 0 rgba(255,255,255,0.06), inset 1px 0 0 rgba(255,255,255,0.04)`
    : `inset 0 1px 0 rgba(255,255,255,0.95)`)
  set('--island-shadow', dark
    ? '0 2px 12px rgba(0,0,0,0.45)'
    : '0 1px 3px rgba(26,29,35,0.05), 0 4px 14px rgba(26,29,35,0.04)')
  set('--island-gap', '6px')
  set('--island-radius', '10px')
  set('--chrome-sep', dark
    ? `1px solid color-mix(in srgb, ${c.border} 85%, #fff)`
    : `1px solid ${c.border}`)
  set('--radius-sm', '6px')
  set('--radius-md', '8px')
  set('--radius-lg', '12px')
  set('--ease-out', 'cubic-bezier(0.2, 0.8, 0.2, 1)')
  set('--ease-press', 'cubic-bezier(0.3, 0.7, 0.4, 1)')
  set('--ease-spring', 'cubic-bezier(0.22, 1, 0.36, 1)')
  set('--dur-fast', '120ms')
  set('--dur-med', '220ms')
}

export function ThemeProvider({ children }: { children: ReactNode }) {
  const [theme, setThemeState] = useState<Theme>(() => {
    const saved = localStorage.getItem('embed-ide-theme')
    if (saved) {
      const found = themes.find(t => t.name === saved)
      if (found) return found
    }
    return defaultTheme
  })

  useEffect(() => {
    applyTheme(theme)
    localStorage.setItem('embed-ide-theme', theme.name)
    try {
      const raw = JSON.parse(localStorage.getItem('embed-ide-editor-settings') || '{}')
      if (raw?.customAccent) applyAccentOverride(String(raw.customAccent))
    } catch { /* ignore */ }
  }, [theme])

  const setTheme = useCallback((name: string) => {
    const found = themes.find(t => t.name === name)
    if (found) setThemeState(found)
  }, [])

  return (
    <ThemeContext.Provider value={{ theme, setTheme, themeNames: themes.map(t => t.name) }}>
      {children}
    </ThemeContext.Provider>
  )
}

export function useTheme() {
  const ctx = useContext(ThemeContext)
  if (!ctx) throw new Error('useTheme must be used within ThemeProvider')
  return ctx
}
