#!/usr/bin/env python3
"""Install the collector from the currently active, verified root-owned release.

Run the reviewed root-owned copy after deploy-twitch-release has migrated the DB.
No bot process is restarted here. Existing Caddy and PostgreSQL rules are preserved.
"""
import hashlib
import json
import os
import pwd
import shutil
import stat
import subprocess
import sys
import time
from pathlib import Path

ROOT = Path('/opt/deadlock/twitch/current')
OPS = ROOT / 'ops/systemd'
CADDY = Path('/etc/caddy/Caddyfile')
PATHS = '/twitch/kategorie /twitch/kategorie/*'


def run(*args: str, **kwargs) -> subprocess.CompletedProcess:
    return subprocess.run(args, check=True, text=True, capture_output=True, timeout=120, **kwargs)


def verified(relative: str) -> Path:
    release = ROOT.resolve(strict=True)
    if release.parent != Path('/opt/deadlock/twitch/releases'):
        raise ValueError('current does not point to a release')
    source = release / relative
    metadata = source.lstat()
    if not stat.S_ISREG(metadata.st_mode) or metadata.st_uid != 0 or metadata.st_mode & 0o022:
        raise ValueError('release source is not root-owned and immutable')
    manifest = (release / 'SHA256SUMS').read_text().splitlines()
    expected = next((line.split()[0] for line in manifest if line.endswith('  ./' + relative)), None)
    if expected is None or hashlib.sha256(source.read_bytes()).hexdigest() != expected:
        raise ValueError('release source checksum mismatch')
    return source


def install(relative: str, destination: str, mode: int = 0o644) -> None:
    source = verified(relative)
    target = Path(destination)
    temporary = target.with_name(target.name + '.category-new')
    if temporary.exists() or temporary.is_symlink():
        raise ValueError('installation staging path already exists')
    target.parent.mkdir(parents=True, exist_ok=True)
    with temporary.open('xb') as handle:
        handle.write(source.read_bytes())
    temporary.chmod(mode)
    temporary.replace(target)


def caddy_text(original: str) -> str:
    lines = original.splitlines(keepends=True)
    seen = set()
    in_public = False
    for index, line in enumerate(lines):
        stripped = line.strip()
        if stripped.startswith('@public_twitch {'):
            in_public = True
            continue
        kind = None
        if stripped.startswith('@non_demo_embed not path '):
            kind = 'csp_exclusion'
        elif stripped.startswith('@dashboard_paths path '):
            kind = 'dashboard_csp'
        elif in_public and stripped.startswith('path '):
            kind = 'public_route'
            in_public = False
        if kind:
            seen.add(kind)
            if '/twitch/kategorie' not in line.split():
                lines[index] = line.rstrip('\n') + ' ' + PATHS + '\n'
    if seen != {'csp_exclusion', 'dashboard_csp', 'public_route'}:
        raise ValueError('expected Caddy matchers were not found')
    return ''.join(lines)


def update_caddy() -> None:
    original = CADDY.read_text()
    modified = caddy_text(original)
    if modified == original:
        return
    candidate = CADDY.with_name('Caddyfile.category-candidate')
    if candidate.exists() or candidate.is_symlink():
        raise ValueError('Caddy candidate already exists')
    with candidate.open('x') as handle:
        handle.write(modified)
    candidate.chmod(0o600)
    try:
        run('/usr/bin/caddy', 'validate', '--config', str(candidate), '--adapter', 'caddyfile')
        shutil.copy2(CADDY, CADDY.with_name('Caddyfile.before-category-' + str(int(time.time()))))
        candidate.chmod(stat.S_IMODE(CADDY.stat().st_mode))
        candidate.replace(CADDY)
        run('/usr/bin/systemctl', 'reload', 'caddy')
    except (OSError, subprocess.SubprocessError):
        CADDY.write_text(original)
        candidate.unlink(missing_ok=True)
        raise


def update_hba() -> None:
    psql = ('/usr/sbin/runuser', '-u', 'postgres', '--', '/usr/bin/psql', '-X', '-v', 'ON_ERROR_STOP=1', '-d', 'twitch_analytics', '-Atqc')
    path = Path(run(*psql, 'SHOW hba_file').stdout.strip()).resolve()
    if not str(path).startswith('/etc/postgresql/') or not path.is_file():
        raise ValueError('unexpected PostgreSQL HBA location')
    original = path.read_text()
    marker = '# BEGIN anonymous Deadlock category collector\n'
    block = marker + 'local twitch_analytics twitchcollector peer\nlocal all twitchcollector reject\n# END anonymous Deadlock category collector\n'
    if marker in original:
        if block not in original:
            raise ValueError('existing category HBA block differs from expected rules')
        return
    shutil.copy2(path, path.with_name(path.name + '.before-category-' + str(int(time.time()))))
    path.write_text(block + original)
    try:
        errors = run(*psql, "SELECT count(*) FROM pg_hba_file_rules WHERE error IS NOT NULL").stdout.strip()
        if errors != '0':
            raise ValueError('PostgreSQL rejected the candidate HBA rules')
        if run(*psql, 'SELECT pg_reload_conf()').stdout.strip() != 't':
            raise ValueError('PostgreSQL reload failed')
    except (OSError, ValueError, subprocess.SubprocessError):
        path.write_text(original)
        raise


def main() -> int:
    if os.geteuid() != 0 or sys.argv[1:]:
        print('Run the installed root-owned collector installer as root, without arguments.', file=sys.stderr)
        return 1
    try:
        verified('rust/target/release/tb-category-collector')
        try:
            account = pwd.getpwnam('twitchcollector')
            if account.pw_uid == 0:
                raise ValueError('invalid collector identity')
        except KeyError:
            run('/usr/sbin/useradd', '--system', '--user-group', '--no-create-home', '--home-dir', '/nonexistent', '--shell', '/usr/sbin/nologin', 'twitchcollector')
        roles = verified('ops/systemd/category-runtime-roles.sql')
        run('/usr/sbin/runuser', '-u', 'postgres', '--', '/usr/bin/psql', '-X', '-v', 'ON_ERROR_STOP=1', '-d', 'twitch_analytics', '-f', str(roles))
        update_hba()
        install('ops/systemd/provision-category-credentials.py', '/usr/local/libexec/provision-category-credentials', 0o755)
        install('ops/systemd/tb-category-notify.py', '/usr/local/libexec/tb-category-notify.py', 0o755)
        run('/usr/bin/python3', '/usr/local/libexec/provision-category-credentials')
        install('ops/systemd/tb-category-collector.service', '/etc/systemd/system/tb-category-collector.service')
        install('ops/systemd/tb-category-collector-failure.service', '/etc/systemd/system/tb-category-collector-failure.service')
        config = Path('/etc/deadlock-twitch/category-collector.json')
        if not config.exists():
            install('ops/systemd/category-collector.example.json', str(config))
        json.loads(config.read_text())
        update_caddy()
        run('/usr/bin/systemctl', 'daemon-reload')
        run('/usr/bin/systemctl', 'enable', '--now', 'tb-category-collector.service')
        print('Collector installed and enabled. Verify health, actual rows, anonymous IRC coverage and admin authentication separately.')
        return 0
    except (OSError, ValueError, KeyError, subprocess.SubprocessError) as error:
        print('Collector installation stopped: ' + type(error).__name__ + '. No credential contents were logged.', file=sys.stderr)
        return 1


if __name__ == '__main__':
    raise SystemExit(main())
