import { useTranslation } from '../core/TranslationContext'

interface ToolbarProps {
  projectName?: string
  onBuild: () => void
  onCancelBuild: () => void
  onFlash: () => void
  onDebug: () => void
  onOpenSettings: () => void
  onNewProject: () => void
  onSerial: () => void
  onOpenProject: () => void
  leftPanelVisible: boolean
  rightPanelVisible: boolean
  onToggleLeftPanel: () => void
  onToggleRightPanel: () => void
  isBuilding: boolean
}

export function Toolbar({
  projectName, onBuild, onCancelBuild, onFlash, onDebug, onOpenSettings, onNewProject, onSerial,
  onOpenProject, leftPanelVisible, rightPanelVisible, onToggleLeftPanel, onToggleRightPanel, isBuilding
}: ToolbarProps) {
  const handleMinimize = () => window.electronAPI?.minimize()
  const handleMaximize = () => window.electronAPI?.maximize()
  const handleClose = () => window.electronAPI?.close()
  const { t } = useTranslation()

  return (
    <div className="toolbar">
      <div className="toolbar-drag">
        <div className="toolbar-brand">
          <svg className="toolbar-logo-svg" width="20" height="20" viewBox="0 0 24 24" fill="none">
            <rect x="2" y="2" width="20" height="20" rx="3" stroke="currentColor" strokeWidth="1.5" />
            <path d="M7 8h10M7 12h6M7 16h8" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" />
            <circle cx="18" cy="16" r="2" fill="var(--accent)" />
          </svg>
          <span className="toolbar-title">{t('toolbar.title')}</span>
          {projectName && <span className="toolbar-project-name">{projectName}</span>}
        </div>

        <div className="toolbar-actions">
          <button className="toolbar-btn" onClick={onNewProject} title={`${t('toolbar.newProject')} (Ctrl+N)`}>
            <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
              <path d="M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8z" />
              <polyline points="14 2 14 8 20 8" />
              <line x1="12" y1="18" x2="12" y2="12" />
              <line x1="9" y1="15" x2="15" y2="15" />
            </svg>
            <span className="label">{t('toolbar.new')}</span>
          </button>

          <button className="toolbar-btn" onClick={onOpenProject} title={`${t('toolbar.openProject')} (Ctrl+O)`}>
            <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
              <path d="M22 19a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h5l2 3h9a2 2 0 0 1 2 2z" />
            </svg>
            <span className="label">{t('toolbar.open')}</span>
          </button>

          <div className="toolbar-separator" />

          <button className={`toolbar-btn toolbar-btn-icon ${!leftPanelVisible ? 'toolbar-btn-muted' : ''}`} onClick={onToggleLeftPanel} title={`${t('toolbar.toggleExplorer')} (Ctrl+Shift+E)`}>
            <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
              <rect x="3" y="3" width="7" height="18" />
              <rect x="14" y="3" width="7" height="7" />
              <rect x="14" y="14" width="7" height="7" />
            </svg>
          </button>

          <button className={`toolbar-btn toolbar-btn-icon ${!rightPanelVisible ? 'toolbar-btn-muted' : ''}`} onClick={onToggleRightPanel} title={`${t('toolbar.toggleAgents')} (Ctrl+Shift+A)`}>
            <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
              <rect x="3" y="3" width="7" height="7" />
              <rect x="14" y="3" width="7" height="7" />
              <rect x="3" y="14" width="7" height="7" />
              <rect x="14" y="14" width="7" height="7" />
            </svg>
          </button>

          <div className="toolbar-separator" />

          <button className="toolbar-btn" onClick={onSerial} title={t('toolbar.serialMonitor')}>
            <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
              <polyline points="22 12 18 12 15 21 9 3 6 12 2 12" />
            </svg>
            <span className="label">{t('toolbar.serial')}</span>
          </button>

          <div className="toolbar-separator" />

          {isBuilding ? (
            <button className="toolbar-btn toolbar-btn-danger" onClick={onCancelBuild} title={t('toolbar.cancelBuild')}>
              <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round">
                <rect x="6" y="6" width="12" height="12" rx="2" />
              </svg>
              <span className="label">{t('toolbar.cancel')}</span>
            </button>
          ) : (
            <button className="toolbar-btn" onClick={onBuild} title={`${t('toolbar.build')} (Ctrl+B)`}>
              <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
                <path d="M14.7 6.3a1 1 0 0 0 0 1.4l1.6 1.6a1 1 0 0 0 1.4 0l3.77-3.77a6 6 0 0 1-7.94 7.94l-6.91 6.91a2.12 2.12 0 0 1-3-3l6.91-6.91a6 6 0 0 1 7.94-7.94l-3.76 3.76z" />
              </svg>
              <span className="label">{t('toolbar.build')}</span>
            </button>
          )}
          <button className="toolbar-btn toolbar-btn-accent" onClick={onFlash} title={t('toolbar.flash')}>
            <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
              <polygon points="13 2 3 14 12 14 11 22 21 10 12 10 13 2" />
            </svg>
            <span className="label">{t('toolbar.flash')}</span>
          </button>
          <button className="toolbar-btn" onClick={onDebug} title={t('toolbar.debugSession')}>
            <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
              <circle cx="12" cy="12" r="10" />
              <path d="M12 6v6l4 2" />
            </svg>
            <span className="label">{t('toolbar.debug')}</span>
          </button>

          <div className="toolbar-separator" />

          <button className="toolbar-btn toolbar-btn-icon" onClick={onOpenSettings} title={`${t('toolbar.settings')} (Ctrl+,)`}>
            <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
              <circle cx="12" cy="12" r="3" />
              <path d="M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 0 1 0 2.83 2 2 0 0 1-2.83 0l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 0 1-2 2 2 2 0 0 1-2-2v-.09A1.65 1.65 0 0 0 9 19.4a1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 0 1-2.83 0 2 2 0 0 1 0-2.83l.06-.06A1.65 1.65 0 0 0 4.68 15a1.65 1.65 0 0 0-1.51-1H3a2 2 0 0 1-2-2 2 2 0 0 1 2-2h.09A1.65 1.65 0 0 0 4.6 9a1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 0 1 0-2.83 2 2 0 0 1 2.83 0l.06.06A1.65 1.65 0 0 0 9 4.68a1.65 1.65 0 0 0 1-1.51V3a2 2 0 0 1 2-2 2 2 0 0 1 2 2v.09a1.65 1.65 0 0 0 1 1.51 1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 0 1 2.83 0 2 2 0 0 1 0 2.83l-.06.06a1.65 1.65 0 0 0-.33 1.82V9a1.65 1.65 0 0 0 1.51 1H21a2 2 0 0 1 2 2 2 2 0 0 1-2 2h-.09a1.65 1.65 0 0 0-1.51 1z" />
            </svg>
          </button>
        </div>
      </div>

      <div className="toolbar-window-controls">
        <button className="win-btn" onClick={handleMinimize} title={t('toolbar.minimize')}>─</button>
        <button className="win-btn" onClick={handleMaximize} title={t('toolbar.maximize')}>□</button>
        <button className="win-btn win-btn-close" onClick={handleClose} title={t('toolbar.close')}>×</button>
      </div>
    </div>
  )
}
