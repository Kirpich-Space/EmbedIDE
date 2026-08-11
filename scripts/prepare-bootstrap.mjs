#!/usr/bin/env node
/**
 * Copy portable make into build/bootstrap/<platform>/ and build/bootstrap/ for slim packages.
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
const makeName = process.platform === 'win32' ? 'make.exe' : 'make'
const src = join(ROOT, 'vendor', 'toolchain', pk, 'bin', makeName)
const destDir = join(ROOT, 'build', 'bootstrap', pk)
const dest = join(destDir, makeName)
const flatDest = join(ROOT, 'build', 'bootstrap', makeName)

if (!existsSync(src)) {
  console.warn('No bundled make at', src, '— bootstrap make skipped (installer will download/build make)')
  process.exit(0)
}
mkdirSync(destDir, { recursive: true })
cpSync(src, dest)
cpSync(src, flatDest)
try { chmodSync(dest, 0o755) } catch {}
try { chmodSync(flatDest, 0o755) } catch {}
console.log('Bootstrap make →', dest)
console.log('Bootstrap make →', flatDest)
