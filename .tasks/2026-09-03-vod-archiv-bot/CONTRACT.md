status: aktiv

# CONTRACT: VOD-Archiv als aktives Feature im Twitch-Bot

Angelegt 2026-09-03. Nach dem Anlegen unveränderlich; Korrekturen nur als `## Amendments`.

## Ausgangslage

Die Recherche (EVIDENCE.md) zeigt: Der Kern des Features ist bereits gebaut und im Bot
verdrahtet (Crate `tb-vod-archive`, pro-Streamer-Tabelle `social_media_vod_archive`,
Migrationen `20260814100000` und `20260814150000`, resumable Upload über
`tb-social-media`, Download über yt-dlp, Dashboard-Endpunkt
`/social-media/api/admin/settings/vod-archive`). Der YouTube-Upload läuft über den
Social-Media-OAuth (`social_media_platform_auth`), dessen Scope-String bereits
`youtube.upload` enthält. Dieses Vorhaben ist deshalb kein Neubau, sondern Konsolidierung:
Regelkonformität herstellen, die letzte Lücke (UI) schließen und den Live-Betrieb belegen.

## Ziel

Das bestehende VOD-Archiv wird regelkonform, streamer-selbstbedienbar und live nachweisbar,
ohne einen zweiten OAuth-Weg oder ein dauerhaftes VOD-File zu erzeugen.

## Anforderungen (prüfbar)

- REQ-1: Der YouTube-Upload nutzt ausschließlich den bereits vorhandenen Social-Media-OAuth
  in `social_media_platform_auth` über `CredentialManager::get_credentials("youtube", …)`.
  Es wird kein zweiter OAuth-Weg und keine zweite Token-Ablage angelegt. Prüfbar: kein neuer
  Code-Pfad liest oder schreibt `platform_connections` für VOD-Zwecke.
- REQ-2: Der Upload-Scope `https://www.googleapis.com/auth/youtube.upload` ist im genutzten
  OAuth-Flow gesetzt. Da `oauth.rs:813` ihn bereits führt, ist REQ-2 durch Verifikation
  erfüllt, nicht durch Anhängen. Falls die Verifikation ergibt, dass produktiv ein anderer
  Scope-Stand gespeichert ist, wird der Scope im Social-Media-Flow ergänzt, nicht im
  Uplink-Flow.
- REQ-3: Die VOD-Archiv-Config wird von `std::env::var`/`from_env()` gelöst und aus einer
  normalen Config-Datei bzw. den vorhandenen DB-Settings gespeist. Nach Abschluss liest
  `tb-vod-archive/src/config.rs` keine `TB_VOD_ARCHIVE_*`-Environment-Variablen mehr für
  Betriebsparameter. Secrets bleiben aus Infisical.
- REQ-4: Der pro-Kanal-Schalter ist im Dashboard-Frontend bedienbar: Ein Streamer sieht mit
  seiner normalen Twitch-Identität den VOD-Archiv-Schalter (an/aus plus Sichtbarkeit
  private/unlisted/public), schaltet ihn selbst und die Änderung landet über
  `PUT /social-media/api/admin/settings/vod-archive` in `social_media_vod_archive`. Die
  Admin-/Scope-Prüfung bleibt serverseitig (`DashboardAuthLevel`, `resolve_streamer_scope`).
- REQ-5: Speichermodell bleibt: VOD temporär laden, hochladen, danach löschen. Für die
  eigenen Kanäle des Betreibers ist optionales lokales Behalten per Flag
  (`keep_local_days`) möglich. Kein dauerhaftes Ablegen fremder VODs.
- REQ-6: Dedup: Kein VOD wird zweimal hochgeladen. Der Ledger `twitch_vod_archive_vods`
  (UNIQUE `twitch_id`) plus Status `uploaded/archived` bleibt die Quelle der Wahrheit;
  bestehendes Verhalten wird nicht aufgeweicht.
- REQ-7: Multi-Streamer: Der Worker verarbeitet alle in `social_media_vod_archive`
  eingeschalteten Kanäle und lädt je Kanal nur auf dessen eigenen YouTube-Zugang hoch
  (kein globaler Fallback auf den Betreiber-Kanal).
- REQ-8: Live-Nachweis: Nach dem Merge ist belegt, dass der Worker läuft (Field-Cipher
  vorhanden, `vod_archive_worker` aktiv) und dass ein eingeschalteter Testkanal einen
  Lauf mit erwartetem Ergebnis durchläuft (Discovery, Download, Upload oder dokumentierter
  Grund fürs Nur-lokal). Nachweis an der laufenden Strecke, nicht nur per Test.

## Invarianten

- INV-1: Kein zweiter OAuth-Weg und keine zweite Token-Ablage. Genau ein YouTube-OAuth für
  Upload: `social_media_platform_auth`.
- INV-2: Kein dauerhaftes VOD-File außer beim ausdrücklich per Flag behaltenen
  Betreiber-Kanal. Temporärdatei wird nach Upload gelöscht.
- INV-3: Dedup per Upload-Ledger; kein Doppel-Upload, auch nicht nach Neustart mitten im
  resumable Upload.
- INV-4: Identität und Zugriff laufen über Twitch-User-ID bzw. serverseitig aufgelösten
  Streamer-Scope, nie über ein clientseitig gesetztes verstecktes Feld.
- INV-5: Keine Betriebs-Config aus Environment-Variablen; Secrets nur aus Infisical.
- INV-6: Keine Code-Kommentare neu schreiben; bestehende in angefassten Dateien entfernen.
- INV-7: LLM-Modelle werden nicht berührt; das Feature braucht keine.

## Nicht-Ziele

- Kein Echtzeit-Passthrough-Streaming und keine Uplink-Relay-Änderung.
- Kein TikTok- oder Kick-Upload; nur YouTube.
- Kein Umbau des Uplink-YouTube-Flows (`platform_connections`) und kein Anfassen des
  `platform-token`-Endpunkts.
- Kein neuer Helix-Pfad für die VOD-Liste; yt-dlp bleibt die Quelle.
- Kein Refactoring der Social-Media-Uploader über das für REQ nötige Maß hinaus.

## Erlaubter Bereich (je Zeile genau ein Pfad)

rust/crates/tb-vod-archive/src/config.rs
rust/crates/tb-vod-archive/src/worker.rs
rust/crates/tb-vod-archive/src/lib.rs
rust/crates/tb-social-media/src/vod_archive.rs
rust/crates/tb-social-media/src/oauth.rs
rust/crates/tb-dashboard-api/src/handlers/social_media.rs
rust/crates/tb-dashboard-api/src/lib.rs
rust/bin/tb-bot/src/main.rs
rust/migrations
bot/admin_dashboard/src

## Amendments

- 2026-09-03: OAuth-Store-Wahl (R1) `platform_connections + youtube.upload anhaengen` -> `social_media_platform_auth via tb-social-media/src/credentials.rs:64`, weil der Uplink-Store nur Chat/Streamkey traegt und ein zweiter OAuth-Weg INV-1 verletzen wuerde; der Upload-Scope ist in oauth.rs:813 bereits gesetzt, nichts anzuhaengen; die urspruengliche Vorgabe ist verworfen, entschieden von Orchestrator.
- 2026-09-03: Live-Status (R4) `unklar/moeglicherweise totgeschaltet` -> `live scharf`, weil der VOD-Worker in main.rs:1591 bedingungslos gespawnt wird und nur am gemeinsamen Field-Cipher (main.rs:1626, DB_MASTER_KEY_V1 aus Infisical via ExecStartPre wait-for-infisical) haengt, den die live laufenden Social-Media- und Uplink-Token-Worker ohnehin brauchen; kein VOD-spezifisches Env-Gate; das tatsaechliche Archivieren haengt allein an social_media_vod_archive.enabled je Kanal, entschieden von Orchestrator.
