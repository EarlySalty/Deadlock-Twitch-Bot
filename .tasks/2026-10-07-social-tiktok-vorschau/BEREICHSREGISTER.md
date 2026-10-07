# Bereichsregister TikTok

status: aktiv, 2026-10-07

## Ausgangsstand und Vertrag

Worktree `/home/nathanael/.worktrees/tb-tiktok-preview-freigabe-20261007`, Branch `fix/tiktok-preview-freigabe-20261007`, HEAD `b0bd68248c3accc1771e938e6166c3a122ac154e`. Beim Start ausschließlich AUFTRAG.md und REGISTER.md untracked. Der zentrale REGISTER gehört der Hauptsession. Der geteilte Checkout bleibt unangetastet.

Auftrag: Vorschau und ausdrückliche TikTok-Freigabe zusammen mit dem Upload-Warteschlangenpfad reparieren. Keine Veröffentlichung, keine Nutzerzustimmung ändern, keine OAuth- oder Archivänderungen. Gemeinsame Abnahme, Gate und ein Deploy. Alte verstrichene Termine lösen keine sofortige Veröffentlichung aus. Erfolgreiche YouTube-Aufträge bleiben unverändert.

## Native Bauagenten

| Paket | Native ID | Modell und Auswahl | Eigentum | Stand |
| --- | --- | --- | --- | --- |
| P | a808e9877ddc86a39 | geerbtes gpt-6.1-sol, high, `pyramide worker_mittel --frei --json` | preview.rs, social_media_tiktok_direct.rs, TikTokPostDialog.tsx, dictionary.ts nur neue Vorschau- und Q-Texte | Implementierung in Abschlussprüfung |
| Q | aae459057e450a736 | geerbtes gpt-6.1-sol, high, `pyramide worker_mittel --frei --json` | upload_worker.rs, approval.rs, scheduler.rs, posting_plan.rs, clip_queue.rs, social_media.rs, SocialMedia.tsx, gemeinsame API/Typen erst nach genauer Benennung | gestartet |
| B | a21c0aa6882213abd | geerbtes gpt-6.1-sol, high, dieselbe freigegebene Workerwahl | nur neue isolierte Moli-Fixtures und Belege unter browser/, B-BERICHT.md | gestartet am 8. Oktober 2026 |

Native Agent-Aufrufe am 7. Oktober 2026 tatsächlich als Hintergrundaufträge gestartet. Kein Modellwechsel, kein zusätzlicher T3-Thread, keine weitere Orchestrierungsebene. Mittelgroßer begrenzter Fix mit zwei Blatt-Workern, kein umfangreicher Workflow gestartet. Beide Worker arbeiten im vorhandenen eigenen Integrationsworktree. Keine Worker-Commits oder Deploys.

## Schnittstellen

P meldet den Speichern-Pfad der TikTok-Optionen. Q liefert `apply_tiktok_choice(&mut PgConnection, clip_id: i64, options: &Value) -> Result<u64, sqlx::Error>` in clip_queue.rs. P ruft den Helfer unter der vorhandenen Clip-Sperre auf. Zukunftstermine bleiben erhalten, vergangene Termine warten auf ausdrückliche Neuplanung. Andere Plattformen und abgeschlossene TikTok-Aufträge bleiben unverändert. Gemeinsame Schreibpfade haben genau einen Eigentümer. social_media.rs und SocialMedia.tsx gehören Q, P fordert nötige Änderungen über den Teil-Orchestrator an. queuePresentation.ts wurde Q ausdrücklich zugeordnet, dictionary.ts für neun neue Texte P.

Moli ist der einzige erlaubte Browser. Die Browserregel wurde beiden Workern samt Pfad `/home/nathanael/Documents/claude-config/wissen/agent-browser.md` mitgegeben. Brave, persönliche Browser und fremde Dienste sind tabu. Rust-Prüfungen über cargo-slot mit --jobs 3, keine Worker-Release-Builds. Keine Secrets oder ENV-Dateien lesen.

## Verantwortung und Abschluss

Teil-Orchestrator hält BEREICHSREGISTER.md, REVIEW.md, ABSCHLUSS.md, Integration, Belege und Paket-T-Statusereignisse. Hauptsession `d71ef3f0-d1c3-420d-948c-320ff0cc9670` hält REGISTER.md. Workerberichte: P-BERICHT.md und Q-BERICHT.md. Aktive Worker nach etwa 20 Minuten prüfen, spätestens nach 30 Minuten. Kombinierte unabhängige Intent-Abnahme vor zentralem Bug-/Security-Gate. Bei BLOCK je Runde frischer Fixer, höchstens fünf erfolglose Runden. Keine getrennten Merges oder Deploys.
