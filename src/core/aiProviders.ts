/** Presets for chat providers (OpenAI-compatible + Anthropic Messages) */

export type AiProviderId =
  | 'ollama'
  | 'openai'
  | 'claude'
  | 'openrouter'
  | 'groq'
  | 'deepseek'
  | 'qwen'
  | 'kimi'
  | 'together'
  | 'mistral'
  | 'fireworks'
  | 'custom'

export type AiApiStyle = 'openai' | 'anthropic'

export interface AiProviderPreset {
  id: AiProviderId
  name: string
  /** Short label under the logo */
  shortName: string
  endpoint: string
  defaultModel: string
  needsKey: boolean
  local?: boolean
  /** openai = /chat/completions; anthropic = /messages */
  apiStyle?: AiApiStyle
  /** Suggested models shown in Settings datalist */
  models?: string[]
  /** Extra request headers (e.g. OpenRouter) */
  headers?: Record<string, string>
  hint?: string
  /** Accent used on the provider card */
  accent: string
}

export const AI_PROVIDERS: AiProviderPreset[] = [
  {
    id: 'ollama',
    name: 'Ollama',
    shortName: 'Local',
    endpoint: 'http://127.0.0.1:11434',
    defaultModel: 'llama3.2',
    needsKey: false,
    local: true,
    models: ['llama3.2', 'llama3.1', 'codellama', 'qwen2.5-coder', 'deepseek-coder-v2', 'mistral'],
    hint: 'Local OpenAI-compatible server (ollama serve)',
    accent: '#D4CFC4',
  },
  {
    id: 'openai',
    name: 'OpenAI',
    shortName: 'GPT',
    endpoint: 'https://api.openai.com/v1',
    defaultModel: 'gpt-4o',
    needsKey: true,
    models: ['gpt-4o', 'gpt-4o-mini', 'gpt-4.1', 'o4-mini', 'o3-mini'],
    accent: '#10A37F',
  },
  {
    id: 'claude',
    name: 'Claude',
    shortName: 'Anthropic',
    endpoint: 'https://api.anthropic.com/v1',
    defaultModel: 'claude-sonnet-4-5',
    needsKey: true,
    apiStyle: 'anthropic',
    models: [
      'claude-sonnet-4-5',
      'claude-opus-4-5',
      'claude-haiku-4-5',
      'claude-sonnet-4-20250514',
      'claude-3-5-haiku-latest',
    ],
    hint: 'Anthropic Messages API (x-api-key). Get a key at console.anthropic.com',
    accent: '#D97757',
  },
  {
    id: 'openrouter',
    name: 'OpenRouter',
    shortName: 'Multi',
    endpoint: 'https://openrouter.ai/api/v1',
    defaultModel: 'openrouter/auto',
    needsKey: true,
    models: [
      'openrouter/auto',
      'anthropic/claude-sonnet-4',
      'openai/gpt-4o',
      'google/gemini-2.5-pro',
      'meta-llama/llama-3.3-70b-instruct',
      'qwen/qwen3-coder',
      'deepseek/deepseek-chat',
    ],
    headers: {
      'HTTP-Referer': 'https://github.com/Kirpich-Space/EmbedIDE',
      'X-Title': 'EmbedIDE',
    },
    hint: 'One key for many models (Claude, GPT, Gemini, Llama…)',
    accent: '#A78BFA',
  },
  {
    id: 'groq',
    name: 'Groq',
    shortName: 'Fast',
    endpoint: 'https://api.groq.com/openai/v1',
    defaultModel: 'llama-3.3-70b-versatile',
    needsKey: true,
    models: ['llama-3.3-70b-versatile', 'llama-3.1-8b-instant', 'mixtral-8x7b-32768', 'gemma2-9b-it'],
    accent: '#F55036',
  },
  {
    id: 'deepseek',
    name: 'DeepSeek',
    shortName: 'Code',
    endpoint: 'https://api.deepseek.com/v1',
    defaultModel: 'deepseek-chat',
    needsKey: true,
    models: ['deepseek-chat', 'deepseek-reasoner', 'deepseek-coder'],
    accent: '#4D6BFE',
  },
  {
    id: 'qwen',
    name: 'Qwen',
    shortName: 'Alibaba',
    endpoint: 'https://dashscope-intl.aliyuncs.com/compatible-mode/v1',
    defaultModel: 'qwen-plus',
    needsKey: true,
    models: [
      'qwen-plus',
      'qwen-max',
      'qwen-turbo',
      'qwen3-coder-plus',
      'qwen2.5-coder-32b-instruct',
    ],
    hint: 'DashScope OpenAI-compatible (intl). CN: dashscope.aliyuncs.com/compatible-mode/v1',
    accent: '#615CED',
  },
  {
    id: 'kimi',
    name: 'Kimi',
    shortName: 'Moonshot',
    endpoint: 'https://api.moonshot.ai/v1',
    defaultModel: 'kimi-k2-0905-preview',
    needsKey: true,
    models: [
      'kimi-k2-0905-preview',
      'kimi-latest',
      'moonshot-v1-128k',
      'moonshot-v1-32k',
      'moonshot-v1-8k',
    ],
    hint: 'Moonshot AI OpenAI-compatible. CN: api.moonshot.cn/v1',
    accent: '#1783FF',
  },
  {
    id: 'together',
    name: 'Together AI',
    shortName: 'Open',
    endpoint: 'https://api.together.xyz/v1',
    defaultModel: 'meta-llama/Meta-Llama-3.1-70B-Instruct-Turbo',
    needsKey: true,
    models: [
      'meta-llama/Meta-Llama-3.1-70B-Instruct-Turbo',
      'meta-llama/Meta-Llama-3.1-8B-Instruct-Turbo',
      'Qwen/Qwen2.5-Coder-32B-Instruct',
    ],
    accent: '#0EA5E9',
  },
  {
    id: 'mistral',
    name: 'Mistral AI',
    shortName: 'EU',
    endpoint: 'https://api.mistral.ai/v1',
    defaultModel: 'mistral-large-latest',
    needsKey: true,
    models: ['mistral-large-latest', 'mistral-small-latest', 'codestral-latest', 'pixtral-large-latest'],
    accent: '#FF7000',
  },
  {
    id: 'fireworks',
    name: 'Fireworks',
    shortName: 'Infer',
    endpoint: 'https://api.fireworks.ai/inference/v1',
    defaultModel: 'accounts/fireworks/models/llama-v3p1-70b-instruct',
    needsKey: true,
    models: [
      'accounts/fireworks/models/llama-v3p1-70b-instruct',
      'accounts/fireworks/models/qwen2p5-coder-32b-instruct',
    ],
    accent: '#FF4D8D',
  },
  {
    id: 'custom',
    name: 'Custom',
    shortName: 'API',
    endpoint: 'http://127.0.0.1:8080/v1',
    defaultModel: '',
    needsKey: false,
    hint: 'Any /v1/chat/completions compatible endpoint (LM Studio, vLLM, LiteLLM…)',
    accent: '#94A3B8',
  },
]

export function getAiProvider(id: string | undefined): AiProviderPreset {
  return AI_PROVIDERS.find(p => p.id === id) || AI_PROVIDERS[0]
}

/** Migrate legacy aiMode local|cloud → provider id */
export function migrateAiProvider(settings: {
  aiProvider?: string
  aiMode?: string
}): AiProviderId {
  if (settings.aiProvider && AI_PROVIDERS.some(p => p.id === settings.aiProvider)) {
    return settings.aiProvider as AiProviderId
  }
  if (settings.aiMode === 'cloud') return 'openai'
  return 'ollama'
}

export type ChatMessage = { role: 'system' | 'user' | 'assistant'; content: string }

/** Call OpenAI-compatible or Anthropic Messages chat API; returns assistant text. */
export async function chatCompletion(opts: {
  provider: AiProviderPreset
  endpoint: string
  model: string
  apiKey: string
  messages: ChatMessage[]
  signal?: AbortSignal
  temperature?: number
}): Promise<string> {
  const { provider, endpoint, model, apiKey, messages, signal, temperature = 0.3 } = opts
  const base = endpoint.replace(/\/+$/, '')
  const style = provider.apiStyle || 'openai'

  if (style === 'anthropic') {
    const system = messages.find(m => m.role === 'system')?.content
    const anthropicMessages = messages
      .filter(m => m.role !== 'system')
      .map(m => ({ role: m.role as 'user' | 'assistant', content: m.content }))

    const headers: Record<string, string> = {
      'Content-Type': 'application/json',
      'anthropic-version': '2023-06-01',
      ...(provider.headers || {}),
    }
    if (apiKey) headers['x-api-key'] = apiKey

    const res = await fetch(`${base}/messages`, {
      method: 'POST',
      headers,
      body: JSON.stringify({
        model,
        max_tokens: 8192,
        temperature,
        ...(system ? { system } : {}),
        messages: anthropicMessages,
      }),
      signal,
    })

    if (!res.ok) {
      const errBody = await res.text().catch(() => '')
      throw new Error(`API ${res.status}: ${errBody || res.statusText}`)
    }

    const data = await res.json()
    const parts = Array.isArray(data.content) ? data.content : []
    const text = parts
      .filter((p: { type?: string }) => p.type === 'text')
      .map((p: { text?: string }) => p.text || '')
      .join('')
    if (text) return text
    if (typeof data.error?.message === 'string') throw new Error(data.error.message)
    return ''
  }

  const headers: Record<string, string> = {
    'Content-Type': 'application/json',
    ...(provider.headers || {}),
  }
  if (apiKey) headers.Authorization = `Bearer ${apiKey}`

  const res = await fetch(`${base}/chat/completions`, {
    method: 'POST',
    headers,
    body: JSON.stringify({
      model,
      messages,
      temperature,
      max_tokens: 8192,
    }),
    signal,
  })

  if (!res.ok) {
    const errBody = await res.text().catch(() => '')
    throw new Error(`API ${res.status}: ${errBody || res.statusText}`)
  }

  const data = await res.json()
  return data.choices?.[0]?.message?.content || ''
}
