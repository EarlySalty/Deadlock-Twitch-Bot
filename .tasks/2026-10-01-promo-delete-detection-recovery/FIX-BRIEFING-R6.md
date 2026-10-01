status: aktiv | Datum: 2026-10-01

# Fix-Briefing R6: Promo-Delete-Detection

## Auftrag

Behebe ausschließlich Gate R6 aus `REVIEW.md`. Twitch-gelöschte, vom Bot gesendete Promo-Ankündigungen müssen weiterhin im bestehenden Bot-Log erkennbar sein, auch wenn die Datenbank länger als das zehnminütige Korrelationsfenster nicht erreichbar ist oder der Bot währenddessen neu startet.

## Referenzen

- `AUFTRAG.md`: ursprünglicher Intent und Produktverhalten.
- `REVIEW.md`, Abschnitt „Gate Review R6“: aktueller Gate-Befund.
- `rust/crates/tb-chat/src/promos.rs`: `record_promo_delivery` wird nach einem erfolgreichen Twitch-Send für die drei Quellen `reason`, `timeout_pitch` und `lurker_tax` aufgerufen.
- R5-Fix laut gepushtem Commit `9c6b3a4ec417a76762f0d00ac34a941eef54f5d5`: fehlgeschlagene Audit-Persistierung landet in einem In-Memory-Puffer mit Kapazität 256 und zehn Minuten Ablaufzeit. Der bestehende 60-Sekunden-Loop versucht die Persistierung erneut.
- R6-Befund: bleibt die Datenbank über zehn Minuten nicht erreichbar, verfällt der Puffer und die angenommene Sendung hat keinen dauerhaften Audit-Datensatz.

## Beweisziel

Eine angenommene Twitch-Lieferung darf nach Datenbankausfall oder Bot-Neustart nicht unbemerkt ihre Löschkorrelation verlieren. Eine Promo darf nicht erneut gesendet werden, um den Audit-Datensatz nachzuholen. Stelle eine dauerhafte Wiederherstellung sicher. Wenn vor dem Versand kein dauerhafter Korrelationsdatensatz geschrieben werden kann, darf der Sendepfad keinen nicht nachverfolgbaren Send auslösen. Erhalte die bestehenden Cooldowns, die Event-Korrelation, die R5-Deduplizierung, die Delete-Locks und das Bot-Log.

## Scope-Zaun

Exakt Gate R6 und notwendige Regressionstests, kein Refactoring und kein Gesamtformatieren. Keine Änderungen außerhalb des Promo-Scopes. `REGISTER.md` hat eine uncommittete Änderung, die nicht von dir stammt: nicht lesen, ändern, stagen oder committen. Keine Änderungen an fremden Branches oder Worktrees, kein Force-Push, kein Merge und kein Deploy. Keine schwere Testsuite parallel zu anderen Läufen. Keine Unter-Threads starten.

## Branch und Zustand

- Repo/Worktree: `/home/nathanael/.worktrees/luna-dispatch-deadlock-twitch-bot-promo-delete-detection-20260915-364ad1c8`
- Branch: `codex/luna-dispatch/deadlock-twitch-bot/promo-delete-detection-20260915-364ad1c8`
- Gate-R6-HEAD: `9c6b3a4ec417a76762f0d00ac34a941eef54f5d5`
- Der aktuelle Branch ist auf origin gesichert. Commit und Push ausschließlich auf diesen Branch sind erlaubt.
- Der Branch liegt laut Intent-Bericht 618 Commits hinter `origin/main`; nicht rebasen oder integrieren. Merge bleibt beim Gruppenintegrator.

## Ablauf

Du bist der einzige Thread für dieses Paket. Keine Unter-Threads oder Unter-Agenten spawnen. Der Gate ist der einzige Code-Reviewer. Nach Fix und Sicherung melde dich mit `[Bump-up] Paket R6: Grund: ... Erledigt: ... Worktree: /home/nathanael/.worktrees/luna-dispatch-deadlock-twitch-bot-promo-delete-detection-20260915-364ad1c8 Offen: ...` an den Intent-Thread `a7e16de3-e682-42da-a3d7-6c8c7d434f5e` und stoppe danach.
