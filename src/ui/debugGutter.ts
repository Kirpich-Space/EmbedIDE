/**
 * Breakpoint gutter and current-execution-line highlight for CodeMirror.
 *
 * The editor only displays state owned by the main process: the parent
 * passes breakpoint line numbers and the stopped line for the active file,
 * and receives toggle requests (gutter click / F9).
 */
import { EditorView, GutterMarker, gutter, Decoration, type DecorationSet, keymap } from '@codemirror/view'
import { StateEffect, StateField, RangeSet, type Extension } from '@codemirror/state'

export interface DebugMarks {
  /** 1-based lines with a breakpoint; verified=false renders hollow */
  breakpoints: { line: number; verified: boolean }[]
  /** 1-based line where the target is stopped in this file */
  executionLine: number | null
}

export const setDebugMarks = StateEffect.define<DebugMarks>()

class BreakpointMarker extends GutterMarker {
  constructor(readonly verified: boolean) { super() }
  eq(other: BreakpointMarker) { return other.verified === this.verified }
  toDOM() {
    const el = document.createElement('span')
    el.className = this.verified ? 'cm-bp cm-bp-verified' : 'cm-bp cm-bp-pending'
    el.setAttribute('aria-label', this.verified ? 'Breakpoint' : 'Breakpoint (not yet resolved)')
    return el
  }
}

class ExecMarker extends GutterMarker {
  toDOM() {
    const el = document.createElement('span')
    el.className = 'cm-exec-arrow'
    el.textContent = '▶'
    el.setAttribute('aria-label', 'Current execution line')
    return el
  }
}

const verifiedMarker = new BreakpointMarker(true)
const pendingMarker = new BreakpointMarker(false)
const execMarker = new ExecMarker()
const execLineDeco = Decoration.line({ class: 'cm-exec-line' })

const marksField = StateField.define<DebugMarks>({
  create: () => ({ breakpoints: [], executionLine: null }),
  update(value, tr) {
    for (const e of tr.effects) if (e.is(setDebugMarks)) return e.value
    return value
  },
})

function lineStart(view: EditorView, line: number): number | null {
  if (line < 1 || line > view.state.doc.lines) return null
  return view.state.doc.line(line).from
}

const execHighlight = EditorView.decorations.compute([marksField, 'doc'], (state): DecorationSet => {
  const { executionLine } = state.field(marksField)
  if (!executionLine || executionLine > state.doc.lines) return Decoration.none
  return Decoration.set([execLineDeco.range(state.doc.line(executionLine).from)])
})

export function debugGutter(onToggle: (line: number) => void): Extension {
  return [
    marksField,
    execHighlight,
    gutter({
      class: 'cm-debug-gutter',
      markers: (view) => {
        const { breakpoints, executionLine } = view.state.field(marksField)
        const ranges = []
        const seen = new Set<number>()
        const all = [...breakpoints].sort((a, b) => a.line - b.line)
        for (const bp of all) {
          const from = lineStart(view, bp.line)
          if (from == null || seen.has(bp.line)) continue
          seen.add(bp.line)
          ranges.push((bp.verified ? verifiedMarker : pendingMarker).range(from))
        }
        if (executionLine && !seen.has(executionLine)) {
          const from = lineStart(view, executionLine)
          if (from != null) ranges.push(execMarker.range(from))
        }
        ranges.sort((a, b) => a.from - b.from)
        return RangeSet.of(ranges)
      },
      initialSpacer: () => verifiedMarker,
      domEventHandlers: {
        mousedown(view, block) {
          onToggle(view.state.doc.lineAt(block.from).number)
          return true
        },
      },
    }),
    keymap.of([{
      key: 'F9',
      run: (view) => {
        onToggle(view.state.doc.lineAt(view.state.selection.main.head).number)
        return true
      },
    }]),
    EditorView.baseTheme({
      '.cm-debug-gutter': { width: '16px', cursor: 'pointer' },
      '.cm-debug-gutter .cm-gutterElement': { display: 'flex', alignItems: 'center', justifyContent: 'center' },
      '.cm-bp': { display: 'inline-block', width: '9px', height: '9px', borderRadius: '50%' },
      '.cm-bp-verified': { background: '#e5484d' },
      '.cm-bp-pending': { border: '1.5px solid #e5484d', boxSizing: 'border-box' },
      '.cm-exec-arrow': { color: '#f5a524', fontSize: '10px', lineHeight: '1' },
      '.cm-exec-line': { backgroundColor: 'rgba(245, 165, 36, 0.16)' },
    }),
  ]
}
