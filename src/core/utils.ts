export function getLangColor(lang: string): string {
  const colors: Record<string, string> = {
    rust: '#DEA584', c: '#555555', cpp: '#F34B7D',
    asm: '#E37933', toml: '#9C4221', linker: '#4EC9B0',
    python: '#3572A5', javascript: '#F7DF1E', typescript: '#3178C6',
  }
  return colors[lang] ?? '#888'
}

export const fileIcons: Record<string, string> = {
  rust: '🦀', c: '◎', cpp: '◈', asm: '⚙', toml: '◉', h: '◎', hpp: '◈',
  s: '⚙', ld: '🔗', json: '{}', yaml: '📋', python: '🐍',
}
