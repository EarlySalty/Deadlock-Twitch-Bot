#!/usr/bin/env bash
# Lokale Code-Qualität und Security-Prüfung
# Spiegelt lint-and-typecheck.yml + security-fortress.yml (Python-Teil)
# Voraussetzung: pip install ruff mypy bandit pytest pytest-cov

set -euo pipefail

cd "$(dirname "$0")/.."

fail=0

run_check() {
  local name="$1"
  shift
  echo
  echo "=== $name ==="
  if "$@"; then
    echo "✅ $name: OK"
  else
    echo "❌ $name: FEHLGESCHLAGEN"
    fail=1
  fi
}

run_check "Ruff Lint" \
  ruff check bot/ twitch_cog/ tests/ scripts/ --target-version=py311

run_check "Ruff Format" \
  ruff format bot/ twitch_cog/ tests/ scripts/ --check --target-version=py311

run_check "mypy Type Check" \
  mypy bot/ twitch_cog/ --ignore-missing-imports --no-error-summary --show-column-numbers

run_check "Bandit Security SAST" \
  bandit -r bot twitch_cog tests scripts -ll -ii

run_check "pytest" \
  pytest -q --cov=bot --cov=twitch_cog --cov-branch --cov-fail-under=40

echo
if [ "$fail" -eq 0 ]; then
  echo "✅ Alle lokalen Checks bestanden."
else
  echo "❌ Einige Checks sind fehlgeschlagen."
  exit 1
fi
