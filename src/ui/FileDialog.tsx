import { useState, useEffect, useRef, useCallback } from 'react'
import { useTranslation } from '../core/TranslationContext'

interface FileDialogProps {
  mode: 'create-file' | 'create-folder' | 'rename'
  initialName?: string
  parentDir?: string
  projectType?: string
  onSubmit: (name: string) => void | Promise<void>
  onClose: () => void
}

const EXTENSIONS: Record<string, string[]> = {
  rust: ['.rs'],
  c: ['.c', '.h'],
  cpp: ['.cpp', '.hpp', '.h'],
  asm: ['.S', '.s'],
}

export function FileDialog({ mode, initialName, parentDir, projectType, onSubmit, onClose }: FileDialogProps) {
  const { t } = useTranslation()
  const [name, setName] = useState(initialName || '')
  const [error, setError] = useState('')
  const [loading, setLoading] = useState(false)
  const inputRef = useRef<HTMLInputElement>(null)
  const loadingRef = useRef(false)

  const title = t('fileDialog.' + mode)

  const placeholder = mode === 'create-file' ? t('fileDialog.filePlaceholder')
    : mode === 'create-folder' ? t('fileDialog.folderPlaceholder')
    : t('fileDialog.renamePlaceholder')

  const extensions = projectType ? EXTENSIONS[projectType] : undefined

  const handleSubmit = useCallback(async () => {
    if (!name.trim() || loadingRef.current) return
    loadingRef.current = true
    setLoading(true)
    setError('')
    try {
      await onSubmit(name.trim())
    } catch (e: any) {
      setError(e.message || 'Operation failed')
    } finally {
      loadingRef.current = false
      setLoading(false)
    }
  }, [name, onSubmit])

  const handleKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === 'Enter') { e.preventDefault(); handleSubmit() }
    if (e.key === 'Escape') { e.preventDefault(); onClose() }
  }

  useEffect(() => {
    inputRef.current?.focus()
    inputRef.current?.select()
  }, [])

  const appendExt = (ext: string) => {
    const base = name.includes('.') ? name.substring(0, name.lastIndexOf('.')) : name
    setError('')
    setName(base + ext)
    inputRef.current?.focus()
  }

  return (
    <div className="settings-overlay animate-fade-in" onClick={e => { if (e.target === e.currentTarget) onClose() }}
      onKeyDown={handleKeyDown}
    >
      <div className="project-dialog animate-scale-in" onClick={e => e.stopPropagation()}>
        <div className="settings-header">
          <span className="settings-title">{title}</span>
          <button className="settings-close" onClick={onClose}>×</button>
        </div>

        <div className="project-body">
          {parentDir && (
            <div className="dialog-location">
              {t('fileDialog.inFolder', { dir: parentDir })}
            </div>
          )}

          <div className="settings-field">
            <label className="settings-label">{t('fileDialog.name')}</label>
            <input
              ref={inputRef}
              className="project-input"
              value={name}
              onChange={e => { setName(e.target.value); setError('') }}
              onFocus={e => e.target.select()}
              onKeyDown={handleKeyDown}
              placeholder={placeholder}
              autoFocus
            />
          </div>

          {mode === 'create-file' && extensions && (
            <div className="dialog-extensions">
              {extensions.map(ext => (
                <button
                  key={ext}
                  className={`dialog-ext-btn ${name.endsWith(ext) ? 'dialog-ext-active' : ''}`}
                  onClick={() => appendExt(ext)}
                >
                  {ext}
                </button>
              ))}
            </div>
          )}

          {error && <div className="project-error">{error}</div>}
        </div>

        <div className="project-footer">
          <button className="project-btn project-btn-cancel" onClick={onClose}>{t('common.cancel')}</button>
          <button
            className="project-btn project-btn-create"
            onClick={handleSubmit}
            disabled={!name.trim() || loading}
          >
            {loading ? (
              <span className="btn-loading">
                <span className="btn-spinner" />
                {mode === 'rename' ? t('common.renaming') : t('common.creating')}
              </span>
            ) : (
              mode === 'rename' ? t('common.rename') : t('common.create')
            )}
          </button>
        </div>
      </div>
    </div>
  )
}
