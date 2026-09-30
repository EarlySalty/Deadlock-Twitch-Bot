import test from 'node:test'
import assert from 'node:assert/strict'
import fs from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')
const read = (file) => fs.readFileSync(path.join(root, file), 'utf8')

test('clip contest is a dedicated Vite entry', () => {
  const vite = read('vite.config.ts')
  assert.match(vite, /clips:\s*path\.resolve\(__dirname, 'clips\/index\.html'\)/)
  assert.match(read('clips/index.html'), /src\/clips\.tsx/)
})

test('contest stores no votes or identity in browser storage', () => {
  const source = read('src/clips.tsx')
  assert.doesNotMatch(source, /localStorage|sessionStorage/)
  assert.match(source, /\/clips\/api\/vote/)
  assert.match(source, /\/clips\/api\/submit/)
})

test('page contains twitch embeds, phases and hall of fame', () => {
  const source = read('src/clips.tsx')
  assert.match(source, /clips\.twitch\.tv\/embed/)
  assert.match(source, /1\. bis 21\./)
  assert.match(source, /22\. bis Monatsende/)
  assert.match(source, /Hall of Fame/)
  assert.match(source, /Mit Discord anmelden/)
  assert.match(source, /Mit Twitch anmelden/)
})

test('mobile layout collapses contest grids', () => {
  const css = read('src/clips.css')
  assert.match(css, /@media \(max-width: 620px\)/)
  assert.match(css, /grid-template-columns:\s*1fr/)
})
