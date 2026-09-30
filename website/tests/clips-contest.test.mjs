import test from 'node:test'
import assert from 'node:assert/strict'
import fs from 'node:fs'
import path from 'node:path'

const root = path.resolve(import.meta.dirname, '..')
const source = fs.readFileSync(path.join(root, 'src/clips.tsx'), 'utf8')
const css = fs.readFileSync(path.join(root, 'src/clips.css'), 'utf8')
const vite = fs.readFileSync(path.join(root, 'vite.config.ts'), 'utf8')

test('clip contest is a real vite entry', () => {
  assert.match(vite, /clips:\s*path\.resolve\(__dirname, 'clips\/index\.html'\)/)
  assert.ok(fs.existsSync(path.join(root, 'clips/index.html')))
})

test('votes and submissions are server side calls, never localStorage', () => {
  assert.match(source, /\/clips\/api\/submit/)
  assert.match(source, /\/clips\/api\/vote/)
  assert.doesNotMatch(source, /localStorage|sessionStorage/)
})

test('discord and existing twitch login are both offered', () => {
  assert.match(source, /\/clips\/auth\/discord\/login/)
  assert.match(source, /\/twitch\/auth\/login\?next=%2Fclips/)
})

test('embedded twitch clips use the production parent host', () => {
  assert.match(source, /clips\.twitch\.tv\/embed/)
  assert.match(source, /parent=deutsche-deadlock-community\.de/)
})

test('mobile layout and black gold palette are explicit', () => {
  assert.match(css, /--bg:\s*#080706/)
  assert.match(css, /--gold:\s*#c9a86a/)
  assert.match(css, /@media \(max-width: 620px\)/)
  assert.doesNotMatch(css, /radial-gradient/)
})
