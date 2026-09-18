#!/usr/bin/env python3
"""Generate the admin-only, self-contained feature history; no API or LLM calls.

Usage: python3 tools/generate_roadmap.py --ref origin/main --output dist/roadmap-history/index.html
The reference is pinned once. Publication is an atomic file replacement.
"""
import argparse
import collections
import json
import os
from pathlib import Path
import sys
import tempfile

ROOT = Path(__file__).resolve().parents[1]
ASSETS = Path(__file__).resolve().parent / 'roadmap-history'
sys.path.insert(0, str(ASSETS))
from history_data import build_data, load_taxonomy


def safe_json(value):
    content = json.dumps(value, ensure_ascii=False, separators=(',', ':'))
    for character, escaped in [('&', '\\u0026'), ('<', '\\u003c'), ('>', '\\u003e'), ('\u2028', '\\u2028'), ('\u2029', '\\u2029')]:
        content = content.replace(character, escaped)
    return content


def render(data):
    template = (ASSETS / 'index.html').read_text(encoding='utf-8')
    style = (ASSETS / 'style.css').read_text(encoding='utf-8')
    script = (ASSETS / 'model.js').read_text(encoding='utf-8') + '\n' + (ASSETS / 'app.js').read_text(encoding='utf-8')
    return (template.replace('<!-- ROADMAP_STYLE -->', '<style>' + style + '</style>')
            .replace('<!-- ROADMAP_DATA -->', '<script id="roadmap-data" type="application/json">' + safe_json(data) + '</script>')
            .replace('<!-- ROADMAP_SCRIPT -->', '<script>\n' + script + '\n</script>'))


def atomic_write(output, content):
    output = Path(output)
    output.parent.mkdir(parents=True, exist_ok=True)
    temporary = None
    try:
        with tempfile.NamedTemporaryFile(mode='w', encoding='utf-8', dir=output.parent, prefix='.roadmap-', delete=False) as handle:
            temporary = Path(handle.name)
            handle.write(content)
            handle.flush()
            os.fsync(handle.fileno())
        temporary.chmod(0o644)
        temporary.replace(output)
    finally:
        if temporary is not None and temporary.exists():
            temporary.unlink()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--repo', type=Path, default=ROOT)
    parser.add_argument('--ref', default='origin/main')
    parser.add_argument('--output', type=Path, default=ROOT / 'dist/roadmap-history/index.html')
    args = parser.parse_args()
    taxonomy = load_taxonomy(ASSETS / 'features.json')
    data = build_data(args.repo.resolve(), args.ref, taxonomy)
    atomic_write(args.output, render(data))
    print(json.dumps({'output': str(args.output), 'revision': data['revision'], 'commits': len(data['commits']), 'kinds': dict(collections.Counter(c['kind'] for c in data['commits'])), 'shallow': data['shallow']}))


if __name__ == '__main__':
    main()
