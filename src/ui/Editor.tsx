import { useEffect, useRef, useMemo, useCallback, useState } from 'react'
import { EditorView, keymap, lineNumbers, highlightActiveLine, highlightSpecialChars, drawSelection, rectangularSelection, highlightWhitespace } from '@codemirror/view'
import { EditorState, Compartment } from '@codemirror/state'
import { defaultKeymap, history, historyKeymap, indentWithTab, undo, redo } from '@codemirror/commands'
import { syntaxHighlighting, HighlightStyle, bracketMatching, foldGutter, indentOnInput, indentUnit } from '@codemirror/language'
import { tags as t } from '@lezer/highlight'
import { closeBrackets, closeBracketsKeymap, autocompletion, completionKeymap, startCompletion } from '@codemirror/autocomplete'
import { searchKeymap, highlightSelectionMatches } from '@codemirror/search'
import { lintGutter, setDiagnostics, type Diagnostic } from '@codemirror/lint'
import { javascript } from '@codemirror/lang-javascript'
import { cpp } from '@codemirror/lang-cpp'
import { rust as rustLang } from '@codemirror/lang-rust'
import type { EditorTabData, EditorSettings } from '../core/types'
import type { FileDiagnostic } from '../core/parseDiagnostics'
import { getLangColor } from '../core/utils'
import { useTranslation } from '../core/TranslationContext'
import { useTheme } from '../themes/ThemeProvider'
import { createCompletionSource } from './completions'
import { zigLanguage } from './zigLanguage'

const langCompartment = new Compartment()
const themeCompartment = new Compartment()
const tabSizeCompartment = new Compartment()
const lineNumbersCompartment = new Compartment()
const wordWrapCompartment = new Compartment()
const bracketMatchCompartment = new Compartment()
const highlightCompartment = new Compartment()
const activeLineCompartment = new Compartment()
const foldGutterCompartment = new Compartment()
const whitespaceCompartment = new Compartment()
const autocompleteCompartment = new Compartment()
const indentUnitCompartment = new Compartment()

const LANGUAGES: Record<string, () => unknown> = {
  rust: () => rustLang(),
  c: () => cpp(),
  cpp: () => cpp(),
  zig: () => zigLanguage,
}

function getLanguage(lang: string) {
  return (LANGUAGES[lang]?.() ?? javascript()) as import('@codemirror/language').LanguageSupport
}

function indentExt(settings: EditorSettings) {
  return indentUnit.of(settings.insertSpaces ? ' '.repeat(Math.max(1, settings.tabSize)) : '\t')
}

function getEditorTheme(settings: EditorSettings) {
  const caret = Math.max(1, Math.min(4, settings.caretWidth || 2))
  return EditorView.theme({
    '&': {
      fontSize: `${settings.fontSize}px`,
      fontFamily: settings.fontFamily,
      color: 'var(--text)',
      fontWeight: '500',
    },
    '.cm-scroller': {
      overflow: 'auto',
      fontFamily: settings.fontFamily,
      fontWeight: '500',
      lineHeight: String(settings.lineHeight || 1.45),
      ...(settings.smoothScroll ? { scrollBehavior: 'smooth' } : { scrollBehavior: 'auto' }),
    },
    '.cm-content': {
      caretColor: 'var(--accent)',
      color: 'var(--text)',
      fontWeight: '500',
      lineHeight: String(settings.lineHeight || 1.45),
    },
    '&.cm-focused': { outline: 'none' },
    '.cm-cursor, .cm-dropCursor': {
      borderLeftColor: 'var(--accent)',
      borderLeftWidth: `${caret}px`,
      animationDuration: settings.cursorBlinkRate > 0 ? `${settings.cursorBlinkRate}ms` : '0s',
    },
    '.cm-gutters': {
      background: 'var(--editor-bg)',
      borderRight: '1px solid var(--border)',
      userSelect: 'none',
      color: 'var(--text-secondary)',
      fontWeight: '500',
    },
    '.cm-activeLineGutter': { background: 'var(--bg-hover)', color: 'var(--text)' },
    '.cm-activeLine': { background: 'var(--bg-hover)' },
    '.cm-selectionBackground': { background: 'var(--accent-glow) !important' },
    '&.cm-focused .cm-selectionBackground': { background: 'var(--accent-glow) !important' },
    '.cm-matchingBracket': { background: 'rgba(255,107,0,0.2)', outline: '1px solid var(--accent)' },
    '.cm-foldPlaceholder': { background: 'transparent', border: 'none', color: 'var(--text-secondary)' },
    '.cm-selectionMatch': { background: 'rgba(255,107,0,0.15)' },
    '&.cm-focused .cm-cursor': { borderLeftColor: 'var(--accent)' },
    '.cm-highlightSpace::before, .cm-highlightTab': {
      opacity: 0.35,
      color: 'var(--text-secondary)',
    },
    '.cm-tooltip-autocomplete': {
      background: 'var(--bg-panel) !important',
      border: '1px solid var(--border) !important',
      color: 'var(--text) !important',
    },
    '.cm-tooltip-autocomplete ul li[aria-selected]': {
      background: 'var(--accent-glow) !important',
      color: 'var(--accent) !important',
    },
    '.cm-completionLabel': { fontWeight: '500' },
    '.cm-completionDetail': { color: 'var(--text-secondary)', fontStyle: 'normal' },
    '.cm-diagnostic-error': { borderBottom: '2px wavy #E81123' },
    '.cm-diagnostic-warning': { borderBottom: '2px wavy #F5A623' },
    '.cm-diagnostic-info': { borderBottom: '2px wavy #0078D4' },
    '.cm-lintRange-error': { backgroundImage: 'none', borderBottom: '2px wavy #E81123' },
    '.cm-lintRange-warning': { backgroundImage: 'none', borderBottom: '2px wavy #F5A623' },
    '.cm-gutters .cm-lint-marker-error': { color: '#E81123' },
    '.cm-gutters .cm-lint-marker-warning': { color: '#F5A623' },
  })
}

function getSyntaxHighlight() {
  const el = getComputedStyle(document.documentElement)
  const keyword = el.getPropertyValue('--hl-keyword').trim() || '#FFB4A8'
  const type = el.getPropertyValue('--hl-type').trim() || '#A8DCFF'
  const string = el.getPropertyValue('--hl-string').trim() || '#D4F0FF'
  const number = el.getPropertyValue('--hl-number').trim() || '#A8DCFF'
  const comment = el.getPropertyValue('--hl-comment').trim() || '#A0A0A0'
  const text = el.getPropertyValue('--text').trim() || '#FFFFFF'
  const secondary = el.getPropertyValue('--text-secondary').trim() || '#E0E0E0'
  const accent = el.getPropertyValue('--accent').trim() || '#FF6B00'

  return HighlightStyle.define([
    { tag: t.keyword, color: keyword, fontWeight: '600' },
    { tag: t.controlKeyword, color: keyword, fontWeight: '600' },
    { tag: t.definitionKeyword, color: keyword, fontWeight: '600' },
    { tag: t.operatorKeyword, color: keyword },
    { tag: t.moduleKeyword, color: keyword },
    { tag: t.typeName, color: type, fontWeight: '600' },
    { tag: t.className, color: type, fontWeight: '600' },
    { tag: t.namespace, color: type },
    { tag: t.typeOperator, color: type },
    { tag: t.string, color: string },
    { tag: t.character, color: string },
    { tag: t.special(t.string), color: string },
    { tag: t.number, color: number },
    { tag: t.bool, color: number },
    { tag: t.null, color: number },
    { tag: t.comment, color: comment, fontStyle: 'italic' },
    { tag: t.lineComment, color: comment, fontStyle: 'italic' },
    { tag: t.blockComment, color: comment, fontStyle: 'italic' },
    { tag: t.function(t.variableName), color: accent, fontWeight: '600' },
    { tag: t.function(t.propertyName), color: accent },
    { tag: t.definition(t.variableName), color: text, fontWeight: '600' },
    { tag: t.variableName, color: text },
    { tag: t.propertyName, color: text },
    { tag: t.operator, color: text },
    { tag: t.punctuation, color: secondary },
    { tag: t.bracket, color: secondary },
    { tag: t.meta, color: secondary },
    { tag: t.processingInstruction, color: keyword },
    { tag: t.name, color: text },
    { tag: t.literal, color: number },
  ])
}

interface EditorProps {
  tabs: EditorTabData[]
  activeTabId: string
  onTabSelect: (id: string) => void
  onTabClose: (id: string) => void
  onContentChange: (id: string, content: string) => void
  onCursorChange?: (id: string, line: number, col: number) => void
  onSave?: (id: string) => void
  editorSettings: EditorSettings
  /** Build diagnostics for the active file (underlines + gutter) */
  fileDiagnostics?: FileDiagnostic[]
}

function toCmDiagnostics(doc: { lines: number; line: (n: number) => { from: number; to: number } }, items: FileDiagnostic[]): Diagnostic[] {
  const diags: Diagnostic[] = []
  for (const d of items) {
    const lineNo = Math.min(Math.max(1, d.line), doc.lines)
    const line = doc.line(lineNo)
    const col = Math.max(1, d.col || 1)
    const from = Math.min(line.from + col - 1, line.to)
    const to = from < line.to ? Math.min(from + 1, line.to) : line.to
    diags.push({
      from: Math.min(from, line.to),
      to: Math.max(from, to === from ? Math.min(from + 1, line.to) : to),
      severity: d.severity === 'warning' ? 'warning' : d.severity === 'info' ? 'info' : 'error',
      message: d.message,
    })
  }
  return diags
}

export function Editor({
  tabs, activeTabId, onTabSelect, onTabClose, onContentChange, onCursorChange, onSave, editorSettings, fileDiagnostics = [],
}: EditorProps) {
  const editorRef = useRef<HTMLDivElement>(null)
  const viewRef = useRef<EditorView | null>(null)
  const activeTabIdRef = useRef(activeTabId)
  const tabContentRef = useRef<Map<string, string>>(new Map())
  const tabsRef = useRef(tabs)
  const onSaveRef = useRef(onSave)
  const onContentChangeRef = useRef(onContentChange)
  const onCursorChangeRef = useRef(onCursorChange)
  const [contextMenu, setContextMenu] = useState<{ x: number; y: number } | null>(null)
  const { t } = useTranslation()
  const { theme } = useTheme()

  activeTabIdRef.current = activeTabId
  tabsRef.current = tabs
  onSaveRef.current = onSave
  onContentChangeRef.current = onContentChange
  onCursorChangeRef.current = onCursorChange

  const activeTab = tabs.find(t => t.id === activeTabId)

  const settingsRef = useRef(editorSettings)
  settingsRef.current = editorSettings
  const langRef = useRef(activeTab?.language ?? 'c')
  langRef.current = activeTab?.language ?? 'c'

  const CMBindings = useMemo(() => {
    const bindings: any[] = [
      ...defaultKeymap,
      ...historyKeymap,
      ...searchKeymap,
      ...closeBracketsKeymap,
      ...completionKeymap,
      indentWithTab,
      {
        key: 'Mod-s',
        run: () => {
          onSaveRef.current?.(activeTabIdRef.current)
          return true
        },
      },
      {
        key: 'Ctrl-Space',
        run: startCompletion,
      },
    ]
    return bindings
  }, [])

  useEffect(() => {
    if (!editorRef.current) return

    const updateListener = EditorView.updateListener.of(update => {
      if (update.docChanged) {
        const content = update.state.doc.toString()
        const id = activeTabIdRef.current
        tabContentRef.current.set(id, content)
        onContentChangeRef.current(id, content)
      }
      if (update.selectionSet || update.docChanged) {
        const head = update.state.selection.main.head
        const line = update.state.doc.lineAt(head)
        onCursorChangeRef.current?.(activeTabIdRef.current, line.number, head - line.from + 1)
      }
    })

    const completionSource = createCompletionSource(() => langRef.current)
    const s0 = settingsRef.current

    const view = new EditorView({
      state: EditorState.create({
        doc: '',
        extensions: [
          activeLineCompartment.of(s0.highlightActiveLine !== false ? highlightActiveLine() : []),
          drawSelection(),
          rectangularSelection(),
          highlightSpecialChars(),
          bracketMatchCompartment.of(s0.bracketMatch ? bracketMatching() : []),
          closeBrackets(),
          indentOnInput(),
          foldGutterCompartment.of(s0.foldGutter !== false ? foldGutter() : []),
          whitespaceCompartment.of(s0.showWhitespace ? highlightWhitespace() : []),
          highlightSelectionMatches(),
          history(),
          lintGutter(),
          highlightCompartment.of(syntaxHighlighting(getSyntaxHighlight())),
          autocompleteCompartment.of(autocompletion({
            override: [completionSource],
            activateOnTyping: s0.autoComplete !== false,
            maxRenderedOptions: 40,
            defaultKeymap: true,
          })),
          keymap.of(CMBindings),
          langCompartment.of(javascript()),
          themeCompartment.of([getEditorTheme(editorSettings)]),
          tabSizeCompartment.of(EditorState.tabSize.of(editorSettings.tabSize)),
          indentUnitCompartment.of(indentExt(s0)),
          lineNumbersCompartment.of(s0.lineNumbers ? lineNumbers() : []),
          wordWrapCompartment.of(s0.wordWrap ? EditorView.lineWrapping : []),
          updateListener,
        ],
      }),
      parent: editorRef.current,
    })

    viewRef.current = view

    const tab = tabsRef.current.find(t => t.id === activeTabIdRef.current)
    if (tab) {
      tabContentRef.current.set(tab.id, tab.content)
      langRef.current = tab.language
      view.dispatch({
        changes: { from: 0, to: view.state.doc.length, insert: tab.content },
        effects: langCompartment.reconfigure(getLanguage(tab.language)),
      })
    }

    return () => view.destroy()
  }, [])

  // Live-update theme & settings without destroying view
  useEffect(() => {
    const view = viewRef.current
    if (!view) return
    const completionSource = createCompletionSource(() => langRef.current)
    view.dispatch({
      effects: [
        themeCompartment.reconfigure([getEditorTheme(editorSettings)]),
        highlightCompartment.reconfigure(syntaxHighlighting(getSyntaxHighlight())),
        tabSizeCompartment.reconfigure(EditorState.tabSize.of(editorSettings.tabSize)),
        indentUnitCompartment.reconfigure(indentExt(editorSettings)),
        lineNumbersCompartment.reconfigure(editorSettings.lineNumbers ? lineNumbers() : []),
        wordWrapCompartment.reconfigure(editorSettings.wordWrap ? EditorView.lineWrapping : []),
        bracketMatchCompartment.reconfigure(editorSettings.bracketMatch ? bracketMatching() : []),
        activeLineCompartment.reconfigure(editorSettings.highlightActiveLine !== false ? highlightActiveLine() : []),
        foldGutterCompartment.reconfigure(editorSettings.foldGutter !== false ? foldGutter() : []),
        whitespaceCompartment.reconfigure(editorSettings.showWhitespace ? highlightWhitespace() : []),
        autocompleteCompartment.reconfigure(autocompletion({
          override: [completionSource],
          activateOnTyping: editorSettings.autoComplete !== false,
          maxRenderedOptions: 40,
          defaultKeymap: true,
        })),
      ],
    })
  }, [editorSettings, theme.name])

  useEffect(() => {
    const view = viewRef.current
    if (!view || !activeTabId) return

    const tab = tabsRef.current.find(t => t.id === activeTabId)
    let content = tabContentRef.current.get(activeTabId)
    if (content === undefined && tab) {
      content = tab.content
      tabContentRef.current.set(activeTabId, content)
    }
    if (content === undefined) content = ''

    if (tab) langRef.current = tab.language

    const currentDoc = view.state.doc.toString()
    if (currentDoc !== content) {
      view.dispatch({
        changes: { from: 0, to: view.state.doc.length, insert: content },
        effects: langCompartment.reconfigure(getLanguage(tab?.language ?? 'text')),
      })
    } else if (tab) {
      view.dispatch({
        effects: langCompartment.reconfigure(getLanguage(tab.language)),
      })
    }
    view.focus()
  }, [activeTabId])

  // Sync externally updated active tab content (e.g. AI apply) into the editor
  const activeTabCleanContent = !activeTab?.dirty ? activeTab?.content : undefined
  useEffect(() => {
    const view = viewRef.current
    if (!view || activeTabCleanContent === undefined) return
    const id = activeTabIdRef.current
    tabContentRef.current.set(id, activeTabCleanContent)
    if (view.state.doc.toString() !== activeTabCleanContent) {
      view.dispatch({
        changes: { from: 0, to: view.state.doc.length, insert: activeTabCleanContent },
      })
    }
  }, [activeTabId, activeTabCleanContent])

  // Apply build diagnostics to the open buffer (do not re-run on every keystroke)
  useEffect(() => {
    const view = viewRef.current
    if (!view) return
    const diags = toCmDiagnostics(view.state.doc, fileDiagnostics)
    view.dispatch(setDiagnostics(view.state, diags))
  }, [fileDiagnostics, activeTabId])

  const handleContextMenu = useCallback((e: React.MouseEvent) => {
    e.preventDefault()
    setContextMenu({ x: e.clientX, y: e.clientY })
  }, [])

  const execCmd = useCallback((cmd: string) => {
    const view = viewRef.current
    if (!view) return
    view.focus()
    if (cmd === 'undo') { undo(view); setContextMenu(null); return }
    if (cmd === 'redo') { redo(view); setContextMenu(null); return }
    document.execCommand(cmd)
    setContextMenu(null)
  }, [])

  return (
    <div className="editor-panel" onContextMenu={handleContextMenu}>
      <div className="editor-tabs">
        {tabs.map(tab => (
          <div
            key={tab.id}
            className={`editor-tab ${tab.id === activeTabId ? 'editor-tab-active' : ''}`}
            onClick={() => onTabSelect(tab.id)}
            onMouseDown={e => { if (e.button === 1) { e.preventDefault(); onTabClose(tab.id) } }}
          >
            <span className="tab-lang-indicator" style={{ background: getLangColor(tab.language) }} />
            <span className="tab-name">{tab.name}</span>
            {tab.dirty && <span className="tab-dirty">●</span>}
            <span className="tab-close" onClick={e => { e.stopPropagation(); onTabClose(tab.id) }}>×</span>
          </div>
        ))}
      </div>
      {!activeTab && (
        <div className="editor-empty">
          <div className="editor-empty-mark" aria-hidden>◇</div>
          <div className="editor-empty-title">EmbedIDE</div>
          <p className="editor-empty-hint">{t('editor.empty')}</p>
          <div className="editor-empty-keys">
            <span className="editor-empty-key">Ctrl+N</span>
            <span className="editor-empty-key">Ctrl+O</span>
            <span className="editor-empty-key">Ctrl+B</span>
          </div>
        </div>
      )}
      <div className="editor-cm-wrapper" ref={editorRef} style={{ display: activeTab ? '' : 'none' }} />

      {contextMenu && (
        <>
          <div className="editor-ctx-overlay" onClick={() => setContextMenu(null)} />
          <div className="ctx-menu editor-ctx-menu" style={{ left: contextMenu.x, top: contextMenu.y }}>
            <div className="ctx-item" onClick={() => execCmd('undo')}><span>{t('editor.undo')}</span><span className="ctx-shortcut">Ctrl+Z</span></div>
            <div className="ctx-item" onClick={() => execCmd('redo')}><span>{t('editor.redo')}</span><span className="ctx-shortcut">Ctrl+Shift+Z</span></div>
            <div className="ctx-separator" />
            <div className="ctx-item" onClick={() => execCmd('cut')}><span>{t('editor.cut')}</span><span className="ctx-shortcut">Ctrl+X</span></div>
            <div className="ctx-item" onClick={() => execCmd('copy')}><span>{t('editor.copy')}</span><span className="ctx-shortcut">Ctrl+C</span></div>
            <div className="ctx-item" onClick={() => execCmd('paste')}><span>{t('editor.paste')}</span><span className="ctx-shortcut">Ctrl+V</span></div>
            <div className="ctx-separator" />
            <div className="ctx-item" onClick={() => execCmd('selectAll')}><span>{t('editor.selectAll')}</span><span className="ctx-shortcut">Ctrl+A</span></div>
          </div>
        </>
      )}
    </div>
  )
}
