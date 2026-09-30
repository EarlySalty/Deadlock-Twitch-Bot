import assert from 'node:assert/strict'
import fs from 'node:fs'
import test from 'node:test'

const source = fs.readFileSync(new URL('../src/clips.tsx', import.meta.url), 'utf8')
const css = fs.readFileSync(new URL('../src/clips.css', import.meta.url), 'utf8')
const html = fs.readFileSync(new URL('../clips/index.html', import.meta.url), 'utf8')
const vite = fs.readFileSync(new URL('../vite.config.ts', import.meta.url), 'utf8')

test('Clip-Wettbewerb wird als eigener Vite-Einstieg gebaut', () => {
  assert.match(vite, /clips:\s*path\.resolve\(__dirname, 'clips\/index\.html'\)/)
  assert.match(html, /src="\/src\/clips\.tsx"/)
})

test('Stimmen und Einreichungen gehen nur an serverseitige APIs', () => {
  assert.match(source, /\/clips\/api\/submit/)
  assert.match(source, /\/clips\/api\/vote\//)
  assert.doesNotMatch(source, /localStorage|sessionStorage/)
})

test('Twitch-Clips werden als offizielle Embeds gerendert', () => {
  assert.match(source, /https:\/\/clips\.twitch\.tv\/embed\?clip=/)
  assert.match(source, /parent=/)
  assert.match(source, /allowFullScreen/)
})

test('sichtbare Clip-Copy nutzt echte Umlaute und kein SaaS-Vokabular', () => {
  const visible = source + '\n' + html
  assert.doesNotMatch(visible, /SaaS|Software as a Service/i)
  assert.doesNotMatch(visible, /—/)
  assert.match(visible, /höchstens/)
  assert.match(visible, /übrig/)
})

test('Clip-Seite ist mobile-first und nutzt die DDC Design Tokens', () => {
  assert.match(source, /ddc-design-tokens\.css/)
  assert.match(css, /var\(--color-primary\)/)
  assert.match(css, /@media \(min-width: 640px\)/)
  assert.match(css, /@media \(min-width: 900px\)/)
})
