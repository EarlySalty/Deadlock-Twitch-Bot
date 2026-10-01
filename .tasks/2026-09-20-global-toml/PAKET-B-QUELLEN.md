# Bündel B: Bot-, Chat-, Raid- und Monitoring-Verbraucher

## B1: Startschalter, Raid-Zeiten und MCP

Alle Werte stehen in `[bot]`. Der Start übergibt sie explizit an bestehende Konstruktoren/Ports; Bibliotheken lesen keine zweite Betriebsquelle. Keine zusätzlichen Hintergrundaufgaben, keine neuen KI-Aufrufe/Modelle.

| Alte Quelle | Neues Feld | Alter Default / Semantik |
|---|---|---|
| TB_CHAT_ENABLED | chat_enabled | false, bestehende Token-/Helix-Voraussetzungen bleiben |
| TB_CHAT_PERSIST_ALL_GAMES | chat_persist_all_games | true, vorhandener expliziter Tracker-Konstruktor |
| LFG_PITCH_ENABLED | lfg_pitch_enabled | true, bestehender Responder-Bool |
| TB_GOLIVE_TIPS_ENABLED | golive_tips_enabled | false, im ChatHooks-Snapshot |
| TB_IRC_LURKER_ENABLED | irc_lurker_enabled | false, Startfreigabe |
| OUTREACH_SHADOW_ENABLED | outreach_shadow_enabled | false; Retention bleibt unabhängig wie vorher |
| SMALLTALK_LOOP_ENABLED | smalltalk_loop_enabled | false |
| SMALLTALK_LOOP_LIVE_SEND | smalltalk_loop_live_send | false, eigenständiger Schalter |
| TB_AUTO_RAID_OFFLINE_GRACE_SECS | auto_raid_offline_grace_seconds | 5, u64-Dauer |
| TB_AUTO_UNRAID_WINDOW_SECS | auto_unraid_window_seconds | 90.0, endlich und >=0 |
| TB_FLIP_REPEAT_WINDOW_SECS | flip_repeat_window_seconds | 600.0, endlich und >=0 |
| TB_FLIP_PAUSE_SECS | flip_pause_seconds | 43200.0, endlich und >=0 |
| TB_MCP_HOST/PORT | mcp_host / mcp_port | Loopback 127.0.0.1:8892, geprüfter positiver Port |

MCP, EventSub (127.0.0.1), Internal-API, Dashboard und STT werden vor dem Start auf gleiche konfigurierte Listeneradressen geprüft. Das verhindert Teilstarts durch offensichtliche Portkollisionen. Externe bereits belegte Ports erfordern weiterhin Laufzeitprüfung.

`ChatterTracker::new` bleibt ein reiner Bibliothekskonstruktor mit bisherigem Defaulttrue; der einzige produktive Aufrufer nutzt jetzt `with_persist_all_games` mit dem TOML-Wert. Unbenutzter LFG-ENV-Getter entfernt. Alte Stringparser für Outreach/Smalltalk existieren ausschließlich als bisherige Testreferenzen.

Prüfung: Config-Global-Suite26 grün; `cargo check -p tb-bot --tests` grün nach Anpassung der bestehenden Default-Referenzprüfung. Bot-/Dashboard-Normalcheck zuvor grün. Bündel wird zusammengehörend erneut geprüft und gegatet; dieser Commit ist kein fertiger Rollout.

## B2: Discord-Ziele, Rollen und OAuth-Guards

Unter `[discord]` werden die bisherigen Verbraucher getrennt abgebildet:

- `streamer_link`: enabled(true), notify_channel_id(1374364800817303632), streamer_role_id(1313624729466441769), guild_id(1289721245281292288), state_path(bisheriger absoluter Referenzpfad). Quellen: STREAMER_LINK_ENABLED/NOTIFY_CHANNEL_ID/STATE_PATH, STREAMER_ROLE_ID, MAIN_GUILD_ID. Relative neue State-Pfade werden gegen die TOML-Quelle aufgelöst und vor Dienststart geprüft.
- `oauth_followup`: guild_id(1289721245281292288) und streamer_role_id(1313624729466441769). Quellen: STREAMER_GUILD_ID → MAIN_GUILD_ID → Community-Default; STREAMER_ROLE_ID → Default.
- `token_lifecycle`: guild_id(None!) und streamer_role_id(1313624729466441769). Quellen: STREAMER_GUILD_ID → MAIN_GUILD_ID ohne Guild-Default. Das frühere deaktivierte Rollenverhalten bleibt bestehen.
- `raid_oauth`: allowed_guild_ids / allowed_channel_ids / allowed_role_ids als Option<Vec<i64>> statt CSV. Weggelassen bleibt Guard aus, explizite leere Liste bleibt deny-all. Ungültige IDs stoppen die Konfiguration. success_redirect_url behält https://deutsche-deadlock-community.de/twitch/dashboard als Default. Quellen: TWITCH_INTERNAL_API_ALLOWED_*_IDS, TWITCH_RAID_SUCCESS_REDIRECT_URL.
- shadow_review_channel_id(None): bisher ENGAGEMENT_SHADOW_REVIEW_CHANNEL_ID; kein Kanal bedeutet weiterhin kein Scheduler.

Anschluss über explizite Konstruktorparameter an alle drei Token-Lifecycle-Builder, beide produktiven BrokerDiscordDirectory-Wege und Raid-OAuth; keine pro-Poll-Nachkorrektur. Alte Allowlist-Stringparser bleiben ausschließlich Testreferenz. Configsuite27 und kompletter Bot-Test-Codecheck grün. Kein produktiver Restart/Deploy.

## B3: Pfade, Redirects und bestehende Legacy-Ziele

Quellbasis `ca398cc9` (zusätzlich gemeinsames Schema aus A `e937f659`, hier `89d6e063`). Keine produktiven Overrides ausgelesen oder geraten.

- `TWITCH_RAID_REDIRECT_URI`: Main/Telemetry/Moderator bisher HTTPS-Callback als Default, Clip-Port bisher leer. Deshalb getrennte Felder `bot.raid_redirect_uri` und `bot.clip_raid_redirect_uri`; nur vorhandene Token-Refresher konsumieren sie.
- `TB_INTERNAL_API_LEGACY_FALLBACK_URL`: API-Proxy bisher aus ohne Wert; OAuth-Greeter bisher `http://127.0.0.1:8779`. Getrennte `legacy_proxy_base_url`/`legacy_greeter_base_url` erhalten das.
- `TB_CHAT_REVIEW_LOG_DIR` und `TWITCH_SERVICE_WARNING_LOG_DIR`: je `logs`, getrennte Felder; nun relativ zur TOML-Quelle aufgelöst. Warnlog-Test schreibt explizit ins Testverzeichnis statt globale ENV zu setzen.
- `KNOWLEDGE_DIR`: Bot- und Command-Wissensbasis konsumieren ausschließlich gemeinsame `knowledge.directory` aus A (bisher `rust/knowledge`). Lesefehler behalten den bestehenden leeren Wissensbestand; Bot protokolliert den Fehler. Kein zweites Bot-Knowledge-Feld.
- `YT_DLP_PATH`: `bot.yt_dlp_binary`; ohne Wert bestehende ausführbare CWD/Home/Binary-Suche unverändert. HOME bleibt reine OS-Quelle, keine Konfigurationsquelle. Outreach hatte zusätzlich `YTDLP_BIN` vor diesem Fallback: eigenes `outreach_yt_dlp_binary` erhält dessen Vorrang. Explizite Dateipfade werden quellrelativ vor Dienststart geprüft.
- `VOD_EXPORT_REMOTE_BASE`: `bot.vod_export_remote_base`, bisheriger `DEFAULT_REMOTE_BASE` bleibt `gdrive:Deadlock/Twitch-VODs`. Keine Remote-Erstellung oder Datenverschiebung.
- Globale Pfadauflösung wird vor Snapshot-Installation geprüft. Credentials dieses Pakets bleiben für den späteren Infisical-Quellenabschluss sichtbar klassifiziert; kein ENV/Secretinhalt wurde gelesen.

## B4: Wiederholungen und Monitoring

- `TWITCH_ANALYTICS_TX_RETRY_ATTEMPTS/BASE_DELAY_SECONDS/MAX_DELAY_SECONDS`: identische Altdefaults beider Rust-Policies (3, 0.10, 0.75), eine gemeinsame `database.retry`. Processing-Inbox und LiveState bekommen sie explizit am Botstart; die schreibende interne Requeue-Route übernimmt denselben Snapshot. Reine Debug-Lesewege brauchen keine Retry-Policy. Bibliotheks-/Testkonstruktoren behalten explizite Defaults, ohne ENV zu lesen. Der bisher ungenutzte `tb-db`-Getter ist durch einen typisierten Konverter ersetzt.
- `TWITCH_EVENTSUB_CAPACITY_SAMPLE_SECONDS`: 300, Grenzen 30–3600; `TWITCH_EVENTSUB_CAPACITY_RETENTION_DAYS`: 45, Grenzen 7–365. SubscriptionManager bekommt beide beim produktiven Aufbau.
- `TWITCH_OBSERVABILITY_RETENTION_DAYS`: eigener Wert, 45, Grenzen 7–365; einmal an den Retention-Scheduler übergeben. Keine implizite Gleichsetzung mit Capacity-Retention.
- `TB_SCOUT_ENABLED`: false; Scout bekommt den Schalter explizit vom Botstart. Keine Aktivierung durch die Migration.
- Typisierte ungültige Eingaben werden vor Start abgelehnt, statt alte ungültige ENV-Strings pro Aufruf zu parsen und auf Defaults/Clamps zu fallen. Gültige Betriebswerte und Grenzen bleiben gleich.
- Pool-Editor ändert gezielt nur seine drei erlaubten Felder, lässt neu hinzugekommene Retrywerte unverändert; eigener Erhaltungstest.

## B5: Dienstrollen und Discord-Schreibpfade

- Botrolle aus `TWITCH_RUNTIME_ROLE`/`TWITCH_SPLIT_RUNTIME_ROLE`, Enforcement aus gleichnamigen `_ENFORCE`-Alternativen: `bot.runtime_role` (leer), `bot.runtime_enforce` (true). Bot und Dashboard besitzen getrennte dienstbezogene Werte. Härtung prüft Botrolle/Port nun vor --check-config-Erfolg, DB und Hintergrundjobs. Bisheriger Legacy-Portoverride ist `bot.legacy_internal_api_port`; reservierter Master-Port bleibt unzulässig. Kein UI-Schalter hierfür.
- `TWITCH_ALERT_CHANNEL_ID`: `discord.chat.moderation_alert_channel_id`, Default1374364800817303632. Der produktive ModAlerter bekommt den Wert explizit; Test-/Bibliothekskonstruktoren bleiben reine Defaults.
- `PROMO_DISCORD_INVITE`: optional `discord.chat.promo_invite`. Bestehende unterschiedliche Fallbacks bleiben erhalten: Invite-Question/!invite ohne gesetzten Wert kein globaler Fallback; Promo-Resolver und interner Chat-Command behalten ihren bisherigen festen Invite. Streamer-spezifische DB-Einträge haben weiter Vorrang.
- `TWITCH_DASHBOARD_OWNER_DISCORD_ID`: eigene `discord.internal.owner_id`, Default662995601738170389. Nicht mit A-Dashboard-Adminowner (`Option`, andere Aliasquellen) zusammengeführt. Fehlende Config erlaubt keine identifizierte Owner-Aktion; fehlender Ownerheader bleibt beim bisherigen Token-/Loopback-Gate.
- Dieselben `TWITCH_INTERNAL_API_ALLOWED_*`-Altquellen für RaidOAuth, LinkClick und Streamer-Discord-Aktionen konsumieren eine Policy (`discord.raid_oauth.allowed_*`). None bleibt Guard aus, leere Liste bleibt deny-all. Auth-/Idempotenzablauf erhalten. Tests übergeben Scope/Owner/Retry/Notify explizit statt prozessweite ENV zu mutieren.
