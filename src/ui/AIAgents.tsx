import { useState, useRef, useEffect, useCallback } from 'react'
import type { AgentMessage, ProjectConfig, FileNode, EditorSettings } from '../core/types'
import { useTranslation } from '../core/TranslationContext'

interface AIAgentsProps {
  project: ProjectConfig | null
  files: FileNode[]
  settings: EditorSettings
  onSettingsChange: (s: EditorSettings) => void
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
  const blockRegex = /```[\w]*\s*\n(.*?)\n```/gs
  let match
  while ((match = blockRegex.exec(text)) !== null) {
    const block = match[1]
    const lines = block.split('\n')
    const fileLine = lines.find(l =>
      /^(?:\/\/|#|--)\s*File:\s*(.+)/i.test(l.trim())
    )
    if (fileLine) {
      const fileMatch = fileLine.match(/^(?:\/\/|#|--)\s*File:\s*(.+)/i)
      if (fileMatch) {
        const filePath = fileMatch[1].trim()
        const contentLines = lines.filter(l => l !== fileLine)
        ops.push({ file: filePath, content: contentLines.join('\n') })
      }
    }
  }
  return ops
}

export function AIAgents({ project, files, settings, onSettingsChange }: AIAgentsProps) {
  const { t } = useTranslation()
  const isLocal = settings.aiMode === 'local'
  const endpoint = settings.aiEndpoint || (isLocal ? 'http://127.0.0.1:11434/v1' : 'https://api.openai.com/v1')
  const model = settings.aiModel || (isLocal ? 'llama3.2' : 'gpt-4o')
  const apiKey = settings.aiKey || (isLocal ? 'ollama' : '')

  const agents: AgentDef[] = [
    {
      id: 'chat', icon: '◇', label: t('aiAgents.agents.chat.label'),
      description: t('aiAgents.agents.chat.desc'),
      systemPrompt: 'You are an embedded-systems expert for flight computers and avionics. Help with ARM Cortex-M firmware, STM32, linkers, and OpenOCD.',
    },
    {
      id: 'build', icon: '▣', label: t('aiAgents.agents.build.label'),
      description: t('aiAgents.agents.build.desc'),
      systemPrompt: 'You are a build-system expert. Analyze compiler and linker errors for embedded C/C++/Rust Cortex-M projects.',
    },
    {
      id: 'debug', icon: '◎', label: t('aiAgents.agents.debug.label'),
      description: t('aiAgents.agents.debug.desc'),
      systemPrompt: 'You are a debug expert. Analyze crash dumps, stack traces, and register states for ARM Cortex-M devices.',
    },
    {
      id: 'hardware', icon: '⚡', label: t('aiAgents.agents.hardware.label'),
      description: t('aiAgents.agents.hardware.desc'),
      systemPrompt: 'You are an embedded hardware expert for STM32 flight/avionics MCUs: GPIO, UART, SPI, I2C, TIM, ADC, CAN, ETH. Do not suggest Arduino, ESP32, or AVR platforms.',
    },
    {
      id: 'docs', icon: '☰', label: t('aiAgents.agents.docs.label'),
      description: t('aiAgents.agents.docs.desc'),
      systemPrompt: 'You are a technical writer. Generate clear documentation and code comments for embedded flight-computer projects.',
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
  const abortRef = useRef<AbortController | null>(null)
  const messagesEndRef = useRef<HTMLDivElement>(null)
  const inputRef = useRef<HTMLInputElement>(null)

  const activeMessages = messages[activeAgent] || []
  const agent = agents.find(a => a.id === activeAgent)
  const needsCloudKey = !isLocal && !apiKey.trim()

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
      const conversation = [
        {
          role: 'system',
          content: `${agent?.systemPrompt}\n\nProject context:\n${context}\n\nIMPORTANT: If you want to write code to a file, put a comment like "// File: path/file.rs" on the first line of the code block, then the complete file content.`,
        },
        ...history.map(m => ({ role: m.role as 'user' | 'assistant', content: m.content })),
        { role: 'user', content: text },
      ]

      const url = `${endpoint.replace(/\/+$/, '')}/chat/completions`
      const headers: Record<string, string> = { 'Content-Type': 'application/json' }
      if (apiKey) headers.Authorization = `Bearer ${apiKey}`

      const res = await fetch(url, {
        method: 'POST',
        headers,
        body: JSON.stringify({
          model,
          messages: conversation,
          temperature: 0.3,
        }),
        signal: controller.signal,
      })

      if (!res.ok) {
        const errBody = await res.text().catch(() => '')
        if (isLocal && (res.status === 0 || res.status >= 500 || res.status === 404)) {
          throw new Error(t('aiAgents.localUnreachable'))
        }
        throw new Error(`API ${res.status}: ${errBody || res.statusText}`)
      }

      const data = await res.json()
      const reply = data.choices?.[0]?.message?.content || t('aiAgents.noResponse')
      const ops = parseFileOps(reply)

      if (ops.length > 0) {
        for (const op of ops) {
          const api = window.electronAPI
          if (!api || !project) continue
          const fullPath = op.file.startsWith('/') ? op.file : `${project.dir}/${op.file}`
          try {
            const dir = op.file.includes('/') ? op.file.substring(0, op.file.lastIndexOf('/')) : ''
            if (dir) await api.createProjectFile(project.dir, dir + '/')
            await api.writeProjectFile(fullPath, op.content)
          } catch {
            /* ignore write errors in agent apply */
          }
        }
        setFileOps(ops)
      }

      const assistantMsg: AgentMessage = { role: 'assistant', content: reply, timestamp: Date.now() }
      setMessages(prev => ({
        ...prev,
        [activeAgent]: [...(prev[activeAgent] || []), assistantMsg],
      }))
    } catch (e: unknown) {
      const err = e as { name?: string; message?: string }
      if (err.name === 'AbortError') {
        const cancelledMsg: AgentMessage = { role: 'assistant', content: t('aiAgents.requestCancelled'), timestamp: Date.now() }
        setMessages(prev => ({ ...prev, [activeAgent]: [...(prev[activeAgent] || []), cancelledMsg] }))
      } else {
        const msg = err.message || String(e)
        const friendly = isLocal && /Failed to fetch|NetworkError|ECONNREFUSED/i.test(msg)
          ? t('aiAgents.localUnreachable')
          : msg
        const errorMsg: AgentMessage = { role: 'assistant', content: `**Error**: ${friendly}`, timestamp: Date.now() }
        setMessages(prev => ({ ...prev, [activeAgent]: [...(prev[activeAgent] || []), errorMsg] }))
      }
    } finally {
      setLoading(false)
      abortRef.current = null
    }
  }, [activeAgent, agent, apiKey, endpoint, model, loading, messages, buildContext, project, needsCloudKey, isLocal, t])

  const cancelRequest = useCallback(() => {
    abortRef.current?.abort()
    abortRef.current = null
    setLoading(false)
  }, [])

  const clearConversation = useCallback(() => {
    setMessages(prev => ({ ...prev, [activeAgent]: [] }))
  }, [activeAgent])

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
          <span className="agent-mode-badge">{isLocal ? t('settings.aiLocal') : t('settings.aiCloud')}</span>
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
            {t('aiAgents.written', { files: fileOps.map(o => o.file).join(', ') })}
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
    </div>
  )
}
