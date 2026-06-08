# migrations/ — Architektur & Funktionsreferenz

> Pfad: `bot/migrations/` · Stand: 2026-06-08 · 11 Python-Skripte + 8 SQL-Dateien, ~1.170 Z.
>
> Teil der [Architektur-Doku](README.md). Verwandt: [storage.md](storage.md) (`ensure_schema` ist die laufende Schema-Pflege), [DATABASE.md](../DATABASE.md).

## 1. Zweck & Abgrenzung

`migrations/` enthält **einmalige, idempotente CLI-Skripte** für Schema-/Daten-Änderungen, die über das normale Bootstrap (`storage.ensure_schema`) hinausgehen — neue Tabellen, Backfills, das Entfernen von Legacy-Spalten. Jedes Skript hat `parse_args()` + `main()` und wird manuell (oder einmalig bei Deployment) ausgeführt, nicht im laufenden Betrieb.

Abgrenzung: Die **laufende** Schema-Sicherung (idempotentes `CREATE TABLE IF NOT EXISTS …` bei jedem Start) macht `storage/pg.py::ensure_schema`. `migrations/` ist für **gezielte Übergänge** (z. B. „Klartext-Token-Spalten entfernen“), die man bewusst einmal fährt.

## 2. Einordnung & Abhängigkeiten

| Richtung | Beziehung |
|----------|-----------|
| **Ausführung** | manuell als CLI (`python -m bot.migrations.<name> [--dsn …]`). |
| **Nutzt** | `storage`/`psycopg`, DSN aus Argument/ENV (`_resolve_dsn`/`_get_dsn`). |
| **DB** | erstellt/ändert konkrete Tabellen; einige laden eine begleitende `.sql`-Datei. |
| **Secret-Namen** | DSN (z. B. `TWITCH_ANALYTICS_DSN`) — wird nie geloggt. |

## 3. Dateien im Überblick

| Datei | Zeilen | Rolle |
|-------|-------:|-------|
| `exp_backfill.py` | 260 | Idempotentes Backfill der Experimental-Tabellen. |
| `migrate_observability_events.py` | 196 | Migration `twitch_observability_events`. |
| `engagement_layer.py` (+ `engagement_layer.sql`) | 127 | Engagement-Layer-Tabellen anlegen. |
| `exp_tables_migrate.py` | 113 | Die 3 Experimental-Tabellen anlegen. |
| `create_viewer_presence_ticks.py` | 79 | `twitch_viewer_presence_ticks` anlegen. |
| `social_media_phase0_stabilization.py` | 71 | Social-Media Phase-0-Stabilisierung. |
| `social_media_phase1_layout_and_uploads.py` | 71 | Phase 1: Layout + Uploads. |
| `social_media_phase2_enrichment.py` | 70 | Phase 2: Enrichment. |
| `social_media_phase3_analytics.py` | 70 | Phase 3: Analytics. |
| `social_media_phase4_approval.py` | 70 | Phase 4: Approval. |
| `drop_legacy_tokens.py` | 43 | Legacy-Klartext-Token-Spalten aus `twitch_raid_auth` entfernen. |
| **SQL** | — | `affiliate_schema.sql`, `engagement_layer.sql`, `channel_profile_schema.sql`, `global_sentiment_schema.sql` u. a. |

## 4. Datenfluss / Lebenszyklus

Typischer Ablauf eines Skripts:
1. `parse_args()` liest Optionen (u. a. `--dsn`); `_resolve_dsn`/`_get_dsn` löst die DSN auf (Argument → ENV).
2. `main()` öffnet eine Verbindung, prüft den Ist-Zustand (z. B. `_inspect_source_tables` liest vorhandene Spalten) und führt die Änderung **idempotent** aus (mehrfaches Ausführen ist sicher).
3. Skripte mit begleitender `.sql` führen diese aus (z. B. `engagement_layer.py` → `engagement_layer.sql`).

Die Social-Media-Migrationen sind **gestaffelt** (Phase 0–4): Stabilisierung → Layout/Uploads → Enrichment → Analytics → Approval — passend zum Aufbau der [social-media.md](social-media.md)-Pipeline.

## 5. Funktionsreferenz (Muster)

Alle Skripte folgen demselben Muster:
- `parse_args() -> argparse.Namespace` — CLI-Optionen.
- `main() -> int` (bzw. `run()`) — die Migration; Rückgabecode 0 bei Erfolg.
- DSN-Auflösung: `_resolve_dsn(explicit_dsn)` / `_get_dsn(args)`.

Besondere:
- `exp_backfill.py` — `_executemany(conn, sql, params_seq)`, `_inspect_source_tables(conn)` (Ist-Spalten lesen), idempotentes Backfill.
- `drop_legacy_tokens.py` — `run()`: entfernt die Klartext-Token-Spalten aus `twitch_raid_auth` (nachdem die Verschlüsselung — `compat/field_crypto` — aktiv ist). Sicherheitsrelevant.
- `migrate_observability_events.py` — legt/migriert `twitch_observability_events`.
- `create_viewer_presence_ticks.py` — `twitch_viewer_presence_ticks` (siehe [VIEWER_PRESENCE_TIMELINE.md](../VIEWER_PRESENCE_TIMELINE.md)).

## 6. Datenbank & externe Schnittstellen

- **DB:** legt/ändert die genannten Tabellen; SQL-Dateien enthalten die `CREATE`/`ALTER`-Statements.
- **Keine** externen Dienste.

## 7. Stolperfallen / Besonderheiten

- **Einmalig vs. laufend:** `migrations/` ≠ `ensure_schema`. Neue Tabellen, die immer existieren sollen, gehören in `ensure_schema` (idempotent beim Start); echte Übergänge (Daten umziehen, Spalten droppen) in ein Migrations-Skript.
- **Idempotenz ist Pflicht:** Jedes Skript muss mehrfach ausführbar sein, ohne Schaden anzurichten — Deployments fahren sie ggf. erneut.
- **`drop_legacy_tokens` erst nach Verschlüsselung:** Die Klartext-Spalten dürfen erst weg, wenn `field_crypto` aktiv ist und die Tokens verschlüsselt vorliegen — sonst Token-Verlust.
- **DSN nie loggen:** DSN kommt aus ENV/Argument und wird nicht ausgegeben (Secret-Regel).
- **Reihenfolge bei Social-Media:** Phasen 0→4 bauen aufeinander auf; nicht überspringen.
