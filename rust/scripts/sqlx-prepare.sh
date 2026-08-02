#!/usr/bin/env bash
# Erzeugt den sqlx-Offline-Cache (.sqlx/) gegen ein Schema, das dem Prod-Schema
# entspricht.
#
# Warum nicht gegen die Test-DB: die driftet. Ihre Fixtures sind handgeschriebenes
# DDL, kein Migrationslauf — ein dagegen erzeugter Cache behauptet Spalten und
# Typen, die auf Prod anders aussehen, und der Build merkt es nie.
#
# Warum das hier verlaesslich ist: der Vertrag
# `tb-db --test fresh_migrations_schema` haelt fest, dass ein frischer
# Migrationslauf exakt den committeten Schema-Snapshot ergibt. Dieses Skript
# prueft den Vertrag ZUERST und bricht ab, wenn er nicht haelt — dann ist der
# Snapshot veraltet und muss geklaert werden, bevor irgendein Cache entsteht.
#
# Wegwerfbarer Container, freier Port, Throwaway-Passwort. Kein Bezug zu Prod-DSNs
# oder Secrets.
#
# Nutzung:
#   ./sqlx-prepare.sh              # Cache neu erzeugen
#   ./sqlx-prepare.sh --check      # nur pruefen, ob .sqlx aktuell ist (fuer CI)
set -euo pipefail

NAME="tb-sqlx-prepare-$$"
IMAGE="timescale/timescaledb:2.17.2-pg16"
PASS="sqlxprepare"
DB="sqlx_prepare"
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
MODUS="${1:-}"

cleanup() { docker rm -f "$NAME" >/dev/null 2>&1 || true; }
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
  # Das Image loggt "ready to accept connections" zweimal: erst fuer den
  # initdb-Temp-Server, dann fuer den echten TCP-Server. Erst ab dem zweiten
  # nimmt der gemappte Port verlaesslich Verbindungen an.
  count="$(docker logs "$NAME" 2>&1 | grep -c 'database system is ready to accept connections' || true)"
  if [ "${count:-0}" -ge 2 ] && docker exec "$NAME" pg_isready -U postgres -d "$DB" >/dev/null 2>&1; then
    ready=1; echo " ok"; break
  fi
  echo -n "."; sleep 1
done
if [ "$ready" -ne 1 ]; then
  echo " TIMEOUT"; docker logs "$NAME" 2>&1 | tail -50; exit 1
fi
for _ in $(seq 1 15); do
  docker logs "$NAME" 2>&1 | grep -q 'TimescaleDB background worker launcher connected' && break
  sleep 1
done

PORT="$(docker port "$NAME" 5432/tcp | sed -E 's/.*:([0-9]+)$/\1/')"
DSN="postgres://postgres:${PASS}@127.0.0.1:${PORT}/${DB}"
echo "DSN=postgres://postgres:***@127.0.0.1:${PORT}/${DB}"

cd "$ROOT"

# Schritt 1: Migrationen anwenden UND gegen den committeten Snapshot pruefen.
# Der Test legt sich seine DB selbst an und laesst alle Migrationen laufen.
echo "== Schema-Vertrag pruefen =="
if ! TEST_DATABASE_URL="$DSN" SQLX_OFFLINE=true \
     cargo test -p tb-db --test fresh_migrations_schema -- --nocapture; then
  echo >&2
  echo "ABBRUCH: frischer Migrationslauf weicht vom committeten Schema-Snapshot ab." >&2
  echo "Erst klaeren, ob die Abweichung gewollt ist — sonst entsteht ein Cache," >&2
  echo "der ein Schema behauptet, das auf Prod nicht existiert." >&2
  echo "Gewollt? Dann: UPDATE_SCHEMA_SNAPSHOT=1 cargo test -p tb-db --test fresh_migrations_schema" >&2
  exit 1
fi

# Schritt 2: Migrationen in die Zieldatenbank selbst (der Test benutzt eine
# eigene) und Cache dagegen erzeugen.
echo "== Migrationen anwenden =="
DATABASE_URL="$DSN" SQLX_OFFLINE=false cargo sqlx migrate run --source migrations

echo "== sqlx-Cache erzeugen =="
if [ "$MODUS" = "--check" ]; then
  DATABASE_URL="$DSN" SQLX_OFFLINE=false cargo sqlx prepare --workspace --check -- --all-targets
  echo "OK: .sqlx ist aktuell."
else
  DATABASE_URL="$DSN" SQLX_OFFLINE=false cargo sqlx prepare --workspace -- --all-targets
  echo "OK: .sqlx neu erzeugt — Aenderungen mit 'git status .sqlx' pruefen und committen."
fi
