#!/usr/bin/env bash
set -euo pipefail

# Regenerates the SQLx offline cache after the operator has built a fresh DB
# from rust/migrations and exported DATABASE_URL for that disposable schema.
# This script never provides a default DSN; secrets stay in the caller env.

if [[ -z "${DATABASE_URL:-}" ]]; then
  echo "DATABASE_URL is required. Build a fresh DB from rust/migrations, export DATABASE_URL, then rerun." >&2
  exit 1
fi

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "${repo_root}/rust"

exec cargo sqlx prepare --workspace
