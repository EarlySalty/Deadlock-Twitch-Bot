# Plan: Scout-Begleiter

status: aktiv
datum: 2026-08-29
contract: CONTRACT.md (dieser Ordner)
vorgaenger: Slice 1 (.tasks/2026-08-29-tb-scout) liefert Safelist und Kennzahlen
klasse: kritisch — Shadow zuerst, Limits, je-Kanal-Ramp

## N1 — Account und Anbindung

- User legt den Scout-Twitch-Account an und nennt den Namen.
- Änderungen: Token/Chat-Anbindung über den bestehenden OAuth- und
  Token-Store (erweitern), Chat-Send über bestehende ChatApi-Pfade,
  Secret-Ablage ausschließlich in Infisical.
- Validierung: Test-Send in einen eigenen Test-Kanal; Token-Refresh
  nachweisbar. Stop-Regel: kein Send → kein Weiterbau.

## N2 — Begleiter-Kern (Entwurfs-Generierung)

- Änderungen: Kontext-Bau (letzte Chatzeilen des Kanals aus
  `twitch_chat_messages`, Live-Status, Kandidaten-Kennzahlen aus dem Scout),
  Prompt mit Stil-Korpus aus Owner-Nachrichten (Deutsch, du-Form,
  Hilfe-zuerst, Link nur auf Nachfrage), feste Vor-Regeln (Verbotsthemen)
  und Entwurfs-Prüfung, Generierung über Deepseek V4 Flash via tb-llm,
  Kritiker-Zweitmodell mit Ledger je Entwurf (alle Urteilsklassen).
- Erwarteter Zwischenzustand: Gegen Test-Daten entstehen Entwürfe im
  Ledger, Verstöße werden abgelehnt, Kosten-Deckel greift.
- Validierung: Cargo-Tests der neuen Module; Prompt-Runs gegen echte
  Kanal-Verläufe werden im Ledger sichtbar.
- Stop-Regel: Entwürfe mit Verstoß (Link unaufgefordert, Identität/Politik,
  Zahlen-Erfindung über die Community) blockieren die Freigabe.

## N3 — Shadow-Loop und Dashboard

- Änderungen: Entwurfs-Queue im Dashboard ("KI-Entwürfe") mit
  Batch-Freigabe, Versand freigegebener Nachrichten mit Limits (REQ-04),
  Link-Frage und Ja-Erkennung aus dem Chatverlauf, Whisper-Zustellung des
  Invites (max 1/30 Tage), Cooldowns, Wache/Meldungen entprellt.
- Erwarteter Zwischenzustand: Ein Kanal im Shadow: Entwürfe entstehen,
  Owner gibt frei, Nachricht geht raus, Ledger stimmt.
- Validierung: End-to-End in einem vom Owner benannten Test-Kanal.
- Stop-Regel: Doppel-Versand oder Send ohne Freigabe → sofort stoppen.

## N4 — Je-Kanal-Ramp und Auswertung

- Änderungen: Kanal-Freischaltung einzeln im Dashboard, Wochenbericht
  (Antwortrate, Ja-Quote auf Link-Frage, Whisper-Erfolg) in bestehende
  Staff-Flächen; Limits nach Messung justieren.
- Validierung: Live-Check im Dashboard plus Nachricht im echten Kanal.
- Stop-Regel: Antwortrate oder Kritiker-Befunde schlechter als Shadow
  → zurück auf Shadow für den Kanal.

## Status

- 2026-08-29: Plan erstellt. Voraussetzung: Slice 1 liefert Safelist;
  N1 wartet auf den Account-Namen vom Owner.
- 2026-08-29, User-Begrenzung (Contract-Freeze, daher hier): Der Begleiter
  wirkt NUR im frühen Fenster eines Kanals, gemessen an den
  Owner-Nachrichten: unter 100 Nachrichten von earlysalty im Kanal = KI
  darf; ab 100 = Kanal gilt als etabliert, KI tritt zurück (keine Entwürfe
  mehr, Handback an den Owner, Status im Dashboard sichtbar). DB-Befund als
  Grundlage: 51 Kanäle im Fenster, davon 26 in den letzten 30 Tagen aktiv;
  22 Kanäle etabliert (7.228 Nachrichten). Umsetzung in N2 (Fenster-Check
  vor jedem Entwurf, fail-closed) und N3 (Handback-Status im Dashboard).
