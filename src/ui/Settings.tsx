import { useState, useEffect, useCallback, useMemo, startTransition, type CSSProperties } from 'react'
import { useTheme } from '../themes/ThemeProvider'
import { themes } from '../themes/themes'
import type { EditorSettings } from '../core/types'
import { useTranslation } from '../core/TranslationContext'
import { fetchOllamaModels, pingOllama, ollamaBaseUrl } from '../core/ollama'
import { AI_PROVIDERS, getAiProvider, migrateAiProvider, providerSupportsSubscription } from '../core/aiProviders'
import { inspectAiCredential } from '../core/aiCredentials'
import { AiProviderIcon } from './AiProviderIcons'
import { FancySelect } from './FancySelect'
import { SelectionMark } from './CheckIcon'
import { type LangCode } from '../core/translations'
import { ACCENT_PRESETS, DEFAULT_EDITOR_SETTINGS } from '../core/defaultSettings'

interface SettingsProps {
  editorSettings: EditorSettings
  onEditorSettingsChange: (s: EditorSettings) => void
  onClose: () => void
}

type CliStatus = Awaited<ReturnType<NonNullable<Window['electronAPI']>['aiCliStatus']>>


const LANGUAGE_META: { code: LangCode; native: string; region: string }[] = [
  { code: 'en', native: 'English', region: 'EN' },
  { code: 'ru', native: 'Русский', region: 'RU' },
  { code: 'zh', native: '中文', region: 'ZH' },
  { code: 'ja', native: '日本語', region: 'JA' },
  { code: 'de', native: 'Deutsch', region: 'DE' },
  { code: 'fr', native: 'Français', region: 'FR' },
]

const FONT_OPTIONS = [
  { value: "'JetBrains Mono', monospace", label: 'JetBrains Mono', hint: 'Bundled' },
  { value: "ui-monospace, 'SFMono-Regular', Menlo, Monaco, Consolas, monospace", label: 'System Mono', hint: 'OS default' },
  { value: "'IBM Plex Sans', 'Segoe UI', sans-serif", label: 'IBM Plex Sans', hint: 'UI / proportional' },
]

const BAUD_OPTIONS = [9600, 19200, 38400, 57600, 115200, 230400, 460800, 921600]

type Tab = 'general' | 'editor' | 'appearance' | 'themes' | 'ai' | 'toolchain'

function Toggle({
  checked, onChange, label,
}: { checked: boolean; onChange: (v: boolean) => void; label: string }) {
  return (
    <label className="settings-field-row settings-toggle-row">
      <span className={`settings-check ${checked ? 'settings-check-on' : ''}`} aria-hidden>
        {checked ? (
          <svg width="10" height="10" viewBox="0 0 16 16" fill="none">
            <path d="M3.25 8.35L6.55 11.55L12.75 4.4" stroke="currentColor" strokeWidth="2.4" strokeLinecap="round" strokeLinejoin="round" />
          </svg>
        ) : null}
      </span>
      <input
        type="checkbox"
        className="settings-check-input"
        checked={checked}
        onChange={e => onChange(e.target.checked)}
      />
      <span className="settings-label" style={{ margin: 0 }}>{label}</span>
    </label>
  )
}

function SliderField({
  label, value, min, max, step, format, onChange,
}: {
  label: string
  value: number
  min: number
  max: number
  step: number
  format: (v: number) => string
  onChange: (v: number) => void
}) {
  const [local, setLocal] = useState(value)
  useEffect(() => { setLocal(value) }, [value])

  return (
    <div className="settings-field">
      <label className="settings-label">{label}</label>
      <div className="settings-field-row">
        <input
          type="range"
          min={min}
          max={max}
          step={step}
          value={local}
          onChange={e => setLocal(Number(e.target.value))}
          onPointerUp={() => { if (local !== value) onChange(local) }}
          onKeyUp={() => { if (local !== value) onChange(local) }}
          onBlur={() => { if (local !== value) onChange(local) }}
          className="settings-slider"
        />
        <span className="settings-value">{format(local)}</span>
      </div>
    </div>
  )
}

export function Settings({ editorSettings, onEditorSettingsChange, onClose }: SettingsProps) {
  const { t } = useTranslation()
  const { theme, setTheme } = useTheme()
  const [activeTab, setActiveTab] = useState<Tab>('general')
  const [tcInfo, setTcInfo] = useState<ToolchainInfo | null>(null)
  const [tcInstalling, setTcInstalling] = useState(false)
  const [tcInstallMsg, setTcInstallMsg] = useState('')
  const [ollamaModels, setOllamaModels] = useState<string[]>([])
  const [ollamaStatus, setOllamaStatus] = useState<'idle' | 'checking' | 'ok' | 'fail'>('idle')
  const [ollamaInfo, setOllamaInfo] = useState('')
  const [cliStatus, setCliStatus] = useState<CliStatus | null>(null)
  const [cliChecking, setCliChecking] = useState(false)

  const providerId = migrateAiProvider(editorSettings)
  const provider = getAiProvider(providerId)
  const authMode = editorSettings.aiAuthMode === 'subscription' ? 'subscription' : 'api'
  const subSupported = providerSupportsSubscription(providerId)
  const useSubscription = subSupported && authMode === 'subscription'
  const keyIssue = !useSubscription ? inspectAiCredential(providerId, editorSettings.aiKey || '') : null
  const subscriptionNote =
    provider.subscriptionNote === 'openai' ? t('settings.aiSubscriptionNoteOpenAI')
    : provider.subscriptionNote === 'claude' ? t('settings.aiSubscriptionNoteClaude')
    : provider.subscriptionNote === 'gemini' ? t('settings.aiSubscriptionNoteGemini')
    : provider.subscriptionNote === 'xai' ? t('settings.aiSubscriptionNoteXai')
    : null
  const set = (partial: Partial<EditorSettings>) => {
    startTransition(() => onEditorSettingsChange({ ...editorSettings, ...partial }))
  }

  const refreshCliStatus = useCallback(async () => {
    if (!subSupported || !window.electronAPI?.aiCliStatus) {
      setCliStatus(null)
      return
    }
    setCliChecking(true)
    try {
      const st = await window.electronAPI.aiCliStatus(providerId)
      setCliStatus(st)
    } catch {
      setCliStatus(null)
    } finally {
      setCliChecking(false)
    }
  }, [providerId, subSupported])

  const languageOptions = useMemo(() => LANGUAGE_META.map(l => ({
    value: l.code,
    label: l.native,
    hint: l.region,
    preview: l.region,
    labelStyle: (l.code === 'zh' || l.code === 'ja')
      ? { fontFamily: l.code === 'zh'
        ? "'Noto Sans SC', 'IBM Plex Sans', sans-serif"
        : "'Noto Sans JP', 'IBM Plex Sans', sans-serif" }
      : undefined,
  })), [])

  const fontOptions = useMemo(() => FONT_OPTIONS.map(f => ({
    value: f.value,
    label: f.label,
    hint: f.hint,
    labelStyle: { fontFamily: f.value } as CSSProperties,
    preview: <span style={{ fontFamily: f.value, fontSize: 14 }}>Aa</span>,
  })), [])

  const accentOptions = useMemo(() => ACCENT_PRESETS.map(p => ({
    value: p.value || '__theme__',
    label: p.id === 'theme' ? t('settings.accentTheme') : p.label,
    hint: p.value || t('settings.accentThemeHint'),
    previewClassName: 'fancy-preview-accent',
    preview: (
      <span
        className={`settings-accent-swatch ${p.id === 'theme' ? 'settings-accent-swatch-theme' : ''}`}
        style={p.value ? { background: p.value } : undefined}
      />
    ),
  })), [t])

  const baudOptions = useMemo(() => BAUD_OPTIONS.map(b => ({
    value: String(b),
    label: String(b),
    hint: 'baud',
  })), [])

  useEffect(() => {
    if (editorSettings.fontFamily.includes('IBM Plex Mono')) {
      set({ fontFamily: DEFAULT_EDITOR_SETTINGS.fontFamily })
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [])

  const refreshOllama = useCallback(async () => {
    if (!editorSettings.aiEnabled || !provider.local) return
    setOllamaStatus('checking')
    const ping = await pingOllama(editorSettings.aiEndpoint)
    if (!ping.ok) {
      setOllamaStatus('fail')
      setOllamaInfo(ping.error || t('settings.ollamaUnreachable'))
      setOllamaModels([])
      return
    }
    try {
      const models = await fetchOllamaModels(editorSettings.aiEndpoint)
      setOllamaModels(models)
      setOllamaStatus('ok')
      setOllamaInfo(ping.version
        ? t('settings.ollamaOkVersion', { version: ping.version, count: models.length })
        : t('settings.ollamaOk', { count: models.length }))
      if (models.length && !models.includes(editorSettings.aiModel)) {
        onEditorSettingsChange({ ...editorSettings, aiModel: models[0] })
      }
    } catch (e) {
      setOllamaStatus('fail')
      setOllamaInfo(e instanceof Error ? e.message : String(e))
    }
  }, [editorSettings, onEditorSettingsChange, provider.local, t])

  useEffect(() => {
    if (activeTab === 'ai' && editorSettings.aiEnabled && provider.local) {
      void refreshOllama()
    }
    if (activeTab === 'toolchain') {
      window.electronAPI?.detectToolchains({ force: true }).then(setTcInfo).catch(() => setTcInfo(null))
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [activeTab, editorSettings.aiEnabled, providerId, editorSettings.aiEndpoint])

  useEffect(() => {
    if (activeTab !== 'toolchain') return
    const off = window.electronAPI?.onToolchainInstallProgress?.(p => {
      setTcInstalling(true)
      if (p.message) setTcInstallMsg(p.message)
      if (p.phase === 'done') {
        setTcInstalling(false)
        window.electronAPI?.detectToolchains({ force: true }).then(setTcInfo)
      }
      if (p.phase === 'error') setTcInstalling(false)
    })
    return () => { off?.() }
  }, [activeTab])

  const accentValue = editorSettings.customAccent || '__theme__'
  const knownAccent = ACCENT_PRESETS.some(p => (p.value || '__theme__') === accentValue)

  return (
    <div className="settings-overlay" onClick={e => { if (e.target === e.currentTarget) onClose() }}>
      <div className="settings-modal settings-modal-wide">
        <div className="settings-header">
          <span className="settings-title">{t('settings.title')}</span>
          <button className="settings-close" onClick={onClose}>×</button>
        </div>

        <div className="settings-tabs">
          {([
            ['general', t('settings.general')],
            ['editor', t('settings.editor')],
            ['appearance', t('settings.appearance')],
            ['themes', t('settings.themes')],
            ['ai', t('settings.ai')],
            ['toolchain', t('settings.toolchain')],
          ] as const).map(([id, label]) => (
            <button
              key={id}
              className={`settings-tab ${activeTab === id ? 'settings-tab-active' : ''}`}
              onClick={() => setActiveTab(id)}
            >
              {label}
            </button>
          ))}
        </div>

        <div className="settings-body">
          {activeTab === 'general' && (
            <div className="settings-section">
              <span className="settings-section-title">{t('settings.generalPrefs')}</span>

              <div className="settings-field">
                <label className="settings-label">{t('settings.uiLanguage')}</label>
                <FancySelect
                  variant="cards"
                  columns={3}
                  aria-label={t('settings.uiLanguage')}
                  value={editorSettings.language}
                  options={languageOptions}
                  onChange={v => set({ language: v })}
                />
                <span className="settings-hint">{t('settings.langHint')}</span>
              </div>

              <div className="settings-divider" />
              <span className="settings-section-title">{t('settings.workflow')}</span>

              <Toggle
                checked={!!editorSettings.autoSave}
                onChange={v => set({ autoSave: v })}
                label={t('settings.autoSave')}
              />
              <span className="settings-hint">{t('settings.autoSaveHint')}</span>

              {editorSettings.autoSave && (
                <SliderField
                  label={t('settings.autoSaveDelay')}
                  value={editorSettings.autoSaveDelayMs}
                  min={500}
                  max={5000}
                  step={100}
                  format={v => `${(v / 1000).toFixed(1)}s`}
                  onChange={v => set({ autoSaveDelayMs: v })}
                />
              )}

              <Toggle
                checked={editorSettings.confirmDelete !== false}
                onChange={v => set({ confirmDelete: v })}
                label={t('settings.confirmDelete')}
              />

              <div className="settings-field">
                <label className="settings-label">{t('settings.defaultBaud')}</label>
                <FancySelect
                  variant="menu"
                  aria-label={t('settings.defaultBaud')}
                  value={String(editorSettings.defaultBaud)}
                  options={baudOptions}
                  onChange={v => set({ defaultBaud: Number(v) })}
                />
              </div>

              <div className="settings-divider" />
              <button
                type="button"
                className="project-btn settings-reset-btn"
                onClick={() => onEditorSettingsChange({
                  ...DEFAULT_EDITOR_SETTINGS,
                  aiKey: editorSettings.aiKey,
                  language: editorSettings.language,
                })}
              >
                {t('settings.resetDefaults')}
              </button>
            </div>
          )}

          {activeTab === 'editor' && (
            <div className="settings-section">
              <span className="settings-section-title">{t('settings.editorPrefs')}</span>

              <SliderField
                label={t('settings.fontSize')}
                value={editorSettings.fontSize}
                min={10}
                max={28}
                step={1}
                format={v => `${v}px`}
                onChange={v => set({ fontSize: v })}
              />

              <SliderField
                label={t('settings.lineHeight')}
                value={Math.round(editorSettings.lineHeight * 100)}
                min={110}
                max={220}
                step={5}
                format={v => v.toFixed(2)}
                onChange={v => set({ lineHeight: v / 100 })}
              />

              <SliderField
                label={t('settings.tabSize')}
                value={editorSettings.tabSize}
                min={2}
                max={8}
                step={1}
                format={v => `${v} ${t('settings.spaces')}`}
                onChange={v => set({ tabSize: v })}
              />

              <SliderField
                label={t('settings.caretWidth')}
                value={editorSettings.caretWidth}
                min={1}
                max={4}
                step={1}
                format={v => `${v}px`}
                onChange={v => set({ caretWidth: v })}
              />

              <div className="settings-field">
                <label className="settings-label">{t('settings.fontFamily')}</label>
                <FancySelect
                  variant="cards"
                  columns={3}
                  aria-label={t('settings.fontFamily')}
                  value={
                    FONT_OPTIONS.some(f => f.value === editorSettings.fontFamily)
                      ? editorSettings.fontFamily
                      : FONT_OPTIONS[0].value
                  }
                  options={fontOptions}
                  onChange={v => set({ fontFamily: v })}
                />
              </div>

              <div className="settings-divider" />
              <span className="settings-section-title">{t('settings.editorBehavior')}</span>

              <div className="settings-toggle-grid">
                <Toggle checked={!!editorSettings.insertSpaces} onChange={v => set({ insertSpaces: v })} label={t('settings.insertSpaces')} />
                <Toggle checked={!!editorSettings.wordWrap} onChange={v => set({ wordWrap: v })} label={t('settings.wordWrap')} />
                <Toggle checked={!!editorSettings.lineNumbers} onChange={v => set({ lineNumbers: v })} label={t('settings.lineNumbers')} />
                <Toggle checked={!!editorSettings.bracketMatch} onChange={v => set({ bracketMatch: v })} label={t('settings.bracketMatch')} />
                <Toggle checked={editorSettings.highlightActiveLine !== false} onChange={v => set({ highlightActiveLine: v })} label={t('settings.highlightActiveLine')} />
                <Toggle checked={editorSettings.foldGutter !== false} onChange={v => set({ foldGutter: v })} label={t('settings.foldGutter')} />
                <Toggle checked={editorSettings.autoComplete !== false} onChange={v => set({ autoComplete: v })} label={t('settings.autoComplete')} />
                <Toggle checked={!!editorSettings.showWhitespace} onChange={v => set({ showWhitespace: v })} label={t('settings.showWhitespace')} />
                <Toggle checked={!!editorSettings.smoothScroll} onChange={v => set({ smoothScroll: v })} label={t('settings.smoothScroll')} />
              </div>

              <div className="settings-divider" />

              <SliderField
                label={t('settings.cursorBlink')}
                value={editorSettings.cursorBlinkRate}
                min={0}
                max={2000}
                step={100}
                format={v => v === 0 ? t('settings.off') : `${v}${t('settings.ms')}`}
                onChange={v => set({ cursorBlinkRate: v })}
              />
            </div>
          )}

          {activeTab === 'appearance' && (
            <div className="settings-section">
              <span className="settings-section-title">{t('settings.appearancePrefs')}</span>

              <div className="settings-field">
                <label className="settings-label">{t('settings.accentColor')}</label>
                <FancySelect
                  variant="cards"
                  columns={4}
                  aria-label={t('settings.accentColor')}
                  value={knownAccent ? accentValue : '__custom__'}
                  options={[
                    ...accentOptions,
                    {
                      value: '__custom__',
                      label: t('settings.accentCustom'),
                      hint: editorSettings.customAccent || '#RRGGBB',
                      previewClassName: 'fancy-preview-accent',
                      preview: (
                        <span
                          className={`settings-accent-swatch ${
                            /^#[0-9A-Fa-f]{6}$/.test(editorSettings.customAccent)
                              ? ''
                              : 'settings-accent-swatch-custom'
                          }`}
                          style={
                            /^#[0-9A-Fa-f]{6}$/.test(editorSettings.customAccent)
                              ? { background: editorSettings.customAccent }
                              : undefined
                          }
                        />
                      ),
                    },
                  ]}
                  onChange={v => {
                    if (v === '__theme__') set({ customAccent: '' })
                    else if (v === '__custom__') {
                      if (!editorSettings.customAccent) set({ customAccent: theme.colors.accent })
                    } else set({ customAccent: v })
                  }}
                />
                {(!knownAccent || accentValue === '__custom__' || (editorSettings.customAccent && !ACCENT_PRESETS.some(p => p.value === editorSettings.customAccent))) && (
                  <div className="settings-field-row" style={{ marginTop: 8 }}>
                    <input
                      type="color"
                      className="settings-color-input"
                      value={/^#[0-9A-Fa-f]{6}$/.test(editorSettings.customAccent) ? editorSettings.customAccent : theme.colors.accent}
                      onChange={e => set({ customAccent: e.target.value.toUpperCase() })}
                    />
                    <input
                      className="settings-select"
                      value={editorSettings.customAccent}
                      placeholder="#FF6B00"
                      onChange={e => {
                        const v = e.target.value.trim()
                        set({ customAccent: v })
                      }}
                      style={{ flex: 1 }}
                    />
                  </div>
                )}
                <span className="settings-hint">{t('settings.accentHint')}</span>
              </div>

              <SliderField
                label={t('settings.uiScale')}
                value={editorSettings.uiScale}
                min={85}
                max={125}
                step={5}
                format={v => `${v}%`}
                onChange={v => set({ uiScale: v })}
              />

              <div className="settings-toggle-grid">
                <Toggle checked={!!editorSettings.compactUi} onChange={v => set({ compactUi: v })} label={t('settings.compactUi')} />
                <Toggle checked={editorSettings.showStatusBar !== false} onChange={v => set({ showStatusBar: v })} label={t('settings.showStatusBar')} />
                <Toggle checked={editorSettings.glassEffects !== false} onChange={v => set({ glassEffects: v })} label={t('settings.glassEffects')} />
                <Toggle checked={!!editorSettings.reduceMotion} onChange={v => set({ reduceMotion: v })} label={t('settings.reduceMotion')} />
              </div>
            </div>
          )}

          {activeTab === 'themes' && (
            <div className="settings-section">
              <span className="settings-section-title">{t('settings.colorTheme')}</span>
              <div className="settings-themes">
                {themes.map(th => (
                  <div
                    key={th.name}
                    className={`settings-theme-card ${theme.name === th.name ? 'settings-theme-active' : ''}`}
                    onClick={() => {
                      setTheme(th.name)
                      set({ theme: th.name })
                    }}
                  >
                    <div className="settings-theme-preview" style={{ background: th.colors.bg }}>
                      <div className="stp-toolbar" style={{ background: th.colors.toolbarBg, borderBottom: `1px solid ${th.colors.border}` }}>
                        <div className="stp-dot" style={{ background: editorSettings.customAccent || th.colors.accent }} />
                        <div className="stp-line" style={{ background: th.colors.border }} />
                      </div>
                      <div className="stp-body">
                        <div className="stp-sidebar" style={{ background: th.colors.sidebarBg, borderRight: `1px solid ${th.colors.border}` }}>
                          <div className="stp-item" style={{ background: th.colors.bgHover }} />
                          <div className="stp-item" style={{ background: th.colors.bgPanel }} />
                        </div>
                        <div className="stp-editor" style={{ background: th.colors.editorBg }}>
                          <div className="stp-line stp-line-hl" style={{ color: th.colors.hlKeyword }} />
                          <div className="stp-line" style={{ color: th.colors.hlType }} />
                          <div className="stp-line" style={{ color: th.colors.hlString }} />
                        </div>
                      </div>
                    </div>
                    <div className="settings-theme-info">
                      <span className="settings-theme-name">{th.name}</span>
                      <span className="settings-theme-type">{th.type === 'dark' ? t('settings.dark') : t('settings.light')}</span>
                    </div>
                    {theme.name === th.name && (
                      <SelectionMark size={22} className="settings-theme-check" />
                    )}
                  </div>
                ))}
              </div>
            </div>
          )}

          {activeTab === 'ai' && (
            <div className="settings-section">
              <span className="settings-section-title">{t('settings.aiPrefs')}</span>

              <Toggle
                checked={!!editorSettings.aiEnabled}
                onChange={v => set({ aiEnabled: v })}
                label={t('settings.aiEnabled')}
              />
              <span className="settings-hint">{t('settings.aiEnabledHint')}</span>

              <Toggle
                checked={editorSettings.aiAutoApplyFiles !== false}
                onChange={v => set({ aiAutoApplyFiles: v })}
                label={t('settings.aiAutoApplyFiles')}
              />
              <span className="settings-hint">{t('settings.aiAutoApplyFilesHint')}</span>

              <div className="settings-field">
                <label className="settings-label">{t('settings.aiProvider')}</label>
                <div className={`settings-provider-grid ${!editorSettings.aiEnabled ? 'settings-provider-grid-disabled' : ''}`}>
                  {AI_PROVIDERS.map(p => {
                    const active = providerId === p.id
                    return (
                      <button
                        key={p.id}
                        type="button"
                        className={`settings-provider-card ${active ? 'settings-provider-card-active' : ''}`}
                        style={{ ['--provider-accent' as string]: p.accent }}
                        disabled={!editorSettings.aiEnabled}
                        onClick={() => {
                          set({
                            aiProvider: p.id,
                            aiMode: p.local ? 'local' : 'cloud',
                            aiEndpoint: p.endpoint,
                            aiModel: p.defaultModel || editorSettings.aiModel,
                          })
                        }}
                      >
                        <span className="settings-provider-logo">
                          <AiProviderIcon id={p.id} />
                        </span>
                        <span className="settings-provider-meta">
                          <span className="settings-provider-name">{p.name}</span>
                          <span className="settings-provider-tags">
                            <span className={`settings-provider-tag ${p.local ? 'local' : 'cloud'}`}>
                              {p.local ? t('settings.aiTagLocal') : t('settings.aiTagCloud')}
                            </span>
                            {p.needsKey && (
                              <span className="settings-provider-tag key">{t('settings.aiTagKey')}</span>
                            )}
                          </span>
                        </span>
                        {active && (
                          <SelectionMark size={18} className="settings-provider-check" />
                        )}
                      </button>
                    )
                  })}
                </div>
                <span className="settings-hint">{provider.hint || t('settings.aiProviderHint')}</span>
              </div>

              {(subscriptionNote || provider.consoleUrl) && editorSettings.aiEnabled && (
                <div className="settings-ai-callout">
                  {subscriptionNote && (
                    <p className="settings-ai-callout-text">{subscriptionNote}</p>
                  )}
                  {provider.consoleUrl && (
                    <a
                      className="settings-ai-callout-link"
                      href={provider.consoleUrl}
                      target="_blank"
                      rel="noreferrer"
                      onClick={e => {
                        e.preventDefault()
                        const url = provider.consoleUrl!
                        if (window.electronAPI?.openExternal) void window.electronAPI.openExternal(url)
                        else window.open(url, '_blank', 'noopener,noreferrer')
                      }}
                    >
                      {t('settings.aiGetKey')}
                    </a>
                  )}
                </div>
              )}

              <div className="settings-field">
                <label className="settings-label">{t('settings.aiEndpoint')}</label>
                <input
                  className="settings-select"
                  value={editorSettings.aiEndpoint}
                  onChange={e => set({ aiEndpoint: e.target.value })}
                  disabled={!editorSettings.aiEnabled}
                  placeholder={provider.endpoint}
                />
                {provider.local && (
                  <span className="settings-hint">{t('settings.ollamaEndpointHint', { base: ollamaBaseUrl(editorSettings.aiEndpoint) })}</span>
                )}
              </div>

              {provider.local && editorSettings.aiEnabled && (
                <div className="settings-field">
                  <div className="settings-field-row" style={{ gap: 8, flexWrap: 'wrap' }}>
                    <button
                      type="button"
                      className="project-btn"
                      onClick={() => void refreshOllama()}
                      disabled={ollamaStatus === 'checking'}
                      style={{ padding: '6px 12px', fontSize: 12 }}
                    >
                      {ollamaStatus === 'checking' ? t('settings.ollamaChecking') : t('settings.ollamaRefresh')}
                    </button>
                    <span className="settings-hint" style={{
                      margin: 0,
                      color: ollamaStatus === 'ok' ? 'var(--accent)' : ollamaStatus === 'fail' ? '#ff6b6b' : undefined,
                    }}>
                      {ollamaInfo}
                    </span>
                  </div>
                </div>
              )}

              <div className="settings-field">
                <label className="settings-label">{t('settings.aiModel')}</label>
                {provider.local && ollamaModels.length > 0 ? (
                  <FancySelect
                    variant="menu"
                    aria-label={t('settings.aiModel')}
                    disabled={!editorSettings.aiEnabled}
                    value={ollamaModels.includes(editorSettings.aiModel) ? editorSettings.aiModel : ollamaModels[0]}
                    options={ollamaModels.map(m => ({ value: m, label: m }))}
                    onChange={v => set({ aiModel: v })}
                  />
                ) : (
                  <>
                    <input
                      className="settings-select"
                      list={`ai-models-${providerId}`}
                      value={editorSettings.aiModel}
                      onChange={e => set({ aiModel: e.target.value })}
                      disabled={!editorSettings.aiEnabled}
                      placeholder={provider.defaultModel || 'model-id'}
                    />
                    {provider.models && provider.models.length > 0 && (
                      <datalist id={`ai-models-${providerId}`}>
                        {provider.models.map(m => (
                          <option key={m} value={m} />
                        ))}
                      </datalist>
                    )}
                  </>
                )}
                {provider.local && ollamaModels.length === 0 && (
                  <span className="settings-hint">{t('settings.ollamaPullHint')}</span>
                )}
              </div>

              <div className="settings-field">
                <label className="settings-label">{t('settings.aiKey')}</label>
                <input
                  className="settings-select"
                  type="password"
                  value={editorSettings.aiKey}
                  onChange={e => set({ aiKey: e.target.value })}
                  disabled={!editorSettings.aiEnabled || !provider.needsKey}
                  placeholder={provider.needsKey ? (provider.keyPlaceholder || 'sk-…') : '—'}
                />
                {keyIssue === 'anthropic_oauth' && (
                  <span className="settings-hint settings-hint-warn">{t('settings.aiOAuthTokenRejected')}</span>
                )}
                {keyIssue === 'looks_invalid' && (
                  <span className="settings-hint settings-hint-warn">{t('settings.aiKeyLooksInvalid')}</span>
                )}
                {!keyIssue && (
                  <span className="settings-hint">{t('settings.aiKeyHint')}</span>
                )}
              </div>
            </div>
          )}

          {activeTab === 'toolchain' && (
            <div className="settings-section">
              <span className="settings-section-title">{t('settings.toolchainTitle')}</span>
              <p className="settings-hint">{t('settings.toolchainHint')}</p>
              <div className="settings-field" style={{ marginTop: 12 }}>
                <div className="settings-hint">
                  {tcInfo?.bundled?.bundled
                    ? `${t('statusBar.bundled')}: ${tcInfo.bundled.root || ''}`
                    : t('settings.toolchainNotBundled')}
                </div>
                <ul className="settings-toolchain-list" style={{ marginTop: 10, paddingLeft: 18, lineHeight: 1.7 }}>
                  {[
                    ['arm-none-eabi-gcc', tcInfo?.armGcc, tcInfo?.armGccVersion, tcInfo?.armGccBundled],
                    ['arm-none-eabi-gdb', tcInfo?.armGdb, tcInfo?.armGdbVersion, tcInfo?.armGdbBundled],
                    ['openocd', tcInfo?.openocd, tcInfo?.openocdVersion, tcInfo?.openocdBundled],
                    ['make', tcInfo?.make, tcInfo?.makeVersion, tcInfo?.makeBundled],
                    ['zig', tcInfo?.zig, tcInfo?.zigVersion, tcInfo?.zigBundled],
                    ['rustc', tcInfo?.rust, tcInfo?.rustVersion, false],
                    ['python', tcInfo?.python, tcInfo?.pythonVersion, tcInfo?.pythonBundled],
                  ].map(([name, ok, ver, bundled]) => (
                    <li key={String(name)}>
                      <strong>{String(name)}</strong>
                      {': '}
                      {ok ? t('common.success') : t('common.error')}
                      {ver ? ` — ${String(ver).slice(0, 80)}` : ''}
                      {bundled ? ` (${t('statusBar.bundled')})` : ''}
                    </li>
                  ))}
                </ul>
                <div style={{ display: 'flex', gap: 10, flexWrap: 'wrap', marginTop: 12 }}>
                  <button
                    className="project-btn project-btn-create"
                    type="button"
                    disabled={tcInstalling}
                    onClick={async () => {
                      setTcInstalling(true)
                      setTcInstallMsg(t('toolchainSetup.starting'))
                      try {
                        const r = await window.electronAPI?.installToolchain({ includeRust: true, force: true })
                        if (r && !r.ok) setTcInstallMsg(r.error || t('toolchainSetup.failed'))
                        else setTcInstallMsg(t('toolchainSetup.success'))
                        window.electronAPI?.detectToolchains({ force: true }).then(setTcInfo)
                      } catch (e) {
                        setTcInstallMsg(e instanceof Error ? e.message : String(e))
                      } finally {
                        setTcInstalling(false)
                      }
                    }}
                  >
                    {tcInstalling ? t('toolchainSetup.downloading') : t('settings.toolchainDownload')}
                  </button>
                  <button
                    className="project-btn"
                    type="button"
                    onClick={() => window.electronAPI?.detectToolchains({ force: true }).then(setTcInfo)}
                  >
                    {t('settings.toolchainRefresh')}
                  </button>
                </div>
                {tcInstallMsg && <div className="settings-hint" style={{ marginTop: 8 }}>{tcInstallMsg}</div>}
              </div>
            </div>
          )}
        </div>
      </div>
    </div>
  )
}
