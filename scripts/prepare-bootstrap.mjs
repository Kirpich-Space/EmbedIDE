#!/usr/bin/env node
/**
 * Copy portable make into build/bootstrap/<platform>/ for slim packages.
 */
import { existsSync, mkdirSync, cpSync, chmodSync } from 'fs'
import { dirname, join } from 'path'
import { fileURLToPath } from 'url'
import { createRequire } from 'module'

const __dirname = dirname(fileURLToPath(import.meta.url))
const ROOT = join(__dirname, '..')
const require = createRequire(import.meta.url)
const { platformKey } = require('../electron/toolchainPackages.js')
const pk = platformKey()
const src = join(ROOT, 'vendor', 'toolchain', pk, 'bin', process.platform === 'win32' ? 'make.exe' : 'make')
const destDir = join(ROOT, 'build', 'bootstrap', pk)
const dest = join(destDir, process.platform === 'win32' ? 'make.exe' : 'make')

if (!existsSync(src)) {
  console.warn('No bundled make at', src, '— bootstrap make skipped (installer will copy host make if available)')
  process.exit(0)
}
mkdirSync(destDir, { recursive: true })
cpSync(src, dest)
try { chmodSync(dest, 0o755) } catch {}
console.log('Bootstrap make →', dest)
