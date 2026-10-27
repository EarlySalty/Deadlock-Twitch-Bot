# Auftrag F: Clip-Anreicherung und Transkripte abschalten

Auftraggeber: Haupt-Orchestrator (Claude-Hauptsession, Thread d3a1741e-82bc-4a48-865b-2845c663dca7)
Rolle: Blatt-Worker. Genau dieser Auftrag, keine weiteren Threads oder Orchestratoren starten.
Stufe: klein. Repo: Deadlock-Twitch-Bot. Eigener Worktree `~/.worktrees/tb-social-anreicherung-aus` von origin/main, Branch `fix/social-anreicherung-aus`.

## Nutzerwunsch

Keine Transkripte und keine automatische Anreicherung von Clips mehr. Das Thema wird später anders gelöst (eigener Prüf-Agent). Bis dahin ist die Funktion aus.

## Stand

Go-Live-Bericht auf Branch `docs/social-golive-check` (Commit 923b0024), Befunde B1 bis B5: Enrichment-Worker in `rust/bin/tb-bot/src/main.rs` (um 1737, Transcriber um 1732), `enrichment_worker.rs`, `enrich_pipeline.rs`, Handler "Neu generieren" in `social_media.rs` (um 2691), Editor `EnrichmentPanel.tsx`. Dazu der Untertitel-/Vorschaupfad, der Transkripte nutzen könnte.

## Ziel

1. Enrichment-Worker nicht mehr spawnen; kein STT-Aufruf und kein LLM-Aufruf für Clips mehr, auch nicht über "Neu generieren" oder einen anderen Zwilling (alle Aufrufer von Transcriber und Enrichment-Pipeline suchen).
2. Im Dashboard den Anreicherungs-Editor und "Neu generieren" ausblenden. Titel und Beschreibung bleiben von Hand editierbar, soweit es dafür bereits Felder gibt. Clips ohne Transkript laufen durch Aufbereitung und Upload wie bisher (prüfen, dass nichts auf ein fertiges Enrichment wartet).
3. Vorhandene Daten und Tabellen nicht löschen, keine Migration. Code der Pipeline bleibt stehen, nur nicht mehr aufgerufen.
4. Logzeile beim Start nennt die Anreicherung als abgeschaltet, einmal.

## Grenzen

- Parallel laufen A (`fix/social-token-ablauf`), D (`fix/social-golive-upload-texte`, ändert u. a. eine Logzeile in `main.rs`) und E (`feat/social-tiktok-direct-post`). Deren Bereiche nicht anfassen, vor dem Gate auf origin/main rebasen.
- Kein Python. Keine Code-Kommentare.
- Prüfungen: `cargo fmt`, `cargo clippy`, betroffene `cargo test`, Dashboard-Build.

## Abschluss (Agent 1)

Selbstprüfung `gate_hook.py --review`, Merge-Gate, bei ALLOW Merge per `git push origin HEAD:main` (Einzelschritte), `deploy-twitch-release <sha>`, Neustart Bot und Dashboard. Live-Beweis: Journal zeigt keinen Enrichment-Start und keine STT-Aufrufe mehr, Editor ist im Dashboard weg. Branch und Worktree löschen, Bericht an den Haupt-Orchestrator, `t3-thread.py settle --selbst`.
