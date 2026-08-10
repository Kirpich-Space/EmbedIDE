/** Detect API keys that cannot work in EmbedIDE (subscription OAuth, cookies, JWTs). */

export type AiCredentialIssue =
  | 'anthropic_oauth'
  | 'looks_invalid'
  | null

export function inspectAiCredential(_providerId: string | undefined, apiKey: string): AiCredentialIssue {
  const key = apiKey.trim()
  if (!key) return null

  const lower = key.toLowerCase()

  // Claude Pro/Max OAuth tokens — blocked for third-party apps
  if (lower.startsWith('sk-ant-oat')) {
    return 'anthropic_oauth'
  }

  // Session cookies / browser dumps
  if (
    /(?:^|[;\s])(?:session(?:-|_)?token|sessionid|__Secure-|cf_clearance|chatcmpl-auth)=/i.test(key) ||
    (key.includes('=') && key.includes(';') && key.length > 80)
  ) {
    return 'looks_invalid'
  }

  // Bare JWT / "Bearer eyJ..." pasted as the whole key field
  if (/^bearer\s+eyj/i.test(key) || /^eyj[a-z0-9_-]+\.[a-z0-9_-]+\./i.test(key)) {
    return 'looks_invalid'
  }

  return null
}

/** Throws a user-facing Error if the key cannot be used for API calls. */
export function assertUsableAiCredential(providerId: string | undefined, apiKey: string): void {
  const issue = inspectAiCredential(providerId, apiKey)
  if (issue === 'anthropic_oauth') {
    throw new Error('ANTHROPIC_OAUTH_REJECTED')
  }
  if (issue === 'looks_invalid') {
    throw new Error('AI_KEY_LOOKS_INVALID')
  }
}
