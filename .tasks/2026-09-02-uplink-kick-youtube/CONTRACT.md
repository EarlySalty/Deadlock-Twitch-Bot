status: aktiv
datum: 2026-09-02
klasse: hoch (repo-übergreifend Deadlock-Twitch-Bot + rs-relay, OAuth/Tokens)

# Contract: Kick und YouTube an Uplink anschließen

## Ziel
Ein Streamer klickt im Uplink-Dashboard auf "Mit Kick verbinden" bzw. "Mit YouTube verbinden", durchläuft den OAuth-Consent der Plattform und hat danach ohne weiteres Zutun (1) den Stream-Key der Plattform als Uplink-Ziel gesetzt und (2) den Chat der Plattform im OBS-Dock, lesend und schreibend. "Trennen" nimmt beides wieder weg. Twitch bleibt unverändert. Vorbild ist der Twitch-Weg aus `.tasks/2026-08-26-uplink-multichat` (GRILLME B1, B1-A1, B1-A2, C1, C2, D1).

## Verortung (entschieden, GRILLME D1)
- OAuth-Start, Callback, Token-Speicher (`platform_connections`), Refresh, Stream-Key-Abruf, Relay-Ziel setzen/löschen, Dashboard-UI: Deadlock-Twitch-Bot.
- Chat-Adapter (lesen, senden, Aktivität), Kick-Webhook-Empfänger, Dock: rs-relay. rs-relay holt Access-Tokens ausschließlich über `GET /twitch/api/v2/internal/platform-token?streamer=&platform=` und hält keine Refresh-Tokens.

## Anforderungen

### Twitch-Bot
- REQ-01 `GET /twitch/uplink/connect/kick` (Cookie-Session des Streamers) leitet auf `https://id.kick.com/oauth/authorize` mit PKCE S256, State in `oauth_state_tokens` (platform `kick`, TTL 600 s), Scopes `user:read channel:read chat:write streamkey:read events:subscribe`.
- REQ-02 `GET /callback/kick` tauscht den Code (`POST https://id.kick.com/oauth/token`, confidential client mit `KICK_CLIENT_ID`/`KICK_CLIENT_SECRET` aus Infisical), holt `GET /public/v1/users` (user_id, name) und `GET /public/v1/channels` (slug, `stream.url`, `stream.key`), schreibt Access- und Refresh-Token verschlüsselt in `platform_connections` (platform `kick`, platform_user_id = user_id, platform_login = slug, scopes, expires_at), setzt das Relay-Ziel `PUT /v1/me/destinations/kick` mit `rtmp_url = stream.url` und `stream_key = stream.key`, und leitet auf `/twitch/uplink?verbunden=kick`.
- REQ-03 `GET /twitch/uplink/connect/youtube` leitet auf Google OAuth (`access_type=offline`, `prompt=consent`, Scope `https://www.googleapis.com/auth/youtube.force-ssl`) mit State in `oauth_state_tokens` (platform `youtube`). Client-Credentials wie der Social-Media-Uploader: `GOOGLE_OAUTH_ID`/`GOOGLE_CLIENT_SECRET` (Fallback `YOUTUBE_CLIENT_ID`/`YOUTUBE_CLIENT_SECRET`), keine neuen Secrets.
- REQ-04 Der YouTube-Callback tauscht den Code, holt `channels.list?mine=true&part=snippet` (Kanal-ID, Titel) und `liveStreams.list?mine=true&part=cdn,snippet`, wählt den Stream mit `snippet.isDefaultStream=true` (sonst den ersten), schreibt Tokens in `platform_connections` (platform `youtube`, platform_user_id = Kanal-ID, platform_login = Kanaltitel) und setzt das Relay-Ziel `youtube` mit `rtmp_url = cdn.ingestionInfo.rtmpsIngestionAddress` (Fallback `ingestionAddress`) und `stream_key = cdn.ingestionInfo.streamName`. Redirect `/twitch/uplink?verbunden=youtube`. Der Callback-Pfad darf der bereits bei Google registrierte Pfad des Social-Media-Uploaders sein, wenn der Handler per State-Weiche verzweigt (Muster `maybe_delegate_raid_oauth_callback`).
- REQ-05 `platform-token` liefert für `kick` und `youtube` einen gültigen Access-Token: läuft er in unter 300 s ab, wird er vorher über den Refresh-Token erneuert (Kick `grant_type=refresh_token` an `id.kick.com/oauth/token`, Google an `oauth2.googleapis.com/token`) und in `platform_connections` zurückgeschrieben; `invalid_grant` setzt `needs_reauth = TRUE` (Antwort 409). Zusätzlich erneuert ein Hintergrundlauf alle 300 s fällige Zeilen (Muster `refresh_all_due` für Twitch). Ein Schreibpfad je Plattform, Advisory-Lock je Zeile.
- REQ-06 `POST /twitch/api/v2/uplink/connect/{kick|youtube}/disconnect` löscht das Relay-Ziel der Plattform, widerruft den Token best-effort (Kick `POST id.kick.com/oauth/revoke`, Google `https://oauth2.googleapis.com/revoke`) und löscht die Zeile aus `platform_connections`. Antwort wie beim Twitch-Trennen.
- REQ-07 `GET /twitch/api/v2/uplink/me` meldet für kick und youtube `verbunden` (Zeile vorhanden, needs_reauth false), `neu_verbinden` (needs_reauth true oder Scopes unvollständig) oder `getrennt` (keine Zeile).
- REQ-08 Dashboard: `verbindenAktiv` und `uplinkConnectUrl` liefern für kick und youtube echte Werte, die Plattform-Karten zeigen "Mit Kick verbinden" / "Mit YouTube verbinden", "Verbunden", "Neu verbinden", "Trennen" und werten `?verbunden=kick|youtube` aus wie bei Twitch. Fehlt das Kick-Secret in der Konfiguration, liefert der Connect-Start 503 mit dem Text "Kick ist auf dieser Instanz noch nicht eingerichtet" und die Karte zeigt den Knopf ausgegraut mit demselben Text.
- REQ-09 Redirect-URIs und Basis-URLs (`KICK_REDIRECT_URI`, `YOUTUBE_UPLINK_REDIRECT_URI`) kommen aus der normalen Config-Datei mit Defaults auf der Dashboard-Domain, nicht aus ENV.

### rs-relay
- REQ-10 `AdapterFabrik` baut für `Platform::Kick` und `Platform::YouTube` echte Adapter; TikTok bleibt `NichtUnterstuetzt`. Fehlt der Token (platform-token 404/409), liefert `bauen` `ChatFehler::NeuAnmeldungNoetig` bzw. `KeinAdapter`, wie beim Twitch-Adapter.
- REQ-11 Kick-Adapter: `verbinden` legt über `POST https://api.kick.com/public/v1/events/subscriptions` (User-Token, method webhook) Abos für `chat.message.sent`, `channel.followed`, `channel.subscription.new`, `channel.subscription.gifts`, `channel.subscription.renewal` an und merkt sich die IDs; `trennen` löscht sie. `senden` ruft `POST /public/v1/chat` mit `type=user`, `broadcaster_user_id` = platform_user_id. Nachrichten werden als `ChatNachricht` mit `platform: Kick`, Badges aus `sender.identity.badges`, Farbe aus `identity.username_color`, `message_id`, `sent_at = created_at` in den Bus gegeben; Follows und Subs als `Ereignis::Activity`.
- REQ-12 rs-relay nimmt Kick-Webhooks auf `POST /v1/webhooks/kick` entgegen: Signatur `Kick-Event-Signature` (RSA-SHA256 über `message_id.timestamp.body`, Base64) gegen den Public Key von `GET https://api.kick.com/public/v1/public-key` (gecacht, bei Prüf-Fehlschlag einmal neu laden), `Kick-Event-Message-Id` dedupliziert (Fenster 10 Minuten), Zeitstempel älter als 10 Minuten wird verworfen. Zustellung an den Adapter des Streamers über `broadcaster.user_id`; unbekannter Broadcaster wird mit 200 quittiert und verworfen. Ungültige Signatur 401.
- REQ-13 YouTube-Adapter: `verbinden` sucht alle 30 s `liveBroadcasts.list?mine=true&broadcastStatus=active&part=snippet`, bis ein `liveChatId` da ist, dann `liveChatMessages.list?part=snippet,authorDetails` mit `pageToken` und Wartezeit `max(pollingIntervalMillis, 3000 ms)`. Nachrichten als `ChatNachricht` (platform YouTube, sender_display = authorDetails.displayName, Badges für isChatOwner/isChatModerator/isChatSponsor). `senden` ruft `liveChatMessages.insert`. Endet der Broadcast (Chat 403/404 oder Broadcast nicht mehr aktiv), wartet der Adapter wieder auf den nächsten.
- REQ-14 Beide Adapter holen den Token über die bestehende `TokenQuelle` (`&platform=kick|youtube`) und holen ihn bei 401 einmal frisch nach, bevor sie mit `NeuAnmeldungNoetig` enden.
- REQ-15 Das Dock zeigt Kick- und YouTube-Nachrichten mit Plattform-Kennzeichen wie Twitch; Raider-Hervorhebung greift plattformübergreifend, Erstchatter-Hervorhebung nur bei Twitch (Bestand).

## Invarianten
- INV-01 Der Twitch-Pfad (Raid-OAuth, Profil `uplink`, `twitch_raid_auth`, Twitch-Adapter, Twitch-Trennen) ändert sein Verhalten nicht; alle bestehenden Twitch-Tests bleiben grün.
- INV-02 `platform-token` gibt nie einen Refresh-Token heraus; Antwortformat bleibt `{access_token, expires_at, platform_user_id, platform_login, scopes}`.
- INV-03 Secrets kommen aus Infisical über den bestehenden Loader; keine ENV-Dateien, keine neuen Environment-Variablen für Config, kein Klartext in Logs (Tokens, Stream-Keys, Client-Secrets).
- INV-04 Tokens in `platform_connections` verschlüsselt mit `FieldCipher` und AAD-Bindung an (streamer_id, platform) wie im bestehenden `platform_store.rs`.
- INV-05 Kein Modellwechsel, keine LLM-Aufrufe. Kein neues `*_ENABLED`-Flag.
- INV-06 Keine Code-Kommentare. Echte Umlaute in allen nutzersichtbaren Texten, keine Em-Dashes.
- INV-07 rs-relay persistiert keine Plattform-Tokens.

## Nicht-Ziele
- TikTok (Chat und Connect).
- Stream-Info (Titel/Kategorie) für Kick und YouTube; die Stubs bleiben.
- Kanalpunkte für Kick und YouTube.
- Kick-App und Google-Consent-Verification anlegen: das ist Betreiberarbeit außerhalb des Codes.
- Twitch-Änderungen jeder Art.

## Erlaubter Änderungsbereich
- Deadlock-Twitch-Bot: `rust/crates/tb-dashboard-api/src/handlers/{uplink,platform_token,platform_store}.rs`, neue Module unter `rust/crates/tb-dashboard-api/src/handlers/` (z. B. `kick_connect.rs`, `youtube_connect.rs`), `rust/crates/tb-dashboard-api/src/lib.rs` (Routen), `rust/crates/tb-dashboard-api/src/auth/{session,oauth_login}.rs` (State-Weiche), neue Transport-Crates oder Module für Kick/Google-HTTP, `rust/crates/tb-platform-core/src/platform.rs`, `rust/bin/tb-bot/src/main.rs` (Refresh-Job-Wiring), `bot/dashboard_v2/src/api/uplink.ts`, `bot/dashboard_v2/src/pages/Uplink*.tsx` und deren Tests, Migrationen nur, wenn `platform_connections` eine Spalte braucht (dann Snapshot mitziehen).
- rs-relay: `src/chat/**` (neue Module `kick.rs`, `youtube.rs`, `kick_webhook.rs`), `src/api/**` (Webhook-Route), `src/chat/adapter.rs` (Fabrik), `Cargo.toml` (RSA/Base64-Crate), Tests.

## Verbotener Bereich
- `twitch_raid_auth`, `scope_profiles.rs`, `raid_oauth_impl.rs`, Twitch-Adapter `src/chat/twitch.rs`, Encoder/Pusher/Session-Code in rs-relay, Migrationen, die bestehende Tabellen ändern.

## Offene Produktfragen
Keine. Betreiber-Voraussetzungen (Kick-App mit Redirect- und Webhook-URL, Secrets `KICK_CLIENT_ID`/`KICK_CLIENT_SECRET` in Infisical, Google-Redirect-URI) werden am Ende als Liste geliefert; der Code läuft bis dahin mit der 503-Meldung aus REQ-08.

## Amendments
- A1 (2026-09-02, entschieden von Orchestrator): rs-relay darf zusätzlich `src/main.rs` (nur Wiring der Fabrik und des Kick-Drehkreuzes in den AppState) und `Cargo.lock` (Folge der Crate-Ergänzung) ändern.
- A2 (2026-09-02, entschieden von Orchestrator): Der Hintergrund-Refresh für platform_connections (REQ-05) läuft im Dashboard-Prozess (`tb-dashboard-api/src/lib.rs`), nicht in `tb-bot/src/main.rs`, weil Clients, Config und Route dort leben; `tb-bot/main.rs` bleibt unangetastet.
- A3 (2026-09-02, entschieden von Orchestrator): `bot/dashboard_v2/tests/uplinkDock.test.ts` darf den Test `verbindenButton_nur_fuer_twitch_aktiv` auf das neue Verhalten umschreiben; er schrieb den alten Nur-Twitch-Zustand fest, den REQ-08 ersetzt.
- A4 (2026-09-02, entschieden von Orchestrator): REQ-09 gilt als erfüllt, wenn `KICK_REDIRECT_URI` und `YOUTUBE_UPLINK_REDIRECT_URI` über exakt denselben Weg bezogen werden wie `TWITCH_RAID_REDIRECT_URI` (Default im Code, Override über den bestehenden Infisical-Loader); tb-dashboard-api hat keine eigene Config-Datei, ein neuer Config-Mechanismus ist nicht Teil dieses Auftrags.
