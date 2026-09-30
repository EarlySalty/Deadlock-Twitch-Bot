# PLAN: VOD-Archiv konsolidieren (kein Neubau)

Angelegt 2026-09-03. Grundlage: CONTRACT.md + EVIDENCE.md. Der Feature-Kern steht und
läuft live (siehe EVIDENCE, R4). Dieser Plan schließt vier Lücken und löst danach die
Standalone-Vorlage ab. Reihenfolge: AP1 vor AP2 (erst Live-Wahrheit, dann UI), AP3 und AP4
sind unabhängig.

## AP1: VOD-Worker-Live-Lauf verifizieren, Cipher-Gate für VOD prüfen

Zweck: R4 empirisch schließen und entscheiden, ob das geteilte Field-Cipher-Gate für den
VOD-Worker generalisiert werden muss. Der lokale Download-Pfad braucht keinen Cipher (nur
der Upload nutzt Tokens); heute hängt der VOD-Worker trotzdem am geteilten Cipher-Gate und
läuft ohne `DB_MASTER_KEY_V1` gar nicht, obwohl er dann wenigstens lokal archivieren könnte.

Betroffene Pfade:
rust/bin/tb-bot/src/main.rs
rust/crates/tb-vod-archive/src/worker.rs

Vorgehen:
- Am laufenden Dienst belegen, dass `vod_archive_worker` spawnt (Log "Social-Media-Pipeline-
  Worker gestartet (8 Loops inkl. VOD-Archiv)" bzw. VOD-Laufzeilen). Braucht adm-/systemd-
  journal-Leserecht oder Freigabe.
- DB-Read auf `social_media_vod_archive` (welche Kanäle `enabled`), read-only.
- Entscheiden: bleibt der VOD-Worker am geteilten Cipher-Gate (Upload und lokal beide aus,
  wenn kein Cipher), oder wird der Download-Zweig cipher-frei gezogen, damit lokal auch ohne
  `DB_MASTER_KEY_V1` archiviert wird (Upload dann still bis Cipher da). Bei Änderung nur den
  Spawn-Zweig in main.rs anfassen, keine Worker-Logik.

Definition of Done:
- Log-Beleg, dass der Worker live spawnt, plus die aktuelle Liste eingeschalteter Kanäle
  ist dokumentiert (Zahl, keine Namen nötig).
- Wenn generalisiert wurde: der VOD-Worker läuft nachweislich auch ohne Cipher im
  Nur-lokal-Modus; Test deckt den cipher-freien Spawn ab.
- Wenn nicht: begründeter Vermerk, dass das geteilte Gate absichtlich bleibt.

## AP2: Dashboard-Frontend-Schalter für den Streamer

Zweck: REQ-4. Backend-Route und Settings-Store existieren
(`rust/crates/tb-dashboard-api/src/lib.rs:311`, `PUT …/settings/vod-archive`); im Frontend
`bot/admin_dashboard/src` fehlt jeder VOD-Bezug (Grep null Treffer). Ohne UI kann ein
Streamer nichts selbst schalten.

Betroffene Pfade:
bot/admin_dashboard/src
rust/crates/tb-dashboard-api/src/handlers/social_media.rs

Vorgehen:
- Im Social-Media-Dashboard einen VOD-Archiv-Schalter ergänzen: an/aus, Sichtbarkeit
  (private/unlisted/public), und den lokal-behalten-Hinweis für den Betreiber-Kanal.
  GET zum Vorbelegen, PUT zum Speichern, gegen die bestehende Route.
- Die serverseitige Admin-/Scope-Prüfung nutzen, die die vorhandenen Toggles schon tragen
  (`DashboardAuthLevel`, `resolve_streamer_scope`); kein clientseitiges verstecktes Feld
  (INV-4). Optik am bestehenden Social-Media-Dashboard, nicht neu erfinden.
- Nur falls der GET-Handler die für die UI nötigen Felder nicht liefert, den Handler
  minimal ergänzen; keine Route-Semantik ändern.

Definition of Done:
- Ein eingeloggter Streamer schaltet VOD-Archiv mit seiner Twitch-Identität selbst an/aus
  und setzt die Sichtbarkeit; die Änderung steht in `social_media_vod_archive`.
- Sichtprüfung der betroffenen Seite (Screenshot), Verhalten für an und aus geprüft.
- Kein Zugriff für fremde Kanäle (Scope serverseitig durchgesetzt), negativ getestet.

## AP3: Config von ENV auf Config-Datei/Infisical umstellen

Zweck: REQ-3 / INV-5. `config.rs` liest 14 `TB_VOD_ARCHIVE_*`-Variablen über
`std::env::var` (`rust/crates/tb-vod-archive/src/config.rs:74`–`config.rs:141`), Verstoß
gegen die No-ENV-Regel.

Betroffene Pfade:
rust/crates/tb-vod-archive/src/config.rs
rust/bin/tb-bot/src/main.rs

Vorgehen:
- Betriebsparameter (Verzeichnis, Deckel, Intervall, Timeouts, Rate-Limit, Playlist/Kategorie,
  Titelvorlage) aus einer normalen Config-Datei laden statt aus dem Prozess-Environment;
  die per-Streamer-Werte (Sichtbarkeit) bleiben in `social_media_vod_archive`. Secrets bleiben
  in Infisical, hier fällt keines an.
- Die bestehende `load(source)`-Testschnittstelle beibehalten; nur die Default-Quelle wechselt
  von `std::env::var` auf die Config-Datei. `from_env` entfernen oder auf die Datei umbiegen.
- Aufrufstelle `VodArchiveConfig::from_env()` in `main.rs:1674` entsprechend anpassen; yt-dlp
  bleibt zentral über `yt_dlp_path()`.

Definition of Done:
- Keine `TB_VOD_ARCHIVE_*`-Env-Lesung mehr in `config.rs`; Grep leer.
- Bestehende Config-Tests grün, ein Test deckt die neue Dateiquelle ab.
- Der Worker startet mit denselben Defaults wie zuvor (kein Verhaltensbruch).

## AP4: needs_reauth-Signal im Social-Media-Flow

Zweck: R5. Läuft der YouTube-Token eines Kanals ab, liefert
`CredentialManager::get_credentials` schlicht `None`, und der Worker archiviert still nur
lokal (`worker.rs:314`), ohne dass der Streamer erfährt, dass er neu verbinden muss.

Betroffene Pfade:
rust/crates/tb-social-media/src/credentials.rs
rust/crates/tb-vod-archive/src/worker.rs
rust/crates/tb-social-media/src/vod_archive.rs

Vorgehen:
- Den abgelaufenen/fehlenden Token vom Fall "gar keine Verbindung" unterscheiden, damit der
  Worker "Zugang abgelaufen" von "nie verbunden" trennen kann.
- Diese Meldung an das bestehende Fehlermelde- und Entprell-Muster hängen: höchstens eine
  Meldung pro Tag je Kanal, Deckel zwei pro Woche, Wiederholungszahl an der nächsten Meldung
  mitführen (Learned User Preference), nicht bei jedem Lauf melden.
- Kein neuer OAuth-Weg und kein Nachbau des Uplink-409-Pfads; nur das Signal aus dem
  vorhandenen Social-Media-Store ableiten (INV-1).

Definition of Done:
- Ein abgelaufener YouTube-Token erzeugt genau eine entprellte Streamer-sichtbare
  Aufforderung "neu verbinden" statt stiller Nur-lokal-Archivierung.
- Test: abgelaufener Token meldet einmal, wiederholte Läufe am selben Tag melden nicht erneut.
- "Nie verbunden" löst weiterhin keine Aufforderung aus (nur Info-Log wie bisher).

## Nachfolge-AP (nach AP1 bis AP4): Standalone /home/nathanael/vod-archive ablösen

Zweck: Doppelte Wahrheit entfernen, sobald das Feature im Bot vollständig konsolidiert und
live bestätigt ist (Learned Preference: verworfene Module raus, damit Dependabot nur
Genutztes sieht).

Betroffene Pfade:
/home/nathanael/vod-archive

Vorgehen:
- Prüfen, dass die Bot-Fassung alle Fähigkeiten der Vorlage abdeckt (Download, Resume,
  Offline-Erkennung, resumable Upload).
- Standalone-Repo stilllegen/archivieren, `downloads/`- und `state/`-Reste sichern oder
  löschen; keinen zweiten Login-/Auth-Weg (`src/bin/yt_auth.rs`) weiter pflegen.

Definition of Done:
- Die Vorlage wird nicht mehr betrieben und ist als abgelöst markiert; im Bot bleibt die
  einzige Wahrheit.

## Reihenfolge und Gates

- Klasse hoch: pro AP frischer Implementierer, danach adversarialer Review gegen CONTRACT
  (nicht Selbstreview), Fixes als neue Commits.
- Jeder AP einzeln verifizieren (`claude-config/bin/verify-change.sh`), keine roten
  Baselines weiterschieben. Bei Bugfixes zuerst der rote Regressionstest.
- Merge, Deploy, Service-Restart (`sudo systemctl restart deadlock-twitch-bot-rust`),
  Live-Beweis; Branch und Worktree danach löschen.
