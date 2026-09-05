#!/usr/bin/env bash
set -euo pipefail
HERE=$(cd "$(dirname "$0")" && pwd)
ROOT=$(cd "$HERE/.." && pwd)
chmod +x "$ROOT/scripts/security-scan-local.sh" "$ROOT/.githooks/pre-push"
git -C "$ROOT" config core.hooksPath .githooks
printf 'core.hooksPath=%s\n' "$(git -C "$ROOT" config --get core.hooksPath)"
