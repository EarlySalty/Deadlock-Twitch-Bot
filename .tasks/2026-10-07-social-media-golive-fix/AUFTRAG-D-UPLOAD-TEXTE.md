# Auftrag D: Upload-Wartezustand, ehrlicher Funktionsumfang, Texte (Go-Live-Blocker 3 und 4)

Auftraggeber: Haupt-Orchestrator (Claude-Hauptsession, Thread d3a1741e-82bc-4a48-865b-2845c663dca7)
Rolle: Blatt-Worker. Genau dieser Auftrag, keine weiteren Threads oder Orchestratoren starten.
Stufe: mittel. Repo: Deadlock-Twitch-Bot. Eigener Worktree `~/.worktrees/tb-social-golive-upload` von origin/main, Branch `fix/social-golive-upload-texte`.

Grundlage: Go-Live-Bericht, Branch `docs/social-golive-check` (Commit 923b0024), `.tasks/2026-10-07-social-media-golive-check/BERICHT.md`, Befunde B6, B7, B8, C1, C2, C4. Zuerst lesen, am Code verifizieren.

## Produktentscheidungen für die Beta (vom Haupt-Orchestrator gesetzt)

- TikTok läuft als Postfach-Übergabe (Inbox), kein Direct Post. Das Dashboard erklärt das schon vor dem Einplanen: das Video landet als Entwurf im TikTok-Postfach, dort Text ergänzen und veröffentlichen.
- TikTok-Statistiken gibt es in der Beta nicht. Instagram wird nur angeboten, wenn eine Verbindung möglich ist; ohne App-Freigabe nicht als verfügbar darstellen.
- YouTube: die tatsächlich von YouTube gesetzte Sichtbarkeit nach dem Upload auslesen und anzeigen, statt `public` vorauszusetzen.

## Ziel

1. **B6:** Job ohne nutzbaren Plattformzugang bleibt nicht still pending. Prüfung schon beim Einplanen (gemeinsamer Einreihpfad) und im Upload-Worker; nutzerverständlicher Wartezustand mit Grund ("TikTok nicht verbunden") und Weg im Dashboard. Zwilling `insights_worker.rs` als Vorbild.
2. **B7:** Plattformfähigkeiten explizit in der API (Upload, Statistiken je Plattform); TikTok-Insights als "nicht verfügbar" statt API-Retry-Schleife; UI verspricht nichts, was fehlt.
3. **B8:** Bei Sammelverbindung "Eigenes Konto verbinden" anbieten statt eines wirkungslosen "Trennen", Zustand erklären. Achtung: Auftrag A ändert die Verbindungskarten in `SocialMedia.tsx` (Ablaufzeile um Zeile 1988). B8 als letzten Schritt bauen, vorher origin/main holen; ist A noch nicht gemergt, B8 mit kleinstem Diff an der Aktion bauen und vor dem Gate rebasen.
4. **C1:** Inbox-Erklärung vor dem Einplanen.
5. **C2:** YouTube-Sichtbarkeit aus der Upload-Antwort speichern und anzeigen; wird ein Video privat gesetzt, das klar sagen.
6. **C4:** `templates/terms.html` und `templates/privacy.html` auf den echten Funktionsumfang bringen (TikTok-Entwurf statt automatischer Veröffentlichung, automatische Löschfristen laut `retention.rs`), keine Versprechen ohne Code. Texte über die Skills `rolle-doku-redakteur` und `no-em-dashes`, echte Umlaute, kein internes Vokabular.
7. Logtext "8 Loops inkl. VOD-Archiv" in `main.rs` auf die echte Zahl bringen.

## Grenzen

- Nicht anfassen: Anreicherung, STT, Neu-generieren (Auftrag C, `fix/social-anreicherung`), Token-Ablauf und DM (Auftrag A). Vor dem Gate auf origin/main rebasen.
- Migrationen nur neu. `.sqlx` und Schema-Snapshot mitziehen. Kein Python erweitern. Keine Code-Kommentare.
- Prüfungen: `cargo fmt`, `cargo clippy`, betroffene `cargo test`, Dashboard-Build, Sichtprüfung der Social-Media-Seite.

## Abschluss (Agent 1)

Selbstprüfung `gate_hook.py --review`, Merge-Gate, bei ALLOW Merge per `git push origin HEAD:main` (Einzelschritte), ggf. Prod-Migration von Hand als `postgres`, `deploy-twitch-release <sha>`, Neustart Bot und Dashboard, Live-Beweis (Status-Endpunkt liefert Fähigkeiten, Rechtsseiten zeigen neue Texte, Wartezustand per Test oder Wegwerf-Zeile). Echte Streamer-Zugänge nicht trennen. Branch und Worktree löschen, Bericht an den Haupt-Orchestrator, `t3-thread.py settle --selbst`.
