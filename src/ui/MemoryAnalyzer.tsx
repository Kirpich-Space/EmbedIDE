import { useTranslation } from '../core/TranslationContext'

interface MemoryAnalyzerProps {
  flashUsed: number
  flashTotal: number
  ramUsed: number
  ramTotal: number
  stackUsed?: number
  heapUsed?: number
}

export function MemoryAnalyzer({ flashUsed, flashTotal, ramUsed, ramTotal, stackUsed, heapUsed }: MemoryAnalyzerProps) {
  const { t } = useTranslation()
  const flashPercent = flashTotal > 0 ? Math.round((flashUsed / flashTotal) * 100) : 0
  const ramPercent = ramTotal > 0 ? Math.round((ramUsed / ramTotal) * 100) : 0
  const stackPercent = stackUsed != null && ramTotal > 0 ? Math.round((stackUsed / ramTotal) * 100) : 0
  const heapPercent = heapUsed != null && ramTotal > 0 ? Math.round((heapUsed / ramTotal) * 100) : 0

  const flashColor = flashPercent > 85 ? 'var(--accent)' : flashPercent > 60 ? '#FFA500' : 'var(--hl-string)'
  const ramColor = ramPercent > 85 ? 'var(--accent)' : ramPercent > 60 ? '#FFA500' : 'var(--hl-string)'

  return (
    <div className="memory-analyzer">
      <div className="panel-header">
        <span className="panel-title">{t('memory.title')}</span>
      </div>
      <div className="memory-content">
        <div className="memory-section">
          <div className="memory-header">
            <span>{t('memory.flash')}</span>
            <span className="memory-value">{flashPercent}% ({formatBytes(flashUsed)} / {formatBytes(flashTotal)})</span>
          </div>
          <div className="memory-bar-bg">
            <div className="memory-bar-fill" style={{ width: `${Math.min(flashPercent, 100)}%`, background: flashColor }} />
          </div>
        </div>

        <div className="memory-section">
          <div className="memory-header">
            <span>{t('memory.ram')}</span>
            <span className="memory-value">{ramPercent}% ({formatBytes(ramUsed)} / {formatBytes(ramTotal)})</span>
          </div>
          <div className="memory-bar-bg">
            <div className="memory-bar-fill" style={{ width: `${Math.min(ramPercent, 100)}%`, background: ramColor }} />
          </div>
        </div>

        {(stackUsed != null || heapUsed != null) && (
          <div className="memory-details">
            {stackUsed != null && (
              <div className="memory-detail-row">
                <span className="memory-detail-label">{t('memory.stack')}</span>
                <span className="memory-detail-value">{formatBytes(stackUsed)} ({stackPercent}%)</span>
              </div>
            )}
            {heapUsed != null && (
              <div className="memory-detail-row">
                <span className="memory-detail-label">{t('memory.heap')}</span>
                <span className="memory-detail-value">{formatBytes(heapUsed)} ({heapPercent}%)</span>
              </div>
            )}
          </div>
        )}
      </div>
    </div>
  )
}

function formatBytes(bytes: number): string {
  if (bytes >= 1024 * 1024) return `${(bytes / (1024 * 1024)).toFixed(1)} MB`
  if (bytes >= 1024) return `${(bytes / 1024).toFixed(1)} KB`
  return `${bytes} B`
}
