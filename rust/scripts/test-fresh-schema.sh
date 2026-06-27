#!/usr/bin/env bash
# Wegwerfbarer Timescale-Lauf fuer den Fresh-Migrations-Schema-Vertrag.
# Nutzt TEST_DATABASE_URL und hat keinen Bezug zu Prod-DSNs oder Secrets.
set -euo pipefail

NAME="tb-fresh-schema-test-$$"
IMAGE="timescale/timescaledb:2.17.2-pg16"
PASS="test"
DB="fresh_schema_test"
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

cleanup() {
  docker rm -f "$NAME" >/dev/null 2>&1 || true
}
trap cleanup EXIT

docker run --rm -d \
  --name "$NAME" \
  -e POSTGRES_PASSWORD="$PASS" \
  -e POSTGRES_DB="$DB" \
  -p "127.0.0.1::5432" \
  "$IMAGE" >/dev/null

echo -n "warte auf Postgres"
ready=0
for _ in $(seq 1 60); do
  count="$(docker logs "$NAME" 2>&1 | grep -c 'database system is ready to accept connections' || true)"
  if [ "${count:-0}" -ge 2 ] && docker exec "$NAME" pg_isready -U postgres -d "$DB" >/dev/null 2>&1; then
    ready=1
    echo " ok"
    break
  fi
  echo -n "."
  sleep 1
done

if [ "$ready" -ne 1 ]; then
  echo " TIMEOUT"
  docker logs "$NAME" 2>&1 | tail -50
  exit 1
fi

for _ in $(seq 1 15); do
  if docker logs "$NAME" 2>&1 | grep -q 'TimescaleDB background worker launcher connected'; then
    break
  fi
  sleep 1
done

PORT="$(docker port "$NAME" 5432/tcp | sed -E 's/.*:([0-9]+)$/\1/')"
export TEST_DATABASE_URL="postgres://postgres:${PASS}@127.0.0.1:${PORT}/${DB}"
echo "TEST_DATABASE_URL=postgres://postgres:***@127.0.0.1:${PORT}/${DB}"

cd "$ROOT"
SQLX_OFFLINE=true cargo test -p tb-db --test fresh_migrations_schema -- --nocapture
