status: aktiv
datum: 2026-09-02
contract: CONTRACT.md (Ziel und REQ dort)

# Plan: Kick und YouTube an Uplink

Zwei Worktrees, parallel gebaut, gemeinsam reviewt:
- Twitch-Bot: `/home/nathanael/.worktrees/tb-uplink-kick-youtube`, Branch `feat/uplink-kick-youtube` (Basis origin/main 089ac8fb)
- rs-relay: `/home/nathanael/.worktrees/rsr-chat-kick-youtube`, Branch `feat/chat-kick-youtube` (Basis main 1440aa4)

Schnittstelle zwischen den Repos bleibt `platform-token` (INV-02). Kick-Webhook-URL nach außen: `https://deutsche-deadlock-community.de/uplink/v1/webhooks/kick` (Caddy strip_prefix `/uplink` -> rs-relay 8891, Ergänzung in `Caddy/hosts/v50671/Caddyfile:308`). Callbacks `/callback/kick` und `/callback/youtube` laufen wie `/callback/twitch` auf tb-dashboard 8769 (`Caddyfile:270-288`).

## Twitch-Bot

M1 Transport: Modul `tb-transport-kick` (oder `handlers/kick_api.rs`): authorize-URL mit PKCE, Code-Tausch, Refresh, Revoke, `users`, `channels`; Google: authorize-URL, Code-Tausch, Refresh, Revoke, `channels.list`, `liveStreams.list`. Alles hinter Traits mit Wiremock-Tests. Validierung: Unit-Tests je Endpunkt grün. Stop: Kick-Antwortformat unklar -> Doku `docs.kick.com/apis/*.md` lesen, nicht raten.

M2 Store: `platform_store.rs` bekommt `upsert`, `delete`, `set_needs_reauth`, `faellige(puffer)` mit FieldCipher/AAD wie `load`. Tests gegen Test-PG. Stop: AAD-Bindung weicht vom bestehenden `aad()` ab.

M3 Connect-Start und Callback für Kick und YouTube: Routen `GET /twitch/uplink/connect/{kick|youtube}` (Cookie-Session, State in `oauth_state_tokens`, PKCE für Kick), Callback (State konsumieren, Code tauschen, Identität und Stream-Key holen, Store schreiben, `RelayZiele::ziel_setzen`, Redirect `?verbunden=`). 503 bei fehlendem Kick-Secret (REQ-08). Tests: State-Missbrauch, fremder Login, Relay-Fehler, fehlendes Secret.

M4 `platform-token` für kick/youtube mit On-Demand-Refresh und `needs_reauth`; Hintergrund-Refresh alle 300 s im tb-bot-Wiring (`main.rs` neben `refresh_all_due`). Tests: abgelaufen wird erneuert, invalid_grant -> 409, kein refresh_token in der Antwort.

M5 Trennen und Status: `trennen` für kick/youtube (Relay-Ziel löschen, Revoke best-effort, Zeile löschen), `verbindungen_lesen` für beide Plattformen. Tests wie `trennen_*`.

M6 Frontend `uplink.ts`/`Uplink*.tsx`: Connect-URLs, `verbindenAktiv`, `?verbunden=`, Texte, 503-Zustand. Vitest grün, `npm run build` im dashboard_v2.

Validierung gesamt: Rust-Tests des Workspaces gegen bekannte rote Baseline (vorher messen), Frontend-Tests, `cargo clippy`. Selbstprüfung mit `gate_hook.py --review`.

## rs-relay

M1 Kick-Webhook: Modul `src/chat/kick_webhook.rs` (Signaturprüfung RSA-SHA256 mit gecachtem Public Key, Dedupe, Zeitfenster, Verteiler nach broadcaster user_id), Route `POST /v1/webhooks/kick` in `src/api/`. Tests mit eigenem Testschlüssel: gültig, falsche Signatur 401, Replay, alt, unbekannter Broadcaster 200.

M2 Kick-Adapter `src/chat/kick.rs`: `KickFabrik`, Token über `TokenQuelle`, Abos anlegen/löschen, Webhook-Ereignisse -> `ChatNachricht`/`Activity`, `senden`. Wiremock-Tests analog `twitch.rs` (Abos, 401 einmal nachholen, Senden).

M3 YouTube-Adapter `src/chat/youtube.rs`: `YouTubeFabrik`, Broadcast-Suche alle 30 s, Poll-Schleife mit pollingIntervalMillis, `senden`, Ende des Broadcasts. Wiremock-Tests: kein Broadcast -> wartet, Nachrichten kommen im Bus an, Quota-Schonung (kein Poll unter 3 s).

M4 Fabrik-Verteiler: eine `PlattformFabrik`, die nach `Platform` auf Twitch/Kick/YouTube verzweigt, TikTok Stub; Wiring in `main.rs`/Supervisor. Supervisor-Tests für Kick/YouTube-Status anpassen (Text "im Dashboard verbinden" bleibt für getrennt).

M5 Dock: Plattform-Kennzeichen für kick/youtube prüfen (`static/dock/chat.html`), Test in `api/chat.rs`.

Validierung gesamt: `cargo test --no-fail-fast` über `scripts/tests/run_rs_relay_service_test.sh` (Baseline vorher messen, Ziel 611+ grün), `cargo clippy`. Selbstprüfung mit `gate_hook.py --review`.

## Abschluss (Hauptsession)
1. Gemeinsames Review beider Branches (frischer Reviewer, Diff plus Contract).
2. Caddy: `/callback/kick`, `/callback/youtube` -> 8769; `/uplink/v1/webhooks/kick` -> 8891. Caddy neu laden.
3. Merge, Push, Build, Restart `tb-bot`/Dashboard und rs-relay, Live-Prüfung: Kick-Connect liefert 503 mit Einrichtungstext bis Secrets da sind; YouTube-Connect gegen Wegwerf-Konto.
4. Betreiberliste an den Nutzer: Kick-App (Redirect `https://deutsche-deadlock-community.de/callback/kick`, Webhook `https://deutsche-deadlock-community.de/uplink/v1/webhooks/kick`), Secrets `KICK_CLIENT_ID`/`KICK_CLIENT_SECRET` in Infisical, Google-Redirect-URI in der Cloud Console, Google-Verification-Status.

## Status
- [x] TB M1 Transport (handlers/plattform_oauth.rs, Kick+Google, Wiremock-Tests)
- [x] TB M2 Store (upsert/delete/set_needs_reauth/faellige/refresh_and_store, Advisory-Lock, DB-Tests)
- [x] TB M3 Connect-Start + Callback Kick/YouTube (handlers/plattform_connect.rs, State-Store, PKCE, Relay-Ziel)
- [x] TB M4 platform-token On-Demand-Refresh + Hintergrundjob (tb-dashboard main.rs, alle 300 s)
- [x] TB M5 Trennen + Status Kick/YouTube (uplink.rs disconnect-Weiche, verbindbar in /uplink/me)
- [x] TB M6 Frontend (uplink.ts Connect-URLs/verbindbar, Uplink.tsx Rückkehr, UplinkZiel.tsx, Tests+Build grün)
- [ ] RSR M1-M5 (anderer Worktree)
- [ ] Review
- [ ] Caddy, Merge, Deploy, Live-Prüfung
