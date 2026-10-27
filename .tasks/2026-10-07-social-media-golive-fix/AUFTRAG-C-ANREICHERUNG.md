# Auftrag C: Clip-Anreicherung wieder wirksam machen (Go-Live-Blocker 1 und 2)

Auftraggeber: Haupt-Orchestrator (Claude-Hauptsession, Thread d3a1741e-82bc-4a48-865b-2845c663dca7)
Rolle: Blatt-Worker. Genau dieser Auftrag, keine weiteren Threads oder Orchestratoren starten.
Stufe: groß. Repo: Deadlock-Twitch-Bot. Eigener Worktree `~/.worktrees/tb-social-anreicherung` von origin/main, Branch `fix/social-anreicherung`.

Grundlage: Go-Live-Bericht, Branch `docs/social-golive-check` (Commit 923b0024), Datei `.tasks/2026-10-07-social-media-golive-check/BERICHT.md`, Befunde B1 bis B5 mit Datei:Zeile. Zuerst lesen, Befunde am Code verifizieren.

## Ziel

1. **B1 STT:** Der Social-Pfad (`SttTranscriber::from_default()` / `OpenAiTranscriber::from_env()`, Default `127.0.0.1:8791`) trifft heute den Patchnotes-Dienst und bekommt 401. Den echten lokalen STT-Dienst im Bestand finden (Workspace-Wissen `~/.claude/wissen/stt-und-stream-audit.md`, andere Aufrufer wie Stream-Audit) und den Social-Worker an genau diesen Vertrag hängen. Endpunkt aus der normalen Config, keine ENV-Variable neu einführen. Den wirksamen Endpunkt beim Start ohne Geheimnisse ins Log schreiben. Nur lokale Endpunkte, Localhost-Prüfung exakt.
2. **B2 Einwilligung:** `external_llm_consent` bleibt eine bewusste Entscheidung des Streamers und wird nie still gesetzt. Fehlt sie, laufen Transkript und Untertitel trotzdem durch (lokal), nur die LLM-Metadaten entfallen mit einem klaren Zustand statt eines Fehlers. Prüfen, ob es im Dashboard einen Schalter dafür gibt; fehlt er, einen ehrlichen Schalter im bestehenden Einstellungsbereich ergänzen. LLM-Aufrufe nur über den zentralen tb-llm-Hub mit `denken_aus` bei kleinem Budget.
3. **B3 Recovery und faire Retries:** veraltete `transcribing`-Zeilen per Lease/Zeitgrenze zurückholen, Auswahl fair (nicht immer die drei neuesten), Fehlerklassen (dauerhaft vs. vorübergehend) mit Retry-Abstand und Höchstzahl, Pipeline-Fehlschläge sichtbar loggen, einmal je Zustandswechsel.
4. **B4 Neu generieren datenerhaltend:** vorhandenes Transkript, Segmente und Sprache bei reinem Metadaten-Neulauf erhalten; ersetzen erst nach erfolgreicher neuer Transkription. Zwilling Hintergrund-Worker gleich behandeln.
5. **B5 Antwortvertrag:** Rückgabe von "Neu generieren" passend zum Frontend (`api/socialMedia.ts`, `EnrichmentPanel.tsx`), Fehlschlag sichtbar im Editor. Editor-Hinweis "kein LLM-Key" durch zutreffende Texte ersetzen (fehlende Einwilligung, STT gestört).
6. Altbestand: nach dem Deploy die 11 hängenden und 49 fehlgeschlagenen Einträge über den reparierten Weg selbst abarbeiten lassen (kein Hand-Update in der DB).

## Grenzen und Reihenfolge

- Parallel laufen Auftrag A (`fix/social-token-ablauf`, Plattformzugänge/DM, berührt `oauth.rs`, `refresh_worker.rs`, `SocialMedia.tsx` Verbindungskarten) und Auftrag D (`fix/social-golive-upload-texte`, Upload-Wartezustand, Plattformfähigkeiten, Rechtstexte). Diese Bereiche nicht anfassen. Vor dem Gate auf aktuelles origin/main rebasen; Merge-Reihenfolge ist egal, wer später kommt, rebased.
- Migrationen nur neu, nie bestehende ändern. `.sqlx` und Schema-Snapshot mitziehen.
- Kein Python erweitern. Keine Code-Kommentare. Texte mit echten Umlauten, ohne Gedankenstriche.
- Prüfungen: `cargo fmt`, `cargo clippy`, betroffene `cargo test`, Dashboard-Build; Baseline roter Tests vorher messen.

## Abschluss (Agent 1)

Selbstprüfung `gate_hook.py --review`, Merge-Gate, bei ALLOW Merge per `git push origin HEAD:main` (Einzelschritte), Prod-Migration von Hand als `postgres` (Rechte an twitchbot/twitchdash, `_sqlx_migrations`-Eintrag), `deploy-twitch-release <sha>`, Neustart Bot und Dashboard. Live-Beweis: ein neuer Clip läuft ohne Eingriff bis Transkript und Untertitel; Altbestand sinkt sichtbar; Journal zeigt den richtigen STT-Endpunkt. Echte Streamer-Zugänge nicht anfassen. Branch und Worktree löschen, Bericht an den Haupt-Orchestrator, `t3-thread.py settle --selbst`.
