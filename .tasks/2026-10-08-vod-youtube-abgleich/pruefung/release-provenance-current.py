from pathlib import Path
import argparse
import hashlib
import json
import re
import subprocess

parser = argparse.ArgumentParser()
parser.add_argument('--root', required=True)
parser.add_argument('--expected', required=True)
args = parser.parse_args()
root = Path(args.root)
assert re.fullmatch(r'[0-9a-f]{40}', args.expected)

def git(*arguments):
    return subprocess.check_output(['git', '-C', str(root), *arguments], text=True).strip()

def digest(file, algorithm='sha256'):
    result = hashlib.new(algorithm)
    with file.open('rb') as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b''):
            result.update(chunk)
    return result.hexdigest()

assert git('rev-parse', 'HEAD') == args.expected
assert git('rev-parse', 'origin/main') == args.expected
assert not git('status', '--porcelain'), 'Release source has uncommitted files'
names = ['tb-bot', 'tb-dashboard', 'tb-stream-audit', 'tb-config-check', 'tb-llm-usage-recover']
collector = root / 'rust/bin/tb-category-collector'
if (collector / 'Cargo.toml').is_file() and not (collector / 'src/lib.rs').is_file():
    names.append('tb-category-collector')
if (collector / 'src/bin/tb-twitch-watchdog.rs').is_file():
    names.append('tb-twitch-watchdog')
if (root / 'rust/bin/tb-dashboard/src/bin/clip_context_learn.rs').is_file():
    names.append('clip_context_learn')
binaries = {}
for name in names:
    file = root / 'rust/target/release' / name
    assert file.is_file() and not file.is_symlink(), name
    output = subprocess.check_output(['readelf', '--string-dump=.twitch_build', str(file)], text=True)
    revisions = re.findall(r'\[\s*[0-9a-f]+\]\s+(\S+)', output)
    assert revisions == [args.expected], (name, revisions)
    binaries[name] = {'embedded_revision': revisions[0], 'sha256': digest(file), 'bytes': file.stat().st_size}
dashboard = 'bot/dashboard_v2/dist'
if not (root / dashboard).is_dir():
    dashboard = 'bot/analytics/dashboard_v2/dist'
assets = {}
for folder in [dashboard, 'bot/admin_dashboard/dist', 'website/dist']:
    directory = root / folder
    assert (directory / 'index.html').is_file(), folder
    assert not any(file.is_symlink() for file in directory.rglob('*')), folder
    assets[folder] = {str(file.relative_to(directory)): digest(file) for file in sorted(directory.rglob('*')) if file.is_file()}
migration = root / 'rust/migrations/20261008003000_vod_youtube_checks.sql'
print(json.dumps({'source_sha': args.expected, 'required_binary_count': len(names), 'all_required_elf_revisions_match': True, 'native_collector': (collector / 'src/lib.rs').is_file(), 'dirty_revisions': 0, 'binaries': binaries, 'assets': assets, 'migration_sha256': digest(migration), 'migration_sqlx_sha384': digest(migration, 'sha384')}, indent=2))
