# Paket E: Clips aus Twitch (Deadlock-Twitch-Bot)

status: gebaut, Branch `wip/tb-clips-scout`
datum: 2026-10-01

## Was gebaut ist

- Gemeinsamer Dienst `rust/crates/tb-chat/src/clip_contest_submit.rs` (Tests in `clip_contest_submit_tests.rs`), genutzt von Chat und Dashboard:
  1. Kanal muss aktiver Partner sein (`twitch_streamers_partner_state.is_partner_active = 1`).
  2. Clip-URL: nur `clips.twitch.tv/<slug>` und `(www.|m.)twitch.tv/<kanal>/clip/<slug>`, mit oder ohne Schema, Query/Fragment egal. Kanonisch weiter als `https://clips.twitch.tv/<slug>`.
  3. Ohne URL: jüngster Clip der offenen Session (`twitch_stream_sessions.ended_at IS NULL`), aus `twitch_clip_command_events` (`!clip`) und `twitch_clips_social_media` (Clip-Pipeline, nur `source_kind = 'twitch'`, nicht verworfen). Keine offene Session oder kein Clip: Hilfe-Antwort.
  4. Bestand und Tageslimit (3 je Kanal und Berliner Tag) vor dem Helix-Aufruf.
  5. Helix `GET /clips?id=` mit dem vorhandenen App-Token-Client: Clip muss existieren und `broadcaster_id` = Kanal sein.
  6. Claim unter `pg_advisory_xact_lock` je Kanal: Bestand und Limit erneut prüfen, Zeile `pending`. Zweiter gleichzeitiger Aufruf bekommt `InFlight` (keine Antwort im Chat).
  7. `POST /internal/master/v1/clips/submit` exakt nach PLAN, `idempotency_key = "twitch-clip-<clip_id>"`, zugleich `X-Idempotency-Key`. Ergebnis in `twitch_clip_contest_forwards` (`accepted`, `duplicate`, `rejected`, `failed`).
- Broker-Zugang: der vorhandene `BrokerRelay` (`rust/crates/tb-transport-discord/src/relay.rs`, neue Methode `submit_twitch_clip`, Pfad in die Allowlist aufgenommen). Im tb-bot aus `settings.broker` wie Discord-Invites und Voice-Join. Im Dashboard derselbe `BrokerRelay` mit Basis-URL aus der Betriebskonfiguration (`broker.base_url`) und dem bestehenden Token aus dem Infisical-Vertrag (`MASTER_BROKER_TOKEN`, sonst `MAIN_BOT_INTERNAL_TOKEN`, sonst `TWITCH_INTERNAL_API_TOKEN`, gleiche Reihenfolge wie `tb_config::BrokerConfig`). Kein neues Secret, keine ENV.
- Chat: `!clipcontest [clip-url]` in `rust/crates/tb-chat/src/commands.rs`, Katalog-Eintrag (Gruppe Moderation) in `catalog.rs`. Nur Broadcaster und Mods; Shared-Chat-Nachrichten aus anderen Kanälen werden still ignoriert (Abzeichen gelten dort). Bekannte Bots dürfen den Befehl nicht auslösen. Verdrahtung: `chat_wiring::build_clip_contest` (tb-bot), braucht Helix und Broker, sonst Hinweis „nicht erreichbar“.
- Dashboard: `POST /social-media/api/clips/{clip_db_id}/clip-contest` (`rust/crates/tb-dashboard-api/src/handlers/social_media_clip_contest.rs`), Zugriff wie die übrigen Clip-Aktionen (Social-Media-Freigabe, Clip im eigenen Kanal, Partner-Freigabe-Guard). Knopf „Für Clip-Contest einreichen“ im Menü der Clip-Karte (`bot/dashboard_v2/src/pages/SocialMedia.tsx`, API `submitClipToContest`), zeigt den Antwortsatz direkt an. Nur für Twitch-Clips, nicht für Uploads.
- Migration `rust/migrations/20261001105000_clip_contest_twitch_forwarding.sql` (additiv): `twitch_clip_contest_forwards`, Index (`broadcaster_twitch_id`, `created_at`), Rechte `twitchbot` und `twitchdash` lesen/schreiben.
- Doku: `docs/API.md` (Abschnitt Clip-Contest aus Twitch), `docs/DATABASE.md`, `docs/streamer/COMMANDS.md`, Streamer-FAQ `rust/knowledge/bot/chat-befehle.md` (Spiegel nach Deadlock-Docs laut SSOT-Hinweis noch offen).

## Antworten im Chat

| Ergebnis | Text |
| --- | --- |
| angenommen | Clip ist im Wochen-Contest, die Community stimmt im Discord ab. |
| schon drin (eigener Eintrag oder Broker `duplicate`) | Der Clip wurde bereits eingereicht und wird nicht erneut gesendet. |
| abgelehnt | Der Clip wurde nicht angenommen: <Grund>. `not_partner`: „dieser Kanal ist im Discord noch nicht als Partner eingetragen. Bitte melde dich beim Team.“; `invalid_clip_url`, `idempotency_conflict` mit eigenem Satz, sonst allgemein |
| Broker nicht erreichbar | Der Discord ist gerade nicht erreichbar. Versuch es später nochmal. |
| Limit | Heute sind schon 3 Clips aus diesem Kanal eingereicht. Morgen geht es weiter. |
| fremder Clip / nicht gefunden / Twitch weg | Es zählen nur Clips aus diesem Kanal. / Diesen Clip finde ich auf Twitch nicht. / Twitch antwortet gerade nicht. … |
| ohne URL und ohne Clip, falsche URL | Hilfe: So geht's: !clipcontest <Clip-Link> … |
| kein Broadcaster/Mod | Clips einreichen können nur der Broadcaster und Mods. |

## Tests

Gegen Wegwerf-Timescale (`timescale/timescaledb:2.17.2-pg16`), `TB_TEST_DATABASE_URL`, `TB_TEST_REQUIRE_DB=1`:

- `cargo test -p tb-chat --lib clip_contest`: 11 passed. Reine Regeln: URL-Parsing (erlaubte und abgelehnte Formen), Rechte inkl. Shared Chat, Tageslimit, Bestand/Doppelsend, Titel, Helix-Antwort, Broker-Envelope, Payload exakt nach PLAN, Antworttexte ohne Gedankenstriche. DB: angenommen und danach „schon drin“ ohne zweiten Broker-Aufruf; Ablehnungen vor dem Broker (URL, nicht gefunden, fremder Clip, Twitch weg, kein Partner, keine Session); Limit 3 je Tag, Duplikat zählt nicht, Vortag zählt nicht, `rejected` mit Grund; Broker offline, neuer Versuch über das Dashboard; zwei gleichzeitige Einreichungen ergeben genau einen Broker-Aufruf; ohne URL gewinnt der neueste Clip der laufenden Session.
- `cargo test -p tb-chat --lib catalog`: Katalog eindeutig, Dispatch-Befehle registriert.
- `cargo test -p tb-transport-discord`: neue Tests für `submit_twitch_clip` (Payload, Header, Envelope; 503 und unbekannter Status sind Fehler).
- `cargo test -p tb-dashboard-api --lib clip_contest_forward`: HTTP-Abbildung der Ergebnisse, ohne Anmeldung 401.
- `cargo test -p tb-db` (einzeln, `--test-threads=1`): Schema-Snapshot um `twitch_clip_contest_forwards` ergänzt, alle grün.
- Abgleich mit dem fertigen Broker (`Deadlock-Bots`, `rust/crates/dl-broker/src/clips.rs`): Felder, Envelope, `deny_unknown_fields`, Formregeln (Login klein, Twitch-IDs ohne führende Null, Titel bis 200 Zeichen ohne Steuerzeichen). Ungültige `submitted_by`-IDs schickt der Dienst als `null` statt einen 400 zu riskieren.

## Offene Punkte

- Frontend nicht gebaut und nicht typgeprüft (in dieser Umgebung keine `node_modules`). Änderung ist klein (ein Menüpunkt, ein `useMutation`, eine API-Funktion, Übersetzungen); `npm run build` und die Node-Tests (`socialMediaContract`, `i18n`) bitte vor dem Merge laufen lassen.
- `submitted_by_twitch_user_id` ist die Person, die einreicht (bei Mods die Mod-ID, im Dashboard die ID des Partners, Admins ohne). Credit im Contest bleibt laut Paket D der Streamer-Login.
- Der Broker prüft Partner über `bot.twitch_streamer_invites`; Partner ohne Eintrag dort bekommen `rejected`/`not_partner` (Text nennt das).
- Ein Clip wird je Clip-ID genau einmal gezählt (Schlüssel `twitch-clip-<id>` ohne Woche). Ein in einer Vorwoche angenommener Clip meldet „schon drin“.
- Liegengebliebene `pending`-Zeilen (Absturz zwischen Claim und Broker) sind nach 120 s wieder frei; der Broker-Schlüssel ist idempotent.
- Streamer-FAQ nach Deadlock-Docs (`public/twitch-bot/`) spiegeln.
