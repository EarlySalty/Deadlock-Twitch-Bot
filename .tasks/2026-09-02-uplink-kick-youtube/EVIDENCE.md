status: aktiv
datum: 2026-09-02

# Evidence: Bestand für Kick/YouTube-Connect (jede Zeile eine gelesene Fundstelle)

## Deadlock-Twitch-Bot (Repo-Root /home/nathanael/repos/Deadlock-Twitch-Bot)
- rust/crates/tb-dashboard-api/src/lib.rs:510-519 Uplink-Routen; Verbinden läuft über `/twitch/raid/auth?scope_profile=uplink`, kein eigener Callback für Twitch.
- rust/crates/tb-dashboard-api/src/lib.rs:515-517 `POST /twitch/api/v2/uplink/connect/:platform/disconnect` -> `uplink::disconnect_handler`.
- rust/crates/tb-dashboard-api/src/lib.rs:519 `streamkey_handler`-Route.
- rust/crates/tb-dashboard-api/src/handlers/uplink.rs:1068-1069 `rtmp_url_fuer` liefert nur für Twitch eine URL.
- rust/crates/tb-dashboard-api/src/handlers/uplink.rs:1073 `ziel_loesch_pfad`, :1097 Trait `RelayZiele` (ziel_setzen/ziel_loeschen).
- rust/crates/tb-dashboard-api/src/handlers/uplink.rs:1183 Trait `PlattformKonto::stream_key`, :1215-1223 `HelixKonto::stream_key`.
- rust/crates/tb-dashboard-api/src/handlers/uplink.rs:1258 `stream_key_hinterlegen` (holt Key, setzt Relay-Ziel).
- rust/crates/tb-dashboard-api/src/handlers/uplink.rs:1315-1329 `TrennenErgebnis`, :1342-1410 `trennen` (Relay-Ziel löschen, Tokens leeren, Revoke).
- rust/crates/tb-dashboard-api/src/handlers/uplink.rs:302-310 `verbindungen_lesen` -> `status_liste`.
- rust/crates/tb-dashboard-api/src/handlers/uplink.rs:553-554 `TWITCH_CLIENT_ID`/`TWITCH_CLIENT_SECRET` per env.
- rust/crates/tb-dashboard-api/src/handlers/platform_token.rs:434 `internal_platform_token_handler`; :331 `platform_token_antwort`; :366-372 `fremde_plattform_antwort` liest `platform_connections`, 404 bei leer, kein Refresh.
- rust/crates/tb-dashboard-api/src/handlers/platform_token.rs:169-177 `PlatformTokenAntwort` ohne refresh_token; :193 `refresh_faellig` Puffer 300 s; :239 `gueltiger_twitch_token`.
- rust/crates/tb-dashboard-api/src/handlers/platform_token.rs:557-990 Tests (ohne_token_401, fremder_peer_401, fremde_plattform_bekommt_kein_twitch_token, ohne_eintrag_ist_die_fremde_plattform_404).
- rust/crates/tb-dashboard-api/src/handlers/platform_store.rs:31-41 `PlatformConnection`; :58-73 `Zeile`/`SELECT_ZEILE`; :83-84 AAD; :110 `load`; :145 `status_liste`. Produktiv schreibt niemand in die Tabelle.
- rust/migrations/20260827090000_platform_connections.sql:15-34 Schema `platform_connections` (streamer_id, platform, platform_user_id, platform_login, access_token_enc, refresh_token_enc, enc_kid, scopes TEXT[], expires_at NOT NULL, needs_reauth, PK (streamer_id, platform)).
- rust/migrations/20260828090000_platform_connections_rueckbau.sql:31-36 Twitch-Zeilen raus, Tabelle bleibt für Kick/YouTube/TikTok.
- rust/crates/tb-platform-core/src/platform.rs:15-35 Enum `Platform { Twitch, YouTube, Kick }`, serde lowercase, `as_str`.
- rust/crates/tb-raid/src/state_store.rs:99 `StateStore::persist` in `oauth_state_tokens` (state_token, platform, streamer_login, redirect_uri, pkce_verifier, expires_at).
- rust/crates/tb-dashboard-api/src/handlers/auth_login.rs:406 `maybe_delegate_raid_oauth_callback` (State-Weiche im geteilten Callback), :541 `normalize_raid_success_redirect_url`.
- rust/crates/tb-social-media/src/oauth.rs:766-774 `youtube_client_id()`/`youtube_client_secret()` mit Vorrang GOOGLE_OAUTH_ID > GOOGLE_CLIENT_ID > YOUTUBE_CLIENT_ID.
- rust/crates/tb-monitoring/src/webhook_receiver.rs:81-83 Router `POST /twitch/eventsub/callback`; :101-127 HMAC-Signaturprüfung; :128 Timestamp-Alter.
- rust/bin/tb-bot/src/main.rs:975-1000 proaktiver Twitch-Refresh alle 300 s (`refresh_all_due`).
- rust/crates/tb-raid/src/token_refresher.rs:169 `refresh_and_store` (Advisory-Lock, Re-Read, needs_reauth bei invalid_grant).
- bot/dashboard_v2/src/api/uplink.ts:109-111 `uplinkConnectUrl` nur Twitch; :160-161 `verbindenAktiv` nur Twitch; :48 Status-Enum; :219-245 Karten-Rendering; :123 disconnect; :146 streamkey; :509-510 YouTube/Kick-RTMP-Platzhalter.
- Infisical (Namen, keine Werte): GOOGLE_OAUTH_ID, GOOGLE_CLIENT_SECRET, YOUTUBE_CLIENT_ID, YOUTUBE_CLIENT_SECRET vorhanden; KICK_* fehlt.

## rs-relay (Repo-Root /home/nathanael/repos/rs-relay)
- src/chat/adapter.rs:23-55 `ChatFehler` (NichtUnterstuetzt, NichtVerbunden, NeuAnmeldungNoetig, Verworfen, KeinAdapter, Abgelehnt, InternerZugang, Netz).
- src/chat/adapter.rs:86-113 Trait `ChatAdapter` (verbinden, trennen, senden, ende_grund); :119-139 Trait `AdapterFabrik::bauen(streamer_id, platform, eingang)`; :177-178 `stub(platform)` -> NichtUnterstuetzt.
- src/chat/twitch.rs:124-195 `TwitchFabrik` und `bauen`; :374 verbinden; :406 trennen; :424 senden; :468-523 EventSub-Abos anlegen; :646 Leseschleife; :577-590 Senden per Helix; :891 Notification-Verarbeitung.
- src/chat/token.rs:24 `PFAD = /twitch/api/v2/internal/platform-token`; :34-39 `Zugang`; :71-83 `TokenQuelle::new(base_url, internal_token)`; :129-134 Aufruf mit `&platform=` und Header `X-Internal-Token`.
- src/chat/nachricht.rs:23-34 serde-Plattform lowercase; ~130-175 `ChatNachricht`; ~200-215 `Ereignis { Chat, Activity, Points, Info }`.
- src/chat/hervorhebung.rs:77-94 `Art`/`Hervorhebung`; :353 Erstchatter nur über Twitch-Verlauf; :9-12 Raider-Regel.
- src/chat/supervisor.rs:978-1254 Tests mit `FakeFabrik`, :990 YouTube-Verbindungsstatus "im Dashboard verbinden".
- src/api/mod.rs:137 Route `/v1/me/destinations/{platform}`; :146-148 dock-token rotate, `/v1/chat/*`.
- src/api/chat.rs:261 `GET /v1/chat/ws`; :605 `POST /v1/chat/send`.
- src/store.rs:151-163 `Destination { platform, rtmp_url, stream_key, profile, push_adressen }`; :781-810 `destination_mit_key`; :907-934 `upsert_destination`.
- src/transcode/profile.rs:71-77 `Platform { Twitch, Kick, YouTube, TikTok }`; :229-242 `Empfehlung::seed` inkl. Kick 1080p60/8000 und YouTube 1440p60/24000.
- src/transcode/plan.rs:264 Test `jede_plattform_bekommt_ihren_eigenen_encode` (Dual-Target belegt).
- src/session/ziele.rs:32-61 `nur_erreichbare_ziele` (DNS-Prüfung, stilles Streichen).
- migrations/20260819000001_relay_schema.sql:14-22 `relay.destinations` mit `platform check in (twitch,kick,youtube,tiktok)`.
- static/dock/chat.html:402-433 Hervorhebungs-Styles, :114-116 Verbindungsstatus.
- CLAUDE.md:87 Testbefehl `cargo test --no-fail-fast`, DB-Tests über `scripts/tests/run_rs_relay_service_test.sh`.

## Externe API-Fakten (geprüft 2026-09-02)
- docs.kick.com/getting-started/scopes.md: Scopes `user:read`, `channel:read`, `channel:write`, `chat:write`, `streamkey:read` ("Read a user's stream URL and stream key"), `events:subscribe`, `moderation:*`.
- docs.kick.com/apis/channels: `GET /public/v1/channels` liefert `slug`, `stream.url` (rtmps://stream.kick.com/...), `stream.key`.
- docs.kick.com/getting-started/generating-tokens-oauth2-flow: Authorize `https://id.kick.com/oauth/authorize`, Token `POST https://id.kick.com/oauth/token`, PKCE S256 Pflicht, Token-Request mit client_id, client_secret, redirect_uri, code_verifier; Response access_token, refresh_token, expires_in, scope.
- docs.kick.com/events/subscribe-to-events: `POST /public/v1/events/subscriptions` (events[], method webhook), `DELETE ...?id=`; Webhook-URL wird in der Kick-App hinterlegt.
- docs.kick.com/events/webhook-security: Header Kick-Event-Message-Id, Kick-Event-Signature, Kick-Event-Message-Timestamp, Kick-Event-Type; Signatur RSA-SHA256 über `message_id.timestamp.body`, Public Key `GET https://api.kick.com/public/v1/public-key`.
- docs.kick.com/apis/chat: `POST /public/v1/chat` mit type user|bot, content, broadcaster_user_id.
- docs.kick.com/apis/users: `GET /public/v1/users` -> user_id, name, profile_picture.
- developers.google.com/youtube/v3/live/docs/liveStreams: `liveStreams.list?part=cdn&mine=true` -> cdn.ingestionInfo.streamName, ingestionAddress, rtmpsIngestionAddress, snippet.isDefaultStream.
- developers.google.com/youtube/v3/live/docs/liveChatMessages/list: pollingIntervalMillis, nextPageToken; insert braucht youtube.force-ssl; Quota 10000/Tag, list = 1, insert = 50.
- developers.google.com/identity/protocols/oauth2: Testing-Modus lässt Refresh-Tokens nach 7 Tagen verfallen; Produktion braucht Verification.
