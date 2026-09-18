# Register: Titel-Studio Co-Stream und Kreativität

- Intent-Thread: 5ef2c5d8-3f90-4a99-a017-0d6f41cd93c1 (kurz 5ef2c5d8)
- Stufe: mittel
- Auftrag: `.tasks/2026-09-18-titel-studio-costream/AUFTRAG.md`
- Branch: `feat/titel-studio-costream`
- Worktree: `/home/nathanael/.worktrees/tb-titel-costream`

## Thread-Register (T3)

| Paket | Thread-ID | Modell | Status | Worktree | Letzte Meldung |
|---|---|---|---|---|---|
| Vorcheck | ade64d1b | glm-token | tot, gesettelt, nicht wieder aufnehmen | keiner | OpenRouter 402, Guthaben leer; Vorcheck hat der Intent-Agent selbst per Graphify gelesen |
| A | 26e1c46d | opus48 | fertig, gesettelt | `/home/nathanael/.worktrees/tb-titel-costream` | Commit 3f12d6f0 auf origin; Migrations-Kollision 20260918120000 ist auf main schon gelöst (1bc4ba94), Branch braucht vor dem Gate einen Merge von origin/main |
| Review R1 | 52a77b4b | über --rolle review_1 gewählt | tot, gesettelt, nicht wieder aufnehmen | keiner | ProviderAdapterSessionNotFoundError (grok-Adapter), kein Review gelaufen, keine REVIEW.md |
| B (Nachtrag 3) | direkte ChatGPT-Sitzung, keine T3-Thread-ID verfügbar | GPT-6 Astra Pro | fertig, Review offen | `/home/nathanael/.worktrees/tb-titel-costream` | Main-Merge a77524fd; Shared Chat → Steam-Party → Voice, Deduplizierung nach Twitch-ID. 7 neue Tests grün mit Rot-Nachweisen; 9 vorbestehende Fehler unverändert. Clippy, Frontend und Schema-Vertrag geprüft. SQLx-Online-Prüfung mit dokumentierter Brain-Abhängigkeit. Details: REPORT-B.md. Kein weiterer Thread und keine Unter-Agenten gestartet. |
| Review R1 neu | startet der Nutzer selbst nach B | | offen | derselbe, nur lesen | |

## Befund Voice-Quelle

Baubar, kein Bump-up. Alles über reine SQL-Joins im bestehenden zentralen Postgres-Pool des Twitch-Dashboards, kein neuer Gateway, keine Bridge, kein Helix-Poll.

Aktuelle Voice-Belegung: `voice.deadlock_voice_watch` (steam_id PK, guild_id, channel_id, updated_at) in der zentralen DB (dl-central-db). Deadlock-Bots schreibt sie je Tick als Voll-Sync (`Deadlock-Bots rust/crates/dl-voice/src/status.rs:614` `persist_voice_watch`: DELETE-all bei leer, sonst Upsert plus Abräumen aller nicht mehr Anwesenden). Wer den Kanal verlässt, verschwindet, also spiegelt die Tabelle die aktuelle Belegung. `updated_at` dient als Frische-Schranke, falls der Sync steht.

Join-Kette (alle Tabellen im selben Pool; core.steam_links-Prod-Nutzung belegt in `rust/crates/tb-dashboard-api/src/handlers/caster_overlay.rs:840`):
- twitch_user_id <-> discord_user_id: `twitch_streamer_identities` (siehe `title.rs:273` `resolve_discord_user_id`).
- discord_id <-> steam_id: `core.steam_links` (Spalte `steam_id` TEXT; dieselbe Repräsentation wie `voice.deadlock_voice_watch.steam_id` und `activity.live_player_state.steam_id`, Beleg `Deadlock-Bots rust/crates/dl-voice/src/status.rs:426` `steam_ids_for` und `:474` `presence_rows`).
- Live auf Twitch: `twitch_live_state` (twitch_user_id PK, streamer_login, is_live; Beleg `rust/crates/tb-monitoring/src/irc_lurker.rs:95`, `rust/crates/tb-dashboard-api/src/handlers/uplink.rs:206`). Das ist der vorhandene Live-Stand, kein neuer Poll.

Ablauf der Erkennung: eigene discord_id -> eigene steam_id -> eigener channel_id/guild_id aus `deadlock_voice_watch` (frisch) -> andere steam_ids im selben channel_id/guild_id (frisch, ungleich eigener) -> deren discord_id -> deren twitch_user_id -> nur die mit `twitch_live_state.is_live = 1`, deren `streamer_login`, höchstens zwei.

## Befund Party-Hinweis (Nachtrag 1, Punkt 2)

Der Partystand stammt aus Steam-Präsenz, nicht aus Discord-Voice-Daten. Quelle `voice.deadlock_party_members` (party_id TEXT, steam_id TEXT, party_size INTEGER, seen_at TIMESTAMPTZ), geprüft gegen `Deadlock-Bots/rust/crates/dl-central-db/migrations/0005_voice.sql:1`. Der Steam-Schreiber `Deadlock-Steam-Bot/rust/crates/steam-persistence/src/presence.rs:125` (`replace_party_membership`, Insert ab Zeile 142) ersetzt je Steam-ID den bisherigen Partystand. `steam_lookup::get_party_hint_for_discord_user` joint core.steam_links -> deadlock_party_members, nimmt MAX(party_size) mit seen_at jünger als 10 Minuten und mappt zu solo/Duo/Dreier/Vierer/Fünfer/Sechser. Nachtrag 3 korrigiert außerdem den belegten Typfehler: Das INTEGER-Aggregat wird explizit als BIGINT für den bestehenden i64-Decoder ausgegeben; zuvor lieferte echte Steam-Präsenz keinen Party-Hinweis. Keine Namen. Der HTTP-Pfad bleibt unverändert, der Party-Hinweis kommt zusätzlich aus der zentralen DB.

## Befund Twitch Shared Chat (Nachtrag 2)

Baubar. Twitch-Doku "Get Shared Chat Session": `GET /helix/shared_chat/session?broadcaster_id=<id>`, Authorization laut Doku-Abschnitt "App-Token ODER User-Token" (die 401-Zeile nennt zwar nur User-Token, der Authorization-Abschnitt erlaubt aber ausdrücklich App-Token), kein Scope. Antwort: `data[].participants[].broadcaster_id`, keine Logins im Payload, also Auflösung über `/users?id=`. Bestehender `tb-transport-twitch` HelixClient kann beides (App-Token via client_credentials, `get_users_by_id`); Shared Chat war noch nicht implementiert. Neue Methode `HelixClient::get_shared_chat_logins`. Ein App-Token-Aufruf plus eine Users-Auflösung je "Titel bauen", kein Poll, kein EventSub-Abo. Fehlt die Twitch-App-Config oder schlägt der Aufruf fehl, läuft die Erkennung still mit Steam-Party und Discord-Voice weiter (eine Warnung bei einem fehlgeschlagenen Aufruf).

## Befund Steam-Party (Nachtrag 3)

Umgesetzt im gemeinsamen Rust-Einstieg `steam_lookup::detect_co_streamers_all`, den Dashboard und `!title` unverändert aufrufen. Reihenfolge: Shared Chat, Steam-Präsenz mit gleicher nicht leerer Party-ID, Discord-Voice. Eigene und fremde Steam-Präsenz sind jeweils jünger als zehn Minuten. Auflösung ausschließlich über `core.steam_links`, `twitch_streamer_identities` und `twitch_live_state`; Party und Voice benötigen `is_live = 1`. Doppelte Twitch-IDs und die eigene Twitch-ID werden vor dem gemeinsamen Zweierlimit entfernt, auch wenn sich die zugehörigen Logins unterscheiden. Der Helix-Client erhält mit `get_shared_chat_users` die IDs bis zu diesem Schritt; der vorhandene Login-Wrapper bleibt kompatibel. Der Ausfall einer lokalen Quelle lässt die anderen Signale erhalten. Die Dashboard-Erkannt-Zeile bleibt ohne Quellenangabe. Tests, Rot-Nachweise, Schema- und Cache-Befund stehen in `REPORT-B.md`.
