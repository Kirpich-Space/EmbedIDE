import { useState, useRef, useEffect, useCallback, useMemo } from 'react'
import { useTranslation } from '../core/TranslationContext'

const BAUDS = [1200, 2400, 4800, 9600, 19200, 38400, 57600, 115200, 230400, 460800, 921600, 1000000, 2000000]
const MAX_LINES = 2000
const MAX_HEX_BYTES = 64 * 1024
const ENDINGS: Record<string, string> = { none: '', lf: '\n', cr: '\r', crlf: '\r\n' }

type ViewMode = 'text' | 'hex'
type Status = 'closed' | 'connecting' | 'open' | 'lost' | 'reconnecting'

/** Parse "0A 0d ff" / "0a0dff" into bytes; null when malformed. */
export function parseHexInput(s: string): Uint8Array | null {
  const clean = s.replace(/0x/gi, '').replace(/[\s,]/g, '')
  if (!clean || clean.length % 2 !== 0 || /[^0-9a-f]/i.test(clean)) return null
  const out = new Uint8Array(clean.length / 2)
  for (let i = 0; i < out.length; i++) out[i] = parseInt(clean.slice(i * 2, i * 2 + 2), 16)
  return out
}

function hexDump(bytes: Uint8Array): string[] {
  const lines: string[] = []
  for (let off = 0; off < bytes.length; off += 16) {
    const row = bytes.subarray(off, off + 16)
    const hex = Array.from(row, b => b.toString(16).padStart(2, '0')).join(' ')
    const ascii = Array.from(row, b => (b >= 0x20 && b < 0x7f ? String.fromCharCode(b) : '.')).join('')
    lines.push(`${off.toString(16).padStart(6, '0')}  ${hex.padEnd(47)}  ${ascii}`)
  }
  return lines
}

export function SerialMonitor({ defaultBaud = 115200 }: { defaultBaud?: number }) {
  const { t } = useTranslation()
  const [ports, setPorts] = useState<SerialPort[]>([])
  const [listError, setListError] = useState('')
  const [selectedPort, setSelectedPort] = useState('')
  const [baud, setBaud] = useState(defaultBaud)
  const [dataBits, setDataBits] = useState<5 | 6 | 7 | 8>(8)
  const [parity, setParity] = useState<'none' | 'odd' | 'even'>('none')
  const [stopBits, setStopBits] = useState<1 | 2>(1)
  const [flow, setFlow] = useState<'none' | 'software' | 'hardware'>('none')
  const [lineEnding, setLineEnding] = useState('lf')
  const [sendHex, setSendHex] = useState(false)
  const [view, setView] = useState<ViewMode>('text')
  const [autoReconnect, setAutoReconnect] = useState(true)
  const [showAdvanced, setShowAdvanced] = useState(false)
  const [status, setStatus] = useState<Status>('closed')
  const [statusMsg, setStatusMsg] = useState('')
  const [lines, setLines] = useState<string[]>([])
  const [partial, setPartial] = useState('')
  const [rawBytes, setRawBytes] = useState<Uint8Array>(new Uint8Array(0))
  const [input, setInput] = useState('')
  const listRef = useRef<HTMLDivElement>(null)
  const decoderRef = useRef(new TextDecoder('utf-8', { fatal: false }))
  const partialRef = useRef('')

  const connected = status === 'open'
  const busy = status !== 'closed'

  useEffect(() => { setBaud(defaultBaud) }, [defaultBaud])

  const refreshPorts = useCallback(async () => {
    const r = await window.electronAPI?.listSerialPorts()
    if (!r) return
    setPorts(r.ports || [])
    setListError(r.ok ? '' : (r.error || t('serial.listFailed')))
  }, [t])

  // Poll for plugged/unplugged devices only while not connected
  useEffect(() => {
    if (busy) return
    void refreshPorts()
    const iv = setInterval(refreshPorts, 3000)
    return () => clearInterval(iv)
  }, [busy, refreshPorts])

  const appendSystem = useCallback((text: string) => {
    setLines(prev => [...prev, `[${text}]`].slice(-MAX_LINES))
  }, [])

  useEffect(() => {
    const api = window.electronAPI
    if (!api) return
    const unsubData = api.onSerialData((chunk) => {
      const bytes = chunk instanceof Uint8Array ? chunk : new Uint8Array(chunk as ArrayLike<number>)
      setRawBytes(prev => {
        const merged = new Uint8Array(Math.min(prev.length + bytes.length, MAX_HEX_BYTES))
        const keepPrev = merged.length - bytes.length
        if (keepPrev > 0) merged.set(prev.subarray(prev.length - keepPrev), 0)
        merged.set(bytes.subarray(Math.max(0, bytes.length - merged.length)), Math.max(0, keepPrev))
        return merged
      })
      // Streaming decode keeps multi-byte characters split across chunks intact
      const text = partialRef.current + decoderRef.current.decode(bytes, { stream: true })
      const parts = text.split('\n')
      partialRef.current = parts.pop() ?? ''
      setPartial(partialRef.current)
      if (parts.length) {
        setLines(prev => [...prev, ...parts.map(p => p.replace(/\r$/, ''))].slice(-MAX_LINES))
      }
    })
    const unsubErr = api.onSerialError((err) => appendSystem(`${t('serial.error')}: ${err}`))
    const unsubStatus = api.onSerialStatus((st) => {
      if (st.state === 'open') {
        setStatus('open')
        setStatusMsg(st.reconnected ? t('serial.statusReconnected', { port: st.port || '' }) : '')
        if (st.reconnected) appendSystem(t('serial.statusReconnected', { port: st.port || '' }))
      } else if (st.state === 'lost') {
        setStatus('lost')
        setStatusMsg(st.message || '')
        appendSystem(`${t('serial.statusLost')}: ${st.message || ''}`)
      } else if (st.state === 'reconnecting') {
        setStatus('reconnecting')
      } else if (st.state === 'closed') {
        setStatus('closed')
        if (st.message) { setStatusMsg(st.message); appendSystem(st.message) }
      }
    })
    return () => {
      unsubData()
      unsubErr()
      unsubStatus()
      void api.disconnectSerial()
    }
  }, [appendSystem, t])

  useEffect(() => {
    if (listRef.current) listRef.current.scrollTop = listRef.current.scrollHeight
  }, [lines, partial, rawBytes, view])

  const handleConnect = useCallback(async () => {
    if (busy) {
      await window.electronAPI?.disconnectSerial()
      setStatus('closed')
      setStatusMsg('')
      appendSystem(t('serial.disconnected'))
      return
    }
    if (!selectedPort) return
    setStatus('connecting')
    setStatusMsg('')
    decoderRef.current = new TextDecoder('utf-8', { fatal: false })
    partialRef.current = ''
    setPartial('')
    const r = await window.electronAPI?.connectSerial({
      port: selectedPort, baud, dataBits, parity, stopBits, flowControl: flow, autoReconnect,
    })
    if (r?.connected) {
      setStatus('open')
      appendSystem(t('serial.connectedTo', { port: selectedPort, baud }))
    } else {
      setStatus('closed')
      const msg = r?.error || t('serial.connectFailed')
      setStatusMsg(msg)
      appendSystem(`${t('serial.error')}: ${msg}`)
    }
  }, [busy, selectedPort, baud, dataBits, parity, stopBits, flow, autoReconnect, appendSystem, t])

  const handleSend = useCallback(async () => {
    if (!connected || !input) return
    let bytes: Uint8Array | null
    if (sendHex) {
      bytes = parseHexInput(input)
      if (!bytes) { setStatusMsg(t('serial.invalidHex')); return }
    } else {
      bytes = new TextEncoder().encode(input + ENDINGS[lineEnding])
    }
    const r = await window.electronAPI?.sendSerial(bytes)
    if (r && !r.ok) {
      appendSystem(`${t('serial.error')}: ${r.error}`)
      return
    }
    setLines(prev => [...prev, `> ${input}`].slice(-MAX_LINES))
    setInput('')
    setStatusMsg('')
  }, [connected, input, sendHex, lineEnding, appendSystem, t])

  const clear = useCallback(() => {
    setLines([])
    setRawBytes(new Uint8Array(0))
    partialRef.current = ''
    setPartial('')
  }, [])

  const hexLines = useMemo(() => (view === 'hex' ? hexDump(rawBytes) : []), [view, rawBytes])
  const shown = view === 'hex' ? hexLines : (partial ? [...lines, partial] : lines)
  const statusText = status === 'open' ? t('serial.statusOpen')
    : status === 'connecting' ? t('serial.statusConnecting')
    : status === 'lost' ? t('serial.statusLost')
    : status === 'reconnecting' ? t('serial.statusReconnecting')
    : t('serial.statusClosed')

  return (
    <div className="serial-monitor">
      <div className="serial-header">
        <span className="serial-title">{t('serial.title')}</span>
        <div className="serial-controls">
          {/* Free text + suggestions: stable /dev/serial/by-id links and COM ports are not always enumerated */}
          <input
            className="serial-baud-input serial-port-input"
            list="serial-port-list"
            value={selectedPort}
            onChange={e => setSelectedPort(e.target.value.trim())}
            disabled={busy}
            placeholder={ports.length ? t('serial.selectPort') : t('serial.noPorts')}
            aria-label={t('serial.selectPort')}
            style={{ minWidth: 200 }}
          />
          <datalist id="serial-port-list">
            {ports.map(p => (
              <option key={p.device} value={p.device}>{p.description || p.kind || ''}</option>
            ))}
          </datalist>
          <button className="serial-btn" onClick={refreshPorts} disabled={busy} title={t('serial.refresh')}>↻</button>
          <select className="serial-select" value={BAUDS.includes(baud) ? baud : ''} onChange={e => setBaud(Number(e.target.value))} disabled={busy} aria-label={t('serial.baud')}>
            {!BAUDS.includes(baud) && <option value="">{baud}</option>}
            {BAUDS.map(b => <option key={b} value={b}>{b}</option>)}
          </select>
          <input
            className="serial-baud-input"
            type="number"
            min={50}
            value={baud}
            onChange={e => setBaud(Number(e.target.value))}
            disabled={busy}
            placeholder={t('serial.customBaud')}
            aria-label={t('serial.customBaud')}
            style={{ width: 90, marginLeft: 4 }}
          />
          <button className="serial-btn" onClick={() => setShowAdvanced(v => !v)} aria-expanded={showAdvanced}>{t('serial.advanced')}</button>
          <button
            className={`serial-btn ${busy ? 'serial-btn-stop' : 'serial-btn-start'}`}
            onClick={handleConnect}
            disabled={!busy && !selectedPort}
          >
            {busy ? t('serial.disconnect') : t('serial.connect')}
          </button>
        </div>
      </div>
      {showAdvanced && (
        <div className="serial-header serial-advanced">
          <label className="serial-field">{t('serial.dataBits')}
            <select className="serial-select" value={dataBits} onChange={e => setDataBits(Number(e.target.value) as 5 | 6 | 7 | 8)} disabled={busy}>
              {[8, 7, 6, 5].map(n => <option key={n} value={n}>{n}</option>)}
            </select>
          </label>
          <label className="serial-field">{t('serial.parity')}
            <select className="serial-select" value={parity} onChange={e => setParity(e.target.value as 'none' | 'odd' | 'even')} disabled={busy}>
              <option value="none">{t('serial.none')}</option>
              <option value="odd">{t('serial.odd')}</option>
              <option value="even">{t('serial.even')}</option>
            </select>
          </label>
          <label className="serial-field">{t('serial.stopBits')}
            <select className="serial-select" value={stopBits} onChange={e => setStopBits(Number(e.target.value) as 1 | 2)} disabled={busy}>
              <option value={1}>1</option>
              <option value={2}>2</option>
            </select>
          </label>
          <label className="serial-field">{t('serial.flow')}
            <select className="serial-select" value={flow} onChange={e => setFlow(e.target.value as 'none' | 'software' | 'hardware')} disabled={busy}>
              <option value="none">{t('serial.none')}</option>
              <option value="software">XON/XOFF</option>
              <option value="hardware">RTS/CTS</option>
            </select>
          </label>
          <label className="serial-field">
            <input type="checkbox" checked={autoReconnect} onChange={e => setAutoReconnect(e.target.checked)} disabled={busy} />
            {t('serial.autoReconnect')}
          </label>
        </div>
      )}
      <div className="serial-header serial-statusbar">
        <span className={`serial-status serial-status-${status}`}>{statusText}</span>
        {(statusMsg || listError) && <span className="serial-status-msg" role="status">{statusMsg || listError}</span>}
        <span style={{ flex: 1 }} />
        <label className="serial-field">{t('serial.view')}
          <select className="serial-select" value={view} onChange={e => setView(e.target.value as ViewMode)}>
            <option value="text">{t('serial.viewText')}</option>
            <option value="hex">{t('serial.viewHex')}</option>
          </select>
        </label>
        <button className="serial-btn" onClick={clear}>{t('serial.clear')}</button>
      </div>
      <div ref={listRef} className={`serial-body ${view === 'hex' ? 'serial-body-hex' : ''}`}>
        {shown.length === 0 && (
          <div className="serial-empty">{t('serial.empty')}</div>
        )}
        {shown.map((line, i) => (
          <div key={i} className="serial-line">{line}</div>
        ))}
      </div>
      <div className="serial-input-row">
        <input
          className="serial-input"
          value={input}
          onChange={e => setInput(e.target.value)}
          onKeyDown={e => { if (e.key === 'Enter') void handleSend() }}
          placeholder={connected ? (sendHex ? t('serial.hexPlaceholder') : t('serial.sendPlaceholder')) : t('serial.connectFirst')}
          disabled={!connected}
          aria-label={t('serial.send')}
        />
        <select className="serial-select" value={sendHex ? 'hex' : lineEnding} onChange={e => {
          if (e.target.value === 'hex') setSendHex(true)
          else { setSendHex(false); setLineEnding(e.target.value) }
        }} disabled={!connected} aria-label={t('serial.lineEnding')}>
          <option value="none">{t('serial.endNone')}</option>
          <option value="lf">LF</option>
          <option value="cr">CR</option>
          <option value="crlf">CRLF</option>
          <option value="hex">{t('serial.sendHex')}</option>
        </select>
        <button className="serial-btn serial-btn-send" onClick={() => void handleSend()} disabled={!connected || !input}>{t('serial.send')}</button>
      </div>
    </div>
  )
}
