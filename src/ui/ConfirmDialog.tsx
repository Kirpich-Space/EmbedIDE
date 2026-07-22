import { useEffect, useRef } from 'react'
import { useTranslation } from '../core/TranslationContext'

interface ConfirmDialogProps {
  title: string
  message: string
  confirmLabel?: string
  cancelLabel?: string
  danger?: boolean
  onConfirm: () => void
  onCancel: () => void
}

export function ConfirmDialog({ title, message, confirmLabel, cancelLabel, danger = true, onConfirm, onCancel }: ConfirmDialogProps) {
  const { t } = useTranslation()
  confirmLabel ??= t('confirmDialog.deleteTitle')
  cancelLabel ??= t('common.cancel')
  const btnRef = useRef<HTMLButtonElement>(null)

  useEffect(() => {
    const h = (e: KeyboardEvent) => {
      if (e.key === 'Escape') onCancel()
    }
    window.addEventListener('keydown', h)
    return () => window.removeEventListener('keydown', h)
  }, [onCancel])

  return (
    <div className="settings-overlay animate-fade-in" onClick={e => { if (e.target === e.currentTarget) onCancel() }}>
      <div className="project-dialog animate-scale-in confirm-dialog" onClick={e => e.stopPropagation()}>
        <div className="settings-header">
          <span className="settings-title">{title}</span>
        </div>
        <div className="project-body">
          <p className="confirm-message">{message}</p>
        </div>
        <div className="project-footer">
          <button className="project-btn project-btn-cancel" onClick={onCancel}>{cancelLabel}</button>
          <button
            ref={btnRef}
            className={`project-btn ${danger ? 'project-btn-danger' : 'project-btn-create'}`}
            onClick={onConfirm}
            autoFocus
          >
            {confirmLabel}
          </button>
        </div>
      </div>
    </div>
  )
}
