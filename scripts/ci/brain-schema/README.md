# Brain-Schema-Vertrag (CI-only Bootstrap)

Die beiden SQL-Dateien sind **unveränderte Kopien** der kanonischen
dl-central-db-Migrationen aus `EarlySalty/Deadlock-Bots` und bilden das
`brain`-Schema ab, gegen das der in tb-dashboard-api gepinnte
`dbrain-reasoner` (Git-Dependency, rev d8c34270) im Workspace-Build
kompiliert. Der schema-gate-Job in `.github/workflows/rust-sqlx-check.yml`
lädt sie nach den Twitch-Migrationen je Datei mit
`psql "$DATABASE_URL" -v ON_ERROR_STOP=1 --single-transaction` in die
CI-Datenbank, damit `cargo sqlx prepare --workspace --check` die
Dependency-Makros live prüfen kann.

## Provenienz

| Datei | Quelle (EarlySalty/Deadlock-Bots) | Commit | SHA256 |
|---|---|---|---|
| `0012_brain_knowledge_timeline.sql` | `rust/crates/dl-central-db/migrations/0012_brain_knowledge_timeline.sql` | `5ef78559b335389c9b73498f322f816fb24fa7da` (origin/main, 25.09.2026) | `ff2ec4f6d4715918d96958b2022311335c09257f058f0cb366679a0c82a46fe9` |
| `2026070410_brain_ingestion_tables.sql` | `rust/crates/dl-central-db/migrations/2026070410_brain_ingestion_tables.sql` | derselbe Commit | `43c0b2bf974f74684c2f3e34a633449a2ccee261e5fae0054f6144ae261ad6f8` |

Prüfung im Checkout: `sha256sum -c SHA256SUMS`.

## Grenzen

- **CI-only:** Diese Dateien sind bewusst kein Teil der Twitch-Produktiv-
  migrationen (`rust/migrations`). Ownership des brain-Schemas bleibt
  zentral bei Deadlock-Bots; die Twitch-Dienste fahren ihre Migrationen
  gegen eine eigene Datenbank.
- Reine Schema-DDL: keine Datenladung, keine Grants/Rollen, keine
  produktiven Zugangsdaten. Die Dateien enthalten ausschließlich
  `CREATE SCHEMA/TABLE/INDEX IF NOT EXISTS`-Anweisungen.
- Die SQLx-Pflichtprüfung (online gegen die CI-Datenbank) bleibt
  unverändert aktiv; dieser Bootstrap ersetzt oder umgeht kein Gate.

## Pflege

Bei jeder Erhöhung des `dbrain-reasoner`-Pins in
`rust/crates/tb-dashboard-api/Cargo.toml` diesen Ordner gegen die dann
kanonischen Migrationen aus Deadlock-Bots prüfen, Kopien und
`SHA256SUMS` nachziehen und dies im PR-Benennung nennen.
