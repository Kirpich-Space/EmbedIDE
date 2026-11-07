import { useCallback, useEffect, useRef, useState } from 'react'
import { useTranslation } from '../core/TranslationContext'
import './DebugPanel.css'

export interface DebugConfig {
  mode: 'openocd' | 'remote'
  adapter: string
  target: string
  gdbPort: number
  remote: string
  loadFirmware: boolean
}

interface Variable { name: string; type: string | null; value: string | null; arg: boolean }
interface Register { number: number; name: string; value: string }

interface DebugPanelProps {
  hasProject: boolean
  isFirmware: boolean
  state: 'idle' | 'starting' | 'running' | 'stopped'
  frame: DebugFrame | null
  stopSeq: number
  breakpoints: DebugBreakpoint[]
  config: DebugConfig
  onConfigChange: (c: DebugConfig) => void
  boardDefaults: { adapter: string; target: string }
  error: DebugErrorInfo | null
  onStart: () => void
  onStop: () => void
  onOp: (name: 'continue' | 'pause' | 'stepOver' | 'stepInto' | 'stepOut' | 'stepInstruction' | 'reset') => void
  onRemoveBreakpoint: (bp: DebugBreakpoint) => void
  onClearBreakpoints: () => void
  onJumpToFrame: (frame: DebugFrame) => void
}

const ADAPTERS = ['stlink', 'stlink-dap', 'jlink', 'cmsis-dap', 'xds110', 'ftdi/olimex-arm-usb-ocd-h']

function baseName(p: string) {
  return p.split(/[\\/]/).pop() || p
}

export function DebugPanel(props: DebugPanelProps) {
  const { t } = useTranslation()
  const { state, stopSeq, config, onConfigChange } = props
  const [frames, setFrames] = useState<DebugFrame[]>([])
  const [selectedFrame, setSelectedFrame] = useState(0)
  const [vars, setVars] = useState<Variable[]>([])
  const [regs, setRegs] = useState<Register[]>([])
  const [changedRegs, setChangedRegs] = useState<Set<string>>(new Set())
  const prevRegs = useRef<Map<string, string>>(new Map())
  const [memAddr, setMemAddr] = useState('0x20000000')
  const [memLen, setMemLen] = useState(64)
  const [memText, setMemText] = useState<string[]>([])
  const [expr, setExpr] = useState('')
  const [exprResult, setExprResult] = useState('')
  const [panelError, setPanelError] = useState('')
  const active = state !== 'idle'
  const stopped = state === 'stopped'

  const call = useCallback(async (name: Parameters<NonNullable<typeof window.electronAPI>['debugOp']>[0], args?: Record<string, unknown>) => {
    const r = await window.electronAPI?.debugOp(name, args)
    if (!r) return null
    if (!r.ok) { setPanelError(r.error.message); return null }
    return r.result
  }, [])

  // Refresh views whenever the target stops
  useEffect(() => {
    if (!stopped) return
    let cancelled = false
    ;(async () => {
      setPanelError('')
      const st = await call('stack')
      if (cancelled || !st) return
      setFrames(st.frames || [])
      setSelectedFrame(0)
      const lv = await call('locals', { frame: 0 })
      if (!cancelled && lv) setVars(lv.variables || [])
      const rg = await call('registers')
      if (!cancelled && rg) {
        const list: Register[] = rg.registers || []
        const changed = new Set<string>()
        for (const r of list) {
          const before = prevRegs.current.get(r.name)
          if (before !== undefined && before !== r.value) changed.add(r.name)
        }
        prevRegs.current = new Map(list.map(r => [r.name, r.value]))
        setRegs(list)
        setChangedRegs(changed)
      }
    })()
    return () => { cancelled = true }
  }, [stopSeq, stopped, call])

  useEffect(() => {
    if (!active) {
      setFrames([]); setVars([]); setRegs([]); setMemText([]); setExprResult('')
      prevRegs.current = new Map()
    }
  }, [active])

  const selectFrame = useCallback(async (f: DebugFrame) => {
    setSelectedFrame(f.level)
    props.onJumpToFrame(f)
    const lv = await call('locals', { frame: f.level })
    if (lv) setVars(lv.variables || [])
  }, [call, props])

  const readMemory = useCallback(async () => {
    const r = await call('memory', { address: memAddr.trim(), length: memLen })
    if (!r) return
    const out: string[] = []
    for (const b of r.blocks || []) {
      const base = parseInt(b.begin, 16)
      const hex: string = b.contents || ''
      for (let i = 0; i < hex.length; i += 32) {
        const row = hex.slice(i, i + 32)
        const bytes = row.match(/../g) || []
        const ascii = bytes.map(h => { const c = parseInt(h, 16); return c >= 0x20 && c < 0x7f ? String.fromCharCode(c) : '.' }).join('')
        out.push(`${'0x' + (base + i / 2).toString(16).padStart(8, '0')}  ${bytes.join(' ').padEnd(47)}  ${ascii}`)
      }
    }
    setMemText(out)
  }, [call, memAddr, memLen])

  const evaluate = useCallback(async () => {
    if (!expr.trim()) return
    const r = await call('evaluate', { expression: expr })
    setExprResult(r ? `${expr} = ${r.value}` : '')
  }, [call, expr])

  if (!props.hasProject) {
    return <div className="debug-panel debug-empty">{t('debug.noProject')}</div>
  }
  if (!props.isFirmware) {
    return <div className="debug-panel debug-empty">{t('debug.scriptHint')}</div>
  }

  const err = props.error
  return (
    <div className="debug-panel">
      <div className="debug-toolbar" role="toolbar" aria-label={t('debug.title')}>
        {!active ? (
          <button className="debug-btn debug-btn-start" onClick={props.onStart}>{t('debug.start')} <kbd>F5</kbd></button>
        ) : (
          <>
            <button className="debug-btn" onClick={() => props.onOp('continue')} disabled={!stopped} title="F5">{t('debug.continue')}</button>
            <button className="debug-btn" onClick={() => props.onOp('pause')} disabled={state !== 'running'} title="F6">{t('debug.pause')}</button>
            <button className="debug-btn" onClick={() => props.onOp('stepOver')} disabled={!stopped} title="F10">{t('debug.stepOver')}</button>
            <button className="debug-btn" onClick={() => props.onOp('stepInto')} disabled={!stopped} title="F11">{t('debug.stepInto')}</button>
            <button className="debug-btn" onClick={() => props.onOp('stepOut')} disabled={!stopped} title="Shift+F11">{t('debug.stepOut')}</button>
            <button className="debug-btn" onClick={() => props.onOp('stepInstruction')} disabled={!stopped}>{t('debug.stepInstruction')}</button>
            {config.mode === 'openocd' && (
              <button className="debug-btn" onClick={() => props.onOp('reset')} disabled={state === 'starting'}>{t('debug.reset')}</button>
            )}
            <button className="debug-btn debug-btn-stop" onClick={props.onStop} title="Shift+F5">{t('debug.stop')}</button>
          </>
        )}
        <span className={`debug-state debug-state-${state}`}>{t(`debug.state.${state}`)}</span>
        {props.frame && (
          <span className="debug-where">
            {props.frame.func || '??'}
            {props.frame.file ? ` · ${baseName(props.frame.file)}:${props.frame.line}` : ` · ${props.frame.addr}`}
          </span>
        )}
      </div>

      {(err || panelError) && (
        <div className="debug-error" role="alert">
          <strong>{err ? err.message : panelError}</strong>
          {err?.detail && <pre>{err.detail}</pre>}
        </div>
      )}

      {!active ? (
        <div className="debug-config">
          <label>{t('debug.connection')}
            <select value={config.mode} onChange={e => onConfigChange({ ...config, mode: e.target.value as 'openocd' | 'remote' })}>
              <option value="openocd">{t('debug.modeOpenocd')}</option>
              <option value="remote">{t('debug.modeRemote')}</option>
            </select>
          </label>
          {config.mode === 'openocd' ? (
            <>
              <label>{t('debug.adapter')}
                <input list="debug-adapters" value={config.adapter} placeholder={props.boardDefaults.adapter}
                  onChange={e => onConfigChange({ ...config, adapter: e.target.value.trim() })} />
                <datalist id="debug-adapters">{ADAPTERS.map(a => <option key={a} value={a} />)}</datalist>
              </label>
              <label>{t('debug.target')}
                <input value={config.target} placeholder={props.boardDefaults.target}
                  onChange={e => onConfigChange({ ...config, target: e.target.value.trim() })} />
              </label>
              <label>{t('debug.gdbPort')}
                <input type="number" min={1} max={65535} value={config.gdbPort}
                  onChange={e => onConfigChange({ ...config, gdbPort: Number(e.target.value) })} />
              </label>
            </>
          ) : (
            <label>{t('debug.remote')}
              <input value={config.remote} placeholder="127.0.0.1:3333"
                onChange={e => onConfigChange({ ...config, remote: e.target.value.trim() })} />
            </label>
          )}
          <label className="debug-check">
            <input type="checkbox" checked={config.loadFirmware}
              onChange={e => onConfigChange({ ...config, loadFirmware: e.target.checked })} />
            {t('debug.loadFirmware')}
          </label>
          <p className="debug-hint">{config.mode === 'openocd' ? t('debug.hintOpenocd') : t('debug.hintRemote')}</p>
        </div>
      ) : (
        <div className="debug-grid">
          <section className="debug-section">
            <h4>{t('debug.callStack')}</h4>
            <ul className="debug-list">
              {frames.map(f => (
                <li key={f.level} className={f.level === selectedFrame ? 'selected' : ''}>
                  <button onClick={() => void selectFrame(f)} disabled={!stopped}>
                    <span className="debug-func">{f.func || '??'}</span>
                    <span className="debug-loc">{f.file ? `${baseName(f.file)}:${f.line}` : f.addr}</span>
                  </button>
                </li>
              ))}
              {stopped && frames.length === 0 && <li className="debug-muted">{t('debug.noFrames')}</li>}
              {!stopped && <li className="debug-muted">{t('debug.runningHint')}</li>}
            </ul>
          </section>
          <section className="debug-section">
            <h4>{t('debug.variables')}</h4>
            <table className="debug-table">
              <tbody>
                {vars.map(v => (
                  <tr key={`${v.arg}-${v.name}`}>
                    <td className="debug-name">{v.arg ? <em>{v.name}</em> : v.name}</td>
                    <td className="debug-value">{v.value ?? '{…}'}</td>
                    <td className="debug-type">{v.type}</td>
                  </tr>
                ))}
              </tbody>
            </table>
            {stopped && vars.length === 0 && <div className="debug-muted">{t('debug.noVariables')}</div>}
            <div className="debug-eval">
              <input value={expr} placeholder={t('debug.evalPlaceholder')} disabled={!stopped}
                onChange={e => setExpr(e.target.value)} onKeyDown={e => { if (e.key === 'Enter') void evaluate() }} />
              <button className="debug-btn" onClick={() => void evaluate()} disabled={!stopped || !expr.trim()}>{t('debug.evaluate')}</button>
            </div>
            {exprResult && <div className="debug-mono">{exprResult}</div>}
          </section>
          <section className="debug-section">
            <h4>{t('debug.registers')}</h4>
            <table className="debug-table debug-mono">
              <tbody>
                {regs.map(r => (
                  <tr key={r.number} className={changedRegs.has(r.name) ? 'changed' : ''}>
                    <td className="debug-name">{r.name}</td>
                    <td className="debug-value">{r.value}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </section>
          <section className="debug-section">
            <h4>{t('debug.memory')}</h4>
            <div className="debug-eval">
              <input value={memAddr} onChange={e => setMemAddr(e.target.value)} aria-label={t('debug.address')} disabled={!stopped}
                onKeyDown={e => { if (e.key === 'Enter') void readMemory() }} />
              <input type="number" min={1} max={4096} value={memLen} onChange={e => setMemLen(Number(e.target.value))} aria-label={t('debug.length')} disabled={!stopped} style={{ width: 70 }} />
              <button className="debug-btn" onClick={() => void readMemory()} disabled={!stopped}>{t('debug.read')}</button>
            </div>
            <pre className="debug-mono debug-memory">{memText.join('\n')}</pre>
          </section>
          <section className="debug-section">
            <h4>{t('debug.breakpoints')}
              {props.breakpoints.length > 0 && <button className="debug-link" onClick={props.onClearBreakpoints}>{t('debug.clearAll')}</button>}
            </h4>
            <ul className="debug-list">
              {props.breakpoints.map(bp => (
                <li key={`${bp.file}:${bp.line}`}>
                  <span className={`debug-bp-dot ${bp.verified ? 'verified' : ''}`} title={bp.verified ? t('debug.bpVerified') : t('debug.bpPending')} />
                  <span className="debug-loc">{baseName(bp.file)}:{bp.line}</span>
                  <button className="debug-link" onClick={() => props.onRemoveBreakpoint(bp)} aria-label={t('debug.removeBreakpoint')}>×</button>
                </li>
              ))}
              {props.breakpoints.length === 0 && <li className="debug-muted">{t('debug.noBreakpoints')}</li>}
            </ul>
          </section>
        </div>
      )}
    </div>
  )
}
