from pathlib import Path
import difflib
import json
import subprocess

root = Path('/home/nathanael/.worktrees/tb-vod-youtube-abgleich-20261008')
filename = 'rust/crates/tb-vod-archive/src/store.rs'
base = '0ecae1370f1a80d1a101249b5c932663d69be8af'
previous = subprocess.check_output(['git', '-C', str(root), 'show', base + ':' + filename], text=True)
current = (root / filename).read_text()
results = []
changes = []
for name, source in [('baseline', previous), ('current', current)]:
    formatted = subprocess.run(['/home/nathanael/.cargo/bin/rustfmt', '--edition', '2021', '--emit', 'stdout'], input=source, text=True, capture_output=True, cwd=root, check=True).stdout
    diff = list(difflib.unified_diff(source.splitlines(), formatted.splitlines()))
    lines = [line for line in diff if line.startswith(('+', '-')) and not line.startswith(('+++', '---'))]
    changes.append(lines)
    results.append({'state': name, 'format_hunks': sum(line.startswith('@@') for line in diff), 'changed_lines': len(lines)})
identical = changes[0] == changes[1]
print(json.dumps({'baseline_sha': base, 'file': filename, 'results': results, 'identical_formatting_changes': identical}, indent=2))
raise SystemExit(0 if identical else 1)
