import { useTranslation } from '../core/TranslationContext'

interface PeripheralViewerProps {
  boardName?: string
  peripheralNames?: string[]
}

export function PeripheralViewer({ boardName, peripheralNames = [] }: PeripheralViewerProps) {
  const { t } = useTranslation()

  if (!boardName && peripheralNames.length === 0) {
    return (
      <div className="peripheral-viewer">
        <div className="panel-header">
          <span className="panel-title">{t('peripheral.title')}</span>
        </div>
        <div className="peripheral-content">
          <div className="peripheral-empty">{t('peripheral.empty')}</div>
        </div>
      </div>
    )
  }

  return (
    <div className="peripheral-viewer">
      <div className="panel-header">
        <span className="panel-title">{t('peripheral.title')}</span>
        {boardName && <span className="panel-subtitle">{boardName}</span>}
      </div>
      <div className="peripheral-content">
        <div className="peripheral-group">
          <div className="peripheral-header">
            <span className="peripheral-name">{t('peripheral.available')}</span>
          </div>
          <div className="peripheral-pins peripheral-blocks">
            {peripheralNames.map(name => (
              <div key={name} className="peripheral-block-chip">{name}</div>
            ))}
          </div>
        </div>
      </div>
    </div>
  )
}
