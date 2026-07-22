import { getLangColor } from '../core/utils'
import { useTranslation } from '../core/TranslationContext'

interface StatusBarProps {
  line: number
  col: number
  language: string
  projectType?: string
  toolchains: ToolchainInfo | null
}

export function StatusBar({ line, col, language, projectType, toolchains }: StatusBarProps) {
  const { t } = useTranslation()
  const mcuName = projectType === 'rust' ? t('statusBar.mcu') : projectType === 'asm' ? t('statusBar.arch') : t('statusBar.mcu')

  return (
    <div className="statusbar">
      <div className="statusbar-left">
        <span className="statusbar-item statusbar-mcu">
          <span className="statusbar-dot" style={{ background: '#58A6FF' }} />
          {mcuName}
        </span>
        <span className="statusbar-separator" />
        <span className="statusbar-item">
          {toolchains?.armGcc
            ? `${t('statusBar.armGcc')} ${toolchains.armGccVersion?.match(/\d+\.\d+\.\d+/)?.[0] || ''}`
            : toolchains?.rust ? `${t('statusBar.rust')} ${toolchains.rustVersion?.match(/\d+\.\d+/)?.[0] || ''}` : t('statusBar.noCompiler')}
        </span>
        <span className="statusbar-separator" />
        <span className="statusbar-item">
          {toolchains?.openocd ? t('statusBar.openocd') : t('statusBar.noDebugger')}
        </span>
        <span className="statusbar-separator" />
        <span className="statusbar-item">{t('statusBar.clock')}</span>
        <span className="statusbar-separator" />
        <span className="statusbar-item">{projectType?.toUpperCase() || ''}</span>
      </div>

      <div className="statusbar-right">
        {language && (
          <span className="statusbar-item statusbar-lang">
            <span className="statusbar-dot" style={{ background: getLangColor(language) }} />
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
