import { useState, useRef, useEffect, useCallback } from 'react'
import type { AgentMessage, ProjectConfig, FileNode } from '../core/types'
import { useTranslation } from '../core/TranslationContext'

interface AIAgentsProps {
  project: ProjectConfig | null
  files: FileNode[]
}

type AgentDef = { id: string; icon: string; label: string; description: string; systemPrompt: string }

function renderContent(text: string) {
  const parts = text.split(/(```[\s\S]*?```)/)
  return parts.map((part, i) => {
    if (part.startsWith('```')) {
      const lines = part.split('\n')
      const header = lines[0].slice(3).trim()
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

export function AIAgents({ project, files }: AIAgentsProps) {
  const { t } = useTranslation()

  const agents: AgentDef[] = [
    {
      id: 'chat', icon: '💬', label: t('aiAgents.agents.chat.label'),
      description: t('aiAgents.agents.chat.desc'),
      systemPrompt: 'You are an embedded-systems expert. Help the user with code, debugging, and architecture questions.',
    },
    {
      id: 'build', icon: '🔧', label: t('aiAgents.agents.build.label'),
      description: t('aiAgents.agents.build.desc'),
      systemPrompt: 'You are a build-system expert. Analyze compiler and linker errors, suggest fixes for embedded C/C++/Rust projects.',
    },
    {
      id: 'debug', icon: '🐞', label: t('aiAgents.agents.debug.label'),
      description: t('aiAgents.agents.debug.desc'),
      systemPrompt: 'You are a debug expert. Analyze crash dumps, stack traces, and register states for ARM Cortex-M devices.',
    },
    {
      id: 'hardware', icon: '⚡', label: t('aiAgents.agents.hardware.label'),
      description: t('aiAgents.agents.hardware.desc'),
      systemPrompt: 'You are an embedded hardware expert. Know STM32, ESP32, ARM Cortex, AVR, peripherals (GPIO, UART, SPI, I2C, TIM, ADC).',
    },
    {
      id: 'docs', icon: '📚', label: t('aiAgents.agents.docs.label'),
      description: t('aiAgents.agents.docs.desc'),
      systemPrompt: 'You are a technical writer. Generate clear documentation, code comments, and wiring diagrams for embedded projects.',
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
  const [showApiSetup, setShowApiSetup] = useState(false)
  const [apiKey, setApiKey] = useState(() => localStorage.getItem('embed-ide-api-key') || '')
  const [apiEndpoint, setApiEndpoint] = useState(() => localStorage.getItem('embed-ide-api-endpoint') || 'https://api.openai.com/v1')
  const [apiModel, setApiModel] = useState(() => localStorage.getItem('embed-ide-api-model') || 'gpt-4o')
  const [fileOps, setFileOps] = useState<{ file: string; content: string }[]>([])
  const abortRef = useRef<AbortController | null>(null)
  const messagesEndRef = useRef<HTMLDivElement>(null)
  const inputRef = useRef<HTMLInputElement>(null)

  const activeMessages = messages[activeAgent] || []
  const agent = agents.find(a => a.id === activeAgent)

  useEffect(() => {
    messagesEndRef.current?.scrollIntoView({ behavior: 'smooth' })
  }, [activeMessages])

  useEffect(() => {
    if (!apiKey) setShowApiSetup(true)
  }, [apiKey])

  const saveApiConfig = useCallback(() => {
    localStorage.setItem('embed-ide-api-key', apiKey)
    localStorage.setItem('embed-ide-api-endpoint', apiEndpoint)
    localStorage.setItem('embed-ide-api-model', apiModel)
    setShowApiSetup(false)
  }, [apiKey, apiEndpoint, apiModel])

  const clearApiConfig = useCallback(() => {
    localStorage.removeItem('embed-ide-api-key')
    localStorage.removeItem('embed-ide-api-endpoint')
    localStorage.removeItem('embed-ide-api-model')
    setApiKey('')
    setApiEndpoint('https://api.openai.com/v1')
    setApiModel('gpt-4o')
    setMessages({})
  }, [])

  const clearConversation = useCallback(() => {
    setMessages(prev => ({ ...prev, [activeAgent]: [] }))
  }, [activeAgent])

  const cancelRequest = useCallback(() => {
    abortRef.current?.abort()
    abortRef.current = null
    setLoading(false)
  }, [])

  const buildContext = useCallback((): string => {
    if (!project) return t('aiAgents.noProject')
    const fileList = files.map(f => {
      const prefix = f.type === 'directory' ? '📁' : '📄'
      return `${prefix} ${f.id}${f.language ? ` (${f.language})` : ''}`
    }).join('\n')
    return `Project: ${project.name} (${project.type})\nDirectory: ${project.dir}\n\nFiles:\n${fileList}`
  }, [project, files])

  const sendMessage = useCallback(async (text: string) => {
    if (!text.trim() || loading || !apiKey) return

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
        { role: 'system', content: `${agent?.systemPrompt}\n\nProject context:\n${context}\n\nIMPORTANT: If you want to write code to a file, put a comment like "// File: path/file.rs" or "# File: path/file.py" on the first line of the code block, then the file content after it. Always return the COMPLETE file content, not just the changed parts.` },
        ...history.map(m => ({ role: m.role as 'user' | 'assistant', content: m.content })),
        { role: 'user', content: text },
      ]

      const url = `${apiEndpoint.replace(/\/+$/, '')}/chat/completions`
      const res = await fetch(url, {
        method: 'POST',
        headers: {
          'Content-Type': 'application/json',
          'Authorization': `Bearer ${apiKey}`,
        },
        body: JSON.stringify({
          model: apiModel,
          messages: conversation,
          temperature: 0.3,
        }),
        signal: controller.signal,
      })

      if (!res.ok) {
        const errBody = await res.text().catch(() => '')
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
            if (dir) {
              await api.createProjectFile(project.dir, dir + '/')
            }
            await api.writeProjectFile(fullPath, op.content)
          } catch (e: any) {
            console.error('Failed to write file:', e)
          }
        }
        setFileOps(ops)
      }

      const assistantMsg: AgentMessage = { role: 'assistant', content: reply, timestamp: Date.now() }
      setMessages(prev => ({
        ...prev,
        [activeAgent]: [...(prev[activeAgent] || []), assistantMsg],
      }))
    } catch (e: any) {
      if (e.name === 'AbortError') {
        const cancelledMsg: AgentMessage = { role: 'assistant', content: t('aiAgents.requestCancelled'), timestamp: Date.now() }
        setMessages(prev => ({
          ...prev,
          [activeAgent]: [...(prev[activeAgent] || []), cancelledMsg],
        }))
      } else {
        const errorMsg: AgentMessage = { role: 'assistant', content: `**Error**: ${e.message}`, timestamp: Date.now() }
        setMessages(prev => ({
          ...prev,
          [activeAgent]: [...(prev[activeAgent] || []), errorMsg],
        }))
      }
    } finally {
      setLoading(false)
      abortRef.current = null
    }
  }, [activeAgent, agent, apiKey, apiEndpoint, apiModel, loading, messages, buildContext, project])

  if (showApiSetup) {
    return (
      <div className="ai-agents">
        <div className="panel-header">
          <span className="panel-title">{t('aiAgents.title')}</span>
        </div>
        <div className="agent-api-setup">
          <div className="agent-api-title">{t('aiAgents.configure')}</div>
          <div className="agent-api-desc">
            {t('aiAgents.apiDesc')}
          </div>
          <div className="agent-api-field">
            <label className="agent-api-label">{t('aiAgents.apiEndpoint')}</label>
            <input
              className="agent-api-input"
              value={apiEndpoint}
              onChange={e => setApiEndpoint(e.target.value)}
              placeholder="https://api.openai.com/v1"
            />
          </div>
          <div className="agent-api-field">
            <label className="agent-api-label">{t('aiAgents.model')}</label>
            <input
              className="agent-api-input"
              value={apiModel}
              onChange={e => setApiModel(e.target.value)}
              placeholder="gpt-4o"
            />
          </div>
          <div className="agent-api-field">
            <label className="agent-api-label">{t('aiAgents.apiKey')}</label>
            <input
              className="agent-api-input"
              type="password"
              value={apiKey}
              onChange={e => setApiKey(e.target.value)}
              placeholder="sk-..."
              onKeyDown={e => { if (e.key === 'Enter') saveApiConfig() }}
              autoFocus
            />
          </div>
          <button className="agent-api-btn" onClick={saveApiConfig} disabled={!apiKey.trim()}>
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
          <button className="agent-clear-btn" onClick={clearConversation} title={t('aiAgents.clear')}>{t('aiAgents.clear')}</button>
          <button className="agent-clear-btn" onClick={clearApiConfig} title={t('aiAgents.key')}>{t('aiAgents.key')}</button>
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
            <div className="agent-welcome-title">{agent?.label} Agent</div>
            <div className="agent-welcome-desc">{agent?.description}</div>
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
