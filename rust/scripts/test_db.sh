#!/usr/bin/env bash
# Wegwerfbarer Timescale-Testcontainer für hermetische tb-db-Tests.
# Gleiche Engine wie Prod (timescale/timescaledb:2.17.2-pg16), eigener Port 5434,
# Throwaway-Passwort. KEIN Bezug zur echten DB / keinem Secret.
set -euo pipefail
NAME="tb-test-postgres"
PORT="5434"
PASS="tbtest"
IMAGE="timescale/timescaledb:2.17.2-pg16"
export TB_TEST_DATABASE_URL="postgres://postgres:${PASS}@127.0.0.1:${PORT}/postgres"

case "${1:-}" in
  up)
    docker rm -f "$NAME" >/dev/null 2>&1 || true
    docker run -d --rm --name "$NAME" -e POSTGRES_PASSWORD="$PASS" \
      -p "127.0.0.1:${PORT}:5432" "$IMAGE" >/dev/null
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
    echo "TB_TEST_DATABASE_URL=${TB_TEST_DATABASE_URL}"
    ;;
  down)
    docker rm -f "$NAME" >/dev/null 2>&1 || true
    echo "Testcontainer entfernt."
    ;;
  *)
    echo "usage: $0 {up|down}"; exit 1;;
esac
