/**
 * Local CLI bridges for ChatGPT (Codex), Claude Code, and Grok Build subscriptions.
 * API keys remain the default path in the renderer; this module is for aiAuthMode=subscription.
 */
const { spawn, spawnSync } = require('child_process')
const fs = require('fs')
const os = require('os')
const path = require('path')

const SUBSCRIPTION_PROVIDERS = new Set(['openai', 'claude', 'xai'])

/** @type {import('child_process').ChildProcess | null} */
let currentCliProc = null

const CLI_META = {
  openai: {
    bin: 'codex',
    loginHint: 'codex login',
    installHint: 'https://developers.openai.com/codex/cli',
    authPaths: () => [
      path.join(os.homedir(), '.codex', 'auth.json'),
    ],
  },
  claude: {
    bin: 'claude',
    loginHint: 'claude /login   (or: claude auth login)',
    installHint: 'https://docs.anthropic.com/en/docs/claude-code',
    authPaths: () => [
      path.join(os.homedir(), '.claude', '.credentials.json'),
      path.join(os.homedir(), '.claude', 'credentials.json'),
      path.join(os.homedir(), '.config', 'claude', 'credentials.json'),
    ],
  },
  xai: {
    bin: 'grok',
    loginHint: 'grok login',
    installHint: 'https://x.ai/cli/install.sh',
    authPaths: () => [
      path.join(os.homedir(), '.grok', 'auth.json'),
      path.join(os.homedir(), '.config', 'grok', 'auth.json'),
    ],
  },
}

function which(cmd) {
  try {
    if (process.platform === 'win32') {
      const r = spawnSync('where', [cmd], { encoding: 'utf8', timeout: 5000 })
      const line = (r.stdout || '').split(/\r?\n/).map(s => s.trim()).find(Boolean)
      return line || null
    }
    const r = spawnSync('which', [cmd], { encoding: 'utf8', timeout: 5000 })
    const line = (r.stdout || '').trim().split(/\n/)[0]
    return line || null
  } catch {
    return null
  }
}

function fileLooksAuthed(p) {
  try {
    if (!fs.existsSync(p)) return false
    const st = fs.statSync(p)
    if (!st.isFile() || st.size < 8) return false
    const raw = fs.readFileSync(p, 'utf8')
    if (!raw.trim()) return false
    try {
      const j = JSON.parse(raw)
      if (j && typeof j === 'object') {
        if (j.access_token || j.refresh_token || j.token || j.tokens) return true
        if (j.claudeAiOauth || j.oauth || j.credentials) return true
        // non-empty object is a weak positive
        return Object.keys(j).length > 0
      }
    } catch {
      return raw.length > 20
    }
    return true
  } catch {
    return false
  }
}

function probeLoggedIn(providerId, binPath) {
  const meta = CLI_META[providerId]
  if (!meta) return false
  if (meta.authPaths().some(fileLooksAuthed)) return true

  // Soft probe: short version / whoami style (best-effort, ignore failures)
  try {
    if (providerId === 'claude' && binPath) {
      const r = spawnSync(binPath, ['auth', 'status'], {
        encoding: 'utf8',
        timeout: 8000,
        env: process.env,
      })
      const out = `${r.stdout || ''}${r.stderr || ''}`.toLowerCase()
      if (/logged in|authenticated|email|account/i.test(out) && !/not logged|unauthenticated|no account/i.test(out)) {
        return true
      }
    }
    if (providerId === 'openai' && binPath) {
      const r = spawnSync(binPath, ['login', 'status'], {
        encoding: 'utf8',
        timeout: 8000,
        env: process.env,
      })
      const out = `${r.stdout || ''}${r.stderr || ''}`.toLowerCase()
      if (/logged in|authenticated|account/i.test(out) && !/not logged|log in/i.test(out)) {
        return true
      }
    }
    if (providerId === 'xai' && binPath) {
      const r = spawnSync(binPath, ['inspect', '--json'], {
        encoding: 'utf8',
        timeout: 8000,
        env: process.env,
      })
      if (r.status === 0 && (r.stdout || '').trim()) return true
    }
  } catch {}
  return false
}

function getVersion(binPath) {
  try {
    const r = spawnSync(binPath, ['--version'], { encoding: 'utf8', timeout: 5000 })
    const line = `${r.stdout || ''}${r.stderr || ''}`.split(/\r?\n/).map(s => s.trim()).find(Boolean)
    return line || null
  } catch {
    return null
  }
}

function supportsSubscription(providerId) {
  return SUBSCRIPTION_PROVIDERS.has(providerId)
}

function getCliStatus(providerId) {
  if (!supportsSubscription(providerId)) {
    return {
      providerId,
      supported: false,
      found: false,
      loggedIn: false,
      bin: null,
      path: null,
      version: null,
      loginHint: '',
      installHint: '',
    }
  }
  const meta = CLI_META[providerId]
  const binPath = which(meta.bin)
  const found = !!binPath
  const loggedIn = found ? probeLoggedIn(providerId, binPath) : false
  return {
    providerId,
    supported: true,
    found,
    loggedIn,
    bin: meta.bin,
    path: binPath,
    version: found ? getVersion(binPath) : null,
    loginHint: meta.loginHint,
    installHint: meta.installHint,
  }
}

function cancelCliChat() {
  if (!currentCliProc) return false
  try {
    if (process.platform === 'win32' && currentCliProc.pid) {
      spawn('taskkill', ['/pid', String(currentCliProc.pid), '/f', '/t'])
    } else {
      currentCliProc.kill('SIGTERM')
    }
  } catch {}
  currentCliProc = null
  return true
}

function runSpawn(bin, args, { cwd, input, timeoutMs = 180000 } = {}) {
  return new Promise((resolve, reject) => {
    cancelCliChat()
    const proc = spawn(bin, args, {
      cwd: cwd || os.tmpdir(),
      env: { ...process.env, NO_COLOR: '1', CI: '1' },
      stdio: ['pipe', 'pipe', 'pipe'],
      windowsHide: true,
    })
    currentCliProc = proc
    let stdout = ''
    let stderr = ''
    let settled = false

    const timer = setTimeout(() => {
      try { proc.kill('SIGTERM') } catch {}
      if (!settled) {
        settled = true
        currentCliProc = null
        reject(new Error(`CLI timed out after ${Math.round(timeoutMs / 1000)}s`))
      }
    }, timeoutMs)

    proc.stdout.on('data', d => { stdout += d.toString() })
    proc.stderr.on('data', d => { stderr += d.toString() })
    proc.on('error', err => {
      clearTimeout(timer)
      if (settled) return
      settled = true
      currentCliProc = null
      reject(new Error(`${path.basename(bin)} failed to start: ${err.message}`))
    })
    proc.on('close', code => {
      clearTimeout(timer)
      if (settled) return
      settled = true
      currentCliProc = null
      resolve({ code: code ?? 1, stdout, stderr })
    })

    if (input != null) {
      try {
        proc.stdin.write(input)
        proc.stdin.end()
      } catch {}
    } else {
      try { proc.stdin.end() } catch {}
    }
  })
}

function flattenMessages(messages) {
  const parts = []
  for (const m of messages || []) {
    const role = m.role || 'user'
    const content = String(m.content || '')
    if (!content.trim()) continue
    parts.push(`### ${role.toUpperCase()}\n${content}`)
  }
  return parts.join('\n\n')
}

async function chatViaCli(providerId, { messages, model, cwd } = {}) {
  const status = getCliStatus(providerId)
  if (!status.supported) {
    throw new Error('CLI_SUBSCRIPTION_UNSUPPORTED')
  }
  if (!status.found) {
    throw new Error('CLI_NOT_FOUND')
  }
  if (!status.loggedIn) {
    // Still attempt chat — some CLIs auth via env; but warn via dedicated code if clearly missing
    // Prefer failing early with clear error when no auth file
    const meta = CLI_META[providerId]
    const anyAuth = meta.authPaths().some(fileLooksAuthed)
    if (!anyAuth) {
      throw new Error('CLI_NOT_LOGGED_IN')
    }
  }

  const prompt = flattenMessages(messages)
  if (!prompt.trim()) throw new Error('Empty prompt')

  const workDir = cwd && fs.existsSync(cwd) ? cwd : os.tmpdir()
  const outFile = path.join(os.tmpdir(), `embedide-cli-${Date.now()}-${process.pid}.txt`)

  try {
    if (providerId === 'claude') {
      const args = ['-p', '--output-format', 'text']
      if (model) args.push('--model', model)
      const result = await runSpawn(status.path, args, { cwd: workDir, input: prompt })
      const text = (result.stdout || '').trim()
      if (result.code !== 0 && !text) {
        throw new Error((result.stderr || result.stdout || `claude exited ${result.code}`).trim())
      }
      return text || (result.stderr || '').trim()
    }

    if (providerId === 'openai') {
      // codex exec: read prompt from stdin with "-" ; write last message to file
      const args = [
        'exec',
        '--ephemeral',
        '--sandbox', 'read-only',
        '--skip-git-repo-check',
      ]
      if (model) args.push('-m', String(model))
      args.push('-o', outFile, '-')
      const result = await runSpawn(status.path, args, { cwd: workDir, input: prompt })
      let text = ''
      try {
        if (fs.existsSync(outFile)) text = fs.readFileSync(outFile, 'utf8').trim()
      } catch {}
      if (!text) text = (result.stdout || '').trim()
      if (result.code !== 0 && !text) {
        throw new Error((result.stderr || result.stdout || `codex exited ${result.code}`).trim())
      }
      return text
    }

    if (providerId === 'xai') {
      const args = ['-p']
      // Prefer stdin for long prompts when CLI accepts trailing "-"
      let result
      if (prompt.length > 4000) {
        result = await runSpawn(status.path, model ? ['-p', '-', '-m', String(model)] : ['-p', '-'], {
          cwd: workDir,
          input: prompt,
        })
      } else {
        const a = ['-p', prompt]
        if (model) a.push('-m', String(model))
        result = await runSpawn(status.path, a, { cwd: workDir })
      }
      const text = (result.stdout || '').trim()
      if (result.code !== 0 && !text) {
        throw new Error((result.stderr || result.stdout || `grok exited ${result.code}`).trim())
      }
      return text || (result.stderr || '').trim()
    }

    throw new Error('CLI_SUBSCRIPTION_UNSUPPORTED')
  } finally {
    try { fs.unlinkSync(outFile) } catch {}
  }
}

module.exports = {
  supportsSubscription,
  getCliStatus,
  chatViaCli,
  cancelCliChat,
  SUBSCRIPTION_PROVIDERS,
}
