#!/usr/bin/env python3
"""One-time secret handoff, not runtime ENV configuration.

Run after the postgres migration and runtime role grants:
  dl-infisical-env -- python3 provision-category-collector.py --output /private/category-collector.json

The existing secret broker exports app credentials only during this one-time
handoff. The daemon subsequently reads just this 0600 JSON and PG configuration.
Never run the daemon through the secret broker or pass a bot/chat OAuth token.
The dedicated role password is transmitted to local postgres via stdin only.
"""
import argparse
import json
import os
from pathlib import Path
import secrets
import stat
import subprocess
import sys


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument('--output', required=True, type=Path)
    args = parser.parse_args()
    path = args.output
    if path.exists() or path.is_symlink():
        raise RuntimeError('Credential file already exists; refusing to overwrite or rotate it')
    client_id = os.environ.get('TWITCH_CLIENT_ID', '')
    client_secret = os.environ.get('TWITCH_CLIENT_SECRET', '')
    if not client_id or not client_secret:
        raise RuntimeError('App credentials missing from the one-time secret handoff')
    path.parent.mkdir(parents=True, exist_ok=True, mode=0o700)
    if path.parent.is_symlink() or stat.S_IMODE(path.parent.stat().st_mode) & 0o077:
        raise RuntimeError('Credential directory must be private (0700) and not a symlink')
    password = secrets.token_hex(32)
    # The role must already exist from the reviewed migration/grant path.
    # No arbitrary role, SQL, DSN or table name is accepted as an argument.
    result = subprocess.run(
        ['/usr/bin/sudo', '-n', '-u', 'postgres', '/usr/bin/psql', '-X',
         '--set=ON_ERROR_STOP=1', '--dbname=twitch_analytics', '--quiet'],
        input=f"ALTER ROLE twitchcollector PASSWORD '{password}';\n",
        text=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE, check=False,
        env={'PATH': '/usr/bin:/bin'}, timeout=15,
    )
    if result.returncode:
        raise RuntimeError('Dedicated database credential setup failed; no secret output was logged')
    payload = {
        'dsn': f'postgresql://twitchcollector:{password}@127.0.0.1:5432/twitch_analytics',
        'client_id': client_id,
        'client_secret': client_secret,
    }
    descriptor = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW, 0o600)
    with os.fdopen(descriptor, 'w', encoding='utf-8') as handle:
        json.dump(payload, handle)
        handle.write('\n')
        handle.flush()
        os.fsync(handle.fileno())
    print('Private collector credentials provisioned; no bot token included.')


if __name__ == '__main__':
    try:
        main()
    except Exception as error:
        # Exception text from parsers, child processes or OS calls must never
        # expose the secret payload or its DSN. Fixed category only.
        print('Collector credential provisioning failed: ' + type(error).__name__, file=sys.stderr)
        sys.exit(1)
