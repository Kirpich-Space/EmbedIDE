import { useState, useEffect, useRef, useCallback } from 'react'
import { useTranslation } from '../core/TranslationContext'

interface ProjectDialogProps {
  onCreate: (name: string, type: string) => void
  onClose: () => void
}

const defaultName = 'my-project'

export function ProjectDialog({ onCreate, onClose }: ProjectDialogProps) {
  const { t } = useTranslation()
  const [name, setName] = useState(defaultName)
  const [type, setType] = useState('rust')
  const [templates, setTemplates] = useState<{id: string, name: string, ext: string}[]>([])
  const [projectsDir, setProjectsDir] = useState('')
  const [creating, setCreating] = useState(false)
  const [error, setError] = useState('')
  const inputRef = useRef<HTMLInputElement>(null)
  const creatingRef = useRef(false)

  useEffect(() => {
    window.electronAPI?.getProjectTemplates().then(setTemplates)
    window.electronAPI?.getDefaultProjectsDir().then(setProjectsDir)
    inputRef.current?.select()
  }, [])

  const handleCreate = useCallback(async () => {
    if (!name.trim() || creatingRef.current) return
    creatingRef.current = true
    setCreating(true)
    setError('')
    try {
      await onCreate(name.trim(), type)
    } catch (e: any) {
      setError(e.message || 'Failed to create project')
    } finally {
      creatingRef.current = false
      setCreating(false)
    }
  }, [name, type, onCreate])

  const handleKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === 'Enter') { e.preventDefault(); handleCreate() }
    if (e.key === 'Escape') onClose()
  }

  return (
    <div className="settings-overlay" onClick={e => { if (e.target === e.currentTarget) onClose() }}>
      <div className="project-dialog">
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
              {templates.map(t => (
                <div
                  key={t.id}
                  className={`project-type-card ${type === t.id ? 'project-type-active' : ''}`}
                  onClick={() => setType(t.id)}
                >
                  <div className="project-type-icon">
                    {t.id === 'rust' ? '🦀' : t.id === 'c' ? '◎' : t.id === 'cpp' ? '◈' : '⚙'}
                  </div>
                  <div className="project-type-name">{t.name}</div>
                  <div className="project-type-ext">{t.ext}</div>
                </div>
              ))}
            </div>
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
            disabled={!name.trim() || creating}
          >
            {creating ? t('projectDialog.creating') : t('projectDialog.create')}
          </button>
        </div>
      </div>
    </div>
  )
}
