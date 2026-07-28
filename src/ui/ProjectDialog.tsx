import { useState, useEffect, useRef, useCallback, useMemo } from 'react'
import { useTranslation } from '../core/TranslationContext'
import type { BoardInfo } from '../core/types'

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
}

export function ProjectDialog({ onCreate, onClose }: ProjectDialogProps) {
  const { t } = useTranslation()
  const [name, setName] = useState(defaultName)
  const [type, setType] = useState('c')
  const [boardId, setBoardId] = useState('stm32f407vg')
  const [boardFilter, setBoardFilter] = useState('')
  const [templates, setTemplates] = useState<{id: string, name: string, ext: string}[]>([])
  const [boards, setBoards] = useState<BoardInfo[]>([])
  const [projectsDir, setProjectsDir] = useState('')
  const [creating, setCreating] = useState(false)
  const [error, setError] = useState('')
  const inputRef = useRef<HTMLInputElement>(null)
  const creatingRef = useRef(false)

  useEffect(() => {
    window.electronAPI?.getProjectTemplates().then(setTemplates)
    window.electronAPI?.listBoards().then(list => {
      setBoards(list)
      if (list.length && !list.find(b => b.id === boardId)) {
        setBoardId(list[0].id)
      }
    })
    window.electronAPI?.getDefaultProjectsDir().then(setProjectsDir)
    inputRef.current?.select()
  }, [])

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
            <div className="project-types">
              {templates.map(tmpl => (
                <div
                  key={tmpl.id}
                  className={`project-type-card ${type === tmpl.id ? 'project-type-active' : ''}`}
                  onClick={() => setType(tmpl.id)}
                >
                  <div className="project-type-badge">{TYPE_ICONS[tmpl.id] || tmpl.ext}</div>
                  <div className="project-type-name">{tmpl.name}</div>
                  <div className="project-type-ext">{tmpl.ext}</div>
                </div>
              ))}
            </div>
          </div>

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
