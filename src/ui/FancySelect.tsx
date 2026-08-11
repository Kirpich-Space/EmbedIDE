import { useCallback, useEffect, useId, useRef, useState, type CSSProperties, type ReactNode } from 'react'
import { SelectionMark } from './CheckIcon'

export interface FancySelectOption {
  value: string
  label: string
  hint?: string
  preview?: ReactNode
  /** Optional style applied to the label (e.g. font-family for font pickers) */
  labelStyle?: CSSProperties
  /** Extra class on the preview chip (e.g. accent swatches) */
  previewClassName?: string
}

interface FancySelectProps {
  value: string
  options: FancySelectOption[]
  onChange: (value: string) => void
  disabled?: boolean
  placeholder?: string
  /** Card grid for short lists (languages, fonts); list dropdown for long lists (models) */
  variant?: 'cards' | 'menu'
  columns?: 2 | 3 | 4
  className?: string
  'aria-label'?: string
}

export function FancySelect({
  value,
  options,
  onChange,
  disabled,
  placeholder = 'Select…',
  variant = 'cards',
  columns = 3,
  className = '',
  'aria-label': ariaLabel,
}: FancySelectProps) {
  const [open, setOpen] = useState(false)
  const rootRef = useRef<HTMLDivElement>(null)
  const listId = useId()
  const selected = options.find(o => o.value === value) ?? null

  useEffect(() => {
    if (!open) return
    const onDoc = (e: MouseEvent) => {
      if (!rootRef.current?.contains(e.target as Node)) setOpen(false)
    }
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') setOpen(false)
    }
    document.addEventListener('mousedown', onDoc)
    document.addEventListener('keydown', onKey)
    return () => {
      document.removeEventListener('mousedown', onDoc)
      document.removeEventListener('keydown', onKey)
    }
  }, [open])

  const pick = useCallback((v: string) => {
    onChange(v)
    setOpen(false)
  }, [onChange])

  if (variant === 'cards') {
    return (
      <div
        className={`fancy-select fancy-select-cards ${disabled ? 'fancy-select-disabled' : ''} ${className}`}
        style={{ ['--fancy-cols' as string]: columns }}
        role="listbox"
        aria-label={ariaLabel}
        aria-disabled={disabled || undefined}
      >
        {options.map((opt) => {
          const active = opt.value === value
          return (
            <button
              key={opt.value}
              type="button"
              role="option"
              aria-selected={active}
              className={`fancy-select-card ${active ? 'fancy-select-card-active' : ''}`}
              disabled={disabled}
              onClick={() => onChange(opt.value)}
            >
              {opt.preview && (
                <span className={`fancy-select-card-preview ${opt.previewClassName || ''}`}>
                  {opt.preview}
                </span>
              )}
              <span className="fancy-select-card-text">
                <span
                  className="fancy-select-card-label"
                  style={opt.labelStyle}
                  lang={opt.value.length <= 5 ? opt.value : undefined}
                >
                  {opt.label}
                </span>
                {opt.hint && <span className="fancy-select-card-hint">{opt.hint}</span>}
              </span>
              {active ? <SelectionMark size={18} className="fancy-select-card-mark" /> : (
                <span className="fancy-select-check" aria-hidden />
              )}
            </button>
          )
        })}
      </div>
    )
  }

  return (
    <div
      ref={rootRef}
      className={`fancy-select fancy-select-menu ${open ? 'fancy-select-open' : ''} ${disabled ? 'fancy-select-disabled' : ''} ${className}`}
    >
      <button
        type="button"
        className="fancy-select-trigger"
        disabled={disabled}
        aria-haspopup="listbox"
        aria-expanded={open}
        aria-controls={listId}
        aria-label={ariaLabel}
        onClick={() => !disabled && setOpen(v => !v)}
      >
        <span className="fancy-select-trigger-main">
          {selected ? (
            <>
              {selected.preview && (
                <span className={`fancy-select-trigger-preview ${selected.previewClassName || ''}`}>
                  {selected.preview}
                </span>
              )}
              <span className="fancy-select-trigger-label" style={selected.labelStyle}>{selected.label}</span>
            </>
          ) : (
            <span className="fancy-select-trigger-placeholder">{placeholder}</span>
          )}
        </span>
        <span className={`fancy-select-chevron ${open ? 'fancy-select-chevron-up' : ''}`} aria-hidden>
          <svg width="10" height="6" viewBox="0 0 10 6" fill="none">
            <path d="M1 1.2L5 4.8L9 1.2" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round" />
          </svg>
        </span>
      </button>

      {open && (
        <div className="fancy-select-dropdown" role="listbox" id={listId} aria-label={ariaLabel}>
          {options.map((opt, i) => {
            const active = opt.value === value
            return (
              <button
                key={opt.value}
                type="button"
                role="option"
                aria-selected={active}
                className={`fancy-select-option ${active ? 'fancy-select-option-active' : ''}`}
                style={{ animationDelay: `${i * 18}ms` }}
                onClick={() => pick(opt.value)}
              >
                {opt.preview && (
                  <span className={`fancy-select-option-preview ${opt.previewClassName || ''}`}>
                    {opt.preview}
                  </span>
                )}
                <span className="fancy-select-option-text">
                  <span className="fancy-select-option-label" style={opt.labelStyle}>{opt.label}</span>
                  {opt.hint && <span className="fancy-select-option-hint">{opt.hint}</span>}
                </span>
                {active && <SelectionMark size={16} className="fancy-select-option-mark" />}
              </button>
            )
          })}
        </div>
      )}
    </div>
  )
}
