import { useTranslation } from '../core/TranslationContext'

interface StatusBarProps {
  line: number
  col: number
  language: string
  projectType?: string
  boardName?: string
  toolchains: ToolchainInfo | null
}

export function StatusBar({ line, col, language, projectType, boardName, toolchains }: StatusBarProps) {
  const { t } = useTranslation()

  return (
    <div className="statusbar">
      <div className="statusbar-left">
        <span className="statusbar-item statusbar-mcu">
          <span className="statusbar-dot" style={{ background: '#58A6FF' }} />
          {boardName || t('statusBar.mcu')}
        </span>
        <span className="statusbar-separator" />
        <span className="statusbar-item">
          {toolchains?.armGcc
            ? `${t('statusBar.armGcc')} ${toolchains.armGccVersion?.match(/\d+\.\d+\.\d+/)?.[0] || ''}${toolchains.armGccBundled ? ` (${t('statusBar.bundled')})` : ''}`
            : toolchains?.zig
              ? `Zig ${toolchains.zigVersion || ''}${toolchains.zigBundled ? ` (${t('statusBar.bundled')})` : ''}`
              : toolchains?.rust
                ? `${t('statusBar.rust')} ${toolchains.rustVersion?.match(/\d+\.\d+/)?.[0] || ''}`
                : t('statusBar.noCompiler')}
        </span>
        <span className="statusbar-separator" />
        <span className="statusbar-item">
          {toolchains?.openocd || toolchains?.armGdb
            ? [
                toolchains.openocd
                  ? `${t('statusBar.openocd')}${toolchains.openocdBundled ? ` (${t('statusBar.bundled')})` : ''}`
                  : null,
                toolchains.armGdb
                  ? `GDB${toolchains.armGdbVersion?.match(/\d+\.\d+\.\d+/)?.[0] ? ` ${toolchains.armGdbVersion.match(/\d+\.\d+\.\d+/)?.[0]}` : ''}${toolchains.armGdbBundled ? ` (${t('statusBar.bundled')})` : ''}`
                  : null,
              ].filter(Boolean).join(' · ')
            : t('statusBar.noDebugger')}
        </span>
        <span className="statusbar-separator" />
        <span className="statusbar-item">{projectType?.toUpperCase() || ''}</span>
      </div>

      <div className="statusbar-right">
        {language && (
          <span className="statusbar-item statusbar-lang">
            {language.toUpperCase()}
          </span>
        )}
        <span className="statusbar-separator" />
        <span className="statusbar-item statusbar-cursor">
          {t('statusBar.ln')} {line}, {t('statusBar.col')} {col}
        </span>
        <span className="statusbar-separator" />
        <span className="statusbar-item statusbar-ready">
          <span className="statusbar-dot" style={{ background: '#00FF66' }} />
          {t('statusBar.ready')}
        </span>
      </div>
    </div>
  )
}
