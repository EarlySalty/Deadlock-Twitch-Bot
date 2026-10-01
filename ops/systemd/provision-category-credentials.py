#!/usr/bin/env python3
"""Root-only provisioning: emit two host-encrypted, narrowly scoped credentials.

Reads the existing root-owned Infisical configuration and bootstrap credential.
No plaintext credential is written to disk, printed, placed in argv or exported.
The runtime collector only receives the Twitch application's two credentials.
"""
import configparser
import http.client
import json
import os
import shlex
import socket
import stat
import struct
import subprocess
import sys
import urllib.parse
from pathlib import Path

SOCKET = Path('/run/uplink-infisical/api.sock')
BOOTSTRAP = Path('/etc/credstore.encrypted/deadlock-twitch-infisical.cred')
CONFIG = Path('/etc/deadlock-twitch/infisical.conf')
OUTPUT = Path('/etc/credstore.encrypted')


def root_file(path: Path) -> bytes:
    metadata = path.stat()
    if not stat.S_ISREG(metadata.st_mode) or metadata.st_uid != 0 or metadata.st_mode & 0o022:
        raise ValueError('configuration is not root-owned and protected')
    return path.read_bytes()


def read_settings() -> dict[str, str]:
    values = {}
    for line in root_file(CONFIG).decode().splitlines():
        words = shlex.split(line, comments=True)
        if words and words[0] == 'export':
            words = words[1:]
        if len(words) == 1 and '=' in words[0]:
            name, value = words[0].split('=', 1)
            if name in {'INFISICAL_PROJECT_ID', 'INFISICAL_ENV', 'INFISICAL_SECRET_PATH'}:
                if '$' in value or '`' in value:
                    raise ValueError('dynamic configuration is not accepted')
                values[name] = value
    if not values.get('INFISICAL_PROJECT_ID') or not values.get('INFISICAL_ENV'):
        raise ValueError('missing Infisical project/environment')
    return values


class LocalInfisical(http.client.HTTPConnection):
    def connect(self) -> None:
        metadata = SOCKET.stat()
        if not stat.S_ISSOCK(metadata.st_mode) or metadata.st_uid != 0 or metadata.st_mode & 0o007:
            raise ValueError('untrusted Infisical socket')
        self.sock = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        self.sock.settimeout(20)
        self.sock.connect(str(SOCKET))
        _, uid, _ = struct.unpack('3i', self.sock.getsockopt(socket.SOL_SOCKET, socket.SO_PEERCRED, 12))
        if uid != 0:
            raise ValueError('Infisical peer must be root')


def encrypted(name: str, filename: str, value: dict) -> None:
    destination = OUTPUT / filename
    temporary = OUTPUT / (filename + '.new')
    if temporary.exists() or temporary.is_symlink():
        raise ValueError('credential staging path already exists')
    result = subprocess.run(['/usr/bin/systemd-creds', 'encrypt', '--name=' + name, '-', str(temporary)],
        input=json.dumps(value).encode(), stdout=subprocess.PIPE, stderr=subprocess.PIPE, check=False, timeout=30)
    if result.returncode:
        temporary.unlink(missing_ok=True)
        raise ValueError('host credential encryption failed')
    temporary.chmod(0o600)
    temporary.replace(destination)


def main() -> int:
    if os.geteuid() != 0 or sys.argv[1:]:
        print('Run the installed root-owned provisioner as root, without arguments.', file=sys.stderr)
        return 1
    try:
        settings = read_settings()
        root_file(BOOTSTRAP)
        result = subprocess.run(['/usr/bin/systemd-creds', 'decrypt', '--name=infisical-token', str(BOOTSTRAP), '-'],
            stdout=subprocess.PIPE, stderr=subprocess.PIPE, check=False, timeout=30)
        if result.returncode:
            raise ValueError('bootstrap credential unavailable')
        token = result.stdout.decode().strip()
        if not token or any(c.isspace() for c in token):
            raise ValueError('invalid bootstrap credential')
        query = urllib.parse.urlencode({'projectId': settings['INFISICAL_PROJECT_ID'],
            'environment': settings['INFISICAL_ENV'], 'secretPath': settings.get('INFISICAL_SECRET_PATH', '/'),
            'viewSecretValue': 'true', 'includeImports': 'true', 'recursive': 'false'})
        client = LocalInfisical('infisical.local', timeout=20)
        client.request('GET', '/api/v4/secrets/?' + query, headers={'Authorization': 'Bearer ' + token})
        response = client.getresponse()
        if response.status != 200:
            raise ValueError('Infisical rejected credential provisioning')
        body = response.read(4 * 1024 * 1024 + 1)
        client.close()
        if len(body) > 4 * 1024 * 1024:
            raise ValueError('unexpected credential response size')
        reply = json.loads(body)
        entries = [entry for group in reply.get('imports', []) for entry in group.get('secrets', [])]
        entries.extend(reply.get('secrets', []))
        allowed = {'TWITCH_CLIENT_ID', 'TWITCH_CLIENT_SECRET', 'TWITCH_INTERNAL_API_TOKEN'}
        values = {entry['secretKey']: entry['secretValue'] for entry in entries if entry.get('secretKey') in allowed}
        if not all(values.get(name) for name in allowed):
            raise ValueError('required application or notification credential missing')
        watchdog = configparser.ConfigParser(interpolation=None)
        watchdog.read_string(root_file(Path('/etc/deadlock-twitch/bot-watchdog.conf')).decode())
        operator = watchdog.getint('watchdog', 'discord_user_id')
        if operator <= 0:
            raise ValueError('invalid existing operator notification recipient')
        encrypted('category-twitch', 'deadlock-category-twitch.cred',
            {'client_id': values['TWITCH_CLIENT_ID'], 'client_secret': values['TWITCH_CLIENT_SECRET']})
        encrypted('category-notify', 'deadlock-category-notify.cred',
            {'user_id': operator, 'token': values['TWITCH_INTERNAL_API_TOKEN']})
        print('Provisioned encrypted category application and separate operator-notification credentials.')
        return 0
    except (OSError, ValueError, KeyError, http.client.HTTPException, configparser.Error, subprocess.SubprocessError):
        print('Category credential provisioning failed; no credential contents were logged.', file=sys.stderr)
        return 1


if __name__ == '__main__':
    raise SystemExit(main())
