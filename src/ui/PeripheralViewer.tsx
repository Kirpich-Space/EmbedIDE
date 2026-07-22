import { useState } from 'react'
import { useTranslation } from '../core/TranslationContext'

interface PinState {
  pin: string
  mode: string
  level: string
  function: string
}

interface PeripheralState {
  name: string
  pins: PinState[]
}

interface PeripheralViewerProps {
  peripherals: PeripheralState[]
}

const defaultPeripherals: PeripheralState[] = [
  {
    name: 'GPIOA',
    pins: [
      { pin: 'PA0', mode: 'AF', level: 'HIGH', function: 'USART2_TX' },
      { pin: 'PA1', mode: 'AF', level: 'LOW', function: 'USART2_RX' },
      { pin: 'PA2', mode: 'OUT', level: 'HIGH', function: 'GPIO' },
      { pin: 'PA3', mode: 'IN', level: 'LOW', function: 'GPIO' },
      { pin: 'PA4', mode: 'OUT', level: 'LOW', function: 'SPI1_NSS' },
      { pin: 'PA5', mode: 'AF', level: 'PWM', function: 'SPI1_SCK' },
      { pin: 'PA6', mode: 'AF', level: 'LOW', function: 'SPI1_MISO' },
      { pin: 'PA7', mode: 'AF', level: 'HIGH', function: 'SPI1_MOSI' },
    ],
  },
  {
    name: 'GPIOB',
    pins: [
      { pin: 'PB0', mode: 'OUT', level: 'LOW', function: 'LED_RED' },
      { pin: 'PB1', mode: 'OUT', level: 'HIGH', function: 'LED_GREEN' },
      { pin: 'PB10', mode: 'AF', level: 'HIGH', function: 'I2C2_SCL' },
      { pin: 'PB11', mode: 'AF', level: 'HIGH', function: 'I2C2_SDA' },
    ],
  },
  {
    name: 'GPIOC',
    pins: [
      { pin: 'PC0', mode: 'IN', level: 'LOW', function: 'BUTTON' },
      { pin: 'PC1', mode: 'IN', level: 'HIGH', function: 'SWITCH' },
    ],
  },
]

export function PeripheralViewer({ peripherals }: PeripheralViewerProps) {
  const { t } = useTranslation()
  const [expanded, setExpanded] = useState<Set<string>>(new Set(['GPIOA', 'GPIOB']))
  const data = peripherals.length > 0 ? peripherals : defaultPeripherals

  const toggle = (name: string) => {
    setExpanded(prev => {
      const next = new Set(prev)
      if (next.has(name)) next.delete(name)
      else next.add(name)
      return next
    })
  }

  const pinColor = (level: string) => {
    switch (level) {
      case 'HIGH': return 'var(--hl-string)'
      case 'LOW': return 'var(--text-secondary)'
      case 'PWM': return 'var(--accent)'
      default: return 'var(--text)'
    }
  }

  return (
    <div className="peripheral-viewer">
      <div className="panel-header">
        <span className="panel-title">{t('peripheral.title')}</span>
      </div>
      <div className="peripheral-content">
        {data.map(p => (
          <div key={p.name} className="peripheral-group">
            <div className="peripheral-header" onClick={() => toggle(p.name)}>
              <span className="peripheral-expand">{expanded.has(p.name) ? '▼' : '▶'}</span>
              <span className="peripheral-name">{p.name}</span>
            </div>
            {expanded.has(p.name) && (
              <div className="peripheral-pins">
                {p.pins.map(pin => (
                  <div key={pin.pin} className="peripheral-pin">
                    <span className="pin-name">{pin.pin}</span>
                    <span className="pin-mode">{pin.mode}</span>
                    <span className="pin-level" style={{ color: pinColor(pin.level) }}>{pin.level}</span>
                    <span className="pin-func">{pin.function}</span>
                  </div>
                ))}
              </div>
            )}
          </div>
        ))}
      </div>
    </div>
  )
}
