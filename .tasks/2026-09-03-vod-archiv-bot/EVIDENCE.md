# EVIDENCE: VOD-Archiv im Twitch-Bot

Stand 2026-09-03. Alle Pfade relativ zu `/home/nathanael/Documents/Deadlock-Twitch-Bot`,
sofern nicht anders angegeben. Recherche zuerst über graphify, Fundstellen im Code
nachgelesen.

## Leitbefund (überschreibt die Prämisse des Auftrags)

Das VOD-Archiv ist nicht mehr abzuwandern, sondern bereits als Feature im Bot gebaut
und verdrahtet. Es existiert die Crate `tb-vod-archive`, ein pro-Streamer-Schalter, echte
Migrationen, resumable Upload, Download über yt-dlp und ein Dashboard-Endpunkt. Der
Standalone-Ordner `/home/nathanael/vod-archive` ist damit Legacy-Vorlage.

- Crate im Workspace: `rust/Cargo.toml:26` (`"crates/tb-vod-archive"`), `rust/Cargo.toml:98`.
- Bot-Abhängigkeit: `rust/bin/tb-bot/Cargo.toml:45` (`tb-vod-archive = { workspace = true }`).
- Worker im Bot gespawnt: `rust/bin/tb-bot/src/main.rs:1674` (`VodArchiveConfig::from_env()`),
  `rust/bin/tb-bot/src/main.rs:1678` (`VodArchiveWorker::new`), `rust/bin/tb-bot/src/main.rs:1683`
  (`supervisor.spawn("vod_archive_worker", …)`), Credentials `main.rs:1673`
  (`CredentialManager::new`), yt-dlp zentral `main.rs:1676` (`vod_config.yt_dlp = yt_dlp_path()`).
- Crate-Dateien: `rust/crates/tb-vod-archive/src/{lib,config,worker,twitch,store,metadata,error}.rs`.

## 1. YouTube-OAuth im Bot: Scopes und Anhängepunkt

Es gibt zwei getrennte YouTube-OAuth-Flows im Bot:

A) Social-Media-Flow (den das VOD-Archiv real nutzt), Store `social_media_platform_auth`.
   - Auth-URL: `rust/crates/tb-social-media/src/oauth.rs:800` (`fn youtube_auth_url`).
   - Aktuell angefragter Scope-String: `rust/crates/tb-social-media/src/oauth.rs:813`
     ergibt `"https://www.googleapis.com/auth/youtube.upload https://www.googleapis.com/auth/youtube.readonly"`.
   - Der Upload-Scope `youtube.upload` ist hier also bereits enthalten. Ein Anhängen
     ist in diesem Flow nicht nötig; der Test `rust/crates/tb-social-media/src/oauth.rs:1286`
     bestätigt gespeicherte `scopes = "youtube.upload"`.
   - Auswahl der Auth-URL je Plattform: `rust/crates/tb-social-media/src/oauth.rs:142`.

B) Uplink-Flow (Chat + Stream-Key), Store `platform_connections`, den der Auftrag als
   fixe Vorgabe nennt.
   - Scopes governt über `tb_raid::scope_profiles::UPLINK_SCOPES`, benutzt in
     `rust/crates/tb-dashboard-api/src/handlers/uplink.rs:280` und im Test `uplink.rs:2446`.
   - Dieser Flow trägt keinen `youtube.upload`-Scope; er ist auf Chat/Streamkey ausgelegt.

Fazit zu Punkt 1: Die Auftrags-Annahme, den Upload-Scope an den bestehenden YouTube-OAuth
(platform_connections) anzuhängen, trifft den falschen Flow. Der Upload-Scope existiert
bereits im Social-Media-Flow, und genau der ist verdrahtet. Siehe Risiko R1.

## 2. Credential-/Token-Speicher pro Kanal

Die VOD-Uploads holen ihren YouTube-Zugang nicht aus `platform_connections`, sondern aus
`social_media_platform_auth` über den `CredentialManager`.

- Zugang je Streamer: `rust/crates/tb-vod-archive/src/worker.rs:134` (`struct StreamerZugang`),
  `worker.rs:140` (`impl HochladerQuelle`), `worker.rs:143`
  (`credentials.get_credentials("youtube", Some(streamer_login))`).
- Bewusster Verzicht auf den globalen Fallback (kein fremder Kanal): `worker.rs:131` (Doku),
  `worker.rs:146` (Prüfung `creds.streamer_login`), `worker.rs:151` (Log "nur globaler Zugang").
- Lesefunktion: `rust/crates/tb-social-media/src/credentials.rs:64` (`fn get_credentials`),
  SELECT aus `social_media_platform_auth` mit `streamer_login`-Vorrang:
  `credentials.rs:70` und `credentials.rs:78` (`ORDER BY CASE WHEN streamer_login = $2 …`).
- Entschlüsselte Felder: `credentials.rs:27` (`access_token`), `credentials.rs:28`
  (`refresh_token`), `credentials.rs:26` (`streamer_login`).
- Token-Refresh im Social-Media-Flow: `rust/crates/tb-social-media/src/oauth.rs:672`
  (`.refresh_youtube()`), Umfeld `oauth.rs:602/625/713` (`refresh_token`, `save_refreshed`).

Hinweis: `platform_connections` (Uplink) hat ein eigenes 409-`needs_reauth`-Signal (Punkt 3).
Der Social-Media-Flow hat kein 409-Signal; ohne gültigen Token liefert `get_credentials`
schlicht `None`, und der Worker archiviert dann nur lokal (`worker.rs:314`).

## 3. platform-token Internal-Endpoint (Uplink-Flow)

- Handler: `rust/crates/tb-dashboard-api/src/handlers/platform_token.rs:434`
  (`internal_platform_token_handler`), Route
  `GET /twitch/api/v2/internal/platform-token?streamer=&platform=` (Doc-Kommentar `platform_token.rs:432`).
- Zugangsschutz loopback + Header: `platform_token.rs:436` (`intern_erlaubt`),
  401 bei Fremdaufruf `platform_token.rs:440`, 503 bei fehlender Config `platform_token.rs:447`.
- 409 needs_reauth: `platform_token.rs:826` (`needs_reauth_409`), Fehlerabbildung
  `TokenFehler::NeuVerbinden` ergibt `StatusCode::CONFLICT` (Test `platform_token.rs:846`).
- Config: `platform_token.rs:68` (`PlatformTokenConfig`), aus Env `platform_token.rs:84`
  (`platform_token_config_from_env`, braucht `TWITCH_CLIENT_ID/SECRET`, `DB_MASTER_KEY_V1`).

Dieser Endpunkt gehört zum Uplink-Flow (Twitch/Kick/YouTube-Chat), nicht zum VOD-Upload.

## 4. Blaupause pro-Kanal-Schalter: für VOD bereits vorhanden

Der Auftrag nennt `twitch_moderation_settings` als Vorbild. Für das VOD-Archiv existiert das
Muster bereits eigenständig; das Vorbild muss nicht neu nachgebaut werden.

- Settings-Struct + DB-Zugriff: `rust/crates/tb-social-media/src/vod_archive.rs:29`
  (`struct VodArchiveSettings { streamer_login, enabled, privacy }`), Lesen einzeln
  `vod_archive.rs:48` (`get_vod_archive_settings`), Schreiben `vod_archive.rs:81`
  (`set_vod_archive_settings`), aktive Kanäle `vod_archive.rs:121`
  (`aktive_vod_archive_streamer`, `WHERE enabled`).
- Settings-Tabelle: `rust/migrations/20260814150000_vod_archive_pro_streamer.sql`
  (`CREATE TABLE public.social_media_vod_archive`, PK `streamer_login`, FK auf
  `twitch_streamers`, `enabled`, `privacy` mit CHECK `('private','unlisted','public')`).
  Diese Migration migriert den alten globalen Schalter weg (`social_media_settings`
  `vod_archive_enabled/privacy` gelöscht).
- Dashboard-Route (serverseitige Auth): `rust/crates/tb-dashboard-api/src/lib.rs:311`
  (`/social-media/api/admin/settings/vod-archive`),
  `lib.rs:312` (`get(vod_archive_get_handler).put(vod_archive_put_handler)`).
- Handler + Auth-Import: `rust/crates/tb-dashboard-api/src/handlers/social_media.rs:77`
  (`use tb_social_media::vod_archive::{…}`), Auth-Ebene `social_media.rs:84`
  (`DashboardAuthLevel`), Streamer-Scope `social_media.rs:85` (`resolve_streamer_scope`).
- Kein cachegestütztes TTL-Muster wie bei `moderation_settings` nötig; der Worker liest
  je Lauf frisch (`worker.rs:263` `aktive_vod_archive_streamer`).

Lücke: Im Frontend fehlt der Schalter. Grep über `bot/admin_dashboard/src` nach
`vod/VOD/Vod` liefert null Treffer. Der Backend-Endpunkt ist da, die UI nicht.

## 5. Twitch-VOD-Zugriff (Download-Weg)

Die VOD-Liste kommt nicht über Helix `videos?user_id=`, sondern über yt-dlp direkt.

- Live-Prüfung: `rust/crates/tb-vod-archive/src/twitch.rs:132` (`ist_live`),
  Offline-Erkennung `twitch.rs:119` (`meldet_offline`).
- VOD-Liste: `rust/crates/tb-vod-archive/src/twitch.rs:184` (`liste_vods`,
  URL `https://www.twitch.tv/{kanal}/videos?filter=archives`, `twitch.rs:193`),
  Playlist-Parsing `twitch.rs:163` (`parse_vod_liste`).
- Download-Argumente: `twitch.rs:216` (`download_args`), Download `twitch.rs:248` (`lade_vod`),
  Zieldatei-Suche `twitch.rs:269`, Aufnahmedatum aus info.json `twitch.rs:297`.
- Schnitt an der 12-h-Grenze: `twitch.rs:337` (`schnitt_args`), `twitch.rs:370`
  (`schneide_bei_bedarf`), Längenmessung via ffprobe `twitch.rs:316` (`miss_laenge`).
- CommandRunner-Abstraktion (testbar): `twitch.rs:40` (`TokioCommandRunner`), `twitch.rs:43`.

Für Helix als Alternative: der Helix-/App-Token-Weg lebt in `tb-transport-twitch`
(`rust/crates/tb-transport-twitch/src/token.rs:234` `.access_token()`) und der Clip-Pfad
nutzt `rust/crates/tb-social-media/src/clip/helix.rs:1`. Für das VOD-Archiv wird er aber
nicht gebraucht; yt-dlp deckt Liste und Download ab.

## 6. Standalone-Vorlage /home/nathanael/vod-archive (Legacy)

- Struktur: `src/{main,config,db,twitch,youtube}.rs`, `src/bin/yt_auth.rs`, `config.env`,
  `state/vods.db` (SQLite), `legacy-python/{vod_archive.py,yt_auth.py}`.
- Der portierbare Kern (yt-dlp-Download, Offline-Erkennung, resumable Upload) ist bereits
  in die Crate übernommen. `config.env` ist die ENV-Herkunft, die im Bot als
  `VodArchiveConfig::from_env()` nachlebt, und genau das ist der Regelverstoß (Punkt 7/R2).
- Kein graphify-Graph für diesen Ordner vorhanden (Query lieferte `NO_GRAPH`); nur direkt
  gelesen. Der Ordner enthält heruntergeladene `.mp4`/`.info.json` unter `downloads/` und
  einen `.mp4.part`/`.mp4.ytdl` (unterbrochener Download), das bestätigt das yt-dlp-Resume-Thema.

## 7. Migrations- und Config-Weg

- Migrationsverzeichnis des Bots: `rust/migrations` (daneben Legacy `bot/migrations`).
- VOD-Tabellen: `rust/migrations/20260814100000_twitch_vod_archive.sql`
  (`twitch_vod_archive_vods`, `twitch_vod_archive_parts`, beide `public.`), Settings-Tabelle
  `rust/migrations/20260814150000_vod_archive_pro_streamer.sql`.
- Dedup-Ledger: `twitch_vod_archive_vods.twitch_id TEXT NOT NULL UNIQUE`
  (`20260814100000_twitch_vod_archive.sql`), Merken via `store.rs:84` (`merke_vod`,
  `INSERT … ON CONFLICT`), offene Arbeit `store.rs:107` (`offene_vods`,
  `status NOT IN ('uploaded','archived')`). Damit kein Doppel-Upload.
- Resumable-Fortschritt in DB: `store.rs:256` (`setze_teil_sitzung`), `store.rs:276`
  (`setze_teil_offset`), Aufräumen `store.rs:294` (`loesche_teil_sitzung`).
- Migrationsdurchsetzung: Der Bot migriert nicht selbst (`TB_DB_MIGRATE=0`, siehe
  Workspace-Memory). Neue Migration als Rolle `postgres` anwenden, danach
  SELECT/INSERT/UPDATE/DELETE an `twitchbot`/`twitchdash`, Version in `_sqlx_migrations`
  eintragen. Für dieses Vorhaben sind bereits alle nötigen Tabellen migriert; eine neue
  Migration wäre nur nötig, falls Punkt R2 (Config in DB) das erfordert.

## Config-ENV-Bestand (Regelverstoß, siehe R2)

`rust/crates/tb-vod-archive/src/config.rs:74` (`from_env`), `config.rs:75` (`std::env::var`),
gelesene Schlüssel `config.rs:99` bis `config.rs:141`:
`TB_VOD_ARCHIVE_DIR`, `TB_VOD_ARCHIVE_MAX_DOWNLOADS`, `TB_VOD_ARCHIVE_MAX_UPLOADS`,
`TB_VOD_ARCHIVE_MIN_FREE_GB`, `TB_VOD_ARCHIVE_KEEP_LOCAL_DAYS`, `TB_VOD_ARCHIVE_RATE_LIMIT`,
`YT_DLP_PATH`, `TB_VOD_ARCHIVE_FFMPEG`, `TB_VOD_ARCHIVE_FFPROBE`,
`TB_VOD_ARCHIVE_DOWNLOAD_TIMEOUT_SECS`, `TB_VOD_ARCHIVE_INTERVAL_HOURS`,
`TB_VOD_ARCHIVE_PLAYLIST_ID`, `TB_VOD_ARCHIVE_CATEGORY_ID`, `TB_VOD_ARCHIVE_TITLE_TEMPLATE`.
Das widerspricht `AGENTS.md`: keine ENV-Dateien und keine Environment-Variablen für
Config, Secrets aus Infisical, alles andere aus einer normalen Config-Datei.

## Risiken / offene Punkte

- R1 (entscheidend): OAuth-Store-Konflikt. Der Auftrag fixiert `platform_connections` plus
  Anhängen von `youtube.upload`. Real läuft der Upload über `social_media_platform_auth`,
  dessen YouTube-OAuth `youtube.upload` bereits trägt (`oauth.rs:813`). Würde man jetzt
  `platform_connections` verdrahten, entstünde ein dritter YouTube-Pfad und ein zweiter
  OAuth-Weg, genau das, was die Regeln verbieten. Urteil: bei
  `social_media_platform_auth` bleiben, `platform_connections` nicht anfassen. Die fixe
  Vorgabe wurde ohne Kenntnis dieses Stores getroffen und sollte als Amendment korrigiert
  werden.
- R2: Config über `from_env()`/`std::env::var` verstößt gegen die No-ENV-Regel. Muss in
  Config-Datei (bzw. für die Grenzwerte optional in die vorhandene Settings-Zeile) wandern.
- R3: Kein Frontend-Schalter im Dashboard (`bot/admin_dashboard/src` ohne VOD-Bezug), obwohl
  der Backend-Endpunkt steht. Ohne UI schaltet kein Streamer das Feature selbst frei.
- R4: Der Worker läuft nur, wenn der Field-Cipher verfügbar ist (`main.rs:1685` Else-Zweig
  "Worker aus"). Live-Status ist nicht verifiziert (0 Zeilen erwartbar, wenn kein Kanal
  eingeschaltet). Wirkung erst an der Live-Strecke belegbar.
- R5: Kein 409/needs_reauth-Signal im Social-Media-Flow; abgelaufener YouTube-Token führt
  still zu "nur lokal archivieren" statt einer Streamer-sichtbaren Aufforderung, neu zu
  verbinden.

## R4 aufgeklärt: Live-Status

Der VOD-Worker ist nicht totgeschaltet, sondern live scharf. Kein VOD-spezifisches Env-Gate.

- Die gesamte Social-Media-Pipeline (inkl. VOD) wird bedingungslos gespawnt: nackter Scope
  `rust/bin/tb-bot/src/main.rs:1591`, Kommentar dort "hier ebenso bedingungslos gespawnt …
  An/Aus wird datengetrieben über social_media_settings gesteuert".
- Einziger Startvorbehalt ist der gemeinsame Field-Cipher:
  `rust/bin/tb-bot/src/main.rs:1626` (`match tb_crypto::FieldCipher::from_env()`), der bei
  `Ok` u. a. `social_upload_worker`, `social_token_refresh_worker`, `insights` und
  `vod_archive_worker` (`main.rs:1683`) spawnt und bei `Err` "kein Field-Cipher, Worker aus"
  loggt (`main.rs:1687`). Das Gate ist geteilt, kein VOD-Kill-Switch.
- `FieldCipher::from_env` braucht `DB_MASTER_KEY_V1` (`rust/crates/tb-crypto/src/field.rs:19`).
  Das Secret kommt aus Infisical: Unit `deadlock-twitch-bot-rust` hat
  `ExecStartPre=/usr/local/libexec/wait-for-infisical`, Dienst ist `active`. Dieselbe
  Cipher-Abhängigkeit trägt die live laufenden Social-Media-Upload/Refresh/Insights-Worker
  und die verschlüsselten Uplink-Tokens (Kick/YouTube seit 2026-09-02), also ist der Cipher
  in Prod vorhanden und der VOD-Worker spawnt.
- Ob tatsächlich archiviert wird, hängt allein am pro-Kanal-Schalter: `run_once` liest
  `aktive_vod_archive_streamer` und loggt bei leerer Liste "VOD-Archiv: kein Kanal
  eingeschaltet" (`rust/crates/tb-vod-archive/src/worker.rs:271`), Initial-Delay 300 s
  (`worker.rs:43`). Die Settings-Migration hat `earlysalty` mit dem alten globalen Flag
  vorbelegt (`rust/migrations/20260814150000_vod_archive_pro_streamer.sql`).
- Nicht direkt verifizierbar (ohne adm-/systemd-journal-Rechte): der Dienst-Log
  (`/var/log/deadlock-twitch-bot` Permission denied, `journalctl -u deadlock-twitch-bot-rust`
  liefert "No entries" für den normalen Nutzer) und die Zahl real eingeschalteter Kanäle
  (DB-Read auf `social_media_vod_archive`, ohne Creds nicht ausgeführt). Beides gehört in die
  DoD von AP1. Dienste wurden nicht angefasst.

Urteil R4: Feature läuft live (spawnt), Gate ist der geteilte Field-Cipher, kein
totgeschalteter Code. Offen bleibt nur die empirische Bestätigung am Log und die
Kanal-Zahl.
