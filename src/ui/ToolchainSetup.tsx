import { useEffect, useState } from 'react'
import { useTranslation } from '../core/TranslationContext'
import './ToolchainSetup.css'

interface Progress {
  phase?: string
  package?: string
  percent?: number
  message?: string
  received?: number
  total?: number
  root?: string
  auto?: boolean
}

interface ToolchainSetupProps {
  open: boolean
  onClose: () => void
  onDone: () => void
}

export function ToolchainSetup({ open, onClose, onDone }: ToolchainSetupProps) {
  const { t } = useTranslation()
  const [progress, setProgress] = useState<Progress | null>(null)
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [includeRust, setIncludeRust] = useState(true)

  useEffect(() => {
    if (!open || !window.electronAPI) return
    const offP = window.electronAPI.onToolchainInstallProgress?.(p => {
      setProgress(p)
      setBusy(true)
      if (p.phase === 'error') setError(p.message || t('toolchainSetup.failed'))
    })
    const offC = window.electronAPI.onToolchainInstallComplete?.(data => {
      setBusy(false)
      if (data.ok) {
        setError(null)
        setProgress({ phase: 'done', percent: 100, message: t('toolchainSetup.success'), root: data.root })
        onDone()
      } else {
        setError(data.error || t('toolchainSetup.failed'))
      }
    })
    window.electronAPI.toolchainInstallStatus?.().then(s => {
      if (s.running && s.progress) {
        setBusy(true)
        setProgress(s.progress)
      }
    }).catch(() => {})
    return () => {
      offP?.()
      offC?.()
    }
  }, [open, onDone, t])

  if (!open) return null

  const pct = Math.max(0, Math.min(100, progress?.percent ?? (busy ? 5 : 0)))

  const start = async (force = false) => {
    setError(null)
    setBusy(true)
    setProgress({ phase: 'start', percent: 1, message: t('toolchainSetup.starting') })
    try {
      const r = await window.electronAPI?.installToolchain({ includeRust, force })
      if (r && !r.ok) setError(r.error || t('toolchainSetup.failed'))
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e))
      setBusy(false)
    }
  }

  return (
    <div className="tc-setup-overlay" role="dialog" aria-modal="true">
      <div className="tc-setup-card">
        <h2 className="tc-setup-title">{t('toolchainSetup.title')}</h2>
        <p className="tc-setup-desc">{t('toolchainSetup.desc')}</p>
        <ul className="tc-setup-list">
          <li>ARM GCC / GDB (Cortex-M)</li>
          <li>OpenOCD</li>
          <li>Zig</li>
          <li>make</li>
          {includeRust && <li>Rust (rustc / cargo + thumb targets)</li>}
        </ul>
        <label className="tc-setup-check">
          <input
            type="checkbox"
            checked={includeRust}
            disabled={busy}
            onChange={e => setIncludeRust(e.target.checked)}
          />
          {t('toolchainSetup.includeRust')}
        </label>
        <div className="tc-setup-bar" aria-valuenow={pct} aria-valuemin={0} aria-valuemax={100}>
          <div className="tc-setup-bar-fill" style={{ width: `${pct}%` }} />
        </div>
        <div className="tc-setup-status">
          {error ? error : (progress?.message || t('toolchainSetup.ready'))}
        </div>
        <div className="tc-setup-actions">
          <button
            type="button"
            className="project-btn project-btn-create"
            disabled={busy}
            onClick={() => start(false)}
          >
            {busy ? t('toolchainSetup.downloading') : t('toolchainSetup.download')}
          </button>
          <button
            type="button"
            className="project-btn"
            disabled={busy}
            onClick={onClose}
          >
            {t('toolchainSetup.later')}
          </button>
        </div>
      </div>
    </div>
  )
}
