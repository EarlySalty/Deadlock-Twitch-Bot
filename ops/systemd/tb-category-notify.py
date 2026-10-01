#!/usr/bin/env python3
"""Separate operator notification. No Twitch credentials or IRC capability."""
import json
import socket
import sys
import time
import urllib.error
import urllib.request
from pathlib import Path

CREDENTIAL = Path('/run/credentials/tb-category-collector-failure.service/category-notify')
BROKER = 'http://127.0.0.1:8770/internal/master/v1/discord/send-message'


def main() -> int:
    try:
        data = json.loads(CREDENTIAL.read_text(encoding='utf-8'))
        user_id = int(data['user_id'])
        token = str(data['token']).strip()
        if user_id <= 0 or not token or '\n' in token or '\r' in token:
            raise ValueError('invalid notification credential')
        if sys.argv[1:] == ['--check']:
            print('Category notification credential validated; no message sent.')
            return 0
        if sys.argv[1:]:
            raise ValueError('unexpected arguments')
        payload = json.dumps({
            'user_id': user_id,
            'content': f'Der anonyme Deadlock-Kategoriesammler auf {socket.gethostname()} ist ausgefallen. '
                       'Bitte tb-category-collector.service prüfen. Kategorie- und Chatdaten können Lücken enthalten; '
                       'der Collector hat keine Twitch-Sendefunktion.',
            'idempotency_key': f'category-collector-failed-{int(time.time()) // 900}',
        }).encode('utf-8')
        request = urllib.request.Request(BROKER, data=payload, method='POST',
            headers={'Content-Type': 'application/json', 'X-Internal-Token': token})

        class NoRedirect(urllib.request.HTTPRedirectHandler):
            def redirect_request(self, req, fp, code, msg, headers, new_url):
                return None

        opener = urllib.request.build_opener(urllib.request.ProxyHandler({}), NoRedirect())
        with opener.open(request, timeout=15) as response:
            if not 200 <= response.status < 300:
                raise ValueError('notification broker rejected request')
        print('Category collector failure notification delivered to configured operator.')
        return 0
    except (OSError, ValueError, KeyError, urllib.error.URLError):
        print('Category failure notification unavailable; inspect service journal.', file=sys.stderr)
        return 1


if __name__ == '__main__':
    raise SystemExit(main())
