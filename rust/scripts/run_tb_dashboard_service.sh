#!/usr/bin/env bash
# Startet das Rust-tb-dashboard (8767: Analytics-API + Legal-Seiten) als Service.
# Secrets kommen wie bei tb-bot aus Infisical; Nicht-Secret-Konfiguration wird hier explizit gesetzt.
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
SYSTEMD_CREDENTIAL_DIR='/run/credentials/deadlock-twitch-dashboard-rust.service'
if [[ -r "$SYSTEMD_CREDENTIAL_DIR/infisical-token" ]]; then
  CREDENTIALS_DIRECTORY="$SYSTEMD_CREDENTIAL_DIR"
  CONFIG_FILE='/etc/deadlock-twitch/infisical.conf'
  INFISICAL_LOADER='/usr/local/libexec/dl-infisical-env'
  RUNTIME_CONFIG_FILE='/etc/deadlock-twitch/dashboard.conf'
else
  CONFIG_FILE="${INFISICAL_CONFIG_FILE:-$HOME/.config/deadlock-twitch-bot/infisical.conf}"
  INFISICAL_LOADER="${INFISICAL_LOADER:-/home/naniadm/.local/bin/dl-infisical-env}"
  RUNTIME_CONFIG_FILE="${TWITCH_RUNTIME_CONFIG_FILE:-}"
fi

if [[ ! -f "$CONFIG_FILE" ]]; then
  echo "Missing Infisical config: $CONFIG_FILE" >&2
  exit 1
fi

# Nicht-geheime Verbindungsparameter laden (API-URL, Project-ID, Env-Name).
# INFISICAL_SERVICE_TOKEN steht seit der systemd-creds-Migration NICHT mehr hier.
set -a
# shellcheck source=/dev/null
source "$CONFIG_FILE"
set +a

if [[ -n "$RUNTIME_CONFIG_FILE" ]]; then
  if [[ ! -f "$RUNTIME_CONFIG_FILE" ]]; then
    echo "Runtime-Konfiguration fehlt: $RUNTIME_CONFIG_FILE" >&2
    exit 1
  fi
  set -a
  # shellcheck source=/dev/null
  source "$RUNTIME_CONFIG_FILE"
  set +a
fi
unset INFISICAL_SERVICE_TOKEN

# Bootstrap-Token aus systemd-Credentials übernehmen (bevorzugt).
if [[ -n "${CREDENTIALS_DIRECTORY:-}" && -f "$CREDENTIALS_DIRECTORY/infisical-token" ]]; then
  INFISICAL_SERVICE_TOKEN="$(<"$CREDENTIALS_DIRECTORY/infisical-token")"
  export INFISICAL_SERVICE_TOKEN
fi

if [[ -z "${INFISICAL_SERVICE_TOKEN:-}" ]]; then
  echo "INFISICAL_SERVICE_TOKEN nicht gesetzt — weder in $CONFIG_FILE noch via systemd-creds." >&2
  exit 1
fi

if [[ "${DL_INFISICAL_READY:-0}" != "1" ]]; then
  if [[ ! -x "$INFISICAL_LOADER" ]]; then
    echo "Infisical loader nicht gefunden oder nicht ausführbar: $INFISICAL_LOADER" >&2
    exit 1
  fi
  export DL_INFISICAL_READY=1
  exec "$INFISICAL_LOADER" --profile all -- "$0" "$@"
fi
unset DL_INFISICAL_READY
unset INFISICAL_SERVICE_TOKEN

# Alte Werte aus dem gemeinsamen Infisical-Profil dürfen weder Rolle,
# Härtungs-Gate noch Port dieses getrennten Dienstes bestimmen.
if [[ -n "$RUNTIME_CONFIG_FILE" ]]; then
  set -a
  # shellcheck source=/dev/null
  source "$RUNTIME_CONFIG_FILE"
  set +a
fi
unset INFISICAL_SERVICE_TOKEN

if [[ -n "${TWITCH_DASHBOARD_ANALYTICS_DSN:-}" ]]; then
  export TWITCH_ANALYTICS_DSN="$TWITCH_DASHBOARD_ANALYTICS_DSN"
fi
unset TWITCH_BOT_ANALYTICS_DSN TWITCH_DASHBOARD_ANALYTICS_DSN

# 8767 (Doku-Plan) ist real vom Turnier-Public-Cog der Deadlock-Bots belegt -> 8769.
export DASHBOARD_PORT="${DASHBOARD_PORT:-8769}"
# Split-Runtime-Härtung erzwingt die Dashboard-Rolle und blockiert den für die
# Master-API reservierten Port. Die gehärtete System-Unit schaltet sie ein und
# setzt den legitimen Dashboard-Port 8769 ausdrücklich.
export TWITCH_SPLIT_RUNTIME_ENFORCE="${TWITCH_SPLIT_RUNTIME_ENFORCE:-0}"
# Legal-Overrides liegen relativ zum Repo-Root (WorkingDirectory der Unit).
export TB_LEGAL_PAGES_PATH="${TB_LEGAL_PAGES_PATH:-$ROOT_DIR/data/admin_dashboard/legal_pages.json}"
export RUST_LOG="${RUST_LOG:-info}"
# B3-2: Nativer Twitch-OAuth-Dashboard-Login. Öffentliche Callback-URL (kein
# Secret) — muss exakt der in der Twitch-Developer-Console registrierten
# Redirect-URI entsprechen. Liegt der Wert in Infisical, gewinnt dieser (wird
# vor diesem Block exportiert); sonst greift der kanonische Default.
export TWITCH_DASHBOARD_AUTH_REDIRECT_URI="${TWITCH_DASHBOARD_AUTH_REDIRECT_URI:-https://deutsche-deadlock-community.de/callback/twitch}"
# Welle D: Strangler-Fallback — nicht portierte Dashboard-Routen gehen nur bei
# gesetzter URL an einen Legacy-Proxy weiter. Leer bedeutet 404 statt Proxy.
export TB_DASHBOARD_LEGACY_FALLBACK_URL="${TB_DASHBOARD_LEGACY_FALLBACK_URL:-}"

# Der self-explainer hat seinen eigenen Use-Case `dashboard_self_explainer`.
# Das MiniMax-Reasoning-Modell (MiniMax-M3) liefert die Antwort nur als
# <think>-Block, den process_response_text entfernt -> leerer Text ->
# grounded:false -> Dauer-Fallback ("Das kann ich dir hier nicht sicher sagen").
# MiniMax-Text-01 antwortet direkt (kein <think>) und ist das passende Modell.
# Der Anbieter wird mitgesetzt: TB_LLM_MODEL_<USE_CASE> gilt fuer jeden
# Anbieter, und ein MiniMax-Modellname an einer Fireworks-Adresse waere ein
# Modellfehler. Nur dieser eine Use-Case wird umgestellt; der uebrige
# `engagement`-Pfad (ai_chat/ai_analysis-MiniMax-Zweige, chat-deep-analysis)
# bleibt auf Auto-Auswahl mit Fireworks vorn und MiniMax als Ausweichweg.
export TB_LLM_PROVIDER_DASHBOARD_SELF_EXPLAINER="${TB_LLM_PROVIDER_DASHBOARD_SELF_EXPLAINER:-minimax}"
export TB_LLM_MODEL_DASHBOARD_SELF_EXPLAINER="${TB_LLM_MODEL_DASHBOARD_SELF_EXPLAINER:-MiniMax-Text-01}"

exec "$ROOT_DIR/rust/target/release/tb-dashboard"
