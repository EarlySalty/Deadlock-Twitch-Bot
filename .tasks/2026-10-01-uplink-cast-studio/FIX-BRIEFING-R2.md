status: aktiv | Datum: 2026-10-01

# Fixer-Briefing Runde 2

## Übergabe

- Arbeitspfad: `/home/nathanael/.worktrees/luna-dispatch-deadlock-twitch-bot-feat-uplink-cast-studio-20260912-1269ad60`
- Branch: `codex/luna-dispatch/deadlock-twitch-bot/feat-uplink-cast-studio-20260912-1269ad60`
- Stand: `c68e5c1b`
- Gate-Funde: `.tasks/2026-10-01-uplink-cast-studio/REVIEW.md`
- Intent-Thread: `85d24a8f-efa8-470e-9482-046f07826dbc`

## Auftrag

Prüfe Runde 1, Funde 3 und 4 im Review-Artefakt. Finde für Rust-Fund 3 anhand von `uplink_config::runtime` die tatsächliche erlaubte URL-Schema-Menge. Behebe den Fehler ausschließlich im bestehenden Cast-Studio-Rust-Pfad, falls die Konfiguration `https` oder andere nicht-WebSocket-kompatible Schemata erlaubt. Ergänze keinen neuen Codekommentar. Aktualisiere `REVIEW.md` mit Verifikation und offenem Status.

## Grenzen

- Produktiver Anwendungscode ausschließlich Rust. `bot/dashboard_v2/src/pages/UplinkCastStudio.tsx` nicht ändern. Die beiden BLOCKING-UI-Funde 1 und 2 bleiben offen und sind als durch die Rust-only-Regel gesperrt zu dokumentieren.
- `rs-relay`, PR #1035, fremde Branches und Worktrees nicht ändern. Keine Live-Abfrage, kein Deploy, Neustart oder Main-Merge.
- Vor schwerem Cargo-Lauf PR-1035-Cargo-Lock prüfen. Testbefehl nach der bestehenden Akte nutzen. Keine fremden Prozesse oder Locks anfassen.
- Finde 4 bleibt offen, solange Relay-Revision und Vertragsbeleg fehlen. Keine Relayimplementierung ergänzen.
- Nur diesen bestehenden Branch ändern. Keine Unter-Threads oder Unter-Agenten starten.

## Abschluss

`git diff --check`, passende Rust-Validierung und gezielter SPA-Regressionstest ausführen. Ergebnis, Commit-SHA und verbleibende Gate-Blocker in `REVIEW.md` und `REGISTER.md` eintragen. Den Branch nicht pushen, mergen oder integrieren. Abschluss als `[Fix] Runde 2 <ALLOW|BLOCK>` an den Intent-Thread melden.
