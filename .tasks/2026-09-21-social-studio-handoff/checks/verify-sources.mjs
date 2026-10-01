import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import { createHash } from 'node:crypto';
import { fileURLToPath } from 'node:url';

const here = path.dirname(fileURLToPath(import.meta.url));
const task = path.resolve(here, '..');
const root = path.join(task, 'reference');
const imported = JSON.parse(fs.readFileSync(path.join(task, 'reference-manifest.json'), 'utf8'));
const sha256 = bytes => createHash('sha256').update(bytes).digest('hex');
const artifacts = {
  'preview.html': '54f480a73604723afa4b00f4ffceee6884adff27a7c30464cea46e347d3acf25',
  'dist/tailwind.css': 'e353aab485b855136e566827d324a3b35ef4368b82a558796453fea0d34da007',
};
assert.equal(Object.keys(imported.files).length, 28);
for (const [name, expected] of Object.entries({ ...imported.files, ...artifacts })) {
  const file = path.resolve(root, name);
  assert.ok(file.startsWith(root + path.sep), 'Path outside reference: ' + name);
  assert.equal(sha256(fs.readFileSync(file)), expected, 'Reference differs from the chat package: ' + name);
}

// Scoped pre-publication check. Report filenames, not potentially sensitive values.
const forbiddenFiles = [];
const credentialPatterns = [
  /-----BEGIN (?:RSA |EC |OPENSSH |DSA )?PRIVATE KEY-----/,
  /\bgh[pousr]_[A-Za-z0-9]{20,}\b/,
  /\bgithub_pat_[A-Za-z0-9_]{30,}\b/,
  /\bsk-[A-Za-z0-9]{24,}\b/,
];
let checkedTextFiles = 0;
const flagged = [];
function inspect(dir) {
  for (const entry of fs.readdirSync(dir, { withFileTypes: true })) {
    if (entry.name === 'node_modules' || entry.name === '__pycache__' || entry.name === '.git') continue;
    const file = path.join(dir, entry.name);
    if (entry.isSymbolicLink()) { forbiddenFiles.push(path.relative(task, file)); continue; }
    if (entry.isDirectory()) { inspect(file); continue; }
    const relative = path.relative(task, file);
    if (/\.(?:woff2?|ttf|otf|pem|key)$/i.test(file) || /^\.env(?:\.|$)/.test(entry.name)) forbiddenFiles.push(relative);
    if (/\.(?:md|html|css|js|mjs|cjs|jsx|json|py)$/.test(file)) {
      checkedTextFiles += 1;
      const text = fs.readFileSync(file, 'utf8');
      if (credentialPatterns.some(pattern => pattern.test(text))) flagged.push(relative);
    }
  }
}
inspect(task);
assert.deepEqual(forbiddenFiles, [], 'Disallowed handoff files');
assert.deepEqual(flagged, [], 'Potential credentials need review');
const result = {
  verified_sources: 28,
  byte_identical_generated_artifacts: Object.keys(artifacts),
  original_chat_zip_sha256: 'd57b0f9d5c514213e903d8a8af6d627bf882e4d75eab659feb43b7ef76001973',
  transferred_source_archive_sha256: imported.source_transfer_sha256,
  checked_text_files: checkedTextFiles,
  forbidden_files: forbiddenFiles.length,
  credential_pattern_findings: flagged.length,
  scope: 'Source provenance and a scoped publication check, not a comprehensive security audit or product integration test',
};
fs.writeFileSync(path.join(here, 'source-results.json'), JSON.stringify(result, null, 2) + '\n');
console.log('Verified 28 unchanged source files and 2 byte-identical generated artifacts.');
console.log('Scoped publication check: no bundled fonts, environment files, private keys or credential-pattern findings.');
