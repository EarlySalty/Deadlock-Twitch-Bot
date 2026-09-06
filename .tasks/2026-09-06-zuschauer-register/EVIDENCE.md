# EVIDENCE: Zuschauer-Register (Twitch-Konto zu Discord-Mitglied)

Bestandsaufnahme, Stand 2026-09-06. Reine Recherche, kein Codeeingriff.
Schluessel: Twitch-User-ID = `chatter_id` / `twitch_user_id`, davon getrennt der `*_login`.

## 1. Twitch-Seite (DB `twitch_analytics`)

Zuschauer-Identitaeten:

- `twitch_session_chatters` (session_id, streamer_login, chatter_login, `chatter_id`=Twitch-User-ID, first_message_at, messages, is_first_time_streamer, seen_via_chatters_api, last_seen_at, confirmed_first_ever). 85287 Zeilen, 14398 distinct `chatter_id`, 33253 distinct `chatter_login`, frueheste `first_message_at` 2026-01-31. dach_lock: 288 distinct `chatter_id` / 327 distinct login. WICHTIG: 42752 der 85287 Zeilen (50 Prozent) haben `chatter_id` NULL/leer, verteilt auf 21655 login-only-Namen. Urteil: nutzbar als Zuschauer-Bestand, aber der Schluessel ist lueckenhaft (halbe Historie ohne User-ID).
- `twitch_chat_messages` (id, session_id, streamer_login, chatter_login, `chatter_id`, message_id, message_ts, is_command, content, moderation_action, moderation_reason). 232438 Zeilen. Zweiter Zuschauer-Bestand mit ID und Login. Urteil: nutzbar.
- `twitch_engagement_conversation` (channel_login, role, `twitch_user_id`, twitch_login, content, message_id, ts). 4027 Zeilen, ab 2026-05-30. Urteil: nutzbar, kleiner Ausschnitt.
- `twitch_first_message_events` (streamer_login, broadcaster_id, chatter_login, `chatter_id`, message_text, event_ts). Erste-Nachricht-Signal je Streamer. Urteil: nutzbar fuer "erstes Mal in DIESEM Kanal", nicht kanaluebergreifend.
- `twitch_greeted_chatters` (streamer_login, chatter_login, `chatter_id`, greeted_at). Begruessungs-Dedup. Urteil: nur Begruessungs-Historie.
- `twitch_login_aliases` (`twitch_user_id`, login, first_seen_at, last_seen_at, is_current). Loest Login zu ID auf. Urteil: nutzbar, um login-only-Zeilen (siehe oben) nachzuschluesseln, soweit die ID je bekannt war.

Streamer / Partner / Ausschluss:

- `twitch_streamers` (twitch_login, `twitch_user_id`, created_at). 70 Zeilen, ab 2025-10-10. Deadlock-Streamer-Pool. Urteil: nutzbar fuer Ausschluss (c, Streamer-Kandidat).
- `twitch_scout_candidates` (streamer_login, `twitch_user_id`, sessions_count, avg_viewers, first_seen, last_seen). 0 Zeilen (leer). Urteil: nicht nutzbar, leer.
- `twitch_raid_auth` (`twitch_user_id`, twitch_login, tokens, scopes, raid_enabled ...). 68 Zeilen. Partner-OAuth. Urteil: nutzbar als Partner-Nachweis.
- `twitch_partners` (id, `twitch_user_id`, twitch_login, require_discord_link, ..., partnered_at, `departnered_at`, status, `admin_archived_at`, `technical_pause_reason`, `inactivity_flagged_at`, verified). 69 Zeilen: 61 status=active, 8 status=departnered (departnered_at gesetzt). Urteil: nutzbar, und ein Bestand "ehemalige Partner" EXISTIERT (departnered_at / admin_archived_at), 8 Zeilen. Deckt Ausschluss (c) fuer aktuelle UND fruehere Partner.
- `twitch_partner_outreach` (streamer_login, streamer_user_id, `twitch_user_id`, detected_at, contacted_at, status, cooldown_until, ...) und `twitch_partner_outreach_conversations` (streamer_user_id, `twitch_user_id`, state, messages_json ...). Outreach-Ledger fuer Streamer. Urteil: nutzbar als "schon kontaktiert".
- `twitch_promo_pitch_log` (channel_login, `target_user_id`, pfad, occasion, trigger_text, generated_text, reject_reason, sent_at, created_at). 5 Zeilen, ab 2026-09-05. Pitch-Ledger. Urteil: nutzbar als "schon gepitcht".
- `twitch_partner_signup_denylist` und `twitch_scout_pitch_blacklist` (je `twitch_user_id`). Ausschlusslisten. Urteil: nutzbar.

Bestehendes Register (Kernfund):

- `twitch_streamer_identities` (`twitch_user_id`, twitch_login, `discord_user_id`, discord_display_name, is_on_discord, created_at, updated_at). 1024 Zeilen, davon nur 62 mit gesetzter `discord_user_id`. Das ist die EINZIGE echte Twitch-User-ID zu Discord-ID Zuordnung, aber ausschliesslich fuer Streamer/Partner befuellt (Fuzzy-Korrelation, siehe Abschnitt 3), nicht fuer Zuschauer. Urteil: als Muster und Ziel-Schema nutzbar, als Datenbestand fuer Zuschauer unbrauchbar (62 Mappings).

## 2. Discord-Seite (zentrale DB, `DEADLOCK_CENTRAL_DSN`)

Keine gespeicherte Twitch-Identitaet: Die Suche nach `%twitch%`-Spalten in den echten Schemas (core, activity, public, verify, members, discord, website) liefert NULL Treffer.

- `activity.guild_member_directory` (guild_id, `user_id`=Discord-ID, joined_at, account_created_at, is_bot, present, synced_at). 2751 Zeilen gesamt, 2547 present und nicht-Bot, fruehestes joined_at 2024-09-28. Mitglieder plus Beitritts- und Kontoerstellungsdatum. Urteil: nutzbar als Mitglieder-Grundgesamtheit, aber ohne Namen (nur IDs).
- `activity.member_events` (`user_id`, guild_id, event_type, occurred_at, display_name, account_created_at, join_position, metadata). Namens-Historie je Discord-ID. Urteil: nutzbar als Namensquelle fuer den Abgleich.
- `core.users` (`discord_id`, username, global_name, avatar, first_seen, last_seen, raw jsonb). 1513 Zeilen. Urteil: nutzbar als Namensquelle (username, global_name); `raw` koennte weitere Felder tragen, ungeprueft.
- `core.steam_links` (`discord_id`, steam_id64, verified, deadlock_rank, is_steam_friend, friend_bot_account_id, ...). 538 Zeilen. Verify laeuft ueber Steam. Kein Twitch. Urteil: als Twitch-Bruecke nicht nutzbar.
- `core.discord_role_connection_tokens` (`discord_id`, access_token, refresh_token, scope, provider ...). 1 Zeile, provider=steam, scope = "connections role_connections.write identify guilds.members.read". Der connections-Scope IST vorhanden, aber nur bei einem einzigen Nutzer. Urteil: nicht nutzbar (Datenmenge 1).

Verify- und OAuth-Code (Fundstellen aus Deadlock-Bots und Website):

- Verify erfasst nur Steam: `Deadlock-Bots/rust/bin/dl-bot/src/serversync/rang_guide_publish.rs:74-77` (Panel-Buttons), Schreibpfad `Deadlock-Bots/rust/crates/dl-activity/src/lfg_freetext.rs:960` und `:1085` (INSERT core.steam_links). Kein Twitch. Urteil: nicht nutzbar.
- Discord-Connections werden abgerufen (`Deadlock-Bots/rust/crates/dl-dashboard/src/oauth.rs:183` `fetch_connections`), aber nur Steam extrahiert (`oauth.rs:219` `extract_steam_connection_ids`, aufgerufen `dl-dashboard/src/web.rs:1143-1145`); Twitch wird explizit ignoriert (Testbeleg `oauth.rs:361`). Urteil: Twitch-Connection wird verworfen, nicht gespeichert.
- Website-Login-OAuth nur Scope "identify" (`Website/builds/backend-rust/src/routes/auth.rs:28-40`); Role-Connection-OAuth Scope "identify role_connections.write" ohne connections (`Website/builds/backend-rust/src/discord_role_connection.rs:20`). Urteil: keine Discord-ID plus Twitch-ID Speicherung fuer Zuschauer.
- Website liest read-only die Twitch-DB fuer die Creator-Linked-Role: `Website/builds/backend-rust/src/discord_role_connection.rs:1228-1295` (CREATOR_PROFILE_SQL joint `public.twitch_streamer_identities`, `twitch_raid_auth`, `twitch_partners_all_state` ueber `discord_user_id`), Pool `app.rs:468` `connect_twitch_pool`. Urteil: bestaetigt, dass die einzige Zuordnung in der Twitch-DB liegt, nicht zentral.

## 3. Bestehende Zuordnungslogik Twitch zu Discord

- Fuzzy-Namensabgleich existiert, aber nur fuer Streamer-Onboarding: `Deadlock-Bots/rust/crates/dl-bridges/src/streamer_intent.rs:257-279` (`correlate`, `similarity(login_key, norm_key(discord_name))`, Schwelle `FUZZY_FLOOR = 0.62`), Normalisierung `Deadlock-Bots/rust/crates/dl-bridges/src/matcher.rs`. Ergebnis wird per `POST /streamers/{login}/discord-profile` an den Twitch-Bot geschickt (`dl-bridges/src/twitch.rs:203-221`) und landet in `twitch_streamer_identities`.
- Zweiter Matcher im Twitch-Bot: `Deadlock-Twitch-Bot/rust/bin/tb-bot/src/streamer_link.rs:377` baut einen `MemberIndex` aus der Guild-Mitgliederliste, `best_match` `:401`, schreibt `discord_user_id` nur fuer Streamer-Kandidaten. Guild-Liste per `Deadlock-Twitch-Bot/rust/crates/tb-transport-discord/src/relay.rs:443` `list_members()` (Broker `/internal/master/v1/discord/members`).
- Partner-seitige Zuordnung im Dashboard: `Deadlock-Twitch-Bot/rust/crates/tb-dashboard-api/src/handlers/discord_link.rs:167` `set_discord_profile`, Rueckaufloesung `tb-analytics/src/admin_streamers.rs:551` `login_for_discord_user`.

Urteil: Das Muster (Fuzzy-Match plus Guild-Mitgliederliste) ist vorhanden und wiederverwendbar, aber komplett auf Streamer/Partner ausgerichtet. Fuer gewoehnliche Zuschauer gibt es KEINEN Namens- oder Mitgliedschaftsabgleich.

## 4. Pitch-Pfade heute (`rust/crates/tb-chat/src/promos.rs`)

Anlass-Pitch `on_message_pitch` (`promos.rs:750`), Gate-Reihenfolge vor dem Senden: Textlaenge/`!`-Prefix `:752`, Broadcaster-Ausschluss und leere ID `:755`, Semaphore `:764`, Judge-Drossel `:769`, Partnerkanal `:775`, Kanal-Allowlist `:783`, Werbefrei-Plan `:788`, Outbound-Suppression `:793`, Startverzoegerung `:798`, Partner-Weiche `:804`, User-Limit `:819`, Kanal-Limit `:824`, danach LLM-Judge/Filter/Injection/Doppelsend-Lock `:838`-`:891`. Kein Mod-Ausschluss, kein Bot-Konto-Ausschluss, kein First-Time-Gate, kein Discord-Check.

Partner-Pitch `run_partner_pitch` / `partner_candidate` (`promos.rs:1205`), gemergt und live: eine SQL-Query rein ueber `twitch_user_id` gegen `twitch_stream_sessions` (Deadlock-Session 60 Tage `:1209-1211`), NOT EXISTS `twitch_partners` `:1212`, nicht `twitch_scout_pitch_blacklist` `:1213`, nicht `twitch_partner_outreach` `:1216`, nicht im Ledger `twitch_scout_pitch_ledger` action='posted' `:1219`; Limits `partner_user_limit_ok` `:1245`, `partner_channel_limit_ok` `:1274`, `partner_daily_limit_ok` `:1311`; Ledger-Insert `record_partner_ledger` `:1330`.

Erstes-Mal-Erkennung: `confirmed_first_ever` geschrieben `tb-monitoring/src/telemetry.rs:565` (Insert twitch_first_message_events) und `:585` (UPDATE twitch_session_chatters), gelesen `tb-analytics/src/chatter_verlauf.rs:94`. `is_first_time_streamer` geschrieben `tb-analytics/src/overview.rs:1026`, `tb-monitoring/src/chatters_poller.rs:443`, `.../irc_lurker.rs:119`, gelesen im Scam-Guard `tb-chat/src/conversation_scam.rs:363` und `:716`.

Urteil: Ausschluss (c) ist mit `partner_candidate` fertig und ID-basiert; Ledger-Muster ist die Vorlage fuer einen "schon-im-Discord"-Filter. Bedingung (a) existiert nur als Signal je Session/Streamer, nicht kanaluebergreifend und nicht am Pitch-Gate. Bedingung (b) Discord-Check fehlt komplett.

## 5. Datenmenge fuer den Backfill

- Twitch gesamt: 14398 distinct Twitch-User-ID (33253 distinct Login) in `twitch_session_chatters`; dach_lock 288 distinct ID (327 distinct Login). 50 Prozent der Zeilen ohne `chatter_id`.
- Discord: 2547 praesente Nicht-Bot-Mitglieder (2751 total), 1513 in `core.users`, 538 mit Steam-Link.
- Exakter Namensabgleich case-insensitive (nur Zahlen, keine Namen ausgegeben): global 150 Treffer gegen 2428 Discord-Usernamen, 202 gegen 4632 Namen inkl. Display-Namen; dach_lock 57 gegen Usernamen, 63 inkl. Display-Namen von 327 Logins. Das sind rund 18 Prozent der dach_lock-Logins, weit unter der genannten 80-Prozent-Ueberlappung.
- Bestehendes Register `twitch_streamer_identities`: 1024 Zeilen, 62 mit Discord-ID.

Urteil: Ein reiner Exakt-Namensabgleich rekonstruiert die 80-Prozent-Ueberlappung nicht (nur ca. 18 Prozent). Ein Wahrscheinlichkeitsmodell mit mehreren Signalen ist noetig.

## Luecken (was fuer das Register fehlt)

- Kein Zuschauer-Register: die einzige Twitch-User-ID zu Discord-ID Zuordnung (`twitch_streamer_identities`, 62 Mappings) ist streamer-only. Fuer Zuschauer existiert nichts.
- Schluessel lueckenhaft: 50 Prozent der historischen Chatter-Zeilen tragen keine Twitch-User-ID, nur den Login. Ein Backfill muss diese ueber `twitch_login_aliases` oder Helix nachschluesseln, sonst ist die halbe Historie nicht ID-fuehrbar.
- Keine gespeicherte Discord-Connection zu Twitch: der Verify erfasst nur Steam, der connections-Scope holt Twitch zwar theoretisch ein, verwirft es aber (`oauth.rs:219`) und ist nur bei 1 Nutzer aktiv. Diese Quelle muesste breit reaktiviert werden (Opt-in oder Scope-Nutzung), um harte Treffer zu liefern.
- Exakter Namensabgleich deckt nur rund 18 Prozent ab; die Wahrscheinlichkeit muss aus mehreren Signalen gebildet werden (Namensaehnlichkeit wie im vorhandenen `matcher.rs`, Steam-Link-Anzeigenamen, Discord-Connections falls reaktiviert, Beitritts- und Aktivitaetszeit).
- Kein kanaluebergreifendes "erstes Mal in irgendeinem Partnerkanal": `twitch_first_message_events` ist je Streamer; ein Aggregat ueber alle 61 aktiven Partnerkanaele fehlt und muss neu gebaut werden.
- Kein Discord-Beitrittsdatum je Twitch-Zuschauer ohne ID-Bruecke; `guild_member_directory.joined_at` ist nur ueber die noch fehlende Zuordnung nutzbar.
- Ablageort offen: die zentrale DB (`core`) traegt keine Twitch-Daten, die Twitch-DB traegt das streamer-only-Register. Ein Zuschauer-Register braucht eine bewusste Entscheidung, wo es liegt (Twitch-DB analog `twitch_streamer_identities` oder zentral in `core`).
