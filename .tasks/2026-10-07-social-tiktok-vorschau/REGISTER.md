# Auftragsregister

status: aktiv, 2026-10-07

- Auftrag: TikTok-Freigabe wartet auf nicht gestartete Vorschau, einschließlich sicherer Behandlung bestehender Aufträge ohne Freigabe.
- Hauptorchestrator: Claude-Code-Session `d71ef3f0-d1c3-420d-948c-320ff0cc9670`.
- Intent-Thread: aktueller Nutzerthread; volle T3-Zuordnung noch nicht separat geprüft.
- Modellquelle: `t3-thread.py pyramide worker_mittel` ergab `sol`.
- Vorhandene T3-Threads wurden gelesen und nicht verändert. Kein eigener vorheriger Fix-Thread vorhanden.
- Keine zusätzliche Statusrolle bei diesem eng gekoppelten mittleren Auftrag. Hauptorchestrator schreibt dieses Register, Teil-Orchestrator sein BEREICHSREGISTER.md.

## Session-Register

| Paket | Thread/Session | Ersteller | Startnachweis | Harness | Modell | Status | Worktree | Branch | HEAD | Letzte Meldung |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| T, Versuch 1 | acc27df7-c3ba-406b-8617-3106e9e06fcf | d71ef3f0-d1c3-420d-948c-320ff0cc9670 | t3-harness angenommen; native task.started mit gpt-6.1-sol; 2026-10-07T22:07Z laufende Dateiänderungen | Claude Code via t3-harness, provider claudeAgent | sol | aktiv, zwei Bauagenten und ein Browserprüfagent | /home/nathanael/.worktrees/tb-tiktok-preview-freigabe-20261007 | fix/tiktok-preview-freigabe-20261007 | b0bd68248c3accc1771e938e6166c3a122ac154e | P a808e9877ddc86a39, Q aae459057e450a736, B a21c0aa6882213abd; Einzelheiten in BEREICHSREGISTER.md, noch kein Merge |

## Verantwortung und Überwachung

Teil-Orchestrator T führt native Agenten für Vorschau/Freigabe und Warteschlange mit getrennten Schreibbereichen. T integriert beide Bereiche, fährt Gate und Deploy. Keine Zustimmung oder Veröffentlichung im Namen des Nutzers. Hauptsession prüft spätestens nach 20 bis 30 Minuten über den eigenen Thread und schreibt den Stand hier fort.

Nächste einmalige Wache: 2026-10-08 00:14 Uhr lokaler Zeit, Cron-ID `d246fb19`, nur für diese laufende Hauptsession. Bei aktivem Auftrag wird eine nächste einmalige Prüfung geplant, bei Abschluss keine weitere.

INTENT[IA-1]: Stufe mittel | Modell sol | Thread Hauptsession d71ef3f0-d1c3-420d-948c-320ff0cc9670 | Register: .tasks/2026-10-07-social-tiktok-vorschau/REGISTER.md
