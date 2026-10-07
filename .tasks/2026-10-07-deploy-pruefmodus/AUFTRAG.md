# Lesender Twitch-Prüfmodus

Auftrag vom 2026-10-07, Intent-Thread d3a1741e-82bc-4a48-865b-2845c663dca7.

`deploy-twitch-release --pruefen` liest current und vier System-Units einschließlich MainPID, exe, deleted, Release-SHA, NRestarts und ActiveState. Exit 0 nur für vier aktive Prozesse ohne deleted auf demselben Release wie current. Keine Mutation und kein Restart im Prüfpfad. Wrapperquelle: ops/systemd/deploy-twitch-release. Installierte Version entspricht origin/main vor dem Fix. Skills werden in claude-config aktualisiert. Enger Hook-Fix nur bei nachgewiesener Fehlklassifikation.

Abschluss: Validierung, zentraler Gate bis ALLOW, Push nach main, Wrapper installieren, live prüfen, eigene Branches und Worktrees entfernen, settle --selbst. Fremde Änderungen in kanonischen Checkouts bleiben erhalten.
