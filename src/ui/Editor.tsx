import { useEffect, useRef, useMemo, useCallback, useState } from 'react'
import { EditorView, keymap, lineNumbers, highlightActiveLine, highlightSpecialChars, drawSelection, rectangularSelection } from '@codemirror/view'
import { EditorState, Compartment } from '@codemirror/state'
import { defaultKeymap, history, historyKeymap, indentWithTab, undo, redo } from '@codemirror/commands'
import { syntaxHighlighting, defaultHighlightStyle, bracketMatching, foldGutter, indentOnInput } from '@codemirror/language'
import { closeBrackets, closeBracketsKeymap } from '@codemirror/autocomplete'
import { searchKeymap, highlightSelectionMatches } from '@codemirror/search'
import { javascript } from '@codemirror/lang-javascript'
import { cpp } from '@codemirror/lang-cpp'
import { rust as rustLang } from '@codemirror/lang-rust'
import type { EditorTabData, EditorSettings } from '../core/types'
import { getLangColor } from '../core/utils'
import { useTranslation } from '../core/TranslationContext'

const langCompartment = new Compartment()
const themeCompartment = new Compartment()
const tabSizeCompartment = new Compartment()
const lineNumbersCompartment = new Compartment()
const wordWrapCompartment = new Compartment()
const bracketMatchCompartment = new Compartment()

const LANGUAGES: Record<string, () => import('@codemirror/language').LanguageSupport> = {
  rust: () => rustLang(),
  c: () => cpp(),
  cpp: () => cpp(),
}

function getLanguage(lang: string) {
  return LANGUAGES[lang]?.() ?? javascript()
}

function getEditorTheme(settings: EditorSettings) {
  return EditorView.theme({
    '&': {
      fontSize: `${settings.fontSize}px`,
      fontFamily: settings.fontFamily,
    },
    '.cm-scroller': {
      overflow: 'auto',
      fontFamily: settings.fontFamily,
      ...(settings.smoothScroll ? { scrollBehavior: 'smooth' } : {}),
    },
    '&.cm-focused': { outline: 'none' },
    '.cm-cursor': {
      borderLeftColor: 'var(--accent)',
      animationDuration: settings.cursorBlinkRate > 0 ? `${settings.cursorBlinkRate}ms` : '0s',
    },
    '.cm-gutters': {
      background: 'var(--editor-bg)',
      borderRight: '1px solid var(--border)',
      userSelect: 'none',
    },
    '.cm-activeLineGutter': { background: 'var(--bg-hover)' },
    '.cm-activeLine': { background: 'var(--bg-hover)' },
    '.cm-selectionBackground': { background: 'var(--accent-glow) !important' },
    '&.cm-focused .cm-selectionBackground': { background: 'var(--accent-glow) !important' },
    '.cm-matchingBracket': { background: 'rgba(255,107,0,0.2)', outline: '1px solid var(--accent)' },
    '.cm-foldPlaceholder': { background: 'transparent', border: 'none', color: 'var(--text-secondary)' },
    '.cm-selectionMatch': { background: 'rgba(255,107,0,0.15)' },
    '.cm-content': { caretColor: 'var(--accent)' },
    '&.cm-focused .cm-cursor': { borderLeftColor: 'var(--accent)' },
  })
}

function getSyntaxTheme() {
  const el = getComputedStyle(document.documentElement)
  return EditorView.theme({
    '&': { color: el.getPropertyValue('--text') },
    '.cm-keyword': { color: el.getPropertyValue('--hl-keyword') },
    '.cm-atom': { color: el.getPropertyValue('--hl-number') },
    '.cm-number': { color: el.getPropertyValue('--hl-number') },
    '.cm-type': { color: el.getPropertyValue('--hl-type') },
    '.cm-def': { color: el.getPropertyValue('--hl-type') },
    '.cm-string': { color: el.getPropertyValue('--hl-string') },
    '.cm-comment': { color: el.getPropertyValue('--hl-comment') },
    '.cm-variable': { color: el.getPropertyValue('--text') },
    '.cm-variable-2': { color: el.getPropertyValue('--text') },
    '.cm-variable-3': { color: el.getPropertyValue('--hl-type') },
    '.cm-operator': { color: el.getPropertyValue('--text') },
    '.cm-meta': { color: el.getPropertyValue('--text-secondary') },
    '.cm-tag': { color: el.getPropertyValue('--hl-keyword') },
    '.cm-attribute': { color: el.getPropertyValue('--hl-type') },
    '.cm-builtin': { color: el.getPropertyValue('--hl-type') },
    '.cm-bracket': { color: el.getPropertyValue('--text-secondary') },
    '.cm-punctuation': { color: el.getPropertyValue('--text-secondary') },
    '.cm-link': { color: el.getPropertyValue('--accent') },
    '.cm-quote': { color: el.getPropertyValue('--hl-string') },
    '.cm-hr': { color: el.getPropertyValue('--text-secondary') },
  })
}

interface EditorProps {
  tabs: EditorTabData[]
  activeTabId: string
  onTabSelect: (id: string) => void
  onTabClose: (id: string) => void
  onContentChange: (id: string, content: string) => void
  onSave?: (id: string) => void
  editorSettings: EditorSettings
}

export function Editor({ tabs, activeTabId, onTabSelect, onTabClose, onContentChange, onSave, editorSettings }: EditorProps) {
  const editorRef = useRef<HTMLDivElement>(null)
  const viewRef = useRef<EditorView | null>(null)
  const activeTabIdRef = useRef(activeTabId)
  const tabContentRef = useRef<Map<string, string>>(new Map())
  const tabsRef = useRef(tabs)
  const onSaveRef = useRef(onSave)
  const onContentChangeRef = useRef(onContentChange)
  const [contextMenu, setContextMenu] = useState<{ x: number; y: number } | null>(null)
  const { t } = useTranslation()

  activeTabIdRef.current = activeTabId
  tabsRef.current = tabs
  onSaveRef.current = onSave
  onContentChangeRef.current = onContentChange

  const activeTab = tabs.find(t => t.id === activeTabId)

  const settingsRef = useRef(editorSettings)
  settingsRef.current = editorSettings

  const CMBindings = useMemo(() => {
    const bindings: any[] = [
      ...defaultKeymap,
      ...historyKeymap,
      ...searchKeymap,
      ...closeBracketsKeymap,
      indentWithTab,
      {
        key: 'Mod-s',
        run: () => {
          onSaveRef.current?.(activeTabIdRef.current)
          return true
        },
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
    })

    const view = new EditorView({
      state: EditorState.create({
        doc: '',
        extensions: [
          highlightActiveLine(),
          drawSelection(),
          rectangularSelection(),
          highlightSpecialChars(),
          bracketMatchCompartment.of(settingsRef.current.bracketMatch ? bracketMatching() : []),
          closeBrackets(),
          indentOnInput(),
          foldGutter(),
          highlightSelectionMatches(),
          history(),
          syntaxHighlighting(defaultHighlightStyle, { fallback: true }),
          keymap.of(CMBindings),
          langCompartment.of(javascript()),
          themeCompartment.of([getEditorTheme(editorSettings), getSyntaxTheme()]),
          tabSizeCompartment.of(EditorState.tabSize.of(editorSettings.tabSize)),
          lineNumbersCompartment.of(settingsRef.current.lineNumbers ? lineNumbers() : []),
          wordWrapCompartment.of(settingsRef.current.wordWrap ? EditorView.lineWrapping : []),
          updateListener,
        ],
      }),
      parent: editorRef.current,
    })

    viewRef.current = view

    const tab = tabsRef.current.find(t => t.id === activeTabIdRef.current)
    if (tab) {
      tabContentRef.current.set(tab.id, tab.content)
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
    view.dispatch({
      effects: [
        themeCompartment.reconfigure([getEditorTheme(editorSettings), getSyntaxTheme()]),
        tabSizeCompartment.reconfigure(EditorState.tabSize.of(editorSettings.tabSize)),
        lineNumbersCompartment.reconfigure(editorSettings.lineNumbers ? lineNumbers() : []),
        wordWrapCompartment.reconfigure(editorSettings.wordWrap ? EditorView.lineWrapping : []),
        bracketMatchCompartment.reconfigure(editorSettings.bracketMatch ? bracketMatching() : []),
      ],
    })
  }, [editorSettings])

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
      {!activeTab && <div className="editor-empty">{t('editor.empty')}</div>}
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
