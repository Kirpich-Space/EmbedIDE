import { useMemo, useState, useCallback, useRef, useEffect } from 'react'
import type { FileNode, DropdownItem } from '../core/types'
import { fileIcons } from '../core/utils'
import { useTranslation } from '../core/TranslationContext'

interface ContextMenuState {
  x: number
  y: number
  node: FileNode | null
}

interface FileExplorerProps {
  files: any[]
  projectDir?: string
  projectType?: string
  onFileSelect: (node: FileNode) => void
  onFilesChange?: (files: any[]) => void
  activeFileId: string | null
  onNewFile?: (parentDir?: string) => void
  onNewFolder?: (parentDir?: string) => void
  onDelete?: (node: FileNode) => void
  onRename?: (node: FileNode) => void
  onOpenProject?: () => void
  onNewProject?: () => void
}

export function FileExplorer({
  files, projectDir, projectType, onFileSelect, onFilesChange,
  activeFileId, onNewFile, onNewFolder, onDelete, onRename,
  onOpenProject, onNewProject
}: FileExplorerProps) {
  const [expandedDirs, setExpandedDirs] = useState<Set<string>>(new Set())
  const [contextMenu, setContextMenu] = useState<ContextMenuState | null>(null)
  const [searchQuery, setSearchQuery] = useState('')
  const [showSearch, setShowSearch] = useState(false)
  const { t } = useTranslation()
  const ctxRef = useRef<HTMLDivElement>(null)

  useEffect(() => {
    if (projectDir && expandedDirs.size === 0) {
      setExpandedDirs(new Set([projectDir.split('/').pop() || 'project']))
    }
  }, [projectDir])

  useEffect(() => {
    const close = () => setContextMenu(null)
    window.addEventListener('click', close)
    return () => window.removeEventListener('click', close)
  }, [])

  const tree = useMemo(() => {
    const nodeMap = new Map<string, any[]>()
    for (const item of files) {
      const parts = item.id.split('/')
      if (parts.length > 1) {
        const parentDir = parts.slice(0, -1).join('/')
        if (!nodeMap.has(parentDir)) nodeMap.set(parentDir, [])
        nodeMap.get(parentDir)!.push(item)
      }
    }

    const getChildren = (parentId: string): FileNode[] => {
      const children = nodeMap.get(parentId) || []
      const dirs = children.filter(i => i.type === 'directory')
      const fileChildren = children.filter(i => i.type === 'file')
      return [
        ...dirs.map(d => ({ id: d.id, name: d.name, type: 'directory' as const, children: getChildren(d.id) })),
        ...fileChildren.map(f => ({ id: f.id, name: f.name, type: 'file' as const, language: f.language })),
      ]
    }

    const roots = files.filter(i => !i.id.includes('/'))
    return roots.map(r => {
      if (r.type === 'directory') {
        return { id: r.id, name: r.name, type: 'directory' as const, children: getChildren(r.id) }
      }
      return { id: r.id, name: r.name, type: 'file' as const, language: r.language }
    })
  }, [files])

  // Auto-expand directories that contain search-matching files
  useEffect(() => {
    if (!searchQuery) return
    const dirsToExpand = new Set(expandedDirs)
    const walk = (nodes: FileNode[], parents: string[]) => {
      for (const node of nodes) {
        if (node.type === 'directory') {
          walk(node.children || [], [...parents, node.id])
        } else if (node.name.toLowerCase().includes(searchQuery.toLowerCase())) {
          parents.forEach(p => dirsToExpand.add(p))
        }
      }
    }
    walk(tree, [])
    setExpandedDirs(dirsToExpand)
  }, [searchQuery, tree])

  const filteredTree = useMemo(() => {
    if (!searchQuery) return tree
    const filterTree = (nodes: FileNode[]): FileNode[] => {
      return nodes.reduce<FileNode[]>((acc, node) => {
        const nameMatch = node.name.toLowerCase().includes(searchQuery.toLowerCase())
        if (node.type === 'directory') {
          const children = filterTree(node.children || [])
          if (nameMatch || children.length > 0) {
            acc.push({ ...node, children })
          }
        } else if (nameMatch) {
          acc.push(node)
        }
        return acc
      }, [])
    }
    return filterTree(tree)
  }, [tree, searchQuery])

  const toggleDir = useCallback((id: string) => {
    setExpandedDirs(prev => {
      const next = new Set(prev)
      if (next.has(id)) next.delete(id)
      else next.add(id)
      return next
    })
  }, [])

  const clampToViewport = (x: number, y: number, w: number, h: number) => {
    const cx = Math.min(x, window.innerWidth - w - 8)
    const cy = Math.min(y, window.innerHeight - h - 8)
    return { x: Math.max(8, cx), y: Math.max(8, cy) }
  }

  const handleContextMenu = useCallback((e: React.MouseEvent, node: FileNode) => {
    e.preventDefault()
    e.stopPropagation()
    const pos = clampToViewport(e.clientX, e.clientY, 180, 160)
    setContextMenu({ x: pos.x, y: pos.y, node })
  }, [])

  const handleTreeContextMenu = useCallback((e: React.MouseEvent) => {
    if (!projectDir) return
    e.preventDefault()
    const pos = clampToViewport(e.clientX, e.clientY, 180, 100)
    setContextMenu({ x: pos.x, y: pos.y, node: null })
  }, [projectDir])

  const ctxActions = useMemo((): DropdownItem[] => {
    if (!contextMenu) return []
    const node = contextMenu.node
    if (!node) {
      return [
        { label: t('fileExplorer.newFile'), action: () => { onNewFile?.(); setContextMenu(null) } },
        { label: t('fileExplorer.newFolder'), action: () => { onNewFolder?.(); setContextMenu(null) } },
      ]
    }
    return [
      { label: t('fileExplorer.newFile'), action: () => { onNewFile?.(node.type === 'directory' ? node.id : undefined); setContextMenu(null) } },
      { label: t('fileExplorer.newFolder'), action: () => { onNewFolder?.(node.type === 'directory' ? node.id : undefined); setContextMenu(null) } },
      { label: '', action: () => {}, separator: true },
      { label: t('fileExplorer.rename'), shortcut: 'F2', action: () => { onRename?.(node); setContextMenu(null) } },
      { label: t('fileExplorer.delete'), shortcut: 'Del', action: () => { onDelete?.(node); setContextMenu(null) } },
    ]
  }, [contextMenu, onNewFile, onNewFolder, onRename, onDelete])

  return (
    <div className="file-explorer">
      <div className="panel-header">
        <span className="panel-title">{t('fileExplorer.title')}</span>
        <div className="panel-header-actions">
          {projectDir && (
            <>
              <button className="panel-icon-btn" onClick={() => onNewFile?.()} title="New File">
                <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.5" strokeLinecap="round"><path d="M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8z"/><polyline points="14 2 14 8 20 8"/><line x1="12" y1="18" x2="12" y2="12"/><line x1="9" y1="15" x2="15" y2="15"/></svg>
              </button>
              <button className="panel-icon-btn" onClick={() => onNewFolder?.()} title="New Folder">
                <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.5" strokeLinecap="round"><path d="M22 19a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h5l2 3h9a2 2 0 0 1 2 2z"/><line x1="12" y1="11" x2="12" y2="17"/><line x1="9" y1="14" x2="15" y2="14"/></svg>
              </button>
            </>
          )}
          <button className="panel-icon-btn" onClick={() => setShowSearch(v => !v)} title={t('fileExplorer.searchFiles')}>
            <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.5"><circle cx="11" cy="11" r="8"/><line x1="21" y1="21" x2="16.65" y2="16.65"/></svg>
          </button>
          {projectDir && <span className="panel-dir">.../{projectDir.split('/').pop()}</span>}
        </div>
      </div>

      {showSearch && (
        <div className="file-search">
          <input
            className="file-search-input"
            type="text"
            placeholder={t('fileExplorer.searchPlaceholder')}
            value={searchQuery}
            onChange={e => setSearchQuery(e.target.value)}
            autoFocus
            onKeyDown={e => e.key === 'Escape' && setShowSearch(false)}
          />
          {searchQuery && <button className="file-search-clear" onClick={() => setSearchQuery('')}>×</button>}
        </div>
      )}

      <div className="file-tree" onContextMenu={handleTreeContextMenu}>
        {filteredTree.length === 0 && (
          <div className="file-empty">
            {searchQuery ? (
              <div className="file-empty-text">{t('fileExplorer.noMatch')}</div>
            ) : projectDir ? (
              <>
                <div className="file-empty-icon">📂</div>
                <div className="file-empty-text">{t('fileExplorer.emptyProject')}</div>
                <div className="file-empty-actions">
                  <button className="file-empty-btn" onClick={() => onNewFile?.()}>
                    <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.5" strokeLinecap="round"><path d="M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8z"/><polyline points="14 2 14 8 20 8"/><line x1="12" y1="18" x2="12" y2="12"/><line x1="9" y1="15" x2="15" y2="15"/></svg>
                    {t('fileExplorer.newFile')}
                  </button>
                  <button className="file-empty-btn" onClick={() => onNewFolder?.()}>
                    <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.5" strokeLinecap="round"><path d="M22 19a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h5l2 3h9a2 2 0 0 1 2 2z"/><line x1="12" y1="11" x2="12" y2="17"/><line x1="9" y1="14" x2="15" y2="14"/></svg>
                    {t('fileExplorer.newFolder')}
                  </button>
                </div>
              </>
            ) : (
              <>
                <div className="file-empty-icon">📁</div>
                <div className="file-empty-text">{t('fileExplorer.noProject')}</div>
                <div className="file-empty-actions">
                  <button className="file-empty-btn" onClick={onNewProject}>
                    <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.5" strokeLinecap="round"><path d="M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8z"/><polyline points="14 2 14 8 20 8"/><line x1="12" y1="18" x2="12" y2="12"/><line x1="9" y1="15" x2="15" y2="15"/></svg>
                    {t('fileExplorer.newProject')}
                  </button>
                  <button className="file-empty-btn" onClick={onOpenProject}>
                    <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.5" strokeLinecap="round"><path d="M22 19a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h5l2 3h9a2 2 0 0 1 2 2z"/></svg>
                    {t('fileExplorer.openProject')}
                  </button>
                </div>
                <div className="file-empty-hint">{t('fileExplorer.shortcuts')}</div>
              </>
            )}
          </div>
        )}
        <TreeView
          nodes={filteredTree}
          depth={0}
          expandedDirs={expandedDirs}
          onToggleDir={toggleDir}
          onSelect={onFileSelect}
          selectedId={activeFileId}
          projectDir={projectDir}
          onContextMenu={handleContextMenu}
        />
      </div>

      {contextMenu && (
        <div
          ref={ctxRef}
          className="ctx-menu"
          style={{ left: contextMenu.x, top: contextMenu.y }}
        >
          {ctxActions.map((item, i) =>
            item.separator ? (
              <div key={i} className="ctx-separator" />
            ) : (
              <div key={i} className="ctx-item" onClick={item.action}>
                <span>{item.label}</span>
                {item.shortcut && <span className="ctx-shortcut">{item.shortcut}</span>}
              </div>
            )
          )}
        </div>
      )}
    </div>
  )
}

interface TreeViewProps {
  nodes: FileNode[]
  depth: number
  expandedDirs: Set<string>
  onToggleDir: (id: string) => void
  onSelect: (node: FileNode) => void
  selectedId: string | null
  projectDir?: string
  onContextMenu?: (e: React.MouseEvent, node: FileNode) => void
}

function TreeView({ nodes, depth, expandedDirs, onToggleDir, onSelect, selectedId, projectDir, onContextMenu }: TreeViewProps) {
  return (
    <>
      {nodes.map(node => {
        const isDir = node.type === 'directory'
        const expanded = expandedDirs.has(node.id)
        const fullId = projectDir ? `${projectDir}/${node.id}` : node.id
        const isSelected = selectedId === fullId

        return (
          <div key={node.id}>
            <div
              className={`file-item ${isSelected ? 'file-item-active' : ''}`}
              style={{ paddingLeft: 8 + depth * 16 }}
              onClick={() => isDir ? onToggleDir(node.id) : onSelect(node)}
              onContextMenu={e => onContextMenu?.(e, node)}
              title={node.id}
            >
              <span className="file-icon">
                {isDir ? (expanded ? '📂' : '📁') : (fileIcons[node.language ?? ''] ?? '📄')}
              </span>
              <span className="file-name">{node.name}</span>
              {!isDir && node.language && <span className="file-lang">{node.language}</span>}
            </div>
            {isDir && expanded && node.children && (
              <TreeView
                nodes={node.children}
                depth={depth + 1}
                expandedDirs={expandedDirs}
                onToggleDir={onToggleDir}
                onSelect={onSelect}
                selectedId={selectedId}
                projectDir={projectDir}
                onContextMenu={onContextMenu}
              />
            )}
          </div>
        )
      })}
    </>
  )
}
