status: aktiv | Datum: 2026-10-01

# Fix-Briefing R5: Promo-Delete-Detection

## Auftrag

Behebe ausschließlich die zwei offenen Befunde aus Gate Review R5 in `REVIEW.md`. Ziel bleibt: Wenn Twitch eine vom Bot gesendete Promo-Ankündigung löscht, wird dies zuverlässig im bestehenden Bot-Log gemeldet.

## Referenzen

- `AUFTRAG.md`: ursprünglicher Intent, gewünschtes Verhalten und Scope.
- `REVIEW.md`, Abschnitt „Gate Review R5“: wörtliche Befunde des aktuellen Merge-Gates.
- `rust/crates/tb-chat/src/promos.rs:754-758`: Der Codekommentar hält fest, dass `record_promo_delivery` nach der von Twitch akzeptierten Promo-Ankündigung den Audit-Eintrag anlegt und die Message-ID nachträglich aus `channel.chat.notification` bindet.
- `rust/crates/tb-chat/src/promos.rs:778-789`: `record_promo_delivery` führt den Audit-Insert in einer Transaktion aus.
- `rust/crates/tb-chat/src/promos.rs:861-874`: Ein fehlgeschlagener Insert wird derzeit nur protokolliert.
- `rust/crates/tb-chat/src/promos.rs:2457-2458`: Aufruf nach akzeptiertem regulärem Promo-Send.
- `rust/crates/tb-chat/src/promos.rs:2508-2509`: Aufruf nach akzeptiertem `timeout_pitch`-Send.
- `rust/crates/tb-chat/src/promos.rs:3131-3132`: Aufruf nach akzeptiertem `lurker_tax`-Send.
- `rust/crates/tb-chat/src/promos.rs:1075-1097`: Der Announcement-Handler versucht, einen noch ungebundenen Audit-Datensatz zu binden.
- `rust/crates/tb-chat/src/promos.rs:1123-1148`: Erst nach dem Bindungsversuch prüft er eine bereits bekannte Message-ID oder puffert das Event.

## Beweisziel

1. Ein temporärer Datenbankfehler nach erfolgreichem Twitch-Send darf die spätere Löschmeldung nicht dauerhaft verlieren. Promo-Ankündigungen dürfen nicht erneut gesendet werden, nur um den Audit-Datensatz wiederherzustellen. Berücksichtige die drei genannten Aufrufer und erhalte den Versand-Cooldown.
2. Ein wiederholtes Announcement-Event mit bereits gebundener Message-ID darf nicht an der Unique-Constraint scheitern, wenn zusätzlich ein passender ungebundener Audit-Datensatz existiert. Der bestehende Alert-Retry muss weiterhin erreichbar sein.
3. Erhalte Event-Puffer, Delete-Korrelation und die R4-Synchronisierung mit dem Message-ID-Advisory-Lock. Ergänze oder passe gezielte Tests an, falls sie diese Zustände ohne schwere parallele Suite abdecken können.

## Scope-Zaun

Exakt diese zwei R5-Befunde, kein Refactoring und kein Formatieren des Gesamt-Repositories. Änderungen am Promo-Sendepfad nur soweit für den Beweis nötig. Keine Änderungen an fremden Branches oder Worktrees, keine Deploys, kein Force-Push. Keine schweren Suites parallel zu anderen Läufen. Keine Unter-Threads starten.

## Branch und Zustand

- Repo/Worktree: `/home/nathanael/.worktrees/luna-dispatch-deadlock-twitch-bot-promo-delete-detection-20260915-364ad1c8`
- Branch: `codex/luna-dispatch/deadlock-twitch-bot/promo-delete-detection-20260915-364ad1c8`
- Gate R5 prüfte Code-Commit: `dd7e9e64ccf2856c2fcd34da084d5d2d36f9bee6`
- Aktueller Übergabe-HEAD: `68aea565`; dieser Commit ergänzt nur Review-Artefakte.
- Änderungen gehören ausschließlich auf diesen Branch. Commit und Push auf diesen Branch sind erlaubt. Niemals nach `main` mergen.

## Ablauf

Du bist der einzige Thread für dieses Paket. Keine Unter-Threads oder Unter-Agenten spawnen. Der Gate ist der einzige Code-Reviewer. Nach Fix und Sicherung melde dich mit `[Bump-up] Paket R5: Grund: ... Erledigt: ... Worktree: /home/nathanael/.worktrees/luna-dispatch-deadlock-twitch-bot-promo-delete-detection-20260915-364ad1c8 Offen: ...` an den Intent-Thread `a7e16de3-e682-42da-a3d7-6c8c7d434f5e` und stoppe danach.
