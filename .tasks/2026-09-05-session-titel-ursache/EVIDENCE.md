# Evidence: Session-Titel, Ursache statt Symptom

## Ursache 1: EventSub eröffnet ohne Titel
- `handle_stream_online` legt die Session mit `StreamSnapshot { id, started_at, ..Default::default() }` an: `rust/crates/tb-monitoring/src/handlers.rs:281-285` (Effekt `stream_online_session`)
- Der Kanal-Lookup mit Titel und Spiel läuft erst danach (Effekt `stream_online_channel_info`, `handlers.rs:305-349`) und schreibt nur in den Live-State (`apply_channel_info`), nicht in die Session
- `ChannelInfoSource` `rust/crates/tb-monitoring/src/poller/source.rs:58`, Verdrahtung `rust/bin/tb-bot/src/wiring.rs:205`

## Ursache 2: Einmal-Nachfüllen greift bei `''` nie
- `start_session` schreibt `stream_title = &new.title` (getrimmter String, bei EventSub leer, also `''`): `rust/crates/tb-monitoring/src/sessions/store.rs:273-296`
- `adopt_incomplete` füllt per `COALESCE(stream_title, $4)`; `COALESCE` sieht `''` als gesetzt, der Titel bleibt leer: `store.rs:437`
- Fenster `WHERE id = $5 AND samples = 0 AND start_viewers = 0`: `store.rs:438`; nach dem ersten Poll ist `start_viewers` gesetzt, der Titel bleibt für immer leer
- Datenlage vor dem Backfill: 419 titellose Sessions in 30 Tagen, alle `''`, 386 davon mit `start_viewers > 0`

## Symptom-Fix, der wieder raus soll
- Commit 2bd53d33: `backfill_missing_meta` `store.rs:451-475`, Aufruf pro Poll in `ensure_session` `tracker.rs:228-238`, Test `offene_session_bekommt_titel_nach_verpasstem_adopt_fenster` in `tests/write_core.rs`

## Leser von `stream_title` / `game_name` (auf NULL prüfen)
- `overview_sessions`: `COALESCE(bs.stream_title, '') AS title`, `title: Option<String>` (`rust/crates/tb-analytics/src/overview.rs`)
- Weitere Leser: `tb-analytics` (`post_stream.rs`, `ai_analysis.rs`, `title_performance`, `tag_analysis.rs`), `tb-dashboard-api/handlers/session_detail.rs`, `title_performance.rs`; jede Stelle vom Implementierer einzeln nachweisen

## Roter Lauf
Worktree `/home/nathanael/.worktrees/tb-session-titel`, Branch `fix/session-titel-ursache`, Test-DB `postgres://postgres:tbtest@127.0.0.1:33093/postgres` (`SQLX_OFFLINE=true`, `TB_TEST_REQUIRE_DB=1`).

- T1 `start_session_leerer_titel_wird_null_und_adopt_fuellt_ihn` (`tests/write_core.rs`): FAILED. Meldung `assertion left == right failed: leerer Titel muss NULL sein, nicht leerer String`, `left: Some("")`, `right: None` (write_core.rs:265). `start_session` schreibt einen leeren String statt `NULL`.
- T2 `stream_online_eroeffnet_session_mit_titel_und_kategorie` (`tests/eventsub_dispatch.rs`): FAILED. Meldung `assertion left == right failed`, `left: Some("")`, `right: Some("Ranked Grind")` (eventsub_dispatch.rs:711). Die Session wird ohne Titel eröffnet, der Lookup läuft erst danach und nur in den Live-State.

## Roter Lauf (Runde 2)
Blockierender Review-Befund: `adopt_incomplete` schrieb pro Poll für die ganze Stream-Dauer, weil die WHERE-Klausel `WHERE id = $5 AND ended_at IS NULL` die Leere nicht prüfte (REQ1 erlaubt das Füllen nur, solange Felder leer sind). Fix: WHERE-Klausel um `AND (stream_title IS NULL OR stream_title = '' OR game_name IS NULL OR game_name = '' OR (samples = 0 AND start_viewers = 0))` erweitert; `adopt_incomplete` gibt jetzt `rows_affected()` als `u64` zurück.

Neuer Regressionstest `adopt_incomplete_trifft_vollstaendige_session_nicht` (`tests/write_core.rs`): Session mit gesetztem Titel und Spiel, danach `record_sample` (samples > 0, start_viewers > 0); `adopt_incomplete` muss `rows_affected() == 0` liefern.

Roter Lauf mit erweiterter Rückgabe, aber noch alter WHERE-Klausel (nur `id`/`ended_at`): FAILED. Meldung `assertion left == right failed: vollstaendige Session darf pro Poll nicht neu geschrieben werden`, `left: 1`, `right: 0` (write_core.rs:331). Das UPDATE traf die vollständige Session weiterhin.

Nach dem WHERE-Fix: grün. `cargo test --no-fail-fast -p tb-monitoring` alle Suiten ok (u. a. `write_core` 18 passed), `cargo clippy -p tb-monitoring --all-targets` 0 Warnungen.

## sqlx-Offline-Entscheidung
`cargo sqlx prepare` ohne DB ist nicht möglich (`rust/scripts/sqlx-prepare.sh` fährt eine DB hoch und migriert). Die Bind-Änderung in `start_session` lässt den SQL-Text unverändert, also bleibt die vorhandene `.sqlx`-Cache-Datei gültig. Die geänderte `adopt_incomplete`-Abfrage ändert den SQL-Text. Statt eine neue Cache-Datei per DB zu erzeugen und die ganze Workspace-Prepare anzustoßen, wird `adopt_incomplete` auf `sqlx::query` (Laufzeit-geprüft, ohne Makro) umgestellt, wie es `backfill_missing_meta` schon tat. Die verwaiste Cache-Datei der alten Makro-Abfrage (`rust/.sqlx/query-16aad6252223b3870eb3eb1166546ce0131252e2db948b20a32182e9ff38b40f.json`) wird gelöscht, damit `cargo sqlx prepare --check` in CI keine Abweichung meldet.

## REQ4: Leser von twitch_stream_sessions.stream_title / game_name
`game_name` war schon vor dieser Änderung nullable (Poll-Pfad und `start_session` schrieben leeres Spiel über `game_name_opt()` als `NULL`), also besteht dort kein neues Risiko. Neu ist nur, dass `stream_title` bei leerem Titel jetzt `NULL` statt `''` ist. Makro-Leser (`query!`/`query_as!`) sind unkritisch, weil die Spalte im Schema nullable ist und sqlx sie ohnehin als `Option` erzwingt oder eine `!`-Override plus NULL-Filter verlangt. Geprüft wurde jede Nicht-Makro-Stelle und jede `!`-Override einzeln:

- `tb-analytics/src/overview.rs:608` (`SessionRaw`, non-macro `query_as`): Projektion `COALESCE(bs.stream_title, '') AS title`, Feld `title: Option<String>` (overview.rs:527). NULL-sicher.
- `tb-analytics/src/ai_analysis.rs:138` (`SessionRow`, non-macro `query_as`): `stream_title` roh, Zieltyp `Option<String>` (Tuple-Feld 1, ai_analysis.rs:24), Nutzung `r.1.as_deref()` (ai_analysis.rs:305). NULL-sicher.
- `tb-analytics/src/ai_analysis.rs:169,181` (`RankedSessionRow`): `COALESCE(stream_title, '')`. NULL-sicher.
- `tb-analytics/src/coaching.rs:266,312`: `s.stream_title AS "stream_title!"`, aber `WHERE s.stream_title IS NOT NULL AND s.stream_title != ''` filtert NULL vor dem Decode. NULL-sicher.
- `tb-analytics/src/post_stream.rs:638,755`: Feld `stream_title: Option<String>`, `.as_deref().filter(|s| !s.is_empty()).unwrap_or("")`. NULL-sicher.
- `tb-analytics/src/admin_streamers.rs:330,596`: Feld `stream_title: Option<String>` (Makro). NULL-sicher.
- `tb-dashboard-api/src/handlers/session_detail.rs:51,277`: `Option<String>`, `.unwrap_or_default()`. NULL-sicher.
- `tb-dashboard-api/src/handlers/title_performance.rs:91`: `!`-Override, aber `WHERE s.stream_title IS NOT NULL AND s.stream_title != ''`. NULL-sicher.
- `tb-dashboard-api/src/handlers/internal_home.rs:1350,2529`: `try_get::<Option<String>, _>("stream_title")`. NULL-sicher.
- `tb-internal-api/src/handlers/session_detail.rs:132,205`: `Option` (Makro) plus `insert_opt`. NULL-sicher.
- `tb-internal-api/src/handlers/stats_native.rs:1567,1774`: `stream_title` roh, Nutzung `r.stream_title.as_deref().unwrap_or_default()`. NULL-sicher.
- `tb-chat/src/promos.rs`: liest aus `twitch_stream_sessions` nur `avg_viewers`, kein `stream_title`-Read. Kein Risiko.

`game_name`-Override `ai_analysis.rs:256` nutzt `COALESCE(game_name, 'Unbekannt')`, `social_media.rs:1877` und `clip/helix.rs:171` lesen `game_name` aus Clip-/Helix-Kontext, nicht aus der Session. Kein neues Risiko.

Urteil: keine Anpassung an Lesern nötig, alle vertragen `NULL`.
