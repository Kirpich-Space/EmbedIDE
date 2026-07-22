import { useEffect, useRef, useState, type ReactNode } from 'react'

interface SlidePanelProps {
  visible: boolean
  side: 'left' | 'right' | 'bottom'
  width?: number
  height?: number
  children: ReactNode
}

export function SlidePanel({ visible, side, width, height, children }: SlidePanelProps) {
  const [state, setState] = useState<'closed' | 'open' | 'opening' | 'closing'>(
    visible ? 'open' : 'closed'
  )
  const prevVisible = useRef(visible)

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
      className={`slide-panel slide-panel-${side}`}
      onTransitionEnd={handleTransitionEnd}
      style={{
        width: isHorizontal ? (state === 'open' ? dim : 0) : undefined,
        height: !isHorizontal ? (state === 'open' ? dim : 0) : undefined,
        flexShrink: 0,
      }}
    >
      <div className="slide-panel-content">
        {children}
      </div>
    </div>
  )
}
