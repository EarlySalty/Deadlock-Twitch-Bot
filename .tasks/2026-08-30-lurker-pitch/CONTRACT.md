status: aktiv
Datum: 2026-08-30

# Contract: Lurker-Discord-Pitch + LFG-Pitch-Kreis

## Ziel

Marketing mit dem Discord-Link weg vom Gießkanne-Prinzip hin zur gezielten Ansprache:
1. Der LFG-Pitch antwortet nur noch im eigenen Kanal der Community (Bot-Konto ist Broadcaster),
   nicht mehr in fremden Partnerkanälen ohne Gegenleistung.
2. Häufige stille Lurker, die noch nie angesprochen wurden, bekommen im eigenen Kanal einmalig
   eine Discord-Einladung ("aus dem Lurk holen"), betrieben über die bestehende
   Lurker-Tax-Maschinerie.

## Anforderungen (user-sichtbar, prüfbar)

- REQ-1: LFG-Pitch-Antworten (`LFG_PITCH_REPLY`) gehen ausschließlich an Chatter im Kanal,
  dessen Broadcaster-ID der Bot-Account-ID entspricht. Fremdkanäle: still, log mit Grund
  `foreign_channel`. Live prüfbar: kein Community-Invite mehr in Fremdkanälen.
- REQ-2: Läuft `lurker_pitch_enabled = 1` für den eigenen Kanal, bekommen stille Lurker
  (Kandidaten-Engine identisch zur Lurker-Tax: 0 Nachrichten, per Chatters-API gesehen,
  >= 3 vergangene Sessions, >= 240 Lurk-Minuten, live, frisch, ohne bekannte Bots), die noch
  NIE einen Pitch erhalten haben, eine Chat-Ansage mit dem Discord-Invite. Je Person genau
  einmal immer (Persistenz in `twitch_lurker_pitch_log`). Sind alle Kandidaten bereits
  gepitched, fällt der Pfad auf die bestehende Channel-Points-Erinnerung zurück.
  In Fremdkanälen passiert der Pitch nie, auch nicht bei Flag = 1.
- REQ-3: Der bestehende Dashboard-Endpoint `GET/POST /twitch/api/v2/streamer/lurker-tax-settings`
  liefert und setzt zusätzlich `lurker_pitch_enabled` (konsolidiert, kein neuer Endpoint).

## Invarianten (was sich nicht ändern darf)

- INV-1: Lurker-Tax-Erinnerung in Fremdkanälen (Opt-in je Streamer, Channel-Points-Text,
  Cooldowns, Drosseln, Bot-Filter, Scope-Gate) verhält sich exakt wie heute.
- INV-2: Kein neuer öffentlicher Text ohne Betreiber-Flag: `lurker_pitch_enabled` Default 0,
  damit geht ohne ausdrückliches Go des Nutzers keine neue Zeile live.
- INV-3: Keine neuen LLM-Calls, keine Modelländerungen, kein neuer OAuth-Weg, keine ENV-Config.
- INV-4: Bestehende Daten (streamer_plans-Bestand, twitch_session_chatters) werden nicht
  verändert oder zurückgeschrieben; Migration ist ausschließlich additiv.
- INV-5: `!lurkersteuer_off`-Command und sein Verhalten bleiben unangetastet.

## Nicht-Ziele

- Reichweite über Partnerkanäle mit Opt-in (später, eigener Slice).
- Änderung am LFG-Pitch-Text, am Judge oder an Cooldown-Werten.
- Anderer Trigger-Kreis für den Lurker-Pitch (z. B. Streamer-eigene Lurker-Erinnerung mit Discord).

## Erlaubter Änderungsbereich

- `rust/migrations/` (eine neue additive Migration)
- `rust/crates/tb-chat/src/lfg_pitch.rs` (Kanal-Gate + Test)
- `rust/crates/tb-chat/src/promos.rs` (Pitch-Zweig + Test)
- `rust/bin/tb-bot/src/chat_wiring.rs` (nur Konstruktor-Argumente)
- `rust/crates/tb-dashboard-api/src/handlers/lurker_tax_settings.rs` (zweites Flag + Test)
- `rust/.sqlx/` (Cache-Neupräparierung)
- `.tasks/2026-08-30-lurker-pitch/`

## Verboten

- Alles außerhalb des Bereichs, Refactoring am Rand, fmt-Läufe über fremde Dateien,
  Umstellung von `query!` auf Runtime-Queries, Löschen oder Überspringen bestehender Tests.

## Offene Produktfragen

- Finaler Wortlaut der Pitch-Zeile (Entwurf liegt vor). Der Nutzer hat das Frage-Dialogfeld
  abgelehnt; das Flag bleibt bis zu seinem Go auf 0. Das ist kein Implement-Blocker, da der
  Pfad default-aus ist.
