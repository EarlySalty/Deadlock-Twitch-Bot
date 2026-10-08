from pathlib import Path
import argparse
import hashlib
import json
import re
import subprocess

parser = argparse.ArgumentParser()
parser.add_argument('--root', default='/home/nathanael/.worktrees/tb-vod-youtube-abgleich-20261008')
parser.add_argument('--expected', required=True)
parser.add_argument('--moli-proof')
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
binaries = {}
for name in ['tb-bot', 'tb-dashboard', 'tb-stream-audit', 'tb-config-check', 'tb-llm-usage-recover', 'tb-category-collector', 'tb-twitch-watchdog', 'clip_context_learn']:
    file = root / 'rust/target/release' / name
    output = subprocess.check_output(['readelf', '--string-dump=.twitch_build', str(file)], text=True)
    revisions = re.findall(r'\[\s*[0-9a-f]+\]\s+(\S+)', output)
    assert revisions == [args.expected], (name, revisions)
    binaries[name] = {'embedded_revision': revisions[0], 'sha256': digest(file), 'bytes': file.stat().st_size}
assets = {}
for folder in ['bot/analytics/dashboard_v2/dist', 'bot/admin_dashboard/dist', 'website/dist']:
    directory = root / folder
    assert (directory / 'index.html').is_file(), folder
    assets[folder] = {str(file.relative_to(directory)): digest(file) for file in sorted(directory.rglob('*')) if file.is_file()}
if args.moli_proof:
    proof = json.loads(Path(args.moli_proof).read_text())
    subprocess.run(['git', '-C', str(root), 'diff', '--quiet', proof['sha'], args.expected, '--', 'bot/dashboard_v2', 'website/public/fonts'], check=True)
    dashboard = assets['bot/analytics/dashboard_v2/dist']
    assert all(dashboard.get(name) == value for name, value in proof['hashes'].items()), 'Release assets differ from inspected dashboard'
migration = root / 'rust/migrations/20261008003000_vod_youtube_checks.sql'
print(json.dumps({'source_sha': args.expected, 'all_eight_elf_revisions_match': True, 'dirty_revisions': 0, 'binaries': binaries, 'assets': assets, 'migration_sha256': digest(migration), 'migration_sqlx_sha384': digest(migration, 'sha384')}, indent=2))
