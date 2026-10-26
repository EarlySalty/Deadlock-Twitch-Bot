#!/bin/bash
eval "$(/home/nathanael/Documents/Infisical/export_claude_secret.py --secret DEADLOCK_CENTRAL_DSN 2>/dev/null)"
export LC_ALL=C
DSN="${DEADLOCK_CENTRAL_DSN%/*}/twitch_analytics"; unset DEADLOCK_CENTRAL_DSN
psql "$DSN" -P pager=off "$@" 2>&1 | sed 's/postgres[^ ]*@/***@/g'
