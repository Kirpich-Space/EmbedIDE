import { useEffect, useRef, useState, useCallback, type ReactNode } from 'react'

interface SlidePanelProps {
  visible: boolean
  side: 'left' | 'right' | 'bottom'
  width?: number
  height?: number
  /** Enable drag-to-resize on the inner edge (left/right panels). */
  resizable?: boolean
  minSize?: number
  maxSize?: number
  onSizeChange?: (size: number) => void
  children: ReactNode
}

export function SlidePanel({
  visible,
  side,
  width,
  height,
  resizable = false,
  minSize = 180,
  maxSize = 900,
  onSizeChange,
  children,
}: SlidePanelProps) {
  const [state, setState] = useState<'closed' | 'open' | 'opening' | 'closing'>(
    visible ? 'open' : 'closed'
  )
  const [dragging, setDragging] = useState(false)
  const prevVisible = useRef(visible)
  const sizeRef = useRef(side === 'bottom' ? (height ?? 200) : (width ?? 260))

  useEffect(() => {
    sizeRef.current = side === 'bottom' ? (height ?? 200) : (width ?? 260)
  }, [width, height, side])

  useEffect(() => {
    if (visible && !prevVisible.current) {
      if (state === 'closed') {
        setState('opening')
      } else if (state === 'closing') {
        setState('open')
      }
    } else if (!visible && prevVisible.current) {
      if (state === 'open' || state === 'opening') {
        setState('closing')
      }
    }
    prevVisible.current = visible
  }, [visible, state])

  useEffect(() => {
    if (state === 'opening') {
      const frame = requestAnimationFrame(() => setState('open'))
      return () => cancelAnimationFrame(frame)
    }
  }, [state])

  const onPointerDown = useCallback((e: React.PointerEvent) => {
    if (!resizable || !onSizeChange || side === 'bottom') return
    e.preventDefault()
    e.stopPropagation()
    const startX = e.clientX
    const startSize = sizeRef.current
    setDragging(true)

    const onMove = (ev: PointerEvent) => {
      const dx = ev.clientX - startX
      const delta = side === 'left' ? dx : -dx
      const next = Math.min(maxSize, Math.max(minSize, startSize + delta))
      sizeRef.current = next
      onSizeChange(next)
    }
    const onUp = () => {
      setDragging(false)
      window.removeEventListener('pointermove', onMove)
      window.removeEventListener('pointerup', onUp)
      window.removeEventListener('pointercancel', onUp)
    }
    window.addEventListener('pointermove', onMove)
    window.addEventListener('pointerup', onUp)
    window.addEventListener('pointercancel', onUp)
  }, [resizable, onSizeChange, side, minSize, maxSize])

  function handleTransitionEnd() {
    if (state === 'closing') {
      setState('closed')
    }
  }

  if (state === 'closed') return null

  const isHorizontal = side !== 'bottom'
  const dim = isHorizontal ? (width ?? 260) : (height ?? 200)

  return (
    <div
      className={`slide-panel slide-panel-${side}${dragging ? ' slide-panel-dragging' : ''}${resizable && isHorizontal ? ' slide-panel-resizable' : ''}`}
      onTransitionEnd={handleTransitionEnd}
      style={{
        width: isHorizontal ? (state === 'open' ? dim : 0) : undefined,
        height: !isHorizontal ? (state === 'open' ? dim : 0) : undefined,
        flexShrink: 0,
      }}
    >
      <div className="slide-panel-content" style={isHorizontal ? { minWidth: minSize } : undefined}>
        {children}
      </div>
      {resizable && isHorizontal && (
        <div
          className={`slide-panel-resizer slide-panel-resizer-${side}`}
          onPointerDown={onPointerDown}
          role="separator"
          aria-orientation="vertical"
          aria-valuenow={dim}
          aria-valuemin={minSize}
          aria-valuemax={maxSize}
          title="Drag to resize"
        />
      )}
    </div>
  )
}
