import { useRef, useEffect, useState } from 'react'
import { useTranslation } from '../core/TranslationContext'
import type { BuildMessage } from '../core/types'

interface OutputPanelProps {
  messages: BuildMessage[]
  onClose: () => void
}

const FILTER_OPTIONS = ['all', 'info', 'warn', 'error', 'success'] as const
type FilterType = (typeof FILTER_OPTIONS)[number]

export function OutputPanel({ messages, onClose }: OutputPanelProps) {
  const listRef = useRef<HTMLDivElement>(null)
  const [filter, setFilter] = useState<FilterType>('all')
  const { t } = useTranslation()
  const [autoScroll, setAutoScroll] = useState(true)

  const filtered = filter === 'all' ? messages : messages.filter(m => m.type === filter)

  useEffect(() => {
    if (autoScroll && listRef.current) {
      listRef.current.scrollTop = listRef.current.scrollHeight
    }
  }, [filtered, autoScroll])

  const handleScroll = () => {
    if (!listRef.current) return
    const el = listRef.current
    const atBottom = el.scrollHeight - el.scrollTop - el.clientHeight < 30
    setAutoScroll(atBottom)
  }

  return (
    <div className="output-panel">
      <div className="output-header">
        <span className="output-title">{t('output.title')}</span>
        <div className="output-actions">
          <div className="output-filters">
            {FILTER_OPTIONS.map(f => (
              <button
                key={f}
                className={`output-filter-btn ${filter === f ? 'output-filter-active' : ''}`}
                onClick={() => setFilter(f)}
              >
                {f === 'all' ? `${t('output.all')} (${messages.length})` : `${f} (${messages.filter(m => m.type === f).length})`}
              </button>
            ))}
          </div>
          <button className="output-close" onClick={onClose}>×</button>
        </div>
      </div>
      <div ref={listRef} className="output-body" onScroll={handleScroll}>
        {filtered.length === 0 ? (
          <div className="output-line output-info">
            <span className="output-text">{filter === 'all' ? t('output.empty') : t('output.noFilter', { filter })}</span>
          </div>
        ) : (
          filtered.map((msg, i) => (
            <div key={i} className={`output-line output-${msg.type}`}>
              <span className="output-time">
                {new Date(msg.timestamp).toLocaleTimeString('en-US', { hour12: false })}
              </span>
              <span className="output-badge">{msg.type.toUpperCase()}</span>
              <span className="output-text">{msg.text}</span>
            </div>
          ))
        )}
      </div>
    </div>
  )
}
