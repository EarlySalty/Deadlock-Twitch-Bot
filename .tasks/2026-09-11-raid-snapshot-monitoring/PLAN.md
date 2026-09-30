# Raid-Snapshot-Monitoring

Stand: 2026-09-11. Klasse: mittel. Repo: Deadlock-Twitch-Bot (tb-raid, tb-bot, tb-dashboard-api, dashboard_v2).

## Warum

Heute loggt der Bot pro Raid nur `candidates_count`, den Sieger und `is_partner_raid`
(`twitch_observability_events`). Wer die anderen Kandidaten waren, welchen Score sie
hatten und warum sie rausflogen, steht nirgends. Der Score-Cache
(`twitch_partner_raid_scores`) wird alle 5 Minuten ueberschrieben, es gibt keine
Historie. Deshalb laesst sich "warum wurde mello geraidet und nicht bezi" nachtraeglich
nicht beweisen, nur der Mechanismus vermuten.

Ziel: je Raid-Entscheidung einen unveraenderlichen Snapshot aller relevanten Kanaele
mit ihren Score-Werten und dem Ausschluss- bzw. Auswahlgrund, sichtbar im Dashboard.

## Nicht-Ziel (bewusst ausgeklammert)

- Keine Aenderung am Scoring oder an der Auswahl. Die Schlagseite zugunsten von
  Langstreamern (Readiness/duration_score dominiert) und der fehlende Recent-Raid-
  Cooldown auf dem Partner-Pfad sind ein separater Auftrag. Dieser Plan macht die
  Entscheidung nur sichtbar, er aendert sie nicht.
- Kein Rueckwirken auf Altdaten. Der Snapshot beginnt ab Deploy; vergangene Raids
  bleiben unbelegt.

## Datenmodell

Eine Tabelle, eine Zeile je Kandidat je Raid-Flow. Header-Felder (source, flow, ts)
werden pro Zeile wiederholt, das haelt die Abfrage im Dashboard trivial.

Tabelle `twitch_raid_candidate_snapshots` (Schema `public`, DB `twitch_analytics`):

| Spalte | Typ | Bedeutung |
|---|---|---|
| id | BIGSERIAL PK | |
| flow_id | TEXT NOT NULL | verbindet mit `twitch_observability_events.flow_id` |
| raid_id | BIGINT NULL | FK-lose Referenz auf `twitch_raid_history.id`, sobald der Raid geschrieben ist |
| source_id | TEXT NOT NULL | raidender Streamer (offline gegangen) |
| source_login | TEXT NOT NULL | |
| reason | TEXT NOT NULL | `auto_raid_on_offline` / `manual_chat_command` |
| created_at | TIMESTAMPTZ NOT NULL DEFAULT now() | |
| candidate_id | TEXT NOT NULL | |
| candidate_login | TEXT NOT NULL | |
| stage | TEXT NOT NULL | `assembly` (Live-Roster-Stufe) oder `scored` (Pipeline) |
| is_live | INTEGER NOT NULL | Live laut Score-Cache zum Zeitpunkt der Auswahl |
| game | TEXT NULL | aktuelle Kategorie (aus dem Helix-Stream, nur assembly-Stufe) |
| on_deadlock | INTEGER NOT NULL | Deadlock-eligible (active/recent) |
| eligibility | TEXT NOT NULL | `active` / `recent` / `wrong_game` / `blacklisted` / `cache_miss` / `stale_not_live` / `excluded_self` / `scored` |
| final_score | DOUBLE PRECISION NULL | aus dem Score-Cache |
| readiness_score | DOUBLE PRECISION NULL | |
| duration_score | DOUBLE PRECISION NULL | der Treiber (Runway) |
| fairness_score | DOUBLE PRECISION NULL | |
| today_received_raids | INTEGER NULL | |
| courtesy_class | TEXT NULL | |
| selected | INTEGER NOT NULL DEFAULT 0 | 1 = dieser Kanal wurde geraidet |
| selection_reason | TEXT NULL | nur beim Sieger, aus `SelectionReason::as_str()` |

Indizes: `(created_at DESC)`, `(flow_id)`, `(source_login, created_at DESC)`,
`(candidate_login, created_at DESC)`.

Warum beide `stage`-Werte: die assembly-Stufe zeigt "war live, aber auf WARDOGS"
(fliegt vor der Pipeline raus, deshalb sonst unsichtbar); die scored-Stufe zeigt
"war Kandidat, verlor auf Score". Nur beide zusammen beantworten "warum nicht bezi".

## Erfassungspunkte im Code

Zwei Stellen liefern die Daten, sie werden ueber eine gemeinsame `flow_id` verbunden.

1. `rust/bin/tb-bot/src/auto_raid.rs`, `assemble_eligible_partners` (Z. 527).
   Hier existieren `online` (alle live Roster-Partner mit `stream.game_name`),
   die Buckets `active` / `recent` und `filtered_out`. Statt nur zu zaehlen, die
   Kandidaten als Liste mitfuehren: je Online-Partner `(login, id, game, bucket)`.
   Bucket `None` wird zu `wrong_game`. Diese Liste als neues Feld in die
   `AutoRaidRequest` (siehe unten) haengen.

2. `rust/crates/tb-raid/src/auto_raid_pipeline.rs`, `run` (Z. 441) und
   `resolve_partner_target` (`target_resolution.rs:161`).
   Hier liegen die `scores` (`PartnerRaidScoreRow` mit final/readiness/duration/
   fairness/today) und die `ScoredCandidate`-Liste plus Sieger und `SelectionReason`.
   `resolve_partner_target` gibt heute nur `target` + `reason` + `stats` zurueck.
   Erweitern um die vollstaendige `scored`-Liste (bereits gebaut, nur nicht
   herausgereicht) und die Ausschluss-Buckets (`cache_miss`, `stale_not_live`,
   `blacklisted`, `excluded_self`), die in der Filterschleife bereits bekannt sind.

### Flow-ID

`run` erzeugt die `flow_id` schon (Z. 519-522), aber nur wenn `observability_raid`
gesetzt ist. Fuer den Snapshot die ID unabhaengig davon einmal je Lauf erzeugen
(gleiche `next_flow_id("raid")`), sowohl an die Observability-Events als auch an den
Snapshot-Writer geben. Die assembly-Liste aus auto_raid.rs reist ueber die
`AutoRaidRequest` in die Pipeline, damit der Writer beide Stufen unter einer ID
schreibt. `raid_id` wird nachgetragen, sobald `twitch_raid_history` die Zeile hat
(gleiche Stelle wie das `raid_started`-Event).

### Writer

Neuer Store `RaidSnapshotStore` in tb-raid (`raid_snapshot_store.rs`), ein einziges
Batch-Insert je Flow. Der Writer laeuft best-effort: ein Fehler beim Snapshot darf
den Raid nie blockieren (nur `tracing::warn!`), analog zu den bestehenden
Observability-Emits. Verkabelt im Composition-Root (`raid_arrival_wiring.rs` /
`main.rs`), wo die Pipeline gebaut wird; Muster wie `with_observability`.

### Retention

Snapshots wachsen mit jedem Raid (ein paar Dutzend Zeilen pro Raid, wenige Raids pro
Tag, unkritisch). Trotzdem eine Obergrenze: taeglicher Prune auf 90 Tage im
bestehenden periodischen Task-Block in `main.rs` (neben dem Score-Refresh),
`DELETE ... WHERE created_at < now() - interval '90 days'`. Kein Partitionieren noetig.

## Dashboard

Neues Panel "Raid-Entscheidungen" im Admin-Bereich (kein oeffentlicher Pfad, laeuft
unter der bestehenden authentifizierten Dashboard-Shell, `dashboard_v2`).

- Route in `tb-dashboard-api` (`GET /twitch/api/v2/dashboard/raid-decisions?days=7`),
  serverseitige Admin-Auth wie beim Assistenten, Identitaet nur aus der Session.
- Liste der letzten Raids: von -> Ziel, Zeit, Grund, Kandidatenzahl.
- Aufklappen je Raid zeigt die volle Tabelle: jeder Kanal mit Live/Spiel/Deadlock,
  final_score und den Komponenten (readiness/duration/fairness), today_received_raids,
  Ausschlussgrund; der Sieger hervorgehoben mit `selection_reason`.
- Optik nach der geltenden Dashboard-Shell (gemeinsame Sidebar, Gold, Karten mit
  Tiefe). Keine Twitch-internen Popouts.

Pruefen: falls die Route doch oeffentlich erreichbar sein muss, in die Caddy-Allowlist
`@public_twitch` und `@dashboard_paths` eintragen und `caddy-config` nachziehen. Als
Admin-Route hinter der Shell ist das nicht noetig.

## Migration und Deploy

- Migration nach dem Twitch-DB-Weg: als `postgres` in `twitch_analytics` anwenden,
  danach SELECT/INSERT/UPDATE/DELETE an `twitchbot` und `twitchdash`, kein CREATE auf
  `public`, Version in `_sqlx_migrations` mit sha384 der Datei eintragen.
- `TB_DB_MIGRATE=0`: der Bot migriert nicht selbst.
- `.sqlx`-Offline-Dateien und der Schema-Snapshot
  (`rust/crates/tb-db/tests/fresh_schema_snapshot.txt`) aendern sich mit und gehoeren
  in den Auftrags-Scope.
- Release im isolierten Worktree bauen (rustc 1.97 im PATH), nicht im geteilten
  `_ttb-main-deploy`. System-Units neu starten
  (`sudo systemctl restart deadlock-twitch-bot-rust deadlock-twitch-dashboard-rust`).

## Tests

- Reine Einheit: Buckets korrekt zugeordnet (wrong_game vs. active vs. blacklisted),
  Sieger genau einmal `selected=1`, `selection_reason` nur beim Sieger.
- Store-Insert gegen die Docker-Test-DB (`rust/scripts/test_db.sh`), Batch-Insert und
  Prune-Query.
- Regression: ein Snapshot-Fehler bricht den Raid nicht ab (best-effort-Pfad).
- Bestehende tb-raid-Suite bleibt gruen (aktuell 799 Tests im Chat-Pfad, tb-raid
  separat); wer bricht, zieht nach.

## Live-Beweis

Nach Deploy einen echten Offline-Raid abwarten (oder per manuellem `!raid` ausloesen),
dann im Dashboard-Panel die Kandidatenliste mit Scores und Sieger zeigen. Gegenprobe
in der DB: `SELECT ... FROM twitch_raid_candidate_snapshots WHERE flow_id = ...`.

## Offene Entscheidungen fuer den Nutzer

1. Retention 90 Tage ok, oder laenger/kuerzer?
2. Fallback-Raids (Kategorie-Streamer, `is_partner_raid=false`): auch snapshoten?
   Empfehlung ja, dann sieht man auch, warum ein Nicht-Partner gewaehlt wurde.
3. Panel nur fuer den Betreiber, oder soll jeder Partner seine eigenen gesendeten
   Raid-Entscheidungen sehen? Empfehlung: v1 nur Betreiber.

## Abgrenzung zum Folgeauftrag

Sobald der Snapshot ein paar Tage Daten hat, damit die Score-Balance empirisch pruefen
(Readiness-Gewicht, today-Malus, Recent-Raid-Cooldown auf dem Partner-Pfad) und als
eigener Auftrag umbauen. Gemessen wird dann an echten Snapshots, nicht an Theorie.
