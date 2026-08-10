import type { Theme, ThemeColors } from '../core/types'

/**
 * Islands layout (JetBrains / VS 2026 / Zed):
 * canvas → chrome → panels → editor (deepest).
 * Hierarchy comes from slight elevation, not flat #000 everywhere.
 */

type DarkAccent = {
  name: string
  accent: string
  accentHover: string
  accentGlow: string
  bgHover: string
  bgActive: string
  border: string
  textSecondary: string
  buttonText?: string
  hlKeyword: string
  hlType: string
  hlString: string
  hlNumber: string
  hlComment: string
  scrollbarThumb?: string
}

function darkIslands(opts: DarkAccent): Theme {
  const colors: ThemeColors = {
    bg: '#050505',
    bgPanel: '#121212',
    bgHover: opts.bgHover,
    bgActive: opts.bgActive,
    border: opts.border,
    text: '#FFFFFF',
    textSecondary: opts.textSecondary,
    accent: opts.accent,
    accentHover: opts.accentHover,
    accentGlow: opts.accentGlow,
    sidebarBg: '#0C0C0C',
    toolbarBg: '#0C0C0C',
    statusBarBg: '#080808',
    editorBg: '#000000',
    buttonBg: opts.accent,
    buttonText: opts.buttonText ?? '#FFFFFF',
    scrollbarBg: '#050505',
    scrollbarThumb: opts.scrollbarThumb ?? opts.border,
    tabActiveBg: '#000000',
    tabInactiveBg: '#0C0C0C',
    outputBg: '#0C0C0C',
    hlKeyword: opts.hlKeyword,
    hlType: opts.hlType,
    hlString: opts.hlString,
    hlNumber: opts.hlNumber,
    hlComment: opts.hlComment,
  }
  return { name: opts.name, type: 'dark', colors }
}

export const darkTheme = darkIslands({
  name: 'Dark Engineering',
  accent: '#FF6B00',
  accentHover: '#FF8A2B',
  accentGlow: 'rgba(255, 107, 0, 0.4)',
  bgHover: '#1A1A1A',
  bgActive: '#242424',
  border: '#3A3A3A',
  textSecondary: '#C8C8C8',
  hlKeyword: '#FFB8B0',
  hlType: '#B0DCFF',
  hlString: '#D8F2FF',
  hlNumber: '#B0DCFF',
  hlComment: '#A8A8A8',
})

/** Soft cool canvas + white islands — modern light, not gray WinXP chrome */
export const lightTheme: Theme = {
  name: 'Light Engineering',
  type: 'light',
  colors: {
    bg: '#E8EAEE',
    bgPanel: '#FFFFFF',
    bgHover: '#F0F2F5',
    bgActive: '#E4E7EC',
    border: '#D0D4DC',
    text: '#1A1D23',
    textSecondary: '#5C6470',
    accent: '#E85D04',
    accentHover: '#F06A14',
    accentGlow: 'rgba(232, 93, 4, 0.22)',
    sidebarBg: '#F5F6F8',
    toolbarBg: '#F5F6F8',
    statusBarBg: '#EEF0F3',
    editorBg: '#FFFFFF',
    buttonBg: '#E85D04',
    buttonText: '#FFFFFF',
    scrollbarBg: '#F0F2F5',
    scrollbarThumb: '#B8BEC8',
    tabActiveBg: '#FFFFFF',
    tabInactiveBg: '#EEF0F3',
    outputBg: '#F5F6F8',
    hlKeyword: '#B42318',
    hlType: '#175CD3',
    hlString: '#067647',
    hlNumber: '#175CD3',
    hlComment: '#667085',
  },
}

export const greenTheme = darkIslands({
  name: 'Green Engineering',
  accent: '#00FF66',
  accentHover: '#33FF88',
  accentGlow: 'rgba(0, 255, 102, 0.4)',
  bgHover: '#0C140E',
  bgActive: '#142018',
  border: '#2A3A2E',
  textSecondary: '#D8F0DC',
  buttonText: '#000000',
  hlKeyword: '#8EFF98',
  hlType: '#70C0FF',
  hlString: '#D8F2FF',
  hlNumber: '#70C0FF',
  hlComment: '#88A890',
})

export const burgundyTheme = darkIslands({
  name: 'Burgundy',
  accent: '#FF1048',
  accentHover: '#FF4068',
  accentGlow: 'rgba(255, 16, 72, 0.42)',
  bgHover: '#14080C',
  bgActive: '#1E0C12',
  border: '#3A1E2A',
  textSecondary: '#F0D0DC',
  hlKeyword: '#FFB8B0',
  hlType: '#B0DCFF',
  hlString: '#D8F2FF',
  hlNumber: '#B0DCFF',
  hlComment: '#A88890',
})

export const redTheme = darkIslands({
  name: 'Red Alert',
  accent: '#FF0000',
  accentHover: '#FF3333',
  accentGlow: 'rgba(255, 0, 0, 0.45)',
  bgHover: '#140808',
  bgActive: '#1E0C0C',
  border: '#3A1E1E',
  textSecondary: '#F0D0D0',
  hlKeyword: '#FFB8B0',
  hlType: '#B0DCFF',
  hlString: '#D8F2FF',
  hlNumber: '#B0DCFF',
  hlComment: '#A88888',
})

export const amberTheme = darkIslands({
  name: 'Amber Glow',
  accent: '#FFA000',
  accentHover: '#FFB833',
  accentGlow: 'rgba(255, 160, 0, 0.42)',
  bgHover: '#141008',
  bgActive: '#1E180C',
  border: '#3A3220',
  textSecondary: '#F0E4C0',
  buttonText: '#000000',
  hlKeyword: '#FFC888',
  hlType: '#80C8F8',
  hlString: '#C8D8A8',
  hlNumber: '#FFC888',
  hlComment: '#A89870',
})

export const cyberpunkTheme = darkIslands({
  name: 'Cyberpunk',
  accent: '#FF00AA',
  accentHover: '#FF40C0',
  accentGlow: 'rgba(255, 0, 170, 0.45)',
  bgHover: '#120818',
  bgActive: '#1A0C24',
  border: '#3A2048',
  textSecondary: '#F0D8FF',
  hlKeyword: '#FF50C0',
  hlType: '#20F8FF',
  hlString: '#C060FF',
  hlNumber: '#20F8FF',
  hlComment: '#9888A8',
})

export const oceanTheme = darkIslands({
  name: 'Ocean Deep',
  accent: '#00D4F8',
  accentHover: '#40E0FF',
  accentGlow: 'rgba(0, 212, 248, 0.42)',
  bgHover: '#081018',
  bgActive: '#0C1824',
  border: '#1E3850',
  textSecondary: '#D0E8F8',
  buttonText: '#000000',
  hlKeyword: '#FF8888',
  hlType: '#FFE080',
  hlString: '#D0F898',
  hlNumber: '#FFB080',
  hlComment: '#7898A8',
})

export const nordTheme = darkIslands({
  name: 'Nord',
  accent: '#88C0D0',
  accentHover: '#A0D8E4',
  accentGlow: 'rgba(136, 192, 208, 0.4)',
  bgHover: '#0A1018',
  bgActive: '#121820',
  border: '#4A5468',
  textSecondary: '#D8ECF8',
  buttonText: '#000000',
  hlKeyword: '#88B0D0',
  hlType: '#98D0CC',
  hlString: '#B0D090',
  hlNumber: '#C8A0C0',
  hlComment: '#8898A8',
})

export const catppuccinTheme = darkIslands({
  name: 'Catppuccin',
  accent: '#F5C2E7',
  accentHover: '#FFD6F0',
  accentGlow: 'rgba(245, 194, 231, 0.4)',
  bgHover: '#0C0C14',
  bgActive: '#161622',
  border: '#4A4A60',
  textSecondary: '#E4ECFF',
  buttonText: '#000000',
  hlKeyword: '#D4B4FF',
  hlType: '#96C0FF',
  hlString: '#B0F0A8',
  hlNumber: '#FFC090',
  hlComment: '#9090A8',
})

export const themes: Theme[] = [
  darkTheme,
  lightTheme,
  greenTheme,
  burgundyTheme,
  redTheme,
  amberTheme,
  cyberpunkTheme,
  oceanTheme,
  nordTheme,
  catppuccinTheme,
]
export const defaultTheme = darkTheme
