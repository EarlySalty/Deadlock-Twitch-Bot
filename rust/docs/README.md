# Twitch-Bot — Rust-Rewrite (Doku)

Interne Architektur- und Entscheidungs-Doku für die Portierung des Python-Twitch-Bots
(`bot/`, ~211k Zeilen) nach Rust. Diese Doku beschreibt **was existiert / entstehen soll,
wie es geschnitten ist und warum** — sie ist die verbindliche Referenz für jede Phase.

> Interne Doku. **Kein** Changelog-Eintrag, keine Discord-/In-App-Spiegelung.
> Bei jeder Code-Änderung am Rust-Teil wird die betroffene Doku hier mitgepflegt.

## Navigation

| Datei | Inhalt | Frage |
|---|---|---|
| [`00-overview.md`](00-overview.md) | Ziel, Scope, Rahmenentscheidungen, Prinzipien | *Was und warum überhaupt?* |
| [`01-architecture.md`](01-architecture.md) | Cargo-Workspace, Crates, Abhängigkeitsgraph | *Wie ist der Code geschnitten?* |
| [`02-db-contract.md`](02-db-contract.md) | PostgreSQL-Tabellen-Inventar (shared DB) | *Welcher Datenvertrag gilt?* |
| [`03-http-contract.md`](03-http-contract.md) | Frontend-, interne und Browser-HTTP-Verträge | *Welche Endpoints müssen stabil bleiben?* |
| [`04-cutover-plan.md`](04-cutover-plan.md) | Strangler-Fig-Reihenfolge, je Schritt Erfolg/Rollback | *In welcher Reihenfolge wird umgeschaltet?* |
| [`05-cleanup-decisions.md`](05-cleanup-decisions.md) | „Aufgeräumt statt 1:1" — Konsolidierungen | *Was wird bewusst anders gelöst?* |
| [`06-open-questions.md`](06-open-questions.md) | Offene Punkte/Risiken vor den Phasen | *Was muss noch geklärt werden?* |
| [`adr/`](adr/) | Architecture Decision Records | *Warum diese Entscheidung?* |

## Status

Stand: 2026-06-08 — **Design abgeschlossen, Foundation (Phase 0) noch nicht begonnen.**
Quell-Repo bleibt unangetastet; der gesamte Rust-Code lebt unter `rust/`.
