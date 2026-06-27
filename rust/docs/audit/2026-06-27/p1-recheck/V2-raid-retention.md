# V2 Raid-Retention Recheck

Datum: 2026-06-27
Rolle: Verifizierer + Git-Archaeologe
Scope: B4-023 `known_from_raider` im periodischen Raid-Retention-Collector.

## Verdict

**FIX-CLEAR.**

Der periodische Rust-Collector ist nicht already-clean: `rust/crates/tb-monitoring/src/raid_retention.rs:173-195` zaehlt `known_from_raider` ueber Ziel-Session, `sc.last_seen_at >= executed_at` und Rollup-Join auf den FROM-Streamer, aber ohne `r.first_seen_at < executed_at`.

Die Soll-Semantik ist die Variante mit `first_seen_at < executed_at`: aktuelle Python-Recalc `bot/analytics/raid_metrics.py:128-166` und Rust-Dashboard-Recalc `rust/crates/tb-dashboard-api/src/handlers/raid_analytics.rs:116-143` haben diese Bedingung. Dadurch zaehlen nur Zuschauer, die bereits vor dem Raid beim Raider bekannt waren. Chatter, die erst nach dem Raid im Raider-Rollup auftauchen, sind fachlich nicht "known_from_raider".

## Aktueller Codevergleich

- Periodischer Collector: `count_known_from_raider` joint `twitch_chatter_rollup r` nur ueber `LOWER(r.streamer_login) = $3 AND r.chatter_login = sc.chatter_login`; danach `sc.session_id = $1` und `sc.last_seen_at >= $2`. Keine Historiengrenze auf `r.first_seen_at`.
- Dashboard-Recalc: `raid_analytics.rs` Query 2 hat `AND cr.first_seen_at < ri.executed_at`.
- Aktuelle Python-Recalc: `raid_metrics.py` Query 2 hat ebenfalls `AND cr.first_seen_at < ri.executed_at`.
- Legacy-Nuance bestaetigt: der alte Python-Loop `bot/analytics/mixin.py:2357-2369` hat dieselbe Luecke wie der Rust-Collector.

## Git-Archaeologie

### Dateihistorie `raid_retention.rs`

`git log --oneline -- rust/crates/tb-monitoring/src/raid_retention.rs`:

- `8685dcd` `fix(chatters): #11 Retention-Fenster + chatter_id-Parität (tb-monitoring)`
- `b8bcda4` `feat(chatters): #11 Helix-Chatters-Poller + Raid-Retention (tb-monitoring)`

### Entstehung des periodischen Collectors

`b8bcda4` vom 2026-06-22 fuehrte `rust/crates/tb-monitoring/src/raid_retention.rs` neu ein. Die Commit-Message sagt explizit: "Port von bot/analytics/mixin.py (... compute_raid_retention)" und beschreibt `raid_retention.rs` als "1h-Loop, reines SQL" mit `known_from_raider/new_to_target/new_chatters`. Der initiale `known_from_raider`-Query hatte keine `first_seen_at < executed_at`-Bedingung und zusaetzlich noch eine spaeter entfernte `+30min`-Obergrenze.

`8685dcd`, nur kurz danach, korrigierte drei Review-Befunde "byte-treu an bot/analytics/mixin.py": Obergrenzen bei `known_from_raider`/`new_to_target` entfernt, `new_chatters` ohne `last_seen_at`, `LOWER()` fuer `streamer_login`. Die Commit-Message erwaehnt keine fachliche Entscheidung gegen `first_seen_at < executed_at`; sie konserviert die alte Mixin-Semantik.

### Gegenbelege in Recalc-Pfaden

`b8cbd65` vom 2026-06-12 fuehrte den nativen Rust-Dashboard-Handler ein: "recalculate_raid_chat_metrics via Postgres json_to_recordset". Der `known_from_raider`-CTE enthielt von Anfang an `AND cr.first_seen_at < ri.executed_at`.

`9ef3c95` vom 2026-02-28 fuehrte `bot/analytics/raid_metrics.py` ein. Die aktuelle Python-Recalc enthaelt dort bereits in der initialen Datei `AND cr.first_seen_at < ri.executed_at`.

`4db719f` vom 2026-02-25 fuehrte den alten periodischen Python-Mixin-Loop ein. Dort fehlt die Bedingung bei `known_from_raider`, waehrend `new_to_target` und `new_chatters` bereits `first_seen_at < executed_at` gegen den TO-Streamer nutzen. Das ist der Ursprung der Luecke, die spaeter in `b8bcda4`/`8685dcd` nach Rust portiert wurde.

### Pickaxe

Globale `git log -S"known_from_raider" --all` und `git log -S"first_seen_at" --all` zeigen viele Spaetere Doku-/Schema-/Analytics-Commits, aber keinen Commit mit einer expliziten Begruendung, dass `known_from_raider` bewusst ohne Historiengrenze zaehlen soll. Die relevante Linie bleibt: alter Mixin-Loop ohne Bedingung -> Rust-Collector-Port ohne Bedingung; parallel existiert die neuere Recalc-Semantik mit Bedingung.

## Intent / Grillme

`rust/docs/audit/2026-06-15-grillme-entscheidungen.md` enthaelt nur Makro-Entscheidungen:

- Block 5: Capacity-Snapshot-Zeitreihe + Retention mitmigrieren.
- Block 6: Chatters-/Lurker-Poller bauen.
- Block 7: Raid-Arrival-Analyse und natives Dashboard-Raid-Analytics bauen.
- Block 16: Analytics-Korrektheit fixen; keine Sonderentscheidung fuer `known_from_raider`.

`rust/docs/audit/2026-06-27/00-baseline.md` sagt nur, dass Partner-Raid-Score/Retention/Chatters als periodische Rust-Tasks laufen und P1.24 "Raid Retention Hourly Loop" als behoben gefuehrt wurde. Es gibt dort keine Semantikentscheidung fuer "known_from_raider ohne `first_seen_at`".

Nuance: `rust/docs/2026-06-22-chatters-presence-poller-design.md:159-174` beschreibt den periodischen Collector ebenfalls ohne `first_seen_at` bei `known_from_raider`. Diese Spec nennt aber selbst `bot/analytics/mixin.py` als Quelle und passt zum `8685dcd`-Commit "byte-treu an mixin.py"; sie ist kein Owner-/Grillme-Intent, der die neuere Recalc-Semantik ueberschreibt.

## Fix-Spec

Minimaler Code-Fix in `rust/crates/tb-monitoring/src/raid_retention.rs`, Funktion `count_known_from_raider`:

```sql
JOIN twitch_chatter_rollup r
  ON LOWER(r.streamer_login) = $3
 AND r.chatter_login = sc.chatter_login
 AND r.first_seen_at < $2
```

Die Doc-Comment sollte entsprechend von "die im Rollup des FROM-Streamers stehen" auf "die vor dem Raid im Rollup des FROM-Streamers standen" geschaerft werden.

Test noetig: ja. Die bestehenden `rust/crates/tb-monitoring/tests/raid_retention.rs`-Tests decken nur "first_seen vor Raid" und "keine +30min-Obergrenze" ab. Es fehlt ein Regressionsfall fuer `first_seen_at >= executed_at`.

Empfohlener Test:

- Seed Target-Session + Raid.
- Seed Ziel-Chatter `late_raider_viewer` mit `last_seen_at >= executed_at`.
- Seed Raider-Rollup fuer denselben Login mit `first_seen_at = executed_at + 5 minutes`.
- Erwartung: `known_from_raider = 0`; Fensterzaehler duerfen den Chatter weiterhin normal zaehlen.

Nach Fix ausfuehren:

```bash
TB_TEST_REQUIRE_DB=1 TB_TEST_DATABASE_URL=postgres://postgres:tbtest@127.0.0.1:5434/postgres cargo test -p tb-monitoring raid_retention
```

## Schluss

Keine Hinweise auf Absicht. Der Befund ist ein Port-Bug aus dem alten Mixin-Loop. Fix soll den periodischen Collector an aktuelle Python-Recalc und Rust-Dashboard-Recalc angleichen.
