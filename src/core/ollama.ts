/** Helpers for Ollama / OpenAI-compatible local endpoints */

export function ollamaBaseUrl(endpoint: string): string {
  const raw = (endpoint || 'http://127.0.0.1:11434').trim()
  return raw
    .replace(/\/+$/, '')
    .replace(/\/v1$/i, '')
    .replace(/\/api$/i, '')
}

export function openAiCompatibleUrl(endpoint: string): string {
  const base = ollamaBaseUrl(endpoint)
  // Already points at .../v1
  if (/\/v1$/i.test((endpoint || '').replace(/\/+$/, ''))) {
    return (endpoint || '').replace(/\/+$/, '')
  }
  return `${base}/v1`
}

export async function fetchOllamaModels(endpoint: string, signal?: AbortSignal): Promise<string[]> {
  const base = ollamaBaseUrl(endpoint)
  const res = await fetch(`${base}/api/tags`, { signal })
  if (!res.ok) throw new Error(`Ollama ${res.status}`)
  const data = await res.json() as { models?: { name?: string; model?: string }[] }
  const names = (data.models || [])
    .map(m => m.name || m.model || '')
    .filter(Boolean)
  return [...new Set(names)].sort()
}

export async function pingOllama(endpoint: string, signal?: AbortSignal): Promise<{ ok: boolean; version?: string; error?: string }> {
  try {
    const base = ollamaBaseUrl(endpoint)
    const res = await fetch(`${base}/api/version`, { signal })
    if (!res.ok) {
      // Older Ollama may lack /api/version — try tags
      const tags = await fetch(`${base}/api/tags`, { signal })
      if (!tags.ok) return { ok: false, error: `HTTP ${res.status}` }
      return { ok: true }
    }
    const data = await res.json() as { version?: string }
    return { ok: true, version: data.version }
  } catch (e) {
    return { ok: false, error: e instanceof Error ? e.message : String(e) }
  }
}
