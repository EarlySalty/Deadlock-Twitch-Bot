# Paket B: Zuschauer-Punkte (Deadlock-Twitch-Bot)

status: gebaut, Branch `wip/points`
datum: 2026-10-01

## Was gebaut ist

- Migration `rust/migrations/20261001100000_community_points.sql` (additiv, GRANTs wie die übrigen Migrationen: `twitchbot` liest/schreibt, `twitchdash` liest):
  - `twitch_community_points_viewer_daily` je (`twitch_user_id`, `channel_twitch_user_id`, `day`)
  - `twitch_community_points_streamer_daily` je (`streamer_twitch_user_id`, `day`), inkl. `streamer_login`, `discord_user_id`
  - `twitch_community_points_discoveries` je (`twitch_user_id`, `channel_twitch_user_id`): erstes Auftauchen, `bonus_awarded` endgültig
  - Cursor-Indizes `(updated_at, Schlüssel)` UNIQUE
- Regeln und Aggregation: `rust/crates/tb-analytics/src/community_points.rs` (Tests in `community_points_tests.rs`). Alle Werte aus PLAN als Konstanten (5 min je Punkt, Deckel 72/144, Chat 30, 10 Zeichen, 60 s Cooldown, Duplikat, Entdecker 10 Punkte, max 3/Tag). Reine Funktionen für Tagesgrenze Berlin, Ausschlüsse, Deckel, Chat-Regeln, Entdecker-Auswahl und die komplette Tagesrechnung (`compute_day`).
- Taktung: tb-bot-Task `community_points_aggregation` (`rust/bin/tb-bot/src/community_points_wiring.rs`), alle 300 s. Rechnet heute neu, den Vortag zusätzlich beim ersten Lauf nach dem Start und in der ersten Stunde nach Berliner Mitternacht. Markierte alte Tage werden bei Rohkorrekturen zusätzlich neu berechnet. Rohänderungen stempeln auch unveränderte gedeckelte Werte neu; Zeilen eines Tages, die nicht mehr vorkommen, werden genullt.
- Endpunkte (`rust/crates/tb-internal-api/src/handlers/community_points.rs`, Router in `lib.rs`): `GET /internal/twitch/v1/community-points/viewers` und `/streamers`, JSON exakt nach PLAN, Auth/Loopback wie `/streamer-invites`, `limit` 1..5000 (Standard 1000), `updated_since` exklusiv.
- Doku: `docs/DATABASE.md` (Abschnitt Community-Punkte), `docs/API.md` (Interne Twitch-API).

## Datenquellen (kein zweiter Erfassungspfad)

- Anwesenheit: `twitch_viewer_presence_ticks` (Chatters-Poller, 30 s) plus jede Chat-Nachricht. Ticks tragen nur den Login; die Twitch-User-ID kommt aus `twitch_session_chatters.chatter_id` derselben Session (vom Poller aus Helix gesetzt). Jede Stichprobe deckt `[t, t+60 s)` ab; Minuten = Vereinigung, geschnitten auf den Berliner Tag und `jetzt`.
- Chat: `twitch_chat_messages` der Partner-Sessions; moderierte Nachrichten (`moderation_action`) zählen nie. Vorgeschichte 2 h vor Tagesbeginn für Duplikat/Cooldown über Mitternacht. `chat_messages` = Zahl zählender Nachrichten, `points_chat` = gedeckelt auf 30.
- Partner: `twitch_streamers_partner_state.is_partner_active = 1`, Discord-ID aus derselben View (Streamer-Identität). Sessions über `twitch_user_id`, Fallback Login.
- Raids: `twitch_raid_history` mit `success`, Ziel aktiver Partner, nicht an sich selbst.
- Ausschlüsse: Broadcaster im eigenen Kanal, `tb_analytics::bekannte_bots` (`WHITELISTED_BOTS` = `KNOWN_CHAT_BOTS`, plus `justinfan*`), nicht numerische IDs, globaler Bann (`twitch_chatter_global_ban`), dauerhafter Kanal-Bann (letztes `twitch_ban_events`-Ereignis `ban` ohne `ends_at`).
- Entdecker: neues Paar ohne Spur in `twitch_chatter_rollup` vor Tagesbeginn (per ID, oder per Login bei Rollup-Zeilen ohne ID); Vergabe in Reihenfolge des ersten Auftauchens, höchstens 3 je Tag.
- Watchtime-Tagesdeckel 144: Kanäle in Reihenfolge der ersten Anwesenheit behalten ihre Punkte.

## Cursor

`updated_at` wird je Tabelle streng monoton und eindeutig vergeben (Advisory-Lock, Basis = max(clock_timestamp, bisheriges Maximum + 1 µs), je Zeile +1 µs). Damit reicht `next_updated_since` allein als stabiler Cursor; er wird in RFC3339 UTC mit Mikrosekunden ausgegeben, wenn nötig, und muss unverändert zurückgegeben werden. Leere Seite: Cursor bleibt der übergebene Wert, ohne Wert `null`.

## Tests

Gegen Wegwerf-Timescale (`timescale/timescaledb:2.17.2-pg16`), `TB_TEST_DATABASE_URL`, `TB_TEST_REQUIRE_DB=1`:

- `cargo test -p tb-analytics --lib community_points`: 18 passed (Regeln, Deckel, Cooldown, Duplikat, Befehle, Mindestlänge, Ausschlüsse, Entdecker max 3/Tag, Berliner Tagesgrenze inkl. Umstellungstage; DB: Aggregation aus Rohdaten, Idempotenz, nachträglicher Bann nullt Zeile, Cursor über 25 Zeilen ohne Verlust/Doppel).
- `cargo test -p tb-internal-api --lib community_points`: 4 passed (Vertrags-JSON beider Endpunkte, Cursor-Folgeseiten, 401/403/400 über den echten Router).
- `cargo test -p tb-db`: Schema-Snapshot aktualisiert, `fresh_migrations_schema`, `migrationsversionen_eindeutig`, `runtime_schema_contract` grün.
- Build `tb-bot`, `tb-internal-api`, `tb-analytics` grün; clippy ohne neue Warnungen in den geänderten Dateien.
- Vorbestehende, nicht von Paket B verursachte Fehlschläge in der Umgebung: Tests mit isoliertem `initdb` (läuft hier als root), LLM-Freigabe-Tests, `session_detail`/`streamers::list_returns_200` (auf HEAD ohne Paket B identisch rot), parallele Migrationsläufe in `tb-db` (`tuple concurrently updated`, einzeln grün).

## Rest-Risiken

- Ticks ohne passende `chatter_id` in `twitch_session_chatters` (alte Zeilen, IRC-Lurker ohne ID) zählen nicht.
- Partner-Status gilt zum Zeitpunkt der Rechnung: verliert ein Kanal den Partnerstatus, werden seine Tageszeilen beim nächsten Lauf für diesen Tag genullt. Markierte alte Tage werden bei Rohdatenkorrekturen erneut berechnet und verwenden ebenfalls den aktuellen Partnerbestand.
- Später eingetroffene oder korrigierte Rohdaten markieren die betroffenen Berliner Tage. Der nächste Aggregationslauf verarbeitet diese Markierungen auch außerhalb des Startup- und Kulanzfensters.
- Chat-Vorgeschichte reicht 2 h zurück; ein Duplikat zu einer älteren letzten Nachricht wird nicht erkannt.
- Entdecker-Bonus ist eine endgültige Entscheidung; wird ein Kanal-Bann später aufgehoben, kommt der Bonus nicht nachträglich.
- Last: Jeder Lauf liest alle Ticks und Nachrichten des Tages der Partnerkanäle. Bei deutlich mehr Partnern/Zuschauern ggf. inkrementell rechnen.
- Leser (Paket C) sollte `next_updated_since` exakt zurückgeben. Ein abgeschnittener Cursor liefert nur Wiederholungen (harmlos, Zeilen sind idempotent), ein aufgerundeter würde Zeilen überspringen.
