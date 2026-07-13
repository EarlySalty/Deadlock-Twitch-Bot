import assert from 'node:assert/strict'
import { readdirSync, readFileSync } from 'node:fs'
import { join } from 'node:path'
import test from 'node:test'
import { fileURLToPath } from 'node:url'

const sourceRoot = fileURLToPath(new URL('../src/', import.meta.url))

function sourceFiles(directory) {
  return readdirSync(directory, { withFileTypes: true }).flatMap((entry) => {
    const path = join(directory, entry.name)
    return entry.isDirectory() ? sourceFiles(path) : /\.(?:css|tsx?)$/.test(entry.name) ? [path] : []
  })
}

test('does not apply Tailwind opacity modifiers to CSS variable colors', () => {
  const invalid = sourceFiles(sourceRoot).flatMap((file) =>
    [...readFileSync(file, 'utf8').matchAll(/\[[^\]]*var\(--[^)]+\)[^\]]*\]\/\d+/g)].map(
      ({ 0: className }) => `${file}:${className}`,
    ),
  )

  assert.deepEqual(invalid, [])
})
