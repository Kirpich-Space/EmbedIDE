/** Parsed compiler/linker diagnostic for editor underlines. */
export interface FileDiagnostic {
  /** Absolute or project-relative path as seen in compiler output */
  file: string
  line: number
  col: number
  severity: 'error' | 'warning' | 'info'
  message: string
}

const GCC_RE = /^(.+?):(\d+)(?::(\d+))?:\s*(fatal error|error|warning|note|info)\s*:\s*(.+)$/i
const ZIG_RE = /^(.+?):(\d+):(\d+):\s*(error|warning|note)\s*:\s*(.+)$/i
const RUST_ARROW_RE = /^\s*-->\s+(.+?):(\d+):(\d+)\s*$/
const RUST_ERROR_RE = /^(error(?:\[[\w\d]+\])?|warning(?:\[[\w\d]+\])?|note):\s*(.+)$/i

function severityOf(kind: string): FileDiagnostic['severity'] {
  const k = kind.toLowerCase()
  if (k.includes('error') || k.includes('fatal')) return 'error'
  if (k.includes('warn')) return 'warning'
  return 'info'
}

function normalizePath(p: string): string {
  return p.replace(/\\/g, '/').trim()
}

/** Extract diagnostics from raw build/flash log lines. */
export function parseBuildDiagnostics(texts: string[]): FileDiagnostic[] {
  const out: FileDiagnostic[] = []
  let pendingRust: { severity: FileDiagnostic['severity']; message: string } | null = null

  for (const raw of texts) {
    const line = raw.replace(/\r$/, '')
    if (!line.trim()) continue

    const rustErr = line.match(RUST_ERROR_RE)
    if (rustErr && !line.includes('-->')) {
      pendingRust = { severity: severityOf(rustErr[1]), message: rustErr[2].trim() }
      continue
    }

    const rustArrow = line.match(RUST_ARROW_RE)
    if (rustArrow) {
      out.push({
        file: normalizePath(rustArrow[1]),
        line: Math.max(1, Number(rustArrow[2]) || 1),
        col: Math.max(1, Number(rustArrow[3]) || 1),
        severity: pendingRust?.severity || 'error',
        message: pendingRust?.message || 'error',
      })
      pendingRust = null
      continue
    }

    const zig = line.match(ZIG_RE)
    if (zig) {
      out.push({
        file: normalizePath(zig[1]),
        line: Math.max(1, Number(zig[2]) || 1),
        col: Math.max(1, Number(zig[3]) || 1),
        severity: severityOf(zig[4]),
        message: zig[5].trim(),
      })
      continue
    }

    const gcc = line.match(GCC_RE)
    if (gcc) {
      out.push({
        file: normalizePath(gcc[1]),
        line: Math.max(1, Number(gcc[2]) || 1),
        col: Math.max(1, Number(gcc[3]) || 1),
        severity: severityOf(gcc[4]),
        message: gcc[5].trim(),
      })
    }
  }

  return out
}

/** Whether diagnostic file refers to the open editor tab (absolute or relative). */
export function diagnosticMatchesTab(diagFile: string, tabId: string, projectDir?: string): boolean {
  const a = normalizePath(diagFile)
  const b = normalizePath(tabId)
  if (a === b) return true
  if (b.endsWith('/' + a) || b.endsWith(a)) return true
  if (a.endsWith('/' + b) || a.endsWith(b)) return true
  if (projectDir) {
    const root = normalizePath(projectDir).replace(/\/+$/, '')
    const relFromAbs = b.startsWith(root + '/') ? b.slice(root.length + 1) : b
    if (a === relFromAbs || a.endsWith('/' + relFromAbs) || relFromAbs.endsWith(a)) return true
    const absFromRel = a.startsWith('/') ? a : `${root}/${a}`
    if (absFromRel === b) return true
  }
  const baseA = a.split('/').pop() || a
  const baseB = b.split('/').pop() || b
  return baseA === baseB && baseA.includes('.')
}
