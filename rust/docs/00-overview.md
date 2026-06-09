# 00 — Überblick

## Ziel

Der Python-Twitch-Bot (`bot/`, ~211k Zeilen, 581 Dateien) wird nach Rust portiert.
Anspruch: **Enterprise-Niveau** — saubere Crate-/Modul-Trennung, keine Monolithen,
Komposition statt Mixin-Vererbung, kleine fokussierte Funktionen. Kein blindes 1:1-Übersetzen:
Ineffizientes, Veraltetes und Doppeltes wird bewusst sauber gelöst (siehe
[`05-cleanup-decisions.md`](05-cleanup-decisions.md)).

## Scope

**In Scope:** das Python-Backend (`bot/` ohne die React-Teile) → Rust.

**Out of Scope (bleibt unverändert):**
- Die React-Frontends `bot/dashboard_v2/` und `bot/admin_dashboard/` (TS/React).
- `website/` (öffentliche Landing-/Legal-Seiten).

Die Frontends konsumieren das Backend über HTTP/JSON. Diese Verträge muss das Rust-Backend
**byte-stabil** halten — siehe [`03-http-contract.md`](03-http-contract.md).

## Rahmenentscheidungen (fix)

1. **Nur Backend → Rust.** Frontends + `website/` bleiben.
2. **Discord über die interne Bridge.** Kein eigener Discord-Gateway in Rust. Discord-Sends
   (Live-Embeds, Rollen-Sync, Invites) laufen über den bestehenden Master-Broker (8770);
   jeder Bot besitzt nur seinen eigenen Teil. Siehe [`adr/0001-discord-via-bridge.md`](adr/0001-discord-via-bridge.md).
3. **Strangler-Fig-Migration.** Rust läuft neben dem Python-Bot; Subsystem für Subsystem wird
   umgeschaltet, jeder Schritt einzeln rückrollbar. Siehe [`04-cutover-plan.md`](04-cutover-plan.md).
4. **DB-Schema 1:1 als Vertrag.** Während der Migration teilen Python und Rust **dieselbe**
   PostgreSQL-DB. Kein Schema-Bruch. Siehe [`02-db-contract.md`](02-db-contract.md) und
   [`adr/0002-db-sqlx-refinery-shared-schema.md`](adr/0002-db-sqlx-refinery-shared-schema.md).

## Leitprinzipien

- **Komposition statt Vererbung.** Pythons Mixin-Gottklassen (`base.py`, `eventsub_mixin.py`,
  `DashboardV2Server` mit 10 Mixins) werden in getrennte Structs/Router mit einem expliziten
  `AppState` aufgelöst. Keine implizite Method-Resolution-Order.
- **Eine Quelle der Wahrheit pro Belang.** Genau ein `migrations/`-Verzeichnis (heute über
  5 Verzeichnisse verstreut), ein Krypto-Modul (heute 3× kopiert), ein Helix-Client.
- **Verträge zuerst.** DB- und HTTP-Verträge sind das Fundament — bei Strangler-Fig ist ein
  falsch verstandener Vertrag die teuerste Fehlerklasse (zeigt sich erst im Parallelbetrieb).
- **State in der DB, nicht im Prozess.** In-Memory-State-Dicts, die heute mit DB-Tabellen
  konkurrieren (`oauth_state_tokens` + Dict), werden DB-only — kein Split-Brain bei Restart.
- **Kein I/O im Hot-Path-DDL.** Heute laufen `_ensure_*`-DDLs pro Request/Webhook; künftig
  einmalig beim Start als Migration.

## Warum diese Reihenfolge (Verträge vor Code)

Bei Strangler-Fig bedienen Python und Rust übergangsweise dieselbe DB und dieselben Frontends.
Ein Bug in einer falsch interpretierten Spalte oder einem weggelassenen JSON-Feld zeigt sich
erst im Parallelbetrieb und ist dann teuer. Deshalb steht das exakte Vermessen der Verträge
(DB + HTTP) **vor** jeder Zeile Rust — das ist die Risiko-Versicherung, die den Parallelbetrieb
überhaupt erst tragfähig macht.
