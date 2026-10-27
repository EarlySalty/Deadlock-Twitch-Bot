# Auftrag E: TikTok Direct Post statt Postfach

Auftraggeber: Haupt-Orchestrator (Claude-Hauptsession, Thread d3a1741e-82bc-4a48-865b-2845c663dca7)
Rolle: Blatt-Worker. Genau dieser Auftrag, keine weiteren Threads oder Orchestratoren starten.
Stufe: groß. Repo: Deadlock-Twitch-Bot. Eigener Worktree `~/.worktrees/tb-social-tiktok-direct` von origin/main, Branch `feat/social-tiktok-direct-post`.

Grundlage: Go-Live-Bericht auf Branch `docs/social-golive-check` (Commit 923b0024), Abschnitt C1 und B7, `git show origin/docs/social-golive-check:.tasks/2026-10-07-social-media-golive-check/BERICHT.md`.

## Nutzerwunsch

Die TikTok-App ist zugelassen. Clips sollen wirklich auf TikTok veröffentlicht werden, nicht als Entwurf im Postfach landen. Die im Dashboard erzeugte Beschreibung geht mit.

## Stand

- Gespeicherte TikTok-Verbindung hat die Scopes `user.info.basic,video.publish,video.upload`. `video.publish` ist also da.
- `rust/crates/tb-social-media/src/uploaders/tiktok.rs`: Direct Post existiert als Bibliotheksoption (`with_privacy_level`), die produktive Fabrik nutzt Inbox und überträgt keine Caption.
- TikTok verlangt für Direct Post eine Pflichtoberfläche (Content Sharing Guidelines, https://developers.tiktok.com/doc/content-sharing-guidelines/): aktuelle Creator-Info mit Nickname, erlaubte Videodauer, Privatsphäre manuell gewählt ohne Vorauswahl aus `privacy_level_options`, Schalter für Kommentare/Duett/Stitch nur soweit erlaubt, Offenlegung kommerzieller Inhalte (eigene Marke / Markenpartner) mit den vorgeschriebenen Hinweisen, Zustimmungstext zur Music Usage Confirmation, Hinweis zur Bearbeitungszeit. Vor dem Bau die aktuelle Fassung der Richtlinie und der Direct-Post-API lesen.
- Nicht auditierte Clients dürfen nur `SELF_ONLY` posten. Live per `creator_info/query` (nur lesend) prüfen, welche Optionen das Konto bekommt, und im Bericht nennen.

## Ziel

1. Upload-Fabrik für TikTok auf Direct Post (`/v2/post/publish/video/init/` mit `post_info`) umstellen, Caption aus dem Dashboard mitgeben (TikTok-Längengrenze beachten).
2. Pflichtoberfläche im Dashboard beim Einplanen eines TikTok-Posts: Creator-Info frisch beim Öffnen laden, Privatsphäre ohne Default, Interaktionsschalter, Werbe-Offenlegung, Zustimmung. Die Wahl wird am Job gespeichert (neue Migration) und beim Upload verwendet. Ein Job ohne vollständige Wahl wird nicht hochgeladen.
3. Zeitgesteuerte Posts: Creator-Info kurz vor dem Upload erneut prüfen (Dauer, Privatsphäre noch erlaubt); passt etwas nicht mehr, Job mit verständlichem Grund anhalten statt still scheitern.
4. Publish-Status nachverfolgen bis `PUBLISH_COMPLETE` bzw. Fehler, Status im Dashboard anzeigen.
5. Inbox-Weg entfernen, falls er danach ungenutzt ist.
6. TikTok-Passagen in `templates/terms.html` und `templates/privacy.html` an das echte Verhalten anpassen (Auftrag D lässt diese TikTok-Passagen dafür aus). Texte über `rolle-doku-redakteur` und `no-em-dashes`, echte Umlaute.

## Grenzen

- Parallel läuft Auftrag D (`fix/social-golive-upload-texte`, Upload-Wartezustand, Plattformfähigkeiten, Trennen, übrige Rechtstexte) und Auftrag A (`fix/social-token-ablauf`). Deren Bereiche nicht anfassen, vor dem Gate auf origin/main rebasen.
- Migrationen nur neu. `.sqlx` und Schema-Snapshot mitziehen. Kein Python. Keine Code-Kommentare. Secrets nie im Klartext.
- Kein echter Post auf einem Streamer-Konto ohne Freigabe des Haupt-Orchestrators. Live-Beweis zuerst mit `SELF_ONLY` auf dem Konto earlysalty, Freigabe dafür vorher per `FRAGE AN ORCHESTRATOR:` einholen.
- Prüfungen: `cargo fmt`, `cargo clippy`, betroffene `cargo test`, Dashboard-Build, Sichtprüfung der Einplan-Oberfläche.

## Abschluss (Agent 1)

Selbstprüfung `gate_hook.py --review`, Merge-Gate, bei ALLOW Merge per `git push origin HEAD:main` (Einzelschritte), Prod-Migration von Hand als `postgres`, `deploy-twitch-release <sha>`, Neustart Bot und Dashboard, Live-Beweis mit freigegebenem Testpost. Branch und Worktree löschen, Bericht an den Haupt-Orchestrator, `t3-thread.py settle --selbst`.
