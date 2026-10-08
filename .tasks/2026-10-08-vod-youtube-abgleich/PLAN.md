# VOD-Archiv direkt mit YouTube abgleichen

## Context

Der ausgelieferte Statusfix zeigt fünf historische VODs ehrlich als unklar, klärt aber nicht, ob die Videos tatsächlich bei YouTube liegen. Gewünscht ist eine automatische, belastbare Abfrage statt eines Hinweises, den Nutzer selbst nachsehen zu lassen. Abgleich und Statuspflege gehören in den bestehenden Rust-Archivdienst, nicht in einen neuen Dienst.

## Bestehende Bausteine

Repo: `/home/nathanael/repos/Deadlock-Twitch-Bot`. Der gemeinsame Checkout ist fremdes WIP und bleibt unverändert; Umsetzung auf frisch geprüftem origin/main im eigenen Worktree.

- `rust/crates/tb-social-media/src/uploaders/youtube.rs`: `YouTubeUploader::video_status`, `get_videos`, gemeinsamer HTTP-/Refresh-/Fehlerpfad. Bisher nur uploadStatus und rejectionReason für eine ID.
- `rust/crates/tb-social-media/src/upload_worker.rs`: `youtube_uploader` baut den vorhandenen Client aus dem bestehenden Zugang.
- `rust/crates/tb-social-media/src/oauth.rs`: bestehender Google-Login verlangt bereits `youtube.upload` und `youtube.readonly`. Alte Verbindungen trotzdem auf tatsächlich erteilte Rechte prüfen.
- `rust/crates/tb-vod-archive/src/worker.rs`: `pruefe_fruehe_uploads` und `TeilHochlader`; bestehende Prüfung behandelt nur aktuelle bekannte IDs. `markiere_verworfen` kann Wiederholungsuploads auslösen und darf nicht unverändert für den neuen historischen Abgleich verwendet werden.
- `rust/crates/tb-vod-archive/src/store.rs`: `frisch_hochgeladene_teile`, Teile-/VOD-Persistenz und Abschlussguard.
- `rust/crates/tb-vod-archive/src/metadata.rs`: `baue_beschreibung` enthält bereits den ursprünglichen Twitch-VOD-Link. Dieser ist eine geeignete eindeutige Zuordnung, anders als gleiche Titel oder dasselbe Datum. Bei alten Videos ist sein Vorhandensein noch nicht belegt.

## Umsetzung

### 1. Vorhandenen YouTube-Client erweitern

Bekannte IDs gesammelt über videos.list lesen. Verarbeitung, Sichtbarkeit, Kanalzugehörigkeit und tatsächliches Ergebnis auswerten. Upload angenommen ist nicht dasselbe wie fertig verarbeitet; privat und nicht gelistet sind keine Uploadfehler.

Für fehlende IDs die Upload-Playlist des tatsächlich verbundenen YouTube-Kanals über channels.list und paginierte playlistItems.list lesen, danach Kandidaten gesammelt über videos.list auflösen. Keine öffentliche Titelsuche und kein fremdes Konto als Ersatz. Fehlende Leserechte führen zum bestehenden Verbindungsweg, nicht zu einer falschen Fehlermeldung über das Video.

### 2. Alte VODs eindeutig zuordnen

Die fünf unklaren Fälle zuerst mit dem eigenen Kanal abgleichen. Automatisch zuordnen nur bei belegter ursprünglicher Twitch-VOD-ID und eindeutiger Teilezuordnung. Gleicher Titel, Tag oder ähnliche Dauer allein reichen nicht. Mehrdeutige Treffer bleiben unklar und lösen keinen Upload aus.

Ein Gesamtabschluss setzt nachgewiesene vollständige Teileabdeckung voraus. Fehlt die alte Teileliste, darf ein einzelner Treffer nicht still das gesamte VOD bestätigen. Dauer und vorhandene Teilkennzeichnungen zur Vollständigkeitsprüfung heranziehen; ohne ausreichenden Nachweis nur den gefundenen Teil bestätigen. Keine erfundenen lokalen Dateipfade oder Uploadzeiten erzeugen.

Für neue Uploads die bestehende Quellenkennzeichnung gegen Abschneiden absichern und eine eindeutige Teilnummer samt Gesamtzahl in den bestehenden Metadaten sichern. Bestehende YouTube-Videos werden nicht umbeschriftet.

### 3. Prüfergebnis dauerhaft speichern und regelmäßig erneuern

Neue Postgres-Migration im bestehenden Migrationsweg für zielgebundene Prüfnachweise: YouTube-Kanal-ID, Video-ID, Zuordnung zum VOD/Teil, beobachteter Zustand, letzter Versuch und letzte erfolgreiche Prüfung. Historisches Uploaddatum bleibt getrennt von der heutigen Bestätigung. Fehler überschreiben keinen letzten erfolgreichen Nachweis.

Den vorhandenen Worker erweitern: neue beziehungsweise noch verarbeitete Uploads bevorzugen, Altfälle abgleichen und abgeschlossene Videos periodisch nachprüfen. Prüfintervalle und Anfragegrenzen in bestehender Config, keine ENV und keine neue Unit. Kanalinventar pro Lauf gemeinsam nutzen, Cursor dauerhaft speichern und Anfragen begrenzen. Abgebrochene oder unvollständige Scans dürfen nicht als vollständige erfolglose Suche gelten. Quotenfehler mit Backoff behandeln.

Timeout, fehlende Rechte und nicht zurückgegebene IDs sind nicht automatisch Löschung. Klar unterscheiden: Verarbeitung läuft, verarbeitet, abgelehnt, nicht abrufbar und Prüfung fehlgeschlagen. Verifizierter YouTube-Status wird atomar gespeichert; Änderungen an Konto oder Upload während der Anfrage dürfen keine veralteten Ergebnisse übernehmen.

Der Abgleich löst weder Neu-Uploads noch Löschungen aus. Vorhandene Wiederholungs- und lokale Aufräumpfade müssen diese Trennung respektieren; ein fehlendes oder unklar verfügbares Ziel darf nicht zum Verlust der letzten lokalen Kopie führen.

### 4. Archivanzeige anschließen

Dateien:
- `rust/crates/tb-dashboard-api/src/handlers/social_media_vod_archive.rs`
- `bot/dashboard_v2/src/api/socialMedia.ts`
- `bot/dashboard_v2/src/components/socialmedia/VodArchiveTab.tsx`
- `bot/dashboard_v2/src/i18n/vodArchive.ts`

Status aus gespeichertem YouTube-Nachweis anzeigen, beispielsweise „Auf YouTube bestätigt“, „YouTube verarbeitet das Video“ oder „Prüfung gerade nicht möglich“. Dazu echter Videolink, Sichtbarkeit und „Zuletzt geprüft“. Historisch gespeicherter Uploadabschluss ohne aktuelle Prüfung bleibt als solcher erkennbar.

Knopf „Bei YouTube prüfen“ über den bestehenden geschützten Aktionsweg: legt nur einen entprellten Prüfauftrag an, keine lange Providerabfrage im Seitenaufruf. Identität und Kanalrechte ausschließlich aus bestehender Sitzung. Den nutzlosen Hinweis auf nicht vorhandene Ziellinks entfernen. Keine zusätzliche Drive-Prüfung oder allgemeine Dashboard-Neugestaltung.

## Nachweis und Abschluss

- Bestehende Rust-/Frontend-Prüfungen ausführen, Fehler gegen echte Baseline abgrenzen. Fokus: vollständige und unvollständige Mehrteiler, eindeutige/mehrdeutige historische Zuordnung, private Videos, Kanalwechsel, fehlende Rechte, Verarbeitung, Ablehnung, leere Antwort, Quota, Pagination und Wiederholungsaufrufe.
- PostgreSQL-Schreibpfad mit isolierten Daten prüfen; keine handgeschriebenen Produktionskorrekturen.
- Read-only-Abfrage am tatsächlich verbundenen YouTube-Kanal belegen und die fünf Altfälle einzeln fachlich einordnen. Fehlende Rechte oder fehlende Identitätsmerkmale als echten Blocker benennen; keine erfolgreiche Wiederherstellung vorwegnehmen.
- Keine echten VODs hochladen, löschen, verstecken oder deren Sichtbarkeit ändern. Keine Secrets oder privaten Videometadaten in externe Berichte geben.
- Oberfläche mit Moli auf Desktop und Mobil prüfen, ausschließlich erlaubte Testdaten und vorhandene legitime Sitzungen verwenden.
- Reguläres Merge-Gate, Merge/Push, Migration, Release über vorhandenen Deploy-Wrapper, Neustart und Live-Nachweis. Erst dann eigenen Branch/Worktree entfernen. Ergebnis nennt bestätigte, noch verarbeitete und weiterhin ungeklärte VODs getrennt.
