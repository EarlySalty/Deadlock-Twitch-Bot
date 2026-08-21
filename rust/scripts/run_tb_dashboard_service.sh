#!/usr/bin/env bash
# Startet das Rust-tb-dashboard (8767: Analytics-API + Legal-Seiten) als Service.
# Secrets kommen wie bei tb-bot aus Infisical; Nicht-Secret-Konfiguration wird hier explizit gesetzt.
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
CONFIG_FILE="${INFISICAL_CONFIG_FILE:-$HOME/.config/deadlock-twitch-bot/infisical.conf}"
INFISICAL_LOADER="${INFISICAL_LOADER:-/home/naniadm/.local/bin/dl-infisical-env}"

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

# 8767 (Doku-Plan) ist real vom Turnier-Public-Cog der Deadlock-Bots belegt -> 8769.
export DASHBOARD_PORT="${DASHBOARD_PORT:-8769}"
# Split-Runtime-Härtung (enforce_dashboard_runtime) ist Cutover-Scope
# (OPS-RUNTIME-006): scharf erzwingt sie Rolle==dashboard UND Port==8765, der
# Live-Dienst läuft aber bewusst auf 8769. Bis der Cutover Rollen/Ports aller
# Dienste sauber verdrahtet, bleibt sie aus (= bisheriges Live-Verhalten).
# Vor dem Scharfschalten (=1): Port-Check in enforce_dashboard_runtime prüft gegen
# die Konstante DASHBOARD_SERVICE_PORT=8765 statt gegen den konfigurierten
# DASHBOARD_PORT -> lehnt den legitimen 8769-Override ab; das muss zuerst gefixt
# und die Rolle (TWITCH_RUNTIME_ROLE=dashboard) gesetzt werden.
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
