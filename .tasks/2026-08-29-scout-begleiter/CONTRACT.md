# Contract: Scout-Begleiter — KI-Chat im Stil des Owners

status: aktiv
datum: 2026-08-29
klasse: kritisch (Außenwirkung, Twitch-Ban-Risiko, KI-Autor in Fremdkanälen)
freigabe: User 2026-08-29 (Richtung + eigener Bot-Account per Frage beantwortet)

## Ziel

Ein KI-Chat-Begleiter unter einem EIGENEN Twitch-Account (nicht earlysalty,
nicht der Haupt-Bot), der in freigegebenen Kandidaten-Kanälen wie der Owner
schreibt: viel Smalltalk, helfen (Spiel/Setup), beiläufig pitchen, im Chat
fragen, ob er einen Link schicken darf, und nach einem Ja des Streamers den
persönlichen Discord-Invite im Chat oder per Whisper zustellen.

## Anforderungen (user-sichtbar, prüfbar)

- REQ-01: Die KI schreibt ausschließlich in Kanälen, die im Scout (Slice 1)
  den Status "approved" haben UND je Kanal im Dashboard für die KI
  freigeschaltet sind. "persönlich"-Kanäle bleiben Owner-Sache.
- REQ-02: Shadow zuerst: jede KI-Nachricht ist erst Entwurf im Dashboard
  (Spalte "KI-Entwürfe"); der Owner gibt Batches frei. Erst freigegebene
  Nachrichten werden gesendet. Die Live-Schaltung erfolgt je Kanal einzeln
  nach Owner-Freigabe.
- REQ-03: Link-Flow wie der Owner: in-Chat nachfragen ("darf ich dir den
  Link schicken?"); erst nach einem Ja des Streamers wird der persönliche
  Invite-URL gesendet (Chat oder Whisper). Höchstens 1 Whisper je Kanal in
  30 Tagen, nur nach ausdrücklichem Ja im Chat-Verlauf.
- REQ-04: Limits: nur bei liveem Stream, wenige Nachrichten je Kanal-Session
  (Deckel konfigurierbar, Default 5), höchstens 1 Kanal parallel aktiv,
  Tagesdeckel über alle Kanäle, Cooldown je Kanal zwischen Besuchen.
- REQ-05: Stil: Deutsch, du-Form, echte Umlaute, keine Werbe-Links von sich
  aus, Hilfe zuerst; Stilgrundlage sind die Chatnachrichten des Owners aus
  der DB. Keine Identitäts-, Politik- oder ToS-Themen; feste Verbotsregeln
  laufen vor dem LLM und auf dem Entwurf.
- REQ-06: Jede Nachricht (Entwurf, freigegeben, gesendet, abgelehnt) mit
  Kritiker-Urteil im Ledger; alle Urteilsklassen sichtbar (Judge-Sichtbarkeit).
- REQ-07: Modell ausschließlich Deepseek V4 Flash über tb-llm (KI-Connector),
  nichts hart verdrahtet.

## Invarianten

- INV-01: Kein Schreiben außerhalb der Safelist; fail-closed bei Listen- oder
  Live-Status-Fehlern.
- INV-02: Kein Versand von Links ohne Streamer-Ja; kein Massen-Whisper.
- INV-03: Nicht im Namen von earlysalty; der Account ist als Bot erkennbar.
- INV-04: OAuth/Token über den bestehenden Token-Store (erweitern, kein
  zweiter OAuth-Weg); Secret-Handling ausschließlich über Infisical.
- INV-05: Die deaktivierte Trust-Leiter (`recruitment_messaging.rs`) wird
  nicht genutzt und nicht reaktiviert.

## Nicht-Ziele

- Keine Raids auslösen, keine Massen-DMs, keine anderen Plattformen, keine
  Nachrichten unter dem Owner-Account, kein Auto-Modus ohne Shadow-Historie.

## Änderungsbereich

- Neu: Begleiter-Kern (Kontext-Bau, Prompt, Kritiker, Ledger) im Twitch-Bot,
  Shadow-Loop, Dashboard-Flächen (Entwurfs-Queue, Kanal-Freischaltung),
  Whisper-Sendeweg auf dem Scout-Account. Details im PLAN.md.
- Verboten: tb-stream-audit, Billing, OAuth-Grundarchitektur, Main-Bot-Chat
  Verhaltensänderungen.

## Offene Produktfragen

- Account-Name des Scout-Accounts (legt der User an, Rest geht ans System).
