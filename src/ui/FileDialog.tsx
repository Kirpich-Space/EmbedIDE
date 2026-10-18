import { useState, useEffect, useRef, useCallback } from 'react'
import { useTranslation } from '../core/TranslationContext'

interface FileDialogProps {
  mode: 'create-file' | 'create-folder' | 'rename' | 'create-script'
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
  zig: ['.zig'],
  'driver-c': ['.c', '.h'],
  'driver-cpp': ['.cpp', '.hpp', '.h'],
  'driver-rust': ['.rs'],
  'driver-asm': ['.S', '.s'],
  'driver-zig': ['.zig'],
  'os-c': ['.c', '.h'],
  'os-cpp': ['.cpp', '.hpp', '.h'],
  'os-rust': ['.rs'],
  'os-asm': ['.S', '.s'],
  'os-zig': ['.zig'],
  driver: ['.c', '.h'],
  os: ['.c', '.h'],
  'script-c': ['.c'],
  'script-cpp': ['.cpp'],
  'script-rust': ['.rs'],
  'script-asm': ['.S', '.s'],
}

const SCRIPT_EXTS = ['.c', '.cpp', '.rs', '.S'] as const

export function FileDialog({ mode, initialName, parentDir, projectType, onSubmit, onClose }: FileDialogProps) {
  const { t } = useTranslation()
  const [name, setName] = useState(initialName || (mode === 'create-script' ? 'script.c' : ''))
  const [error, setError] = useState('')
  const [loading, setLoading] = useState(false)
  const inputRef = useRef<HTMLInputElement>(null)
  const loadingRef = useRef(false)

  const titleKey = mode === 'create-file' ? 'fileDialog.newFile'
    : mode === 'create-folder' ? 'fileDialog.newFolder'
    : mode === 'create-script' ? 'fileDialog.newScript'
    : 'fileDialog.rename'
  const title = t(titleKey)

  const placeholder = mode === 'create-file' || mode === 'create-script' ? t('fileDialog.filePlaceholder')
    : mode === 'create-folder' ? t('fileDialog.folderPlaceholder')
    : t('fileDialog.renamePlaceholder')

  const extensions = mode === 'create-script'
    ? [...SCRIPT_EXTS]
    : (projectType ? EXTENSIONS[projectType] : undefined)

  const handleSubmit = useCallback(async () => {
    if (!name.trim() || loadingRef.current) return
    let finalName = name.trim()
    if (mode === 'create-script' && !SCRIPT_EXTS.some(e => {
      const lower = finalName.toLowerCase()
      return lower.endsWith(e.toLowerCase())
    })) {
      finalName += '.c'
    }
    loadingRef.current = true
    setLoading(true)
    setError('')
    try {
      await onSubmit(finalName)
    } catch (e: any) {
      setError(e.message || 'Operation failed')
    } finally {
      loadingRef.current = false
      setLoading(false)
    }
  }, [name, onSubmit, mode])

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
    setName((base || 'script') + ext)
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

          {mode === 'create-script' && (
            <p className="settings-hint" style={{ marginBottom: 10 }}>{t('fileDialog.scriptHint')}</p>
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

          {(mode === 'create-file' || mode === 'create-script') && extensions && (
            <div className="dialog-extensions">
              {extensions.map(ext => (
                <button
                  key={ext}
                  className={`dialog-ext-btn ${name.toLowerCase().endsWith(ext.toLowerCase()) ? 'dialog-ext-active' : ''}`}
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
