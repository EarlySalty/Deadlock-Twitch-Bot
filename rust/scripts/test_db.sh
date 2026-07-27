#!/usr/bin/env bash
# Wegwerfbarer Timescale-Testcontainer für hermetische tb-db-Tests.
# Gleiche Engine wie Prod (timescale/timescaledb:2.17.2-pg16), FREIER Port (Docker
# vergibt ihn), Throwaway-Passwort. KEIN Bezug zur echten DB / keinem Secret.
#
# Nutzung:
#   ./test_db.sh up             # startet Container, gibt export-Zeile aus
#   eval "$(./test_db.sh env)"  # setzt TB_TEST_DATABASE_URL in der Shell
#   ./test_db.sh down
set -euo pipefail
NAME="${TB_TEST_CONTAINER:-tb-test-postgres}"
PASS="tbtest"
IMAGE="timescale/timescaledb:2.17.2-pg16"

# Port, den Docker dem laufenden Container vergeben hat.
dsn_line() {
  local port
  port="$(docker port "$NAME" 5432/tcp 2>/dev/null | head -1 | sed 's/.*://')"
  [ -n "$port" ] || { echo "Container '$NAME' läuft nicht — erst '$0 up'." >&2; exit 1; }
  echo "export TB_TEST_DATABASE_URL=postgres://postgres:${PASS}@127.0.0.1:${port}/postgres"
}

case "${1:-}" in
  up)
    docker rm -f "$NAME" >/dev/null 2>&1 || true
    # Port 0 = Docker sucht einen freien. Der frühere feste 5434 kollidierte mit
    # deadlock-central-postgres (Prod) — und die Tests fahren DROP SCHEMA CASCADE.
    docker run -d --rm --name "$NAME" -e POSTGRES_PASSWORD="$PASS" \
      -p "127.0.0.1:0:5432" "$IMAGE" >/dev/null
    echo -n "warte auf Postgres"
    # Das postgres-Image loggt "ready to accept connections" ZWEIMAL: zuerst fuer den
    # initdb-Temp-Server (nur Unix-Socket), dann fuer den echten TCP-Server. pg_isready
    # ist schon nach dem ersten Mal true, aber der gemappte Port resettet dann noch
    # (Docker-Proxy akzeptiert, Backend lauscht noch nicht). Erst ab dem zweiten Log-
    # Eintrag nimmt der TCP-Port verlaesslich Verbindungen an.
    ready=0
    for _ in $(seq 1 60); do
      count="$(docker logs "$NAME" 2>&1 | grep -c 'database system is ready to accept connections' || true)"
      if [ "${count:-0}" -ge 2 ] && docker exec "$NAME" pg_isready -U postgres >/dev/null 2>&1; then
        ready=1; echo " ok"; break
      fi
      echo -n "."; sleep 1
    done
    if [ "$ready" -ne 1 ]; then echo " TIMEOUT"; docker logs "$NAME" 2>&1 | tail -20; exit 1; fi
    # Settle: TimescaleDB initialisiert nach "ready to accept connections" noch
    # Background-Worker. Ein Schwung paralleler Test-Verbindungen direkt danach kann
    # sonst sporadisch resettet werden. Kurz warten, bis der bg-worker verbunden ist.
    for _ in $(seq 1 15); do
      if docker logs "$NAME" 2>&1 | grep -q 'TimescaleDB background worker launcher connected'; then break; fi
      sleep 1
    done
    sleep 2
    dsn_line
    ;;
  env)
    dsn_line
    ;;
  down)
    docker rm -f "$NAME" >/dev/null 2>&1 || true
    echo "Testcontainer entfernt."
    ;;
  *)
    echo "usage: $0 {up|env|down}"; exit 1;;
esac
