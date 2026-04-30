#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="/home/naniadm/Documents/Deadlock-Twitch-Bot"
ROTATION_ENV="${TWITCH_PG_ROTATION_ENV:-}"

if [[ -z "${ROTATION_ENV}" ]]; then
  if [[ -f /etc/deadlock-bots/twitch-pg-rotation.env ]]; then
    ROTATION_ENV="/etc/deadlock-bots/twitch-pg-rotation.env"
  elif [[ -f "${HOME}/.config/deadlock-twitch-bot/infisical-rotate.env" ]]; then
    ROTATION_ENV="${HOME}/.config/deadlock-twitch-bot/infisical-rotate.env"
  fi
fi

if [[ -n "${ROTATION_ENV}" ]]; then
  if [[ ! -f "${ROTATION_ENV}" ]]; then
    echo "Rotation env file not found: ${ROTATION_ENV}" >&2
    exit 1
  fi
  set -a
  # shellcheck disable=SC1090
  source "${ROTATION_ENV}"
  set +a
fi

if [[ -z "${ROTATION_INFISICAL_SERVICE_TOKEN:-}" ]]; then
  echo "Missing ROTATION_INFISICAL_SERVICE_TOKEN for unattended rotation." >&2
  echo "Set it in /etc/deadlock-bots/twitch-pg-rotation.env or TWITCH_PG_ROTATION_ENV." >&2
  exit 1
fi

cd "${ROOT_DIR}"
exec "${ROOT_DIR}/scripts/rotate_twitch_pg_dsn.py" \
  --generate-password \
  --yes \
  --audit-log "${ROOT_DIR}/logs/secret_rotation_audit.jsonl" \
  "$@"
