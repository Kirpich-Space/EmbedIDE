import { useState, useCallback, useEffect, useRef, useMemo, startTransition, Component, type ReactNode } from 'react'
import { ThemeProvider, useTheme, applyTheme } from './themes/ThemeProvider'
import { Toolbar } from './ui/Toolbar'
import { FileExplorer } from './ui/FileExplorer'
import { Editor } from './ui/Editor'
import { AIAgents } from './ui/AIAgents'
import { OutputPanel } from './ui/OutputPanel'
import { SerialMonitor } from './ui/SerialMonitor'
import { Settings } from './ui/Settings'
import { ProjectDialog } from './ui/ProjectDialog'
import { SlidePanel } from './ui/SlidePanel'
import { FileDialog } from './ui/FileDialog'
import { ConfirmDialog } from './ui/ConfirmDialog'
import { StatusBar } from './ui/StatusBar'
import { MemoryAnalyzer } from './ui/MemoryAnalyzer'
import { PeripheralViewer } from './ui/PeripheralViewer'
import { TranslationProvider } from './core/TranslationContext'
import { getFlatTranslations, LANG_LABELS } from './core/translations'
import type { FileNode, EditorTabData, BuildMessage, EditorSettings, ProjectConfig, MemoryUsage } from './core/types'
import { parseBuildDiagnostics, diagnosticMatchesTab } from './core/parseDiagnostics'
import type { LangCode } from './core/translations'
import { DEFAULT_EDITOR_SETTINGS, normalizeEditorSettings, applyAccentOverride, applyUiChrome } from './core/defaultSettings'

class ErrorBoundary extends Component<{ children: ReactNode }, { error: Error | null }> {
  constructor(props: { children: ReactNode }) {
    super(props)
    this.state = { error: null }
  }
  static getDerivedStateFromError(error: Error) {
    return { error }
  }
  render() {
    if (this.state.error) {
      return <div className="error-boundary animate-fade-in" style={{ padding: 24, color: 'var(--accent)' }}>
        <h2>Something went wrong</h2>
        <pre style={{ marginTop: 12, fontSize: 12, color: 'var(--text-secondary)' }}>{this.state.error.message}</pre>
        <button className="project-btn" style={{ marginTop: 16 }} onClick={() => { this.setState({ error: null }); window.location.reload() }}>
          Restart
        </button>
      </div>
    }
    return this.props.children
  }
}

function AppContent() {
  const { theme } = useTheme()
  const [project, setProject] = useState<ProjectConfig | null>(null)
  const [projectFiles, setProjectFiles] = useState<FileNode[]>([])
  const [openTabs, setOpenTabs] = useState<EditorTabData[]>([])
  const [activeTabId, setActiveTabId] = useState('')
  const [outputMessages, setOutputMessages] = useState<BuildMessage[]>([])
  const [settingsOpen, setSettingsOpen] = useState(false)
  const [projectDialogOpen, setProjectDialogOpen] = useState(false)
  const [bottomTab, setBottomTab] = useState<'output' | 'serial' | 'memory' | 'peripheral' | null>(null)
  const [toolchains, setToolchains] = useState<ToolchainInfo | null>(null)
  const [memoryUsage, setMemoryUsage] = useState<MemoryUsage>({ flashUsed: 0, flashTotal: 1048576, ramUsed: 0, ramTotal: 131072 })
  const [showLeftPanel, setShowLeftPanel] = useState(true)
  const [showRightPanel, setShowRightPanel] = useState(false)
  const [dirtyConfirm, setDirtyConfirm] = useState<{ tabId: string; callback: () => void } | null>(null)
  const [isBuilding, setIsBuilding] = useState(false)
  const [settingsReady, setSettingsReady] = useState(false)
  const [cursorPos, setCursorPos] = useState({ line: 1, col: 1 })
  const [editorSettings, setEditorSettings] = useState<EditorSettings>(() => {
    try {
      const parsed = JSON.parse(localStorage.getItem('embed-ide-editor-settings') || '')
      return normalizeEditorSettings(parsed)
    } catch {
      return { ...DEFAULT_EDITOR_SETTINGS }
    }
  })

  const t = useMemo(() => {
    const lang = (Object.keys(LANG_LABELS).includes(editorSettings.language) ? editorSettings.language : 'en') as LangCode
    const dict = getFlatTranslations(lang)
    return (key: string, params?: Record<string, string | number>) => {
      let val = dict[key] ?? key
      if (params) {
        for (const [k, v] of Object.entries(params)) {
          val = val.replace(`{${k}}`, String(v))
        }
      }
      return val
    }
  }, [editorSettings.language])

  const activeFileDiagnostics = useMemo(() => {
    const all = parseBuildDiagnostics(outputMessages.map(m => m.text))
    if (!activeTabId) return []
    return all.filter(d => diagnosticMatchesTab(d.file, activeTabId, project?.dir))
  }, [outputMessages, activeTabId, project?.dir])
  const tRef = useRef(t)

  // File dialog state
  const [fileDialog, setFileDialog] = useState<{
    mode: 'create-file' | 'create-folder' | 'rename' | 'create-script'
    node?: FileNode
    parentDir?: string
  } | null>(null)

  // Confirm delete state
  const [confirmDelete, setConfirmDelete] = useState<FileNode | null>(null)

  const openTabsRef = useRef(openTabs)
  const activeTabIdRef = useRef(activeTabId)
  const projectRef = useRef(project)
  const editorSettingsRef = useRef(editorSettings)
  const buildCancelRef = useRef(false)
  openTabsRef.current = openTabs
  activeTabIdRef.current = activeTabId
  projectRef.current = project
  editorSettingsRef.current = editorSettings

  useEffect(() => {
    window.electronAPI?.detectToolchains().then(setToolchains)
    if (!window.electronAPI?.loadSettings) {
      setSettingsReady(true)
      return
    }
    window.electronAPI.loadSettings().then(saved => {
      if (saved && Object.keys(saved).length > 0 && !localStorage.getItem('embed-ide-editor-settings')) {
        setEditorSettings(normalizeEditorSettings(saved as Partial<EditorSettings>))
      } else if (saved && typeof saved === 'object' && 'aiKey' in saved) {
        // Restore API key from userData only (not stored in localStorage)
        setEditorSettings(prev => normalizeEditorSettings({
          ...prev,
          aiKey: String((saved as { aiKey?: string }).aiKey || ''),
        }))
      }
      setSettingsReady(true)
    }).catch(() => {
      setSettingsReady(true)
    })
  }, [])

  useEffect(() => {
    document.documentElement.lang = editorSettings.language || 'en'
    document.documentElement.dataset.lang = editorSettings.language || 'en'
  }, [editorSettings.language])

  useEffect(() => {
    if (!editorSettings.aiEnabled) setShowRightPanel(false)
  }, [editorSettings.aiEnabled])

  useEffect(() => {
    applyUiChrome(editorSettings)
    if (editorSettings.customAccent) {
      applyAccentOverride(editorSettings.customAccent)
    } else {
      applyTheme(theme)
    }
  }, [
    theme,
    editorSettings.customAccent,
    editorSettings.uiScale,
    editorSettings.compactUi,
    editorSettings.reduceMotion,
    editorSettings.glassEffects,
  ])

  // Never persist API keys in localStorage; wait until disk settings loaded
  // so we don't wipe aiKey with the empty default on first mount.
  // Debounce disk writes — Settings/AI keystrokes used to thrash IPC + fs.
  useEffect(() => {
    if (!settingsReady) return
    const { aiKey: _omit, ...safe } = editorSettings
    localStorage.setItem('embed-ide-editor-settings', JSON.stringify(safe))
    const persist: Record<string, unknown> = { ...editorSettings }
    const timer = window.setTimeout(() => {
      window.electronAPI?.saveSettings(persist)
    }, 400)
    return () => window.clearTimeout(timer)
  }, [editorSettings, settingsReady])

  useEffect(() => {
    const kb = project?.flashKb ?? 1024
    const ram = project?.ramKb ?? 128
    setMemoryUsage(prev => ({
      ...prev,
      flashTotal: kb * 1024,
      ramTotal: ram * 1024,
    }))
  }, [project?.flashKb, project?.ramKb])

  // IPC listeners with proper cleanup
  useEffect(() => {
    const api = window.electronAPI
    if (!api) return

    const unsubBuildOut = api.onBuildOutput((data) => {
      const msg: BuildMessage = { type: data.text.includes('error') && data.type === 'stderr' ? 'error' : data.type === 'stderr' ? 'warn' : 'info', text: data.text, timestamp: Date.now(), source: 'build' }
      setOutputMessages(prev => [...prev, msg])
      const text = data.text
      const flashMatch = text.match(/FLASH:\s*(\d+)\s*bytes/i)
      const ramMatch = text.match(/RAM:\s*(\d+)\s*bytes/i)
      if (flashMatch || ramMatch) {
        setMemoryUsage(prev => ({
          ...prev,
          ...(flashMatch ? { flashUsed: parseInt(flashMatch[1]) } : {}),
          ...(ramMatch ? { ramUsed: parseInt(ramMatch[1]) } : {}),
        }))
      }
    })

    const unsubBuildComplete = api.onBuildComplete((data) => {
      setIsBuilding(false)
      if (data.cancelled || buildCancelRef.current) {
        buildCancelRef.current = false
        return
      }
      if (data.code === 0) {
        setOutputMessages(prev => [...prev, { type: 'success', text: tRef.current('build.success'), timestamp: Date.now(), source: 'build' }])
        const typ = projectRef.current?.type || ''
        if (!/^script-|^python$|^shell$/.test(typ)) {
          setBottomTab('memory')
        } else {
          setBottomTab('output')
        }
      } else {
        setOutputMessages(prev => [...prev, { type: 'error', text: data.error || tRef.current('build.failed', { code: data.code ?? -1 }), timestamp: Date.now(), source: 'build' }])
        setBottomTab('output')
      }
    })

    const unsubFlashOut = api.onFlashOutput((data) => {
      setOutputMessages(prev => [...prev, { type: 'info', text: data.text, timestamp: Date.now(), source: 'flash' }])
    })

    const unsubFlashComplete = api.onFlashComplete((data) => {
      setOutputMessages(prev => [...prev, data.code === 0
        ? { type: 'success', text: tRef.current('flash.success'), timestamp: Date.now(), source: 'flash' }
        : { type: 'error', text: data.error || tRef.current('flash.failed', { code: data.code ?? -1 }), timestamp: Date.now(), source: 'flash' }
      ])
    })

    const unsubRunOut = api.onRunOutput?.((data) => {
      setOutputMessages(prev => [...prev, {
        type: data.type === 'stderr' ? 'warn' : 'info',
        text: data.text,
        timestamp: Date.now(),
        source: 'build',
      }])
    })

    const unsubRunComplete = api.onRunComplete?.((data) => {
      setIsBuilding(false)
      if (data.error) {
        setOutputMessages(prev => [...prev, { type: 'error', text: data.error!, timestamp: Date.now(), source: 'build' }])
      } else if (data.code === 0) {
        setOutputMessages(prev => [...prev, { type: 'success', text: tRef.current('build.success'), timestamp: Date.now(), source: 'build' }])
      }
    })

    return () => {
      unsubBuildOut()
      unsubBuildComplete()
      unsubFlashOut()
      unsubFlashComplete()
      unsubRunOut?.()
      unsubRunComplete?.()
    }
  }, [])

  // Menu events — use refs so handlers stay fresh without resubscribing
  const handleOpenProjectRef = useRef<() => void>(() => {})
  const handleSaveRef = useRef<() => void>(() => {})
  const saveAllDirtyRef = useRef<() => void>(() => {})
  const handleBuildRef = useRef<() => void>(() => {})
  const handleFlashRef = useRef<() => void>(() => {})
  const handleDebugRef = useRef<() => void>(() => {})
  const handleTabCloseRef = useRef<(id: string) => void>(() => {})

  useEffect(() => {
    const api = window.electronAPI
    if (!api) return
    const unsubs = [
      api.onMenuNewProject(() => setProjectDialogOpen(true)),
      api.onMenuOpenProject(() => handleOpenProjectRef.current()),
      api.onMenuSave(() => handleSaveRef.current()),
      api.onMenuSaveAll(() => saveAllDirtyRef.current()),
      api.onMenuSettings(() => setSettingsOpen(true)),
      api.onMenuFind(() => {
        const cm = document.querySelector('.cm-editor') as HTMLElement
        cm?.focus()
      }),
      api.onMenuToggleExplorer(() => setShowLeftPanel(v => !v)),
      api.onMenuToggleAgents(() => {
        if (editorSettingsRef.current.aiEnabled) setShowRightPanel(v => !v)
      }),
      api.onMenuBuild(() => handleBuildRef.current()),
      api.onMenuFlash(() => handleFlashRef.current()),
      api.onMenuDebug?.(() => handleDebugRef.current()),
    ]
    return () => unsubs.forEach(u => u?.())
  }, [])

  const addOutput = useCallback((msg: BuildMessage) => {
    setOutputMessages(prev => [...prev, msg])
    setBottomTab('output')
  }, [])

  const saveTab = useCallback(async (tabId: string) => {
    const tabs = openTabsRef.current
    const proj = projectRef.current
    const tab = tabs.find(t => t.id === tabId)
    if (!tab || !tab.dirty || !proj) return false
    try {
      await window.electronAPI!.writeProjectFile(proj.dir, tabId, tab.content)
      setOpenTabs(prev => prev.map(t => t.id === tabId ? { ...t, dirty: false } : t))
      addOutput({ type: 'info', text: tRef.current('fileOps.saved', { name: tab.name }), timestamp: Date.now(), source: 'build' })
      return true
    } catch (e: any) {
      addOutput({ type: 'error', text: tRef.current('fileOps.saveError', { name: tab.name, msg: e.message }), timestamp: Date.now(), source: 'build' })
      return false
    }
  }, [addOutput])

  const saveAllDirty = useCallback(async () => {
    const tabs = openTabsRef.current
    const proj = projectRef.current
    const dirty = tabs.filter(t => t.dirty && proj)
    for (const tab of dirty) {
      await saveTab(tab.id)
    }
  }, [saveTab])

  const loadProject = useCallback(async (
    dir: string,
    name: string,
    type: string,
    extra?: Partial<ProjectConfig>,
  ) => {
    let boardExtra = extra || {}
    if (!boardExtra.boardId) {
      const meta = await window.electronAPI?.getProjectMeta(dir)
      if (meta) {
        boardExtra = {
          boardId: meta.boardId,
          boardName: meta.boardName,
          flashKb: meta.flashKb,
          ramKb: meta.ramKb,
          peripherals: meta.peripherals,
        }
      }
    }
    setProject({ dir, name, type, ...boardExtra })
    const files = await window.electronAPI!.listProjectFiles(dir)
    setProjectFiles(files)
    setOpenTabs([])
    setActiveTabId('')
    setOutputMessages(prev => [...prev, {
      type: 'info',
      text: tRef.current('fileOps.opened', { name, type }) + (boardExtra.boardName ? ` · ${boardExtra.boardName}` : ''),
      timestamp: Date.now(),
      source: 'build',
    }])
  }, [])

  const handleCreateProject = useCallback(async (name: string, type: string, boardId: string) => {
    await saveAllDirty()
    const projectsDir = await window.electronAPI!.getDefaultProjectsDir()
    const dir = await window.electronAPI!.createProject(projectsDir, name, type, boardId)
    setProjectDialogOpen(false)
    const board = await window.electronAPI!.getBoard(boardId)
    await loadProject(dir, name, type, {
      boardId,
      boardName: board?.name,
      flashKb: board?.flashKb,
      ramKb: board?.ramKb,
      peripherals: board?.peripherals,
    })
  }, [loadProject, saveAllDirty])

  const handleOpenProject = useCallback(async () => {
    const result = await window.electronAPI!.openProject()
    if (!result) return
    await saveAllDirty()
    await loadProject(result.dir, result.name, result.type, {
      boardId: result.boardId,
      boardName: result.boardName,
      flashKb: result.flashKb,
      ramKb: result.ramKb,
      peripherals: result.peripherals,
    })
  }, [loadProject, saveAllDirty])

  const refreshFiles = useCallback(async () => {
    const proj = projectRef.current
    if (!proj) return
    const files = await window.electronAPI!.listProjectFiles(proj.dir)
    setProjectFiles(files)
  }, [])

  const openFileTab = useCallback((file: {id: string, name: string, language?: string}, content: string) => {
    setOpenTabs(prev => {
      const existing = prev.find(t => t.id === file.id)
      if (existing) { setActiveTabId(file.id); return prev }
      const newTab: EditorTabData = {
        id: file.id, name: file.name, language: file.language ?? 'text',
        content, dirty: false, cursorLine: 1, cursorCol: 1,
      }
      setActiveTabId(file.id)
      return [...prev, newTab]
    })
  }, [])

  const handleFileSelect = useCallback(async (node: FileNode) => {
    const proj = projectRef.current
    if (node.type !== 'file' || !proj) return
    const filePath = `${proj.dir}/${node.id}`
    const existing = openTabsRef.current.find(t => t.id === filePath)
    if (existing) { setActiveTabId(filePath); return }
    try {
      const content = await window.electronAPI!.readProjectFile(proj.dir, filePath)
      openFileTab({ id: filePath, name: node.name, language: node.language }, content)
    } catch {
      openFileTab({ id: filePath, name: node.name, language: node.language }, `// ${node.name}\n`)
    }
  }, [openFileTab])

  const handleTabClose = useCallback((tabId: string) => {
    const tabs = openTabsRef.current
    const active = activeTabIdRef.current
    const tab = tabs.find(t => t.id === tabId)
    const doClose = () => {
      setOpenTabs(prev => {
        const idx = prev.findIndex(t => t.id === tabId)
        const next = prev.filter(t => t.id !== tabId)
        if (next.length === 0) { setActiveTabId(''); return [] }
        if (tabId === active) setActiveTabId(next[Math.min(idx, next.length - 1)].id)
        return next
      })
    }
    if (tab?.dirty) {
      setDirtyConfirm({ tabId, callback: doClose })
    } else {
      doClose()
    }
  }, [])

  const handleContentChange = useCallback((tabId: string, content: string) => {
    startTransition(() => {
      setOpenTabs(prev => prev.map(t => t.id === tabId ? { ...t, content, dirty: true } : t))
    })
  }, [])

  const handleCursorChange = useCallback((_tabId: string, line: number, col: number) => {
    setCursorPos(prev => (prev.line === line && prev.col === col ? prev : { line, col }))
  }, [])

  // File dialog handlers
  const handleNewFile = useCallback((parentDir?: string) => {
    setFileDialog({ mode: 'create-file', parentDir })
  }, [])

  const handleNewScript = useCallback((parentDir?: string) => {
    setFileDialog({ mode: 'create-script', parentDir })
  }, [])

  const handleNewFolder = useCallback((parentDir?: string) => {
    setFileDialog({ mode: 'create-folder', parentDir })
  }, [])

  const handleRename = useCallback((node: FileNode) => {
    setFileDialog({ mode: 'rename', node })
  }, [])

  const handleFileDialogSubmit = useCallback(async (name: string) => {
    const proj = projectRef.current
    if (!proj || !fileDialog) return

    const invalid = name.startsWith('.') || name.includes('..') || /[<>:"|?*\\]/.test(name)
    if (invalid) throw new Error(tRef.current('fileOps.invalidName'))

    if (fileDialog.mode === 'rename' && fileDialog.node) {
      const node = fileDialog.node
      if (name === node.name) { setFileDialog(null); return }

      const active = activeTabIdRef.current
      const oldPath = `${proj.dir}/${node.id}`
      const parentDir = node.id.includes('/') ? node.id.substring(0, node.id.lastIndexOf('/')) : ''
      const newRelPath = parentDir ? `${parentDir}/${name}` : name
      const newPath = `${proj.dir}/${newRelPath}`
      const ok = await window.electronAPI!.renameProjectFile(proj.dir, oldPath, newPath)
      if (!ok) throw new Error(`${newRelPath} already exists`)

      setOpenTabs(prev => prev.map(t => {
        if (t.id === oldPath) return { ...t, id: newPath, name }
        if (t.id.startsWith(oldPath + '/')) {
          return { ...t, id: newPath + t.id.slice(oldPath.length) }
        }
        return t
      }))
      if (active === oldPath || active.startsWith(oldPath + '/')) {
        setActiveTabId(active === oldPath ? newPath : newPath + active.slice(oldPath.length))
      }
      setFileDialog(null)
      await refreshFiles()
      addOutput({ type: 'info', text: tRef.current('fileOps.renamed', { name: newRelPath }), timestamp: Date.now(), source: 'build' })
      return
    }

    const relPath = fileDialog.parentDir ? `${fileDialog.parentDir}/${name}` : name
    const actualPath = fileDialog.mode === 'create-folder' ? relPath + '/' : relPath

    const ok = await window.electronAPI!.createProjectFile(proj.dir, actualPath)
    if (!ok) throw new Error(`${relPath} already exists`)

    let starter = ''
    if (fileDialog.mode === 'create-script') {
      const base = name.replace(/\.[^.]+$/, '')
      const lower = name.toLowerCase()
      if (lower.endsWith('.c')) {
        starter = `#include <stdio.h>\n\nint main(void) {\n    printf("Hello from ${base}\\n");\n    return 0;\n}\n`
      } else if (lower.endsWith('.cpp') || lower.endsWith('.cc') || lower.endsWith('.cxx')) {
        starter = `#include <iostream>\n\nint main() {\n    std::cout << "Hello from ${base}\\n";\n    return 0;\n}\n`
      } else if (lower.endsWith('.rs')) {
        starter = `fn main() {\n    println!("Hello from ${base}");\n}\n`
      } else if (lower.endsWith('.s')) {
        starter = `#if defined(__APPLE__)\n#  define CNAME(x) _##x\n#else\n#  define CNAME(x) x\n#endif\n\n    .section .rodata\nmsg:\n    .asciz "Hello from ${base}\\n"\n\n    .text\n    .globl CNAME(main)\nCNAME(main):\n    lea     msg(%rip), %rdi\n    call    CNAME(puts)\n    xor     %eax, %eax\n    ret\n`
      }
      if (starter) {
        await window.electronAPI!.writeProjectFile(proj.dir, `${proj.dir}/${relPath}`, starter)
      }
    }

    setFileDialog(null)
    await refreshFiles()

    if (fileDialog.mode !== 'create-folder') {
      const filePath = `${proj.dir}/${relPath}`
      const ext = name.includes('.') ? name.split('.').pop() : ''
      openFileTab({ id: filePath, name, language: ext }, starter)
      setActiveTabId(filePath)
    }
    addOutput({ type: 'info', text: tRef.current('fileOps.created', { name: relPath }), timestamp: Date.now(), source: 'build' })
  }, [fileDialog, refreshFiles, openFileTab, addOutput])

  const performDelete = useCallback(async (node: FileNode) => {
    const proj = projectRef.current
    const active = activeTabIdRef.current
    if (!proj) return
    const fullPath = `${proj.dir}/${node.id}`
    await window.electronAPI!.deleteProjectFile(proj.dir, fullPath)
    setOpenTabs(prev => {
      const next = prev.filter(t => t.id !== fullPath && !t.id.startsWith(fullPath + '/'))
      if (next.length === 0) { setActiveTabId(''); return [] }
      if (active === fullPath || active.startsWith(fullPath + '/')) setActiveTabId(next[next.length - 1].id)
      return next
    })
    setConfirmDelete(null)
    await refreshFiles()
    addOutput({ type: 'info', text: tRef.current('fileOps.deleted', { name: node.name }), timestamp: Date.now(), source: 'build' })
  }, [refreshFiles, addOutput])

  const handleDelete = useCallback((node: FileNode) => {
    if (editorSettingsRef.current.confirmDelete === false) {
      void performDelete(node)
      return
    }
    setConfirmDelete(node)
  }, [performDelete])

  const handleDeleteConfirm = useCallback(async () => {
    if (!confirmDelete) return
    await performDelete(confirmDelete)
  }, [confirmDelete, performDelete])

  const handleBuild = useCallback(async () => {
    const proj = projectRef.current
    if (!proj) { addOutput({ type: 'warn', text: tRef.current('build.noProject'), timestamp: Date.now(), source: 'build' }); return }
    await saveAllDirty()
    const isScript = /^script-|^python$|^shell$/.test(proj.type)
    setOutputMessages([{
      type: 'info',
      text: isScript
        ? tRef.current('build.running', { name: proj.name })
        : tRef.current('build.compiling', { name: proj.name }),
      timestamp: Date.now(),
      source: 'build',
    }])
    setBottomTab('output')
    setIsBuilding(true)
    try {
      const result = await window.electronAPI!.buildProject(proj.dir, proj.type)
      if (result && result.success === false && (result as { error?: string }).error) {
        addOutput({
          type: 'error',
          text: tRef.current('build.error', { msg: (result as { error?: string }).error || 'build failed' }),
          timestamp: Date.now(),
          source: 'build',
        })
        setIsBuilding(false)
      }
    } catch (e: any) {
      addOutput({ type: 'error', text: tRef.current('build.error', { msg: e.message }), timestamp: Date.now(), source: 'build' })
      setIsBuilding(false)
    }
  }, [addOutput, saveAllDirty])

  const handleCancelBuild = useCallback(async () => {
    buildCancelRef.current = true
    await window.electronAPI?.cancelBuild()
    await window.electronAPI?.cancelDebug?.()
    setIsBuilding(false)
    addOutput({ type: 'warn', text: tRef.current('build.cancelled'), timestamp: Date.now(), source: 'build' })
  }, [addOutput])

  const handleFlash = useCallback(async () => {
    const proj = projectRef.current
    if (!proj) { addOutput({ type: 'warn', text: tRef.current('flash.noProject'), timestamp: Date.now(), source: 'flash' }); return }
    if (/^script-|^python$|^shell$/.test(proj.type)) {
      addOutput({ type: 'warn', text: tRef.current('flash.scriptsUnsupported'), timestamp: Date.now(), source: 'flash' })
      return
    }
    await saveAllDirty()
    setBottomTab('output')
    addOutput({ type: 'info', text: tRef.current('flash.starting'), timestamp: Date.now(), source: 'flash' })
    try {
      const result = await window.electronAPI!.flashProject(proj.dir, proj.type, {
        boardId: proj.boardId,
      })
      if (result && result.success === false && (result as { error?: string }).error) {
        addOutput({
          type: 'error',
          text: tRef.current('flash.error', { msg: (result as { error?: string }).error || 'flash failed' }),
          timestamp: Date.now(),
          source: 'flash',
        })
      }
    } catch (e: unknown) {
      const msg = e instanceof Error ? e.message : String(e)
      addOutput({ type: 'error', text: tRef.current('flash.error', { msg }), timestamp: Date.now(), source: 'flash' })
    }
  }, [addOutput, saveAllDirty])

  const handleDebug = useCallback(async () => {
    const proj = projectRef.current
    if (!proj) { addOutput({ type: 'warn', text: tRef.current('build.noProject'), timestamp: Date.now(), source: 'build' }); return }
    await saveAllDirty()
    setBottomTab('output')
    setOutputMessages([{
      type: 'info',
      text: tRef.current('debug.starting', { name: proj.name }),
      timestamp: Date.now(),
      source: 'build',
    }])
    setIsBuilding(true)
    try {
      const active = activeTabIdRef.current
      const filePath = active && active.startsWith(proj.dir) ? active : undefined
      // Firmware: compile first so Debug always has a fresh ELF when possible
      if (!/^script-|^python$|^shell$/.test(proj.type)) {
        addOutput({ type: 'info', text: tRef.current('debug.buildingFirst'), timestamp: Date.now(), source: 'build' })
        const built = await window.electronAPI!.buildProject(proj.dir, proj.type)
        setBottomTab('output')
        if (built && built.success === false) {
          const err = (built as { error?: string }).error || tRef.current('build.failed', { code: -1 })
          addOutput({ type: 'error', text: err, timestamp: Date.now(), source: 'build' })
          setIsBuilding(false)
          return
        }
      }
      setBottomTab('output')
      const result = await window.electronAPI!.startDebug(proj.dir, proj.type, {
        boardId: proj.boardId,
        filePath,
      })
      if (!result?.success && result?.error) {
        addOutput({ type: 'error', text: result.error, timestamp: Date.now(), source: 'build' })
      } else if (result?.success) {
        addOutput({ type: 'success', text: tRef.current('debug.done'), timestamp: Date.now(), source: 'build' })
      }
    } catch (e: unknown) {
      const msg = e instanceof Error ? e.message : String(e)
      addOutput({ type: 'error', text: msg, timestamp: Date.now(), source: 'build' })
    } finally {
      setIsBuilding(false)
    }
  }, [addOutput, saveAllDirty])

  const handleSave = useCallback(async (tabId?: string) => {
    const id = tabId || activeTabIdRef.current
    if (!id) return
    await saveTab(id)
  }, [saveTab])

  const handleSaveDirtyConfirm = useCallback(async () => {
    if (!dirtyConfirm) return
    const { tabId, callback } = dirtyConfirm
    const ok = await saveTab(tabId)
    if (!ok) return
    setDirtyConfirm(null)
    callback()
  }, [dirtyConfirm, saveTab])

  const handleAiFilesApplied = useCallback(async (relPaths: string[]) => {
    const proj = projectRef.current
    if (!proj || relPaths.length === 0) return
    await refreshFiles()
    for (const rel of relPaths) {
      const fullPath = `${proj.dir}/${rel}`
      try {
        const content = await window.electronAPI!.readProjectFile(proj.dir, fullPath)
        setOpenTabs(prev => {
          const existing = prev.find(t => t.id === fullPath)
          if (existing) {
            return prev.map(t => t.id === fullPath ? { ...t, content, dirty: false } : t)
          }
          return prev
        })
      } catch { /* ignore missing */ }
    }
  }, [refreshFiles])

  const handleDiscardDirtyConfirm = useCallback(() => {
    if (!dirtyConfirm) return
    const { callback } = dirtyConfirm
    setDirtyConfirm(null)
    callback()
  }, [dirtyConfirm])

  // Keep action refs in sync for menu/keyboard handlers
  useEffect(() => { handleOpenProjectRef.current = handleOpenProject }, [handleOpenProject])
  useEffect(() => { handleSaveRef.current = handleSave }, [handleSave])
  useEffect(() => { saveAllDirtyRef.current = saveAllDirty }, [saveAllDirty])
  useEffect(() => { handleBuildRef.current = handleBuild }, [handleBuild])
  useEffect(() => { handleFlashRef.current = handleFlash }, [handleFlash])
  useEffect(() => { handleDebugRef.current = handleDebug }, [handleDebug])
  useEffect(() => { handleTabCloseRef.current = handleTabClose }, [handleTabClose])

  // Auto-save dirty tabs
  useEffect(() => {
    if (!editorSettings.autoSave || !projectRef.current) return
    const dirty = openTabs.filter(t => t.dirty)
    if (dirty.length === 0) return
    const timer = window.setTimeout(() => {
      void saveAllDirtyRef.current()
    }, editorSettings.autoSaveDelayMs)
    return () => window.clearTimeout(timer)
  }, [openTabs, editorSettings.autoSave, editorSettings.autoSaveDelayMs])

  // Stable keyboard handler using refs
  useEffect(() => {
    const h = (e: KeyboardEvent) => {
      const c = e.ctrlKey || e.metaKey
      if (c && e.key === 's') { e.preventDefault(); handleSaveRef.current() }
      if (c && e.key === 'b' && !e.shiftKey) { e.preventDefault(); handleBuildRef.current() }
      if (c && e.shiftKey && e.key === 'B') { e.preventDefault(); handleFlashRef.current() }
      if (e.key === 'F5') { e.preventDefault(); handleDebugRef.current() }
      if (e.key === 'F7') { e.preventDefault(); handleBuildRef.current() }
      if (c && e.key === 'w' && activeTabIdRef.current) {
        e.preventDefault()
        handleTabCloseRef.current(activeTabIdRef.current)
      }
      if (c && e.key === 'Tab' && !e.shiftKey) {
        e.preventDefault()
        const tabs = openTabsRef.current
        if (tabs.length === 0) return
        const idx = tabs.findIndex(t => t.id === activeTabIdRef.current)
        const n = (idx + 1) % tabs.length
        setActiveTabId(tabs[n]?.id || '')
      }
      if (c && e.key === ',') { e.preventDefault(); setSettingsOpen(true) }
      if (c && e.key === 'n') { e.preventDefault(); setProjectDialogOpen(true) }
      if (c && e.key === 'o') { e.preventDefault(); handleOpenProjectRef.current() }
      if (c && e.key === 'm') { e.preventDefault(); setBottomTab(v => v === 'memory' ? null : 'memory') }
      if (c && e.key === 'p') { e.preventDefault(); setBottomTab(v => v === 'peripheral' ? null : 'peripheral') }
      if (c && e.shiftKey && e.key === 'E') { e.preventDefault(); setShowLeftPanel(v => !v) }
      if (c && e.shiftKey && e.key === 'A') {
        e.preventDefault()
        if (editorSettingsRef.current.aiEnabled) setShowRightPanel(v => !v)
      }
    }
    window.addEventListener('keydown', h)
    return () => window.removeEventListener('keydown', h)
  }, [])

  useEffect(() => { tRef.current = t }, [t])

  return (
    <ErrorBoundary>
      <TranslationProvider lang={editorSettings.language as LangCode}>
        <div className="app">
          <Toolbar
            projectName={project?.name}
            onBuild={handleBuild}
            onCancelBuild={handleCancelBuild}
            onFlash={handleFlash}
            isBuilding={isBuilding}
            onDebug={handleDebug}
            onOpenSettings={() => setSettingsOpen(true)}
            onNewProject={() => setProjectDialogOpen(true)}
            onSerial={() => setBottomTab(v => v === 'serial' ? null : 'serial')}
            onOpenProject={handleOpenProject}
            leftPanelVisible={showLeftPanel}
            rightPanelVisible={showRightPanel}
            aiEnabled={editorSettings.aiEnabled}
            onToggleLeftPanel={() => setShowLeftPanel(v => !v)}
            onToggleRightPanel={() => {
              if (editorSettings.aiEnabled) setShowRightPanel(v => !v)
              else setSettingsOpen(true)
            }}
          />
          <div className="app-body">
            <SlidePanel visible={showLeftPanel} side="left" width={260}>
              <FileExplorer
                files={projectFiles}
                projectDir={project?.dir}
                projectType={project?.type}
                onFileSelect={handleFileSelect}
                onFilesChange={setProjectFiles}
                activeFileId={activeTabId}
                onNewFile={handleNewFile}
                onNewFolder={handleNewFolder}
                onNewScript={handleNewScript}
                onDelete={handleDelete}
                onRename={handleRename}
                onOpenProject={handleOpenProject}
                onNewProject={() => setProjectDialogOpen(true)}
              />
            </SlidePanel>
            <div className="app-center">
              <Editor
                tabs={openTabs}
                activeTabId={activeTabId}
                onTabSelect={setActiveTabId}
                onTabClose={handleTabClose}
                onContentChange={handleContentChange}
                onCursorChange={handleCursorChange}
                onSave={handleSave}
                editorSettings={editorSettings}
                fileDiagnostics={activeFileDiagnostics}
              />
              <SlidePanel visible={bottomTab != null} side="bottom" height={240}>
                <div className="app-bottom-panels">
                  <div className="app-bottom-tabs">
                    <button className={`app-bottom-tab ${bottomTab === 'output' ? 'active' : ''}`} onClick={() => setBottomTab('output')}>{t('output.title')}</button>
                    <button className={`app-bottom-tab ${bottomTab === 'serial' ? 'active' : ''}`} onClick={() => setBottomTab('serial')}>{t('serial.title')}</button>
                    <button className={`app-bottom-tab ${bottomTab === 'memory' ? 'active' : ''}`} onClick={() => setBottomTab('memory')}>{t('memory.title')}</button>
                    <button className={`app-bottom-tab ${bottomTab === 'peripheral' ? 'active' : ''}`} onClick={() => setBottomTab('peripheral')}>{t('peripheral.title')}</button>
                  </div>
                  <div className="app-bottom-body">
                    {bottomTab === 'memory' && (
                      <MemoryAnalyzer
                        flashUsed={memoryUsage.flashUsed}
                        flashTotal={memoryUsage.flashTotal}
                        ramUsed={memoryUsage.ramUsed}
                        ramTotal={memoryUsage.ramTotal}
                      />
                    )}
                    {bottomTab === 'peripheral' && (
                      <PeripheralViewer
                        boardName={project?.boardName}
                        peripheralNames={project?.peripherals || []}
                      />
                    )}
                    <div className="app-bottom-serial" style={{ display: bottomTab === 'serial' ? undefined : 'none' }}>
                      <SerialMonitor defaultBaud={editorSettings.defaultBaud} />
                    </div>
                    {bottomTab === 'output' && (
                      <OutputPanel messages={outputMessages} onClose={() => setBottomTab(null)} />
                    )}
                  </div>
                </div>
              </SlidePanel>
            </div>
            {editorSettings.aiEnabled && (
              <SlidePanel visible={showRightPanel} side="right" width={320}>
                <AIAgents project={project} files={projectFiles} settings={editorSettings} onSettingsChange={setEditorSettings} onFilesApplied={handleAiFilesApplied} />
              </SlidePanel>
            )}
          </div>
          {editorSettings.showStatusBar !== false && (
            <StatusBar
              line={cursorPos.line}
              col={cursorPos.col}
              language={openTabs.find(t => t.id === activeTabId)?.language ?? ''}
              projectType={project?.type}
              boardName={project?.boardName}
              toolchains={toolchains}
            />
          )}
          {settingsOpen && (
            <Settings
              editorSettings={editorSettings}
              onEditorSettingsChange={s => setEditorSettings(normalizeEditorSettings(s))}
              onClose={() => setSettingsOpen(false)}
            />
          )}
          {projectDialogOpen && <ProjectDialog onCreate={handleCreateProject} onClose={() => setProjectDialogOpen(false)} />}

          {fileDialog && (
            <FileDialog
              mode={fileDialog.mode}
              initialName={fileDialog.mode === 'rename' ? fileDialog.node?.name : undefined}
              parentDir={fileDialog.mode !== 'rename' ? fileDialog.parentDir : undefined}
              projectType={project?.type}
              onSubmit={handleFileDialogSubmit}
              onClose={() => setFileDialog(null)}
            />
          )}

          {confirmDelete && (
            <ConfirmDialog
              title={t('confirmDialog.deleteTitle')}
              message={t('confirmDialog.deleteMsg', { name: confirmDelete.name })}
              onConfirm={handleDeleteConfirm}
              onCancel={() => setConfirmDelete(null)}
            />
          )}

          {dirtyConfirm && (
            <div className="settings-overlay" onClick={() => setDirtyConfirm(null)}>
              <div className="dirty-dialog">
                <div className="settings-header">
                  <span className="settings-title">{t('confirmDialog.unsavedTitle')}</span>
                </div>
                <div className="dirty-body">
                  <p>{t('confirmDialog.unsavedMsg')}</p>
                  <p className="dirty-hint">{t('confirmDialog.unsavedHint')}</p>
                </div>
                <div className="dirty-footer">
                  <button className="project-btn project-btn-cancel" onClick={() => setDirtyConfirm(null)}>{t('common.cancel')}</button>
                  <button className="project-btn" onClick={handleDiscardDirtyConfirm} style={{ border: '1px solid var(--border)', color: 'var(--text)' }}>{t('common.dontSave')}</button>
                  <button className="project-btn project-btn-create" onClick={handleSaveDirtyConfirm}>{t('common.save')}</button>
                </div>
              </div>
            </div>
          )}
        </div>
      </TranslationProvider>
    </ErrorBoundary>
  )
}

export default function App() {
  return (
    <ThemeProvider>
      <AppContent />
    </ThemeProvider>
  )
}
