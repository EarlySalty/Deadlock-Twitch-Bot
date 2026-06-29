# Schema-Drift-Gate

`tb-db` hat einen Voll-Snapshot-Test fuer frisch angewendete Migrationen:

```bash
cargo test -p tb-db --test fresh_migrations_schema
```

Der Test baut mit `TEST_DATABASE_URL` eine leere Testdatenbank auf, aktiviert `timescaledb`, fuehrt alle Migrationen aus und liest danach alle Spalten aus `information_schema.columns`.
`_sqlx_migrations` ist sqlx-interne Buchhaltung und bewusst vom Gate ausgenommen.
Verglichen wird gegen `rust/crates/tb-db/tests/fresh_schema_snapshot.txt` im Format:

```text
table_name|column_name|data_type|is_nullable|column_default
```

Der Vergleich ist reihenfolgeunabhaengig. Bei Abweichungen meldet der Test neue, fehlende und geaenderte Spaltenzeilen.

## Snapshot bewusst aktualisieren

Bei beabsichtigten Schemaaenderungen den Snapshot explizit neu erzeugen:

```bash
UPDATE_SCHEMA_SNAPSHOT=1 TEST_DATABASE_URL=... cargo test -p tb-db --test fresh_migrations_schema
```

Danach den Diff von `fresh_schema_snapshot.txt` reviewen und zusammen mit der Migration committen.

## Grenze

Das Gate prueft das Schema, das aus einer frischen Migration entsteht. Runtime-DDL wie `CREATE TABLE IF NOT EXISTS` ausserhalb der Migrationen ist noch nicht erfasst.
