# Schema Cleanup — Streamer/Partner/Exclusion Trennung

**Datum:** 2026-06-17  
**Status:** Approved, bereit zur Implementierung

## Problem

`twitch_streamers` enthält seit SQLite-Migration Felder die dort nicht hingehören:
- `discord_user_id` — bereits in `twitch_streamer_identities` (Wahrheitsquelle)
- `is_monitored_only` — war ein Workaround für "in Streamers aber kein Partner"
- `archived_at` — ist eine Partner-spezifische Dashboard-Flag

Das führte zu der falschen Annahme `twitch_streamers ≈ twitch_partners` und damit zu Bugs (z.B. `is_partner`-Flag in Stats-Code).

## Ziel-Schema

### `twitch_streamers` — reine Identitätstabelle

Enthält jeden Streamer den der Bot **monitort**. Nur Identität, kein Lifecycle.

```sql
twitch_login   TEXT PRIMARY KEY
twitch_user_id TEXT
```

Entfernt: `discord_user_id`, `is_monitored_only`, `archived_at`

### `twitch_partners` — aktiver Partner-Subset

Subset von `twitch_streamers`. Nur aktive Partner (kein opt-out, kein banned).

Neu: `archived_at TIMESTAMPTZ` — soft visual flag, blendet Partner im Dashboard aus wenn zu lange kein DL-Stream. **Kein user impact**, rein intern.

### `twitch_streamer_identities` — Discord-Verknüpfung

Lifecycle-unabhängig. Bleibt stehen auch wenn Partner opt-out geht, damit die ID bei Reaktivierung noch da ist.

Einzige Wahrheitsquelle für `discord_user_id`. Unverändert.

### `twitch_exclusions` — NEU

Für Streamer die aktiv vom Bot ausgeschlossen sind. Bleiben in `twitch_streamers` (werden weiter gemonitort), aber Bot interagiert nicht.

```sql
twitch_user_id   TEXT PRIMARY KEY
kind             TEXT  -- 'opt_out' | 'banned'
reason           TEXT
excluded_at      TIMESTAMPTZ NOT NULL DEFAULT now()
reactivated_at   TIMESTAMPTZ  -- NULL = noch ausgeschlossen
```

**kind=opt_out:** reversibel. Reactivated_at setzen → wieder in `twitch_partners` aufnehmen.  
**kind=banned:** permanent. Hard Bot-Ban (Token gültig, aber Bot kann nicht in Channel). Kein Dashboard-Login, kein Zugriff auf nichts. `reactivated_at` bleibt NULL für immer.

### `twitch_streamers_partner_state` VIEW — Update

Muss an neue Schema-Struktur angepasst werden (kein `is_monitored_only` mehr, JOIN-Logik ändert sich).

## Daten-Migration

| Streamer | Aktion |
|----------|--------|
| `dead_eye_nika` | Bleibt in `twitch_streamers`, `is_monitored_only` Flag entfernt |
| `ismile_e` | Bleibt in `twitch_streamers`, `is_monitored_only` Flag entfernt |
| `skifahrertv` | Bleibt in `twitch_streamers` + neuer Eintrag in `twitch_exclusions` (kind=banned) |
| `fr4gm1nt` | Bleibt in `twitch_streamers` + neuer Eintrag in `twitch_exclusions` (kind=opt_out) |
| `snaqeu` | Bleibt in `twitch_streamers` + neuer Eintrag in `twitch_exclusions` (kind=opt_out) |
| `archived_at` (Prod-Werte) | Von `twitch_streamers` → `twitch_partners` migrieren wo Partner-Eintrag existiert |
| `discord_user_id` (Prod-Werte) | Bereits in `twitch_streamer_identities`, kein Copy nötig |

## Logik nach Migration

```
Monitored            = SELECT * FROM twitch_streamers
Aktive Partner       = JOIN twitch_partners ON twitch_user_id
Nur-Monitored        = twitch_streamers WHERE NOT EXISTS (SELECT 1 FROM twitch_partners ...)
Ausgeschlossen       = twitch_exclusions WHERE reactivated_at IS NULL
Opt-out (reaktivierbar) = twitch_exclusions WHERE kind='opt_out' AND reactivated_at IS NULL
Hard-Banned          = twitch_exclusions WHERE kind='banned'
```

## Betroffene Codebase-Bereiche

- **Migration:** neue `.sql`-Datei + Baseline-Update
- **tb-db:** Schema-Structs, Query-Funktionen
- **tb-monitoring:** Poll-Loop liest `is_monitored_only` → muss auf exclusions umgestellt werden
- **tb-internal-api:** Handler die Partner/Streamer abfragen
- **tb-analytics:** Market- und Network-Queries
- **Test-Fixtures:** DDL in allen `support/mod.rs`-Dateien

## Invarianten

1. `twitch_partners.twitch_user_id` ist immer auch in `twitch_streamers` → FK-Constraint
2. `twitch_exclusions.twitch_user_id` ist immer auch in `twitch_streamers` → FK-Constraint
3. `is_partner` in Stats-Code IMMER via `twitch_partners`-JOIN, nie via Flag
4. `discord_user_id` IMMER via `twitch_streamer_identities`, nie via `twitch_streamers`
