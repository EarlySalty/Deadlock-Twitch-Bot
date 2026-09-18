# Register: werbemanager-budget-smart

status: aktiv (2026-09-18)

| Rolle | Thread-ID | Modell | Worktree | Branch | Status |
|---|---|---|---|---|---|
| Intent | e65a453e | fable | keiner | keiner | aktiv |
| Worker A (Backend) | 9ba2fd2a | opus48 | ~/.worktrees/tb-werbemanager-a | feat/werbemanager-budget-backend | fertig, Commit c9b549c7, gesettelt |
| Worker B (Dashboard) | c2693aab | opus48 | ~/.worktrees/tb-werbemanager-b | feat/werbemanager-budget-dashboard | fertig, Commit 37b4466a, gesettelt |
| Worker C (Telemetrie, Auswertung) | 856072fb | opus48 | ~/.worktrees/tb-werbemanager-c | feat/werbemanager-telemetrie | gestartet, Basis c9b549c7 |
| Worker D (Chat-Hinweis vor Werbung) | 45976252 | opus48 | ~/.worktrees/tb-werbemanager-d | feat/werbemanager-chat-hinweis | gestartet, Basis c9b549c7 plus Merge von B |
| Review R1 (A und B gemeinsam) | 233fb283 | opus48, frischer Thread | liest a und b | keiner | fertig, Urteil FIX NÖTIG (3 Mängel, 3 Nits, kein Blocker), gesettelt |
| Fixer R1 (A und B) | fb095104 | opus48 (statt glm, weil GLM selbst nach main mergt) | a und b | beide Feature-Branches | fertig, 4e1ce51 + c134ce1f gepusht (A), abgenommen |
| Fixer Gate-BLOCK (A, ggf. B) | f6415d73 | grok-4.6 (Rolle fixer; glm rate-limited, astra/sol/luna leer) | a und b | beide Feature-Branches | gebaut, Tests 16 passed, Wiring cargo check grün |

Nachträge 1 bis 3 am 2026-09-18 an A und B gesendet (Nachtrag 3 streicht den Einstellungsvorschlag aus Nachtrag 2).

Status-Werte: geplant, gestartet, fertig, gestoppt, gebumpt. Gestoppte oder gestorbene Threads bleiben drin und werden nicht wieder aufgenommen.

Hinweise: Vorcheck lief in der Intent-Sitzung selbst (Fundstellen im Auftrag), kein eigener Vorcheck-Thread. Alle Pakete mit opus48 statt GLM, weil GLM-Worker trotz Briefing selbst nach main mergen. Merge-Reihenfolge: A und B nach Review und Fix-Runde, danach C und D mit eigenem gemeinsamen Review. Migration von A ist 20260918120000, von Hand als postgres einspielen.
