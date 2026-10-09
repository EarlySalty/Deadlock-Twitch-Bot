from pathlib import Path
import hashlib
import json
import subprocess

proof_path = Path('/home/nathanael/.worktrees/tb-vod-youtube-abgleich-20261008/.tasks/2026-10-08-vod-youtube-abgleich/pruefung/release-provenance-current.json')
expected = json.loads(proof_path.read_text())
root = Path('/opt/deadlock/twitch/releases') / expected['source_sha']
assert Path('/opt/deadlock/twitch/current').resolve() == root

def digest(file):
    result = hashlib.sha256()
    with file.open('rb') as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b''):
            result.update(chunk)
    return result.hexdigest()

binaries = {}
for name, source in expected['binaries'].items():
    file = root / 'rust/target/release' / name
    assert file.is_file() and not file.is_symlink()
    assert digest(file) == source['sha256'], name
    output = subprocess.check_output(['readelf', '--string-dump=.twitch_build', str(file)], text=True)
    assert expected['source_sha'] in output and '-dirty' not in output, name
    binaries[name] = source['sha256']
assets = {}
for folder, hashes in expected['assets'].items():
    live_folder = 'bot/analytics/dashboard_v2/dist' if folder == 'bot/dashboard_v2/dist' else folder
    directory = root / live_folder
    assert not any(file.is_symlink() for file in directory.rglob('*'))
    actual = {str(file.relative_to(directory)): digest(file) for file in sorted(directory.rglob('*')) if file.is_file()}
    assert actual == hashes, live_folder
    assets[live_folder] = len(actual)
migration = root / 'rust/migrations/20261008003000_vod_youtube_checks.sql'
assert digest(migration) == expected['migration_sha256']
anchor = b'twitch_vod_youtube_continuations'
assert anchor in (root / 'rust/target/release/tb-bot').read_bytes()
print(json.dumps({'proof': 'live_release_provenance', 'release_sha': expected['source_sha'], 'all_seven_binary_hashes_match_own_build': True, 'all_three_asset_trees_match_own_build': True, 'asset_file_counts': assets, 'migration_sha256': expected['migration_sha256'], 'binary_anchor': anchor.decode(), 'binary_anchor_present': True, 'current_matches': True}, sort_keys=True))
