import { useState, useEffect, useRef, useCallback, useMemo } from 'react'
import { useTranslation } from '../core/TranslationContext'
import type { BoardInfo } from '../core/types'
import { SelectionMark } from './CheckIcon'

interface ProjectDialogProps {
  onCreate: (name: string, type: string, boardId: string) => void
  onClose: () => void
}

const defaultName = 'flight-computer'

const TYPE_ICONS: Record<string, string> = {
  rust: 'Rs',
  c: 'C',
  cpp: 'C++',
  asm: 'Asm',
  zig: 'Zig',
  'driver-c': 'C',
  'driver-cpp': 'C++',
  'driver-rust': 'Rs',
  'driver-asm': 'Asm',
  'driver-zig': 'Zig',
  'os-c': 'C',
  'os-cpp': 'C++',
  'os-rust': 'Rs',
  'os-asm': 'Asm',
  'os-zig': 'Zig',
  'script-python': 'Py',
  'script-bash': 'Sh',
  'script-js': 'JS',
}

const CATEGORY_ORDER = ['firmware', 'driver', 'os', 'script'] as const

type TemplateItem = {
  id: string
  name: string
  ext: string
  category?: string
  needsBoard?: boolean
  lang?: string
}

export function ProjectDialog({ onCreate, onClose }: ProjectDialogProps) {
  const { t } = useTranslation()
  const [name, setName] = useState(defaultName)
  const [type, setType] = useState('c')
  const [boardId, setBoardId] = useState('stm32f407vg')
  const [boardFilter, setBoardFilter] = useState('')
  const [templates, setTemplates] = useState<TemplateItem[]>([])
  const [boards, setBoards] = useState<BoardInfo[]>([])
  const [projectsDir, setProjectsDir] = useState('')
  const [creating, setCreating] = useState(false)
  const [error, setError] = useState('')
  const inputRef = useRef<HTMLInputElement>(null)
  const creatingRef = useRef(false)

  useEffect(() => {
    window.electronAPI?.getProjectTemplates().then(list => {
      setTemplates(list as TemplateItem[])
      if (list.length && !list.find(x => x.id === 'c')) {
        setType(list[0].id)
      }
    })
    window.electronAPI?.listBoards()
      .then(list => {
        const next = Array.isArray(list) ? list : []
        setBoards(next)
        if (next.length && !next.find(b => b.id === boardId)) {
          setBoardId(next[0].id)
        }
      })
      .catch(() => setBoards([]))
    window.electronAPI?.getDefaultProjectsDir().then(setProjectsDir)
    inputRef.current?.select()
  }, [])

  const selectedTemplate = templates.find(t => t.id === type)
  const needsBoard = selectedTemplate ? selectedTemplate.needsBoard !== false : true

  const grouped = useMemo(() => {
    const map = new Map<string, TemplateItem[]>()
    for (const tmpl of templates) {
      const cat = tmpl.category || 'firmware'
      const list = map.get(cat) || []
      list.push(tmpl)
      map.set(cat, list)
    }
    return CATEGORY_ORDER
      .filter(c => map.has(c))
      .map(c => [c, map.get(c)!] as const)
  }, [templates])

  const families = useMemo(() => {
    const q = boardFilter.trim().toLowerCase()
    const filtered = boards.filter(b =>
      !q ||
      b.name.toLowerCase().includes(q) ||
      b.family.toLowerCase().includes(q) ||
      b.mcu.toLowerCase().includes(q) ||
      b.id.includes(q)
    )
    const map = new Map<string, BoardInfo[]>()
    for (const b of filtered) {
      const list = map.get(b.family) || []
      list.push(b)
      map.set(b.family, list)
    }
    return [...map.entries()]
  }, [boards, boardFilter])

  const selectedBoard = boards.find(b => b.id === boardId)

  const handleCreate = useCallback(async () => {
    if (!name.trim() || creatingRef.current || !boardId) return
    creatingRef.current = true
    setCreating(true)
    setError('')
    try {
      await onCreate(name.trim(), type, boardId)
    } catch (e: unknown) {
      setError(e instanceof Error ? e.message : 'Failed to create project')
    } finally {
      creatingRef.current = false
      setCreating(false)
    }
  }, [name, type, boardId, onCreate])

  const handleKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === 'Enter') { e.preventDefault(); handleCreate() }
    if (e.key === 'Escape') onClose()
  }

  const categoryLabel = (cat: string) => {
    if (cat === 'firmware') return t('projectDialog.catFirmware')
    if (cat === 'driver') return t('projectDialog.catDrivers')
    if (cat === 'os') return t('projectDialog.catOs')
    if (cat === 'script') return t('projectDialog.catScript')
    return cat
  }

  return (
    <div className="settings-overlay" onClick={e => { if (e.target === e.currentTarget) onClose() }}>
      <div className="project-dialog project-dialog-wide">
        <div className="settings-header">
          <span className="settings-title">{t('projectDialog.title')}</span>
          <button className="settings-close" onClick={onClose}>×</button>
        </div>

        <div className="project-body">
          <div className="settings-field">
            <label className="settings-label">{t('projectDialog.projectName')}</label>
            <input
              ref={inputRef}
              className="project-input"
              value={name}
              onChange={e => setName(e.target.value)}
              onFocus={e => e.target.select()}
              onKeyDown={handleKeyDown}
              placeholder={defaultName}
              autoFocus
            />
          </div>

          <div className="settings-field">
            <label className="settings-label">{t('projectDialog.projectType')}</label>
            {grouped.map(([cat, list]) => (
              <div key={cat} className="project-type-group">
                <div className="project-type-group-label">{categoryLabel(cat)}</div>
                <div className="project-types">
                  {list.map(tmpl => (
                    <div
                      key={tmpl.id}
                      className={`project-type-card ${type === tmpl.id ? 'project-type-active' : ''}`}
                      onClick={() => setType(tmpl.id)}
                    >
                      <div className="project-type-badge">{TYPE_ICONS[tmpl.id] || tmpl.lang || tmpl.ext}</div>
                      <div className="project-type-name">{tmpl.name}</div>
                      <div className="project-type-ext">{tmpl.ext}</div>
                      {type === tmpl.id && <SelectionMark size={18} className="project-type-check" />}
                    </div>
                  ))}
                </div>
              </div>
            ))}
          </div>

          {needsBoard && (
            <div className="settings-field">
              <label className="settings-label">{t('projectDialog.targetBoard')}</label>
              <input
                className="project-input"
                value={boardFilter}
                onChange={e => setBoardFilter(e.target.value)}
                placeholder={t('projectDialog.boardSearch')}
              />
              <div className="board-picker">
                {families.map(([family, list]) => (
                  <div key={family} className="board-family">
                    <div className="board-family-label">{family}</div>
                    <div className="board-family-list">
                      {list.map(b => (
                        <button
                          key={b.id}
                          type="button"
                          className={`board-chip ${boardId === b.id ? 'board-chip-active' : ''}`}
                          onClick={() => setBoardId(b.id)}
                        >
                          <span className="board-chip-name">{b.name}</span>
                          <span className="board-chip-meta">{b.flashKb}K / {b.ramKb}K</span>
                          {boardId === b.id && <SelectionMark size={16} className="board-chip-check" />}
                        </button>
                      ))}
                    </div>
                  </div>
                ))}
                {families.length === 0 && (
                  <div className="board-empty">{t('projectDialog.noBoards')}</div>
                )}
              </div>
              {selectedBoard && (
                <div className="project-info">
                  {selectedBoard.mcu} · {selectedBoard.cpu} · Flash {selectedBoard.flashKb} KB · RAM {selectedBoard.ramKb} KB
                </div>
              )}
            </div>
          )}

          {error && <div className="project-error">{error}</div>}

          <div className="project-info">
            {t('projectDialog.location', { path: `${projectsDir || '...'}/${name}` })}
          </div>
        </div>

        <div className="project-footer">
          <button className="project-btn project-btn-cancel" onClick={onClose}>{t('common.cancel')}</button>
          <button
            className="project-btn project-btn-create"
            onClick={handleCreate}
            disabled={!name.trim() || creating || !boardId}
          >
            {creating ? t('projectDialog.creating') : t('projectDialog.create')}
          </button>
        </div>
      </div>
    </div>
  )
}
