#!/bin/bash
eval "$(/home/nathanael/Documents/Infisical/export_claude_secret.py --secret TWITCH_INTERNAL_API_TOKEN 2>/dev/null)"
export SAI_TOKEN="$TWITCH_INTERNAL_API_TOKEN"; unset TWITCH_INTERNAL_API_TOKEN
mkdir -p "$2"
exec node "$(dirname "$0")/ui.mjs" "$1" "$2"
