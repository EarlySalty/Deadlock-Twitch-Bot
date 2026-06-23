#!/usr/bin/env bash
# Startet das Rust-tb-dashboard (8767: Analytics-API + Legal-Seiten) als Service.
# Secrets kommen wie bei tb-bot aus Infisical (export_infisical_env.py);
# Nicht-Secret-Konfiguration wird hier explizit gesetzt.
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
CONFIG_FILE="${INFISICAL_CONFIG_FILE:-$HOME/.config/deadlock-twitch-bot/infisical.conf}"

if [[ ! -f "$CONFIG_FILE" ]]; then
  echo "Missing Infisical config: $CONFIG_FILE" >&2
  exit 1
fi

# Nicht-geheime Verbindungsparameter laden (API-URL, Project-ID, Env-Name).
# INFISICAL_SERVICE_TOKEN steht seit der systemd-creds-Migration NICHT mehr hier.
set -a
source "$CONFIG_FILE"
set +a

# Bootstrap-Token aus systemd-Credentials übernehmen (bevorzugt).
if [[ -n "${CREDENTIALS_DIRECTORY:-}" && -f "$CREDENTIALS_DIRECTORY/infisical-token" ]]; then
  INFISICAL_SERVICE_TOKEN="$(<"$CREDENTIALS_DIRECTORY/infisical-token")"
  export INFISICAL_SERVICE_TOKEN
fi

if [[ -z "${INFISICAL_SERVICE_TOKEN:-}" ]]; then
  echo "INFISICAL_SERVICE_TOKEN nicht gesetzt — weder in $CONFIG_FILE noch via systemd-creds." >&2
  exit 1
fi

if [[ -x "$ROOT_DIR/.venv/bin/python" ]]; then
  PYTHON_BIN="${PYTHON_BIN:-$ROOT_DIR/.venv/bin/python}"
else
  PYTHON_BIN="${PYTHON_BIN:-python3}"
fi

INFISICAL_RETRY_DELAY="${INFISICAL_RETRY_DELAY:-5}"
INFISICAL_MAX_ATTEMPTS="${INFISICAL_MAX_ATTEMPTS:-0}"
attempt=0

while true; do
  if INFISICAL_EXPORT="$("$PYTHON_BIN" "$ROOT_DIR/scripts/export_infisical_env.py" --format shell)"; then
    eval "$INFISICAL_EXPORT"
    break
  fi

  attempt=$((attempt + 1))
  if [[ "$INFISICAL_MAX_ATTEMPTS" -gt 0 && "$attempt" -ge "$INFISICAL_MAX_ATTEMPTS" ]]; then
    echo "Infisical secrets could not be loaded after $attempt attempt(s)." >&2
    exit 1
  fi

  echo "Infisical not ready for tb-dashboard, retrying in ${INFISICAL_RETRY_DELAY}s (attempt $attempt)." >&2
  sleep "$INFISICAL_RETRY_DELAY"
done

# 8767 (Doku-Plan) ist real vom Turnier-Public-Cog der Deadlock-Bots belegt -> 8769.
export DASHBOARD_PORT="${DASHBOARD_PORT:-8769}"
# Legal-Overrides liegen relativ zum Repo-Root (WorkingDirectory der Unit).
export TB_LEGAL_PAGES_PATH="${TB_LEGAL_PAGES_PATH:-$ROOT_DIR/data/admin_dashboard/legal_pages.json}"
export RUST_LOG="${RUST_LOG:-info}"
# B3-2: Nativer Twitch-OAuth-Dashboard-Login. Öffentliche Callback-URL (kein
# Secret) — muss exakt der in der Twitch-Developer-Console registrierten
# Redirect-URI entsprechen. Liegt der Wert in Infisical, gewinnt dieser (wird
# vor diesem Block exportiert); sonst greift der kanonische Default.
export TWITCH_DASHBOARD_AUTH_REDIRECT_URI="${TWITCH_DASHBOARD_AUTH_REDIRECT_URI:-https://deutsche-deadlock-community.de/callback/twitch}"
# Welle D: Strangler-Fallback — nicht portierte Dashboard-Routen gehen an
# Python (8765) weiter. Leer setzen ("") deaktiviert den Proxy (404 statt
# Weiterleitung). Wird erst wirksam, wenn Caddy v2-Pfade auf 8769 flippt.
export TB_DASHBOARD_LEGACY_FALLBACK_URL="${TB_DASHBOARD_LEGACY_FALLBACK_URL:-}"

exec "$ROOT_DIR/rust/target/release/tb-dashboard"
