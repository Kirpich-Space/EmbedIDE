import { useState } from 'react'
import { useTheme } from '../themes/ThemeProvider'
import { themes } from '../themes/themes'
import type { EditorSettings } from '../core/types'
import { useTranslation } from '../core/TranslationContext'

interface SettingsProps {
  editorSettings: EditorSettings
  onEditorSettingsChange: (s: EditorSettings) => void
  onClose: () => void
}

const LANGUAGES = [
  { code: 'en', label: 'English' },
  { code: 'ru', label: 'Русский' },
  { code: 'zh', label: '中文' },
  { code: 'ja', label: '日本語' },
  { code: 'de', label: 'Deutsch' },
  { code: 'fr', label: 'Français' },
]

type Tab = 'general' | 'editor' | 'themes'

export function Settings({ editorSettings, onEditorSettingsChange, onClose }: SettingsProps) {
  const { t } = useTranslation()
  const { theme, setTheme } = useTheme()
  const [activeTab, setActiveTab] = useState<Tab>('general')

  const set = (partial: Partial<EditorSettings>) => onEditorSettingsChange({ ...editorSettings, ...partial })

  return (
    <div className="settings-overlay" onClick={e => { if (e.target === e.currentTarget) onClose() }}>
      <div className="settings-modal">
        <div className="settings-header">
          <span className="settings-title">{t('settings.title')}</span>
          <button className="settings-close" onClick={onClose}>×</button>
        </div>

        <div className="settings-tabs">
          <button className={`settings-tab ${activeTab === 'general' ? 'settings-tab-active' : ''}`} onClick={() => setActiveTab('general')}>{t('settings.general')}</button>
          <button className={`settings-tab ${activeTab === 'editor' ? 'settings-tab-active' : ''}`} onClick={() => setActiveTab('editor')}>{t('settings.editor')}</button>
          <button className={`settings-tab ${activeTab === 'themes' ? 'settings-tab-active' : ''}`} onClick={() => setActiveTab('themes')}>{t('settings.themes')}</button>
        </div>

        <div className="settings-body">
          {activeTab === 'general' && (
            <div className="settings-section animate-fade-in">
              <span className="settings-section-title">{t('settings.generalPrefs')}</span>

              <div className="settings-field">
                <label className="settings-label">{t('settings.uiLanguage')}</label>
                <select
                  className="settings-select"
                  value={editorSettings.language}
                  onChange={e => set({ language: e.target.value })}
                >
                  {LANGUAGES.map(l => (
                    <option key={l.code} value={l.code}>{l.label}</option>
                  ))}
                </select>
                <span className="settings-hint">{t('settings.langHint')}</span>
              </div>
            </div>
          )}

          {activeTab === 'editor' && (
            <div className="settings-section animate-fade-in">
              <span className="settings-section-title">{t('settings.editorPrefs')}</span>

              <div className="settings-field">
                <label className="settings-label">{t('settings.fontSize')}</label>
                <div className="settings-field-row">
                  <input type="range" min={10} max={24} step={1}
                    value={editorSettings.fontSize}
                    onChange={e => set({ fontSize: Number(e.target.value) })}
                    className="settings-slider" />
                  <span className="settings-value">{editorSettings.fontSize}px</span>
                </div>
              </div>

              <div className="settings-field">
                <label className="settings-label">{t('settings.tabSize')}</label>
                <div className="settings-field-row">
                  <input type="range" min={2} max={8} step={2}
                    value={editorSettings.tabSize}
                    onChange={e => set({ tabSize: Number(e.target.value) })}
                    className="settings-slider" />
                  <span className="settings-value">{editorSettings.tabSize} {t('settings.spaces')}</span>
                </div>
              </div>

              <div className="settings-field">
                <label className="settings-label">{t('settings.fontFamily')}</label>
                <select className="settings-select"
                  value={editorSettings.fontFamily}
                  onChange={e => set({ fontFamily: e.target.value })}
                >
                  <option value="'JetBrains Mono', monospace">JetBrains Mono</option>
                  <option value="'Fira Code', monospace">Fira Code</option>
                  <option value="'Cascadia Code', monospace">Cascadia Code</option>
                  <option value="'Source Code Pro', monospace">Source Code Pro</option>
                  <option value="monospace">Default Monospace</option>
                </select>
              </div>

              <div className="settings-field">
                <label className="settings-field-row">
                  <input type="checkbox"
                    checked={editorSettings.wordWrap}
                    onChange={e => set({ wordWrap: e.target.checked })} />
                  <span className="settings-label" style={{ margin: 0 }}>{t('settings.wordWrap')}</span>
                </label>
              </div>

              <div className="settings-field">
                <label className="settings-field-row">
                  <input type="checkbox"
                    checked={editorSettings.lineNumbers}
                    onChange={e => set({ lineNumbers: e.target.checked })} />
                  <span className="settings-label" style={{ margin: 0 }}>{t('settings.lineNumbers')}</span>
                </label>
              </div>

              <div className="settings-field">
                <label className="settings-field-row">
                  <input type="checkbox"
                    checked={editorSettings.bracketMatch}
                    onChange={e => set({ bracketMatch: e.target.checked })} />
                  <span className="settings-label" style={{ margin: 0 }}>{t('settings.bracketMatch')}</span>
                </label>
              </div>

              <div className="settings-field">
                <label className="settings-field-row">
                  <input type="checkbox"
                    checked={editorSettings.smoothScroll}
                    onChange={e => set({ smoothScroll: e.target.checked })} />
                  <span className="settings-label" style={{ margin: 0 }}>{t('settings.smoothScroll')}</span>
                </label>
              </div>

              <div className="settings-divider" />

              <div className="settings-field">
                <label className="settings-label">{t('settings.cursorBlink')}</label>
                <div className="settings-field-row">
                  <input type="range" min={0} max={2000} step={100}
                    value={editorSettings.cursorBlinkRate}
                    onChange={e => set({ cursorBlinkRate: Number(e.target.value) })}
                    className="settings-slider" />
                  <span className="settings-value">
                    {editorSettings.cursorBlinkRate === 0 ? t('settings.off') : `${editorSettings.cursorBlinkRate}${t('settings.ms')}`}
                  </span>
                </div>
              </div>
            </div>
          )}

          {activeTab === 'themes' && (
            <div className="settings-section animate-fade-in">
              <span className="settings-section-title">{t('settings.colorTheme')}</span>
              <div className="settings-themes">
                {themes.map(th => (
                  <div key={th.name}
                    className={`settings-theme-card ${theme.name === th.name ? 'settings-theme-active' : ''}`}
                    onClick={() => setTheme(th.name)}
                  >
                    <div className="settings-theme-preview" style={{ background: th.colors.bg }}>
                      <div className="stp-toolbar" style={{ background: th.colors.toolbarBg, borderBottom: `1px solid ${th.colors.border}` }}>
                        <div className="stp-dot" style={{ background: th.colors.accent }} />
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
                    {theme.name === th.name && <div className="settings-theme-check">✓</div>}
                  </div>
                ))}
              </div>
            </div>
          )}
        </div>
      </div>
    </div>
  )
}
