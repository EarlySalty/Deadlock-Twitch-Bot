# Register: Kategoriesammler nativ

## Session-Register

| Paket | Thread-ID | Ersteller | Startnachweis | Harness | Modell | Status | Worktree | Branch | HEAD | Letzte Meldung |
|---|---|---|---|---|---|---|---|---|---|---|
| A | 0712a6dd-cf2a-4a39-907a-f50b19e7930c | Claude-Hauptsession 8604000d-b8ca-40d7-a1f9-8fe5fd44aa65 | t3-harness: Turn angenommen | Claude Code | gpt-6.1-sol, ultracode | Integration und Prüfungen | /home/nathanael/.worktrees/tb-kategoriesammler-nativ | feat/kategoriesammler-nativ | d782ab2b | Nativen Umbau zuerst durch Gate, Merge und Live-Abnahme führen. Nachtrag 1 am 2026-10-08 ausdrücklich beauftragt, mitcommitten und danach separat umsetzen. |
| A, erster Workflow | wbqfebm6h; Agenten a83f7cb1694d77d7a, a831ec2b49f2f2fb2, af22906ab85024cef | Teil-Orchestrator 0712a6dd-cf2a-4a39-907a-f50b19e7930c | Native Workflow-Transkripte | Claude Code | gpt-6.1-sol, xhigh | gestoppt, nicht wieder aufnehmen | /home/nathanael/.worktrees/tb-kategoriesammler-nativ | feat/kategoriesammler-nativ | d782ab2b | Schreibkollision nach Nachrichten an laufende Workflow-Agenten. Eigene Läufe am 2026-10-08 gestoppt, Änderungen erhalten. |
| A, Integration | w2z142fw3; wf_893f538e-efc; af3c6aedac0ce8e8c | Teil-Orchestrator 0712a6dd-cf2a-4a39-907a-f50b19e7930c | Workflow-Journal und Agenttranskript, Start 2026-10-08 16:21 CEST | Claude Code | gpt-6.1-sol, xhigh | angehalten, Änderungen und Belege übernommen | /home/nathanael/.worktrees/tb-kategoriesammler-nativ | feat/kategoriesammler-nativ | d782ab2b | 8 Collector-/Watchdog-Tests und 4 Supervisor-Tests bestanden, 0 ignoriert; Wrapper: 7 Tests bestanden. Eine zusätzliche wartende Abschlussprüfung wurde gestoppt. Der Teil-Orchestrator übernimmt Gate und Release. Kein Commit oder Produktiveingriff durch den Worker. |
| A, Fixrunde 1 | a8bc3a25cc3536cdd | Teil-Orchestrator 0712a6dd-cf2a-4a39-907a-f50b19e7930c | Native Agent-Werkzeugbestätigung | Claude Code | gpt-6.1-sol | abgeschlossen | /home/nathanael/.worktrees/tb-kategoriesammler-nativ | detached | a1a49e95 | 34 Tests bestanden, 0 ignoriert; Gate mit gpt-6.1-sol ALLOW. Nach Integration des aktuellen origin/main ist eine erneute Prüfung nötig. |

## Gate Runde 1

Kandidat `fd0dbebdc391b05dbccecec1bd384e567b352237`, Basis `origin/main`, Kritiker `gpt-6.1-sol`: BLOCK. Kein Merge oder Deploy.

1. Budget-Hysterese von Abschaltung und Plattenpause trennen; Budgetpause darf nicht irrtümlich dauerhaft einrasten.
2. Historische Messlücken nur um die tatsächlich erklärten Pausezeiten kürzen; eine kurze Pause darf keinen längeren Ausfall verdecken.
3. Wartende native Prozesse dürfen die Identität des aktiven Lease-Inhabers nicht überschreiben.
4. Hinweise mitprüfen: komplette Rollenmatrix, leere Kategorie bei der Umschaltung, Zählerlebensdauer und Moli-Bildnachweis für Pause beziehungsweise veraltete Daten.

Ein frischer nativer Fixer übernimmt diese Runde. Folgeprüfung mit demselben Kritiker, keine Produktivschreibrechte für den Fixer.

## Gate Runde 2 nach aktueller Main-Integration

Kandidat `aeb05ce2bbe781e5c01ef002bed385d7cbb550c0`, aktuelle Basis `origin/main` mit `12987b68`, Kritiker `gpt-6.1-sol`: BLOCK. Kein Push oder Deploy.

1. category-runtime-roles.sql entzieht dem Bot Rechte auf category_watchdog_suspensions, category_watchdog_storage_incidents und category_watchdog_storage_notifications, ohne sie in derselben Matrix wieder zu vergeben. Die eigenständige Wiederanwendung muss sicher sein.
2. Die technische Doku beschreibt wartende Prozesse noch in category_native_runtime und verlangt Streamzeilen. Tatsächlich werden category_native_processes und bestätigte leere Messläufe verwendet.
3. Hinweis: Rechte für store_snapshot, store_messages, flush_rollups und delete_chat im echten Test-Postgres als twitchbot prüfen.

Die zweite Fixrunde erhält wieder einen frischen nativen Kontext und denselben Kritiker. Die Hinweise aus Runde 1 bleiben mit ihren Nachweisen erhalten.

## Reihenfolge und Freigaben

1. AUFTRAG.md: Native Integration fertigstellen, Merge-Gate ALLOW, Merge und Deploy, Live-Beweis. NACHTRAG-1-SPEICHER.md wird als beauftragte Datei mitgesichert. Ungenutzte Legacy-Quellen werden für einen kleineren Gate-Diff separat entfernt; die Produktivumschaltung und Entfernung alter Zugänge erfolgen erst nach dem nativen Messbeweis.
2. NACHTRAG-1-SPEICHER.md: Erst nach dem Live-Beweis beginnen. Eigene Folgecommits für verschlüsselte Drive-Ablage und verlustfreie Snapshot-Normalisierung.
3. Vor der ersten lokalen Archivlöschung den Trockenlauf mit Zeilen und Bytes je Tag, getrennt nach Partnerstatus, vorlegen. Kein Altbestand wird im ersten Release gelöscht.
