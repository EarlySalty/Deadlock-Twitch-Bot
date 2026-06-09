# ADR 0002 — sqlx + refinery, geteiltes Schema als unveränderte Baseline

- **Status:** akzeptiert (2026-06-08)
- **Kontext-Doku:** [`02-db-contract.md`](../02-db-contract.md)

## Kontext

Während der Strangler-Fig-Migration bedienen Python und Rust **dieselbe** PostgreSQL-DB. Das Schema
darf nicht gebrochen werden (Rahmenentscheidung 4). Teile der DB nutzen TimescaleDB-Features
(Hypertables, Compression), für die sqlx keine compile-time-geprüften Typen kennt. Das Python-System
verteilt DDL heute über fünf Verzeichnisse und führt teils `_ensure_*`-DDL pro Request aus.

## Entscheidung

- **Treiber/Query:** `sqlx` mit dem `postgres`+`tokio`-Feature. Primär **runtime-`query()`** statt
  der `query!`-Makros, damit Timescale-Tabellen und das bestehende (nicht von Rust erzeugte) Schema
  ohne Compile-Zeit-Schemaabgleich nutzbar sind. Wo eine Tabelle voll unter Rust-Kontrolle steht,
  dürfen geprüfte Makros (offline-Modus) verwendet werden.
- **Migrations:** `refinery` als einzige DDL-SSOT unter `rust/migrations/`. Das **bestehende
  Prod-Schema wird als Baseline markiert** (nicht neu angelegt, nicht verändert). Rust-Migrations
  beginnen erst oberhalb dieser Baseline; Timescale-spezifische DDL als raw SQL.
- **Pool:** sqlx-Pool ersetzt den Eigenbau-LIFO-Pool (`_pool.py`) komplett.
- **Idempotenz:** ein zentraler Idempotency-Store in `tb-db` (für die `X-Idempotency-Key`-Logik der
  internen API).

## Konsequenzen

**Positiv:**
- Kein Schema-Bruch; Python + Rust lesen/schreiben dieselben Tabellen parallel.
- DDL an einem Ort, einmalig beim Start — kein DDL im Request-Hot-Path mehr.
- Timescale bleibt nutzbar.

**Negativ / zu beachten:**
- Runtime-`query()` verliert den Compile-Zeit-Schemaschutz → Row-Mapping muss durch Tests
  abgesichert werden (Vertrags-Tests gegen das echte Schema in Phase 0).
- Die Baseline-Markierung muss sauber gesetzt sein, bevor irgendein Rust-Schreibzugriff erfolgt,
  sonst riskiert refinery DDL gegen Prod.

## Offen

- Konkrete Hypertable-Tabellen identifizieren (siehe [`06-open-questions.md`](../06-open-questions.md) Punkt 3).
- Verschlüsselte Spalten siehe [`adr/0003`](0003-crypto-interop-or-reauth.md).
