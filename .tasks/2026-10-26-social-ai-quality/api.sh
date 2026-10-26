#!/bin/bash
eval "$(/home/nathanael/Documents/Infisical/export_claude_secret.py --secret TWITCH_INTERNAL_API_TOKEN 2>/dev/null)"
T="$TWITCH_INTERNAL_API_TOKEN"; unset TWITCH_INTERNAL_API_TOKEN
method="$1"; path="$2"; body="$3"
args=(-sS -X "$method" -H "X-Internal-Token: $T" -w '\n%{http_code}')
[ -n "$OUT" ] && args=(-sS -X "$method" -H "X-Internal-Token: $T" -o "$OUT" -w '%{http_code}')
[ -n "$body" ] && args+=(-H 'Content-Type: application/json' --data "$body")
exec curl "${args[@]}" "http://127.0.0.1:8769$path"
