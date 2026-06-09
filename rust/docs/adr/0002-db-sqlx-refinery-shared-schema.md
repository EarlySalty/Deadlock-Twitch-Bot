# ADR 0002 — sqlx (Pool + native Migrationen), geteiltes Schema als unveränderte Baseline

- **Status:** akzeptiert (2026-06-08); **verfeinert 2026-06-09 (Phase 0b):** Migrations-Engine =
  sqlx-native (`sqlx::migrate!`) statt refinery — Begründung unten.
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
- **Migrations:** **sqlx-native** (`sqlx::migrate!`, Tracking-Tabelle `_sqlx_migrations`) als einzige
  DDL-SSOT unter `rust/migrations/`. **refinery verworfen**, weil es einen zweiten PostgreSQL-Treiber
  (tokio-postgres) neben sqlx erzwingen würde — unnötige Doppelung. Das **bestehende Prod-Schema ist
  die Baseline** (nicht neu angelegt, nicht verändert; `rust/migrations/` ist vorerst leer, `run`
  legt nur `_sqlx_migrations` an). Rust-Migrationen beginnen erst oberhalb dieser Baseline;
  Timescale-spezifische DDL als raw SQL. `_sqlx_migrations` ist getrennt von der Python-`schema_version`
  (component-PK) → kein Konflikt im Parallelbetrieb (in Phase 0b empirisch bestätigt).
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
  sonst riskiert die Migration DDL gegen Prod. (In 0b läuft `run_migrations` nur gegen den
  Wegwerf-Testcontainer; gegen Prod wird ausschließlich read-only verifiziert.)

## Offen

- Konkrete Hypertable-Tabellen identifizieren (siehe [`06-open-questions.md`](../06-open-questions.md) Punkt 3).
- Verschlüsselte Spalten siehe [`adr/0003`](0003-crypto-interop-or-reauth.md).
