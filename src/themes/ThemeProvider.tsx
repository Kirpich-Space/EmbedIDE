import { createContext, useContext, useState, useCallback, useEffect, type ReactNode } from 'react'
import type { Theme } from '../core/types'
import { themes, defaultTheme } from './themes'

interface ThemeContextType {
  theme: Theme
  setTheme: (name: string) => void
  themeNames: string[]
}

const ThemeContext = createContext<ThemeContextType | null>(null)

function applyTheme(theme: Theme) {
  const root = document.documentElement
  const c = theme.colors
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

  root.style.setProperty('--shadow-sm', `0 1px 3px rgba(0,0,0,${theme.type === 'dark' ? 0.3 : 0.1})`)
  root.style.setProperty('--shadow-md', `0 4px 12px rgba(0,0,0,${theme.type === 'dark' ? 0.35 : 0.12})`)
  root.style.setProperty('--shadow-lg', `0 12px 40px rgba(0,0,0,${theme.type === 'dark' ? 0.45 : 0.15})`)
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
