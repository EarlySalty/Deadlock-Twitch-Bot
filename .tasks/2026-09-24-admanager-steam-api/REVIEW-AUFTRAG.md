# Unabhängige Abnahme: Matchschutz und angefragte In-Game-Builds

Intent-Session: dieser ChatGPT-Auftrag über codex-mcp. Es gibt keine T3-Intent-ID. Du bist der einzige Review-Thread für dieses Paket. Keine Unter-Threads oder Unter-Agenten spawnen.

## Auftrag

Read-only Review der vier vorhandenen Feature-Commits gegen den Nutzerwunsch. Keine Implementierung, keine Branchwechsel, keine Commits oder GitHub-Schreibaktionen. Keine Produktionsabfragen, Secrets, Ads, Builds im Spiel, Community-Nachrichten oder Dienstneustarts. PR-first-Testbetrieb bleibt verbindlich. Schreibe genau einen Ergebnisbericht in die unten angegebene Datei; sonst keine Dateien ändern.

1. Twitch: `/home/nathanael/.worktrees/tb-admanager-steam-api-20260924`, Commit `5f92ca7c1aeb0c0ce1c841aac880bf80c4dec446`, PR #958. Steam-Producer: `/home/nathanael/.worktrees/steam-player-live-by-steamid-20260924`, Commit `7cfacba9d7db068fccb3e05a2de3f598cc1fa244`, PR #69. Nutzer meldet Werbung während des Matches trotz erwarteter Queue-Verteilung. Prüfe die beiden Seiten gemeinsam: echte Steam-Quelle statt Cross-DB-JOIN, ID-Zuordnung und Trennungen, Quellzeitpunkt, unbekannte/fehlerhafte Daten sperren Werbestarts, frisches Match blockiert, verfügbare Twitch-Pausen werden genutzt, Auth geschlossen, Queue-Verhalten und Grenzen ehrlich. Keine Schutzgarantie behaupten, die Twitch-Snooze-Limits oder bereits laufende Werbung nicht hergeben.
2. Brain: `/home/nathanael/.worktrees/brain-direct-build-publish-20260924`, Commit `11664151b73e8dd7dd73f6f84e4aed8d9aa57171`, PR #11. Discord-Verbraucher: `/home/nathanael/.worktrees/dl-bots-brain-direct-build-publish-20260924`, Commit `1de9176434b0025fc53e20b8e9a210c6f568a528`, PR #451. Nutzer will auf Anfrage Builds direkt im Spiel erstellen. Prüfe beide Seiten gemeinsam: ausdrückliche Anfrage statt bloßem Build-Wort, reiner Erklärungs-/Entwurfswunsch bleibt ohne Schreibaktion, vorhandener Testmodus ist erkennbar experimentell, reguläre Freigabe bleibt erhalten, Argumentgrenzen, Fehler/Timeout/ausstehende Aufträge, Erfolg mit tatsächlich bestätigter positiver Build-ID.

## Ausgabe

Bericht: `/home/nathanael/.worktrees/tb-admanager-steam-api-20260924/.tasks/2026-09-24-admanager-steam-api/REVIEW-R2.md`.

Nenne geprüfte SHAs, belegte blockierende Findings mit Datei und Zeile, nicht blockierende Risiken und Grenzen. Keine Findings erfinden. Urteil je Paket: Nutzer-Intent umgesetzt ja/nein/teilweise; Code-Fix erforderlich ja/nein. GitHub-CI ist separat und derzeit nicht vollständig grün. Bewerte keinen lokalen Test als Live-Beweis. Lies die bestehenden EVIDENCE.md-Dateien; prüfe die entscheidenden Codepfade selbst. Kein Merge-Gate-ALLOW vortäuschen, dies ist die unabhängige fachliche Abnahme.
