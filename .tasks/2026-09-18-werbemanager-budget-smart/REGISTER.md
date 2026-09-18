# Register: werbemanager-budget-smart

status: aktiv (2026-09-18)

| Rolle | Thread-ID | Modell | Worktree | Branch | Status |
|---|---|---|---|---|---|
| Intent | e65a453e | fable | keiner | keiner | aktiv |
| Worker A (Backend) | 9ba2fd2a | opus48 | ~/.worktrees/tb-werbemanager-a | feat/werbemanager-budget-backend | gestartet |
| Worker B (Dashboard) | c2693aab | opus48 | ~/.worktrees/tb-werbemanager-b | feat/werbemanager-budget-dashboard | gestartet |
| Worker C (Telemetrie, Auswertung) | offen | offen | auf Branch-Stand von A | feat/werbemanager-telemetrie | geplant, startet nach Fertigmeldung A |
| Worker D (Chat-Hinweis vor Werbung) | offen | offen | auf Branch-Stand von A | feat/werbemanager-chat-hinweis | geplant, startet nach Fertigmeldung A |

Nachtrag 2 und 3 am 2026-09-18 an A und B gesendet (Nachtrag 3 streicht den Einstellungsvorschlag aus Nachtrag 2).
Nachtrag 1 (NACHTRAG-1.md) am 2026-09-18 an A und B gesendet.

Status-Werte: geplant, gestartet, fertig, gestoppt, gebumpt. Gestoppte oder gestorbene Threads bleiben drin und werden nicht wieder aufgenommen.

Hinweise: Vorcheck lief in der Intent-Sitzung selbst (Fundstellen im Auftrag), kein eigener Vorcheck-Thread. Paket B mit opus48 statt GLM, weil GLM-Worker trotz Briefing selbst nach main mergen. Review beider Branches gemeinsam, nicht nacheinander.
