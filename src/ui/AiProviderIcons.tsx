import type { AiProviderId } from '../core/aiProviders'

const LOGOS: Record<string, string> = {
  ollama: './ai-providers/ollama.svg',
  openai: './ai-providers/openai.svg',
  claude: './ai-providers/claude.svg',
  gemini: './ai-providers/gemini.svg',
  xai: './ai-providers/xai.svg',
  openrouter: './ai-providers/openrouter.svg',
  groq: './ai-providers/groq.svg',
  deepseek: './ai-providers/deepseek.svg',
  qwen: './ai-providers/qwen.svg',
  kimi: './ai-providers/kimi.svg',
  together: './ai-providers/together.svg',
  mistral: './ai-providers/mistral.svg',
  fireworks: './ai-providers/fireworks.svg',
  custom: './ai-providers/custom.svg',
}

/** Real provider logos (bundled under public/ai-providers). */
export function AiProviderIcon({ id }: { id: AiProviderId | string }) {
  const src = LOGOS[id] || LOGOS.custom
  return (
    <img
      className="settings-provider-icon-img"
      src={src}
      alt=""
      width={22}
      height={22}
      draggable={false}
    />
  )
}
