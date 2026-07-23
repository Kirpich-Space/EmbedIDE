import { useState, useCallback, useEffect, useRef, useMemo, Component, type ReactNode } from 'react'
import { ThemeProvider } from './themes/ThemeProvider'
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
import type { LangCode } from './core/translations'

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
  const [project, setProject] = useState<ProjectConfig | null>(null)
  const [projectFiles, setProjectFiles] = useState<FileNode[]>([])
  const [openTabs, setOpenTabs] = useState<EditorTabData[]>([])
  const [activeTabId, setActiveTabId] = useState('')
  const [outputVisible, setOutputVisible] = useState(false)
  const [outputMessages, setOutputMessages] = useState<BuildMessage[]>([])
  const [settingsOpen, setSettingsOpen] = useState(false)
  const [projectDialogOpen, setProjectDialogOpen] = useState(false)
  const [serialVisible, setSerialVisible] = useState(false)
  const [memoryVisible, setMemoryVisible] = useState(false)
  const [peripheralVisible, setPeripheralVisible] = useState(false)
  const [toolchains, setToolchains] = useState<ToolchainInfo | null>(null)
  const [memoryUsage, setMemoryUsage] = useState<MemoryUsage>({ flashUsed: 0, flashTotal: 1048576, ramUsed: 0, ramTotal: 131072 })
  const [showLeftPanel, setShowLeftPanel] = useState(true)
  const [showRightPanel, setShowRightPanel] = useState(true)
  const [dirtyConfirm, setDirtyConfirm] = useState<{ tabId: string; callback: () => void } | null>(null)
  const [isBuilding, setIsBuilding] = useState(false)
  const [editorSettings, setEditorSettings] = useState<EditorSettings>(() => {
    try { return JSON.parse(localStorage.getItem('embed-ide-editor-settings') || '') }
    catch { return { fontSize: 14, tabSize: 4, fontFamily: "'JetBrains Mono', monospace", wordWrap: false, minimap: false, lineNumbers: true, cursorBlinkRate: 1200, smoothScroll: true, bracketMatch: true, language: 'en', theme: 'dark' } }
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
  const tRef = useRef(t)

  // File dialog state
  const [fileDialog, setFileDialog] = useState<{
    mode: 'create-file' | 'create-folder' | 'rename'
    node?: FileNode
    parentDir?: string
  } | null>(null)

  // Confirm delete state
  const [confirmDelete, setConfirmDelete] = useState<FileNode | null>(null)

  const openTabsRef = useRef(openTabs)
  const activeTabIdRef = useRef(activeTabId)
  const projectRef = useRef(project)
  openTabsRef.current = openTabs
  activeTabIdRef.current = activeTabId
  projectRef.current = project

  useEffect(() => {
    window.electronAPI?.detectToolchains().then(setToolchains)
    window.electronAPI?.loadSettings().then(saved => {
      if (saved && Object.keys(saved).length > 0 && !localStorage.getItem('embed-ide-editor-settings')) {
        setEditorSettings(prev => ({ ...prev, ...saved }))
      }
    })
  }, [])

  useEffect(() => {
    localStorage.setItem('embed-ide-editor-settings', JSON.stringify(editorSettings))
    window.electronAPI?.saveSettings(editorSettings)
  }, [editorSettings])

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
      if (data.code === 0) {
        setOutputMessages(prev => [...prev, { type: 'success', text: tRef.current('build.success'), timestamp: Date.now(), source: 'build' }])
        setMemoryVisible(true)
      } else {
        setOutputMessages(prev => [...prev, { type: 'error', text: data.error || tRef.current('build.failed', { code: data.code }), timestamp: Date.now(), source: 'build' }])
      }
    })

    const unsubFlashOut = api.onFlashOutput((data) => {
      setOutputMessages(prev => [...prev, { type: 'info', text: data.text, timestamp: Date.now(), source: 'flash' }])
    })

    const unsubFlashComplete = api.onFlashComplete((data) => {
      setOutputMessages(prev => [...prev, data.code === 0
        ? { type: 'success', text: tRef.current('flash.success'), timestamp: Date.now(), source: 'flash' }
        : { type: 'error', text: data.error || tRef.current('flash.failed', { code: data.code }), timestamp: Date.now(), source: 'flash' }
      ])
    })

    return () => {
      unsubBuildOut()
      unsubBuildComplete()
      unsubFlashOut()
      unsubFlashComplete()
    }
  }, [])

  // Menu events
  useEffect(() => {
    const api = window.electronAPI
    if (!api) return
    const unsubs = [
      api.onMenuNewProject(() => setProjectDialogOpen(true)),
      api.onMenuOpenProject(() => handleOpenProject()),
      api.onMenuSave(() => handleSave()),
      api.onMenuSaveAll(() => saveAllDirty()),
      api.onMenuSettings(() => setSettingsOpen(true)),
      api.onMenuFind(() => {
        const cm = document.querySelector('.cm-editor') as HTMLElement
        cm?.focus()
      }),
      api.onMenuToggleExplorer(() => setShowLeftPanel(v => !v)),
      api.onMenuToggleAgents(() => setShowRightPanel(v => !v)),
      api.onMenuBuild(() => handleBuild()),
      api.onMenuFlash(() => handleFlash()),
    ]
    return () => unsubs.forEach(u => u())
  }, [])

  const addOutput = useCallback((msg: BuildMessage) => {
    setOutputMessages(prev => [...prev, msg])
    setOutputVisible(true)
  }, [])

  const saveTab = useCallback(async (tabId: string) => {
    const tabs = openTabsRef.current
    const proj = projectRef.current
    const tab = tabs.find(t => t.id === tabId)
    if (!tab || !tab.dirty || !proj) return false
    try {
      await window.electronAPI!.writeProjectFile(tabId, tab.content)
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

  const loadProject = useCallback(async (dir: string, name: string, type: string) => {
    setProject({ dir, name, type })
    const files = await window.electronAPI!.listProjectFiles(dir)
    setProjectFiles(files)
    setOpenTabs([])
    setActiveTabId('')
    setOutputMessages(prev => [...prev, { type: 'info', text: tRef.current('fileOps.opened', { name, type }), timestamp: Date.now(), source: 'build' }])
  }, [addOutput])

  const handleCreateProject = useCallback(async (name: string, type: string) => {
    const projectsDir = await window.electronAPI!.getDefaultProjectsDir()
    const dir = await window.electronAPI!.createProject(projectsDir, name, type)
    setProjectDialogOpen(false)
    await loadProject(dir, name, type)
  }, [loadProject])

  const handleOpenProject = useCallback(async () => {
    const result = await window.electronAPI!.openProject()
    if (!result) return
    await saveAllDirty()
    await loadProject(result.dir, result.name, result.type)
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
      const content = await window.electronAPI!.readProjectFile(filePath)
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
    setOpenTabs(prev => prev.map(t => t.id === tabId ? { ...t, content, dirty: true } : t))
  }, [])

  // File dialog handlers
  const handleNewFile = useCallback((parentDir?: string) => {
    setFileDialog({ mode: 'create-file', parentDir })
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
      const ok = await window.electronAPI!.renameProjectFile(oldPath, newPath)
      if (!ok) throw new Error(`${newRelPath} already exists`)

      setOpenTabs(prev => prev.map(t => t.id === oldPath ? { ...t, id: newPath, name } : t))
      if (active === oldPath) setActiveTabId(newPath)
      setFileDialog(null)
      await refreshFiles()
      addOutput({ type: 'info', text: tRef.current('fileOps.renamed', { name: newRelPath }), timestamp: Date.now(), source: 'build' })
      return
    }

    const relPath = fileDialog.parentDir ? `${fileDialog.parentDir}/${name}` : name
    const actualPath = fileDialog.mode === 'create-folder' ? relPath + '/' : relPath

    const ok = await window.electronAPI!.createProjectFile(proj.dir, actualPath)
    if (!ok) throw new Error(`${relPath} already exists`)

    setFileDialog(null)
    await refreshFiles()

    if (fileDialog.mode !== 'create-folder') {
      const filePath = `${proj.dir}/${relPath}`
      const ext = name.includes('.') ? name.split('.').pop() : ''
      openFileTab({ id: filePath, name, language: ext }, '')
      setActiveTabId(filePath)
    }
    addOutput({ type: 'info', text: tRef.current('fileOps.created', { name: relPath }), timestamp: Date.now(), source: 'build' })
  }, [projectRef, fileDialog, refreshFiles, openFileTab, addOutput])

  const handleDelete = useCallback((node: FileNode) => {
    setConfirmDelete(node)
  }, [])

  const handleDeleteConfirm = useCallback(async () => {
    const node = confirmDelete
    const proj = projectRef.current
    const active = activeTabIdRef.current
    if (!node || !proj) return
    const fullPath = `${proj.dir}/${node.id}`
    await window.electronAPI!.deleteProjectFile(fullPath)
    setOpenTabs(prev => {
      const next = prev.filter(t => t.id !== fullPath)
      if (next.length === 0) { setActiveTabId(''); return [] }
      if (active === fullPath) setActiveTabId(next[next.length - 1].id)
      return next
    })
    setConfirmDelete(null)
    await refreshFiles()
    addOutput({ type: 'info', text: tRef.current('fileOps.deleted', { name: node.name }), timestamp: Date.now(), source: 'build' })
  }, [confirmDelete, refreshFiles, addOutput])

  const handleBuild = useCallback(async () => {
    const proj = projectRef.current
    if (!proj) { addOutput({ type: 'warn', text: tRef.current('build.noProject'), timestamp: Date.now(), source: 'build' }); return }
    await saveAllDirty()
    setOutputMessages(prev => [...prev, { type: 'info', text: `Building ${proj.name}...`, timestamp: Date.now(), source: 'build' }])
    setOutputVisible(true)
    setMemoryVisible(false)
    setIsBuilding(true)
    try {
      await window.electronAPI!.buildProject(proj.dir, proj.type)
    } catch (e: any) {
      addOutput({ type: 'error', text: tRef.current('build.error', { msg: e.message }), timestamp: Date.now(), source: 'build' })
      setIsBuilding(false)
    }
  }, [addOutput, saveAllDirty])

  const handleCancelBuild = useCallback(async () => {
    await window.electronAPI?.cancelBuild()
    setIsBuilding(false)
    addOutput({ type: 'warn', text: tRef.current('build.cancelled'), timestamp: Date.now(), source: 'build' })
  }, [addOutput])

  const handleFlash = useCallback(async () => {
    const proj = projectRef.current
    if (!proj) { addOutput({ type: 'warn', text: tRef.current('flash.noProject'), timestamp: Date.now(), source: 'flash' }); return }
    await saveAllDirty()
    setOutputVisible(true)
    setSerialVisible(false)
    addOutput({ type: 'info', text: tRef.current('flash.starting'), timestamp: Date.now(), source: 'flash' })
    try { await window.electronAPI!.flashProject(proj.dir, proj.type, { adapter: 'stlink', target: 'stm32f4x' }) }
    catch (e: any) { addOutput({ type: 'error', text: tRef.current('flash.error', { msg: e.message }), timestamp: Date.now(), source: 'flash' }) }
  }, [addOutput, saveAllDirty])

  const handleSave = useCallback(async (tabId?: string) => {
    const id = tabId || activeTabIdRef.current
    if (!id) return
    await saveTab(id)
  }, [saveTab])

  const handleSaveDirtyConfirm = useCallback(async () => {
    if (!dirtyConfirm) return
    await saveTab(dirtyConfirm.tabId)
    dirtyConfirm.callback()
  }, [dirtyConfirm, saveTab])

  const handleDiscardDirtyConfirm = useCallback(() => {
    dirtyConfirm?.callback()
  }, [dirtyConfirm])

  // Stable keyboard handler using refs
  useEffect(() => {
    const h = (e: KeyboardEvent) => {
      const c = e.ctrlKey || e.metaKey
      if (c && e.key === 's') { e.preventDefault(); handleSave() }
      if (c && e.key === 'b' && !e.shiftKey) { e.preventDefault(); handleBuild() }
      if (c && e.shiftKey && e.key === 'B') { e.preventDefault(); handleFlash() }
      if (c && e.key === 'w' && activeTabIdRef.current) {
        e.preventDefault()
        handleTabClose(activeTabIdRef.current)
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
      if (c && e.key === 'o') { e.preventDefault(); handleOpenProject() }
      if (c && e.key === 'm') { e.preventDefault(); setMemoryVisible(v => !v) }
      if (c && e.key === 'p') { e.preventDefault(); setPeripheralVisible(v => !v) }
      if (c && e.shiftKey && e.key === 'E') { e.preventDefault(); setShowLeftPanel(v => !v) }
      if (c && e.shiftKey && e.key === 'A') { e.preventDefault(); setShowRightPanel(v => !v) }
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
            onDebug={() => addOutput({ type: 'info', text: t('common.info'), timestamp: Date.now(), source: 'build' })}
            onOpenSettings={() => setSettingsOpen(true)}
            onNewProject={() => setProjectDialogOpen(true)}
            onSerial={() => setSerialVisible(v => !v)}
            onOpenProject={handleOpenProject}
            leftPanelVisible={showLeftPanel}
            rightPanelVisible={showRightPanel}
            onToggleLeftPanel={() => setShowLeftPanel(v => !v)}
            onToggleRightPanel={() => setShowRightPanel(v => !v)}
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
                onSave={handleSave}
                editorSettings={editorSettings}
              />
              <SlidePanel visible={memoryVisible || peripheralVisible || serialVisible || outputVisible} side="bottom" height={200}>
                <div className="app-bottom-panels">
                  {memoryVisible && (
                    <MemoryAnalyzer
                      flashUsed={memoryUsage.flashUsed}
                      flashTotal={memoryUsage.flashTotal}
                      ramUsed={memoryUsage.ramUsed}
                      ramTotal={memoryUsage.ramTotal}
                    />
                  )}
                  {peripheralVisible && <PeripheralViewer peripherals={[]} />}
                  {serialVisible && <SerialMonitor />}
                  {outputVisible && !serialVisible && (
                    <OutputPanel messages={outputMessages} onClose={() => setOutputVisible(false)} />
                  )}
                </div>
              </SlidePanel>
            </div>
            <SlidePanel visible={showRightPanel} side="right" width={320}>
              <AIAgents project={project} files={projectFiles} />
            </SlidePanel>
          </div>
          <StatusBar
            line={openTabs.find(t => t.id === activeTabId)?.cursorLine ?? 1}
            col={openTabs.find(t => t.id === activeTabId)?.cursorCol ?? 1}
            language={openTabs.find(t => t.id === activeTabId)?.language ?? ''}
            projectType={project?.type}
            toolchains={toolchains}
          />
          {settingsOpen && <Settings editorSettings={editorSettings} onEditorSettingsChange={setEditorSettings} onClose={() => setSettingsOpen(false)} />}
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
