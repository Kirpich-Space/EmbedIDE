import { useState, useRef, useEffect, useCallback } from 'react'
import type { AgentMessage, ProjectConfig, FileNode, EditorSettings } from '../core/types'
import { useTranslation } from '../core/TranslationContext'
import { openAiCompatibleUrl } from '../core/ollama'
import { chatCompletion, getAiProvider, migrateAiProvider, type ChatMessage } from '../core/aiProviders'
import { ConfirmDialog } from './ConfirmDialog'

interface AIAgentsProps {
  project: ProjectConfig | null
  files: FileNode[]
  settings: EditorSettings
  onSettingsChange: (s: EditorSettings) => void
  onFilesApplied?: (paths: string[]) => void
}

type AgentDef = { id: string; icon: string; label: string; description: string; systemPrompt: string }

function renderContent(text: string) {
  const parts = text.split(/(```[\s\S]*?```)/)
  return parts.map((part, i) => {
    if (part.startsWith('```')) {
      const lines = part.split('\n')
      const code = lines.slice(1, -1).join('\n')
      return <pre key={i}><code>{code}</code></pre>
    }
    const inlineParts = part.split(/(`[^`]+`)/)
    return inlineParts.map((p, j) => {
      if (p.startsWith('`') && p.endsWith('`')) {
        return <code key={`${i}-${j}`}>{p.slice(1, -1)}</code>
      }
      return <span key={`${i}-${j}`}>{p}</span>
    })
  })
}

function parseFileOps(text: string): { file: string; content: string }[] {
  const ops: { file: string; content: string }[] = []
  const seen = new Set<string>()

  const push = (file: string, content: string) => {
    const f = file.replace(/\\/g, '/').replace(/^\.\//, '').trim()
    if (!f || seen.has(f)) return
    seen.add(f)
    // Drop leading file marker lines from content
    const lines = content.replace(/^\uFEFF/, '').split('\n')
    while (lines.length && /^(?:\/\/|#|--|;)\s*File:\s*/i.test(lines[0].trim())) lines.shift()
    while (lines.length && lines[0].trim() === '') lines.shift()
    ops.push({ file: f, content: lines.join('\n') })
  }

  // ```lang\n// File: path\n...\n```  OR  ```path/to/file.ext\n...\n```
  const fenceRe = /```([^\n`]*)\n([\s\S]*?)```/g
  let m: RegExpExecArray | null
  while ((m = fenceRe.exec(text)) !== null) {
    const header = (m[1] || '').trim()
    const body = m[2] || ''
    const bodyLines = body.split('\n')
    const marker = bodyLines.find(l => /^(?:\/\/|#|--|;)\s*File:\s*(.+)/i.test(l.trim()))
    if (marker) {
      const fm = marker.trim().match(/^(?:\/\/|#|--|;)\s*File:\s*(.+)/i)
      if (fm) push(fm[1], body)
      continue
    }
    // Fence info string looks like a path (has slash or known extension)
    if (/[./]/.test(header) && /\.(c|cc|cpp|h|hpp|rs|zig|s|S|asm|ld|toml|json|md|txt|cmake|mak|py)$/i.test(header)) {
      push(header.replace(/^.*\s+/, ''), body)
      continue
    }
    if (/^(?:[\w./-]+\/)+[\w./-]+\.\w+$/.test(header) || /^[\w.-]+\.(c|cc|cpp|h|hpp|rs|zig|s|S|asm)$/i.test(header)) {
      push(header, body)
    }
  }

  // File: path immediately before a fence
  const beforeRe = /(?:^|\n)(?:\*\*)?(?:File|Файл)\s*:\s*([^\n*`]+)(?:\*\*)?\s*\n\s*```[^\n]*\n([\s\S]*?)```/gi
  while ((m = beforeRe.exec(text)) !== null) {
    push(m[1], m[2])
  }

  return ops
}

function isSafeProjectPath(file: string): boolean {
  const f = file.replace(/\\/g, '/')
  if (!f || f.startsWith('/') || f.includes('..') || /^[A-Za-z]:/.test(f)) return false
  return true
}

export function AIAgents({ project, files, settings, onSettingsChange, onFilesApplied }: AIAgentsProps) {
  const { t } = useTranslation()
  const provider = getAiProvider(migrateAiProvider(settings))
  const isLocal = !!provider.local
  const endpoint = openAiCompatibleUrl(
    settings.aiEndpoint || provider.endpoint || 'http://127.0.0.1:11434',
  )
  const model = settings.aiModel || provider.defaultModel || 'llama3.2'
  const apiKey = settings.aiKey || (isLocal ? 'ollama' : '')

  const agents: AgentDef[] = [
    {
      id: 'chat', icon: '◇', label: t('aiAgents.agents.chat.label'),
      description: t('aiAgents.agents.chat.desc'),
      systemPrompt: 'You are an embedded-systems expert for firmware, device drivers, minimal OS/kernels, ARM Cortex-M, STM32, Zig, Rust, C/C++, Assembly, linkers, and OpenOCD.',
    },
    {
      id: 'build', icon: '▣', label: t('aiAgents.agents.build.label'),
      description: t('aiAgents.agents.build.desc'),
      systemPrompt: 'You are a build-system expert. Analyze compiler and linker errors for embedded C/C++/Rust/Zig/ASM, Make, and Cargo (firmware, drivers, kernels).',
    },
    {
      id: 'debug', icon: '◎', label: t('aiAgents.agents.debug.label'),
      description: t('aiAgents.agents.debug.desc'),
      systemPrompt: 'You are a debug expert. Analyze crash dumps, stack traces, and register states for ARM Cortex-M devices and bare-metal kernels.',
    },
    {
      id: 'hardware', icon: '⚡', label: t('aiAgents.agents.hardware.label'),
      description: t('aiAgents.agents.hardware.desc'),
      systemPrompt: 'You are an embedded hardware and driver expert for STM32 flight/avionics MCUs: GPIO, UART, SPI, I2C, TIM, ADC, CAN, ETH. Help design HAL drivers and OS primitives. Do not suggest Arduino, ESP32, or AVR platforms.',
    },
    {
      id: 'docs', icon: '☰', label: t('aiAgents.agents.docs.label'),
      description: t('aiAgents.agents.docs.desc'),
      systemPrompt: 'You are a technical writer. Generate clear documentation and code comments for firmware, drivers, kernels, and automation scripts.',
    },
  ]

  const suggestions: Record<string, string[]> = {
    chat: [t('aiAgents.suggestions.chat.0'), t('aiAgents.suggestions.chat.1'), t('aiAgents.suggestions.chat.2'), t('aiAgents.suggestions.chat.3')],
    build: [t('aiAgents.suggestions.build.0'), t('aiAgents.suggestions.build.1'), t('aiAgents.suggestions.build.2'), t('aiAgents.suggestions.build.3')],
    debug: [t('aiAgents.suggestions.debug.0'), t('aiAgents.suggestions.debug.1'), t('aiAgents.suggestions.debug.2'), t('aiAgents.suggestions.debug.3')],
    hardware: [t('aiAgents.suggestions.hardware.0'), t('aiAgents.suggestions.hardware.1'), t('aiAgents.suggestions.hardware.2'), t('aiAgents.suggestions.hardware.3')],
    docs: [t('aiAgents.suggestions.docs.0'), t('aiAgents.suggestions.docs.1'), t('aiAgents.suggestions.docs.2'), t('aiAgents.suggestions.docs.3')],
  }

  const [activeAgent, setActiveAgent] = useState('chat')
  const [messages, setMessages] = useState<Record<string, AgentMessage[]>>({})
  const [input, setInput] = useState('')
  const [loading, setLoading] = useState(false)
  const [fileOps, setFileOps] = useState<{ file: string; content: string }[]>([])
  const [confirmApply, setConfirmApply] = useState<{ file: string; content: string }[] | null>(null)
  const abortRef = useRef<AbortController | null>(null)
  const messagesEndRef = useRef<HTMLDivElement>(null)
  const inputRef = useRef<HTMLInputElement>(null)

  const activeMessages = messages[activeAgent] || []
  const agent = agents.find(a => a.id === activeAgent)
  const needsCloudKey = provider.needsKey && !apiKey.trim()

  useEffect(() => {
    messagesEndRef.current?.scrollIntoView({ behavior: 'smooth' })
  }, [activeMessages])

  const buildContext = useCallback((): string => {
    if (!project) return t('aiAgents.noProject')
    const fileList = files.map(f => {
      const prefix = f.type === 'directory' ? '[dir]' : '[file]'
      return `${prefix} ${f.id}${f.language ? ` (${f.language})` : ''}`
    }).join('\n')
    const board = project.boardName ? `\nBoard: ${project.boardName} (${project.boardId || ''})` : ''
    return `Project: ${project.name} (${project.type})${board}\nDirectory: ${project.dir}\n\nFiles:\n${fileList}`
  }, [project, files, t])

  const writeFileOps = useCallback(async (ops: { file: string; content: string }[]) => {
    const api = window.electronAPI
    if (!api || !project || ops.length === 0) return []
    const applied: string[] = []
    for (const op of ops) {
      const rel = op.file.replace(/\\/g, '/')
      if (!isSafeProjectPath(rel)) continue
      try {
        const dir = rel.includes('/') ? rel.substring(0, rel.lastIndexOf('/')) : ''
        if (dir) await api.createProjectFile(project.dir, dir + '/')
        await api.writeProjectFile(project.dir, `${project.dir}/${rel}`, op.content)
        applied.push(rel)
      } catch {
        /* skip failed writes */
      }
    }
    return applied
  }, [project])

  const finishApplied = useCallback((applied: string[]) => {
    setFileOps([])
    if (applied.length === 0) return
    onFilesApplied?.(applied)
    const msg: AgentMessage = {
      role: 'assistant',
      content: t('aiAgents.written', { files: applied.join(', ') }),
      timestamp: Date.now(),
    }
    setMessages(prev => ({
      ...prev,
      [activeAgent]: [...(prev[activeAgent] || []), msg],
    }))
  }, [activeAgent, onFilesApplied, t])

  const sendMessage = useCallback(async (text: string) => {
    if (!text.trim() || loading) return
    if (needsCloudKey) return

    const userMsg: AgentMessage = { role: 'user', content: text, timestamp: Date.now() }
    setMessages(prev => ({
      ...prev,
      [activeAgent]: [...(prev[activeAgent] || []), userMsg],
    }))
    setInput('')
    setLoading(true)
    setFileOps([])

    const controller = new AbortController()
    abortRef.current = controller

    try {
      const context = buildContext()
      const history = (messages[activeAgent] || []).slice(-20)
      const conversation: ChatMessage[] = [
        {
          role: 'system',
          content: `${agent?.systemPrompt}

Project context:
${context}

FILE EDITS (local and cloud models — both may change project files):
When you need to create or overwrite a project file, output ONE fenced code block per file.
Put this as the first line inside the fence (language comment style):
  // File: relative/path/from/project/root.ext
For Zig/ASM you may use: // File: …  or  # File: …  or  ; File: …
Then the FULL file contents. Example:

\`\`\`c
// File: src/main.c
#include <stdint.h>
int main(void) { return 0; }
\`\`\`

Do not use absolute paths. Only relative paths inside the project.`,
        },
        ...history.map((m): ChatMessage => ({
          role: m.role === 'assistant' ? 'assistant' : 'user',
          content: m.content,
        })),
        { role: 'user', content: text },
      ]

      let reply: string
      try {
        reply = await chatCompletion({
          provider,
          endpoint,
          model,
          apiKey,
          messages: conversation,
          signal: controller.signal,
          temperature: 0.3,
        })
      } catch (e: unknown) {
        const msg = e instanceof Error ? e.message : String(e)
        if (isLocal && (/API (0|5\d\d|404):/.test(msg) || /Failed to fetch|NetworkError|ECONNREFUSED/i.test(msg))) {
          throw new Error(t('aiAgents.localUnreachable'))
        }
        throw e
      }
      if (!reply) reply = t('aiAgents.noResponse')

      const assistantMsg: AgentMessage = { role: 'assistant', content: reply, timestamp: Date.now() }
      setMessages(prev => ({
        ...prev,
        [activeAgent]: [...(prev[activeAgent] || []), assistantMsg],
      }))

      const safeOps = parseFileOps(reply).filter(op => isSafeProjectPath(op.file))
      if (safeOps.length > 0) {
        setFileOps(safeOps)
        // Always confirm before writing; auto-apply only auto-opens the dialog
        if (settings.aiAutoApplyFiles !== false) {
          setConfirmApply(safeOps)
        }
      }
    } catch (e: unknown) {
      const err = e as { name?: string; message?: string }
      if (err.name === 'AbortError') {
        const cancelledMsg: AgentMessage = { role: 'assistant', content: t('aiAgents.requestCancelled'), timestamp: Date.now() }
        setMessages(prev => ({ ...prev, [activeAgent]: [...(prev[activeAgent] || []), cancelledMsg] }))
      } else {
        const msg = err.message || String(e)
        let friendly = msg
        if (msg === 'ANTHROPIC_OAUTH_REJECTED') friendly = t('settings.aiOAuthTokenRejected')
        else if (msg === 'AI_KEY_LOOKS_INVALID') friendly = t('settings.aiKeyLooksInvalid')
        else if (isLocal && /Failed to fetch|NetworkError|ECONNREFUSED/i.test(msg)) friendly = t('aiAgents.localUnreachable')
        const errorMsg: AgentMessage = { role: 'assistant', content: `**Error**: ${friendly}`, timestamp: Date.now() }
        setMessages(prev => ({ ...prev, [activeAgent]: [...(prev[activeAgent] || []), errorMsg] }))
      }
    } finally {
      setLoading(false)
      abortRef.current = null
    }
  }, [activeAgent, agent, apiKey, endpoint, model, loading, messages, buildContext, needsCloudKey, isLocal, provider, t, settings.aiAutoApplyFiles, writeFileOps, finishApplied])

  const cancelRequest = useCallback(() => {
    abortRef.current?.abort()
    abortRef.current = null
    setLoading(false)
  }, [])

  const clearConversation = useCallback(() => {
    setMessages(prev => ({ ...prev, [activeAgent]: [] }))
    setFileOps([])
    setConfirmApply(null)
  }, [activeAgent])

  const applyFileOps = useCallback(() => {
    if (fileOps.length === 0) return
    setConfirmApply(fileOps)
  }, [fileOps])

  const discardFileOps = useCallback(() => {
    setFileOps([])
    setConfirmApply(null)
  }, [])

  const confirmApplyFiles = useCallback(async () => {
    const ops = confirmApply
    setConfirmApply(null)
    if (!ops || ops.length === 0) return
    const applied = await writeFileOps(ops)
    finishApplied(applied)
  }, [confirmApply, writeFileOps, finishApplied])

  if (needsCloudKey) {
    return (
      <div className="ai-agents">
        <div className="panel-header">
          <span className="panel-title">{t('aiAgents.title')}</span>
        </div>
        <div className="agent-api-setup">
          <div className="agent-api-title">{t('aiAgents.configure')}</div>
          <div className="agent-api-desc">{t('aiAgents.cloudKeyRequired')}</div>
          <div className="agent-api-field">
            <label className="agent-api-label">{t('aiAgents.apiKey')}</label>
            <input
              className="agent-api-input"
              type="password"
              value={settings.aiKey}
              onChange={e => onSettingsChange({ ...settings, aiKey: e.target.value })}
              placeholder="sk-..."
              autoFocus
            />
          </div>
          <button
            className="agent-api-btn"
            disabled={!settings.aiKey.trim()}
            onClick={() => onSettingsChange({ ...settings, aiKey: settings.aiKey.trim() })}
          >
            {t('aiAgents.saveStart')}
          </button>
        </div>
      </div>
    )
  }

  return (
    <div className="ai-agents">
      <div className="panel-header">
        <span className="panel-title">{t('aiAgents.title')}</span>
        <div className="panel-header-actions">
          <span className="agent-mode-badge">{isLocal ? t('settings.aiLocalOllama') : t('settings.aiCloud')}</span>
          <button className="agent-clear-btn" onClick={clearConversation} title={t('aiAgents.clear')}>{t('aiAgents.clear')}</button>
        </div>
      </div>

      <div className="agent-tabs">
        {agents.map(a => (
          <div
            key={a.id}
            className={`agent-tab ${activeAgent === a.id ? 'agent-tab-active' : ''}`}
            onClick={() => setActiveAgent(a.id)}
            title={a.description}
          >
            <span className="agent-tab-icon">{a.icon}</span>
            <span className="agent-tab-label">{a.label}</span>
          </div>
        ))}
      </div>

      <div className="agent-messages">
        {activeMessages.length === 0 && (
          <div className="agent-welcome">
            <div className="agent-welcome-icon">{agent?.icon}</div>
            <div className="agent-welcome-title">{agent?.label}</div>
            <div className="agent-welcome-desc">{agent?.description}</div>
            {isLocal && <div className="agent-welcome-desc">{t('aiAgents.localHint', { endpoint })}</div>}
            <div className="agent-suggestions">
              {suggestions[activeAgent]?.map(s => (
                <div key={s} className="agent-suggestion" onClick={() => sendMessage(s)}>
                  {s}
                </div>
              ))}
            </div>
          </div>
        )}
        {activeMessages.map((msg, i) => (
          <div key={i} className={`agent-message agent-message-${msg.role}`}>
            <div className="agent-msg-header">
              <span className="agent-msg-role">{msg.role === 'user' ? t('aiAgents.you') : agent?.label}</span>
              <span className="agent-msg-time" style={{ fontSize: 10, opacity: 0.5 }}>{new Date(msg.timestamp).toLocaleTimeString()}</span>
            </div>
            <div className="agent-msg-content">{renderContent(msg.content)}</div>
          </div>
        ))}
        {loading && (
          <div className="agent-loading">
            <div className="agent-loading-dots"><span /><span /><span /></div>
            {t('aiAgents.thinking')}
          </div>
        )}
        {fileOps.length > 0 && !loading && (
          <div className="agent-file-write">
            <div>{t('aiAgents.applyPending', { count: fileOps.length, files: fileOps.map(o => o.file).join(', ') })}</div>
            <div className="agent-file-write-actions">
              <button className="agent-api-btn" onClick={applyFileOps}>{t('aiAgents.apply')}</button>
              <button className="agent-clear-btn" onClick={discardFileOps}>{t('aiAgents.discard')}</button>
            </div>
          </div>
        )}
        <div ref={messagesEndRef} />
      </div>

      <div className="agent-input-area">
        <input
          ref={inputRef}
          className="agent-input"
          type="text"
          placeholder={t('aiAgents.ask')}
          value={input}
          onChange={e => setInput(e.target.value)}
          onKeyDown={e => { if (e.key === 'Enter' && !loading) sendMessage(input) }}
          disabled={loading}
        />
        {loading ? (
          <button className="agent-send-btn agent-cancel-btn" onClick={cancelRequest} title={t('aiAgents.cancel')}>
            <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.5"><rect x="6" y="6" width="12" height="12" rx="2"/></svg>
          </button>
        ) : (
          <button className="agent-send-btn" onClick={() => sendMessage(input)} disabled={!input.trim()}>
            <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.5"><line x1="22" y1="2" x2="11" y2="13"/><polygon points="22 2 15 22 11 13 2 9 22 2"/></svg>
          </button>
        )}
      </div>

      {confirmApply && (
        <ConfirmDialog
          title={t('aiAgents.confirmTitle')}
          message={t('aiAgents.confirmMsg', {
            count: confirmApply.length,
            files: confirmApply.map(o => o.file).join(', '),
          })}
          confirmLabel={t('aiAgents.apply')}
          danger={false}
          onConfirm={() => { void confirmApplyFiles() }}
          onCancel={() => setConfirmApply(null)}
        />
      )}
    </div>
  )
}
