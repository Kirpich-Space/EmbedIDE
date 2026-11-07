#!/usr/bin/env node
/**
 * Runs the Electron-side tests with the built-in node:test runner.
 * An explicit file list keeps the behaviour identical on Linux, macOS and
 * Windows (no shell globbing) and never wanders into vendor/ or node_modules.
 *
 *   node test/run.mjs            unit tests (no toolchain or hardware needed)
 *   node test/run.mjs --all      + integration tests that need the bundled toolchain
 */
import { readdirSync } from 'fs'
import { join, dirname } from 'path'
import { fileURLToPath } from 'url'
import { run } from 'node:test'
import { spec } from 'node:test/reporters'

const here = dirname(fileURLToPath(import.meta.url))
const dirs = ['unit']
if (process.argv.includes('--all')) dirs.push('integration')
const files = dirs.flatMap((d) =>
  readdirSync(join(here, d))
    .filter((f) => f.endsWith('.test.js') || f.endsWith('.test.mjs'))
    .sort()
    .map((f) => join(here, d, f)),
)

let failed = false
const stream = run({ files, concurrency: 1 })
stream.on('test:fail', (e) => {
  // Suite-level failure events also arrive for each failing child
  if (!e.todo) failed = true
})
stream.compose(spec).pipe(process.stdout)
stream.on('end', () => {
  process.exitCode = failed ? 1 : 0
})
