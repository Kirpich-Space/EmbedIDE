/** Check stroke — inherits container color (dark on accent disc). */
export function CheckIcon({
  size = 12,
  color = 'currentColor',
}: {
  size?: number
  color?: string
}) {
  return (
    <svg
      width={size}
      height={size}
      viewBox="0 0 16 16"
      fill="none"
      xmlns="http://www.w3.org/2000/svg"
      aria-hidden
      style={{ display: 'block', flexShrink: 0, pointerEvents: 'none' }}
    >
      <path
        d="M3.25 8.35L6.55 11.55L12.75 4.4"
        stroke={color}
        strokeWidth="2.35"
        strokeLinecap="round"
        strokeLinejoin="round"
      />
    </svg>
  )
}

/** Accent-colored disc + dark check — container matches theme accent, not white. */
export function SelectionMark({
  size = 18,
  className = '',
}: {
  size?: number
  className?: string
}) {
  const icon = Math.max(9, Math.round(size * 0.55))
  return (
    <span
      className={`selection-mark ${className}`.trim()}
      style={{
        width: size,
        height: size,
        minWidth: size,
        minHeight: size,
      }}
      aria-hidden
    >
      <CheckIcon size={icon} />
    </span>
  )
}
