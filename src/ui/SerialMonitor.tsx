import { useState, useRef, useEffect, useCallback } from 'react'
import { useTranslation } from '../core/TranslationContext'

export function SerialMonitor() {
  const { t } = useTranslation()
  const [ports, setPorts] = useState<{device: string, description: string}[]>([])
  const [selectedPort, setSelectedPort] = useState('')
  const [baud, setBaud] = useState(115200)
  const [connected, setConnected] = useState(false)
  const [lines, setLines] = useState<string[]>([])
  const [input, setInput] = useState('')
  const listRef = useRef<HTMLDivElement>(null)

  useEffect(() => {
    const fetch = () => window.electronAPI?.listSerialPorts().then(setPorts)
    fetch()
    const iv = setInterval(fetch, 3000)
    return () => clearInterval(iv)
  }, [])

  useEffect(() => {
    const onData = (data: string) => setLines(prev => [...prev.slice(-999), data])
    const onErr = (err: string) => setLines(prev => [...prev.slice(-999), `[ERROR] ${err}`])
    const unsubData = window.electronAPI?.onSerialData(onData)
    const unsubErr = window.electronAPI?.onSerialError(onErr)
    return () => {
      unsubData?.()
      unsubErr?.()
    }
  }, [])

  useEffect(() => {
    if (listRef.current) listRef.current.scrollTop = listRef.current.scrollHeight
  }, [lines])

  const handleConnect = useCallback(async () => {
    if (!selectedPort) return
    if (connected) {
      await window.electronAPI?.disconnectSerial()
      setConnected(false)
      setLines(prev => [...prev, t('serial.disconnected')])
    } else {
      await window.electronAPI?.connectSerial(selectedPort, baud)
      setConnected(true)
    }
  }, [selectedPort, baud, connected])

  const handleSend = useCallback(() => {
    if (!input || !connected) return
    window.electronAPI?.sendSerial(input)
    setLines(prev => [...prev, `> ${input}`])
    setInput('')
  }, [input, connected])

  const handleKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === 'Enter') handleSend()
  }

  const refreshPorts = async () => {
    const p = await window.electronAPI?.listSerialPorts()
    if (p) setPorts(p)
  }

  return (
    <div className="serial-monitor">
      <div className="serial-header">
        <span className="serial-title">{t('serial.title')}</span>
        <div className="serial-controls">
          <select
            className="serial-select"
            value={selectedPort}
            onChange={e => setSelectedPort(e.target.value)}
            disabled={connected}
          >
            <option value="">{t('serial.selectPort')}</option>
            {ports.map(p => (
              <option key={p.device} value={p.device}>{p.device} — {p.description}</option>
            ))}
          </select>
          <button className="serial-btn" onClick={refreshPorts} disabled={connected} title={t('serial.refresh')}>↻</button>
          <select className="serial-select" value={baud} onChange={e => setBaud(Number(e.target.value))} disabled={connected}>
            {[9600, 19200, 38400, 57600, 115200, 230400, 460800, 921600].map(b => (
              <option key={b} value={b}>{b}</option>
            ))}
          </select>
          <input
            className="serial-baud-input"
            type="number"
            value={baud}
            onChange={e => setBaud(Number(e.target.value))}
            disabled={connected}
            placeholder={t('serial.customBaud')}
            style={{ width: 80, marginLeft: 4 }}
          />
          <button className={`serial-btn ${connected ? 'serial-btn-stop' : 'serial-btn-start'}`} onClick={handleConnect}>
            {connected ? t('serial.disconnect') : t('serial.connect')}
          </button>
        </div>
      </div>
      <div ref={listRef} className="serial-body">
        {lines.length === 0 && (
          <div className="serial-empty">{t('serial.empty')}</div>
        )}
        {lines.map((line, i) => (
          <div key={i} className="serial-line">{line}</div>
        ))}
      </div>
      <div className="serial-input-row">
        <input
          className="serial-input"
          value={input}
          onChange={e => setInput(e.target.value)}
          onKeyDown={handleKeyDown}
          placeholder={connected ? t('serial.sendPlaceholder') : t('serial.connectFirst')}
          disabled={!connected}
        />
        <button className="serial-btn serial-btn-send" onClick={handleSend} disabled={!connected}>{t('serial.send')}</button>
      </div>
    </div>
  )
}
