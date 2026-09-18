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
| A | 26e1c46d | opus48 | gestartet | `/home/nathanael/.worktrees/tb-titel-costream` | |

## Befund Voice-Quelle

Baubar, kein Bump-up. Alles über reine SQL-Joins im bestehenden zentralen Postgres-Pool des Twitch-Dashboards, kein neuer Gateway, keine Bridge, kein Helix-Poll.

Aktuelle Voice-Belegung: `voice.deadlock_voice_watch` (steam_id PK, guild_id, channel_id, updated_at) in der zentralen DB (dl-central-db). Deadlock-Bots schreibt sie je Tick als Voll-Sync (`Deadlock-Bots rust/crates/dl-voice/src/status.rs:614` `persist_voice_watch`: DELETE-all bei leer, sonst Upsert plus Abräumen aller nicht mehr Anwesenden). Wer den Kanal verlässt, verschwindet, also spiegelt die Tabelle die aktuelle Belegung. `updated_at` dient als Frische-Schranke, falls der Sync steht.

Join-Kette (alle Tabellen im selben Pool; core.steam_links-Prod-Nutzung belegt in `rust/crates/tb-dashboard-api/src/handlers/caster_overlay.rs:840`):
- twitch_user_id <-> discord_user_id: `twitch_streamer_identities` (siehe `title.rs:273` `resolve_discord_user_id`).
- discord_id <-> steam_id: `core.steam_links` (Spalte `steam_id` TEXT; dieselbe Repräsentation wie `voice.deadlock_voice_watch.steam_id` und `activity.live_player_state.steam_id`, Beleg `Deadlock-Bots rust/crates/dl-voice/src/status.rs:426` `steam_ids_for` und `:474` `presence_rows`).
- Live auf Twitch: `twitch_live_state` (twitch_user_id PK, streamer_login, is_live; Beleg `rust/crates/tb-monitoring/src/irc_lurker.rs:95`, `rust/crates/tb-dashboard-api/src/handlers/uplink.rs:206`). Das ist der vorhandene Live-Stand, kein neuer Poll.

Ablauf der Erkennung: eigene discord_id -> eigene steam_id -> eigener channel_id/guild_id aus `deadlock_voice_watch` (frisch) -> andere steam_ids im selben channel_id/guild_id (frisch, ungleich eigener) -> deren discord_id -> deren twitch_user_id -> nur die mit `twitch_live_state.is_live = 1`, deren `streamer_login`, höchstens zwei.

## Befund Party-Hinweis (Nachtrag 1, Punkt 2)

Partystand ist da, wird gefüllt. Quelle `voice.deadlock_party_members` (steam_id, party_size 1..=6, seen_at), Schreiber `Deadlock-Bots rust/crates/dl-voice/src/status.rs:1504` (Insert) und Normierung `:375` `normalize_party_size`. Neue Funktion `steam_lookup::get_party_hint_for_discord_user` joint core.steam_links -> deadlock_party_members, nimmt MAX(party_size) mit seen_at frisch (10 min), mappt zu solo/Duo/Dreier/Vierer/Fünfer/Sechser. Keine Namen. Der bisherige feste `party_hint: None` in `steam_lookup.rs:64` (HTTP-Pfad) bleibt, der Party-Hinweis kommt jetzt zusätzlich aus der zentralen DB. Schalter-Text "Party" bleibt ehrlich, weil die Wirkung jetzt da ist.

## Befund Twitch Shared Chat (Nachtrag 2)

Baubar. Twitch-Doku "Get Shared Chat Session": `GET /helix/shared_chat/session?broadcaster_id=<id>`, Authorization laut Doku-Abschnitt "App-Token ODER User-Token" (die 401-Zeile nennt zwar nur User-Token, der Authorization-Abschnitt erlaubt aber ausdrücklich App-Token), kein Scope. Antwort: `data[].participants[].broadcaster_id`, keine Logins im Payload, also Auflösung über `/users?id=`. Bestehender `tb-transport-twitch` HelixClient kann beides (App-Token via client_credentials, `get_users_by_id`); Shared Chat war noch nicht implementiert. Neue Methode `HelixClient::get_shared_chat_logins`. Ein App-Token-Aufruf plus eine Users-Auflösung je "Titel bauen", kein Poll, kein EventSub-Abo. Fehlt die Twitch-App-Config oder schlägt der Aufruf fehl, läuft die Erkennung still mit dem Voice-Signal weiter (eine Warnung).
