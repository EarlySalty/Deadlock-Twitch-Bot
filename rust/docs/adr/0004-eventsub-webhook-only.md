# ADR 0004 — EventSub-Transport: Webhook-only, WS-Pool entfällt

Status: akzeptiert · Datum: 2026-06-09 · Kontext: Schritt 4 (Monitoring)

## Kontext

Python betreibt zwei EventSub-Transporte: einen Webhook-Pfad (öffentlicher Callback am
Dashboard-Service, HMAC-verifiziert, Zustellung an den Bot über die interne Bridge
`POST /eventsub/dispatch` auf Port 8776) und einen WebSocket-Pool als Fallback
(bis zu 3 Transporte à `MAX_SUBSCRIPTIONS_PER_TRANSPORT`, ~1.200 Zeilen Code).
Der WS-Pool greift nur, wenn `TWITCH_WEBHOOK_SECRET` fehlt.

Prod-Befund (2026-06-09): Webhook-Modus aktiv (laufende
`EventSub Webhook: Internal notification accepted`-Dispatches im Bot-Log), keinerlei
WS-Listener-Aktivität seit dem letzten Start.

## Entscheidung

Rust portiert **nur den Webhook-Pfad** — konkret den Bridge-Vertrag
`POST /eventsub/dispatch` als Ingress des Rust-Monitorings. Der öffentliche
HMAC-Callback bleibt beim Python-Dashboard-Service, bis dessen eigene
Migrationsphase kommt. Der WS-Pool wird nicht nachgebaut.

## Begründung

- Kein Prod-Einsatz des WS-Pools → toter Code wäre ein 1:1-Port von ~1.200 Zeilen Fallback.
- Die Verarbeitungsschicht ist ohnehin transport-agnostisch: Empfang und Verarbeitung
  sind über die durable Processing-Inbox entkoppelt; ein Transport ist nur ein
  „Enqueue-Produzent". Ein WS-Adapter kann später additiv ergänzt werden.
- Weniger Cutover-Risiko: keine parallele Subscription-Ownership über zwei Transporte.

## Konsequenzen

- Fällt der Webhook-Pfad aus (Dashboard down), puffert die Python-Bridge durable in
  `twitch_eventsub_bridge_outbox` — Events gehen nicht verloren, nur später.
  Das Poll-Loop-Monitoring (15 s) bleibt als Selbstheilung für verpasste Events.
- Die Webhook-Secret-Rotation bleibt eine Dashboard-Service-Angelegenheit.
- Sollte Twitch-Webhook-Zustellung dauerhaft unbrauchbar werden, ist der WS-Adapter
  als neues Modul gegen die bestehende Inbox zu bauen (kein Re-Design nötig).
