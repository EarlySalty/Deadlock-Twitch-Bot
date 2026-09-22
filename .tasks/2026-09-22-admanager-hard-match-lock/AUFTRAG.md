status: aktiv
datum: 2026-09-22

# Auftrag: Werbemanager sperrt Werbung im laufenden Match

## Ziel

Sobald der frische Steam Status ein laufendes Deadlock Match meldet, darf die Smart Strategie keinen eigenen Werbeblock starten und keine geplante Twitch Werbung aktiv vorziehen. Das gilt ab dem ersten erkannten Match Tick, unabhängig von einer intern gespeicherten Match Startzeit.

Queue und Menü bleiben Werbefenster. Steht eine Twitch Werbung während eines Matches unmittelbar an, wird sie bei verfügbarer Pause verschoben. Ohne Pause startet der Bot keinen eigenen Ersatzblock.

## Ursache

Der Entscheider behandelte die erste erkannte Match Minute als `match_start_window`. Zusätzlich akzeptierte das Vorziehfenster seit Commit 83f6753 jeden frischen Deadlock Zustand, auch `in_match=true`. Eine verzögerte Steam Presence konnte dadurch bei real bereits laufendem Match Werbung auslösen.

## Umfang

- `rust/crates/tb-analytics/src/ad_manager.rs`
- `rust/crates/tb-analytics/tests/ad_manager_decision.rs`
- bestehende Grundcodes für historische Einträge bleiben lesbar
- keine Migration
- keine neuen Einstellungen

## Abnahme

- Eigener Budgetblock wird bei `in_match=true` ab dem ersten erkannten Tick verschoben.
- `pulled_forward` ist bei `in_match=true` ausgeschlossen.
- Imminente Twitch Werbung im Match nutzt Snooze, wenn verfügbar.
- Queue oder Menü kann weiter Werbung starten oder vorziehen.
