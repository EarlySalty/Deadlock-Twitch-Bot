# Schritt 4 — Monitoring (Poll-Loop, EventSub, Sessions, Embeds)

> Status: in Arbeit · Slices 4a–4f · Cutover ist **atomar** und user-gated (4f).

## Ausgangslage (Python)

`bot/monitoring/` (13 Dateien, ~11.360 Zeilen) ist das Daten-Ingestion-Herz: 3 Ingress-Pfade
speisen einen gemeinsamen Write-Core über ~31 Tabellen.

| Pfad | Rolle | Schreibt |
|---|---|---|
| Poll-Loop (15 s) | primär für Live-State, Sessions, Stats | `twitch_live_state`, `twitch_stream_sessions`, `twitch_session_viewers`, `twitch_stats_tracked/_category` |
| EventSub (Webhook via Bridge) | primär für Raids + Offline | Telemetrie-Events (bits/subs/follows/…), minimale Live-State-Row bei online |
| Embeds (Discord via Broker 8770) | poll-getrieben, nur `game == Deadlock` | `last_discord_message_id`, `twitch_link_clicks` |

Idempotenz-Fundament: `eventsub_guard_state` (conditional-upsert-Claim) +
`twitch_eventsub_processing_inbox`/`_dead_letter` (durable Queue, `FOR UPDATE SKIP LOCKED`).

## EventSub-Topologie in Prod (verifiziert 2026-06-09)

Twitch → HTTPS → Caddy → **Python-Dashboard-Service** (`/twitch/eventsub/callback`,
HMAC-Verify + Challenge) → Bridge (`dashboard_service/eventsub_bridge.py`):
direkt-zustellen per `POST {internal-api}/eventsub/dispatch` (Port 8776) oder bei
Nichterreichbarkeit durable in `twitch_eventsub_bridge_outbox` puffern. Im Bot:
`_internal_eventsub_dispatch` → Message-Dedup (Guard) → Processing-Inbox → Handler.

Beim Cutover übernimmt Rust Port 8776 — der Bridge-Vertrag (`/eventsub/dispatch`)
ist damit die Transport-Schnittstelle des Rust-Monitorings. Der öffentliche
Webhook-Empfang bleibt beim Python-Dashboard, bis dessen eigene Phase kommt.

## Fork-Entscheidungen

1. **Transport-Mode = Webhook-only.** Prod läuft nachweislich im Webhook-Modus
   (Log-Beleg: laufende `EventSub Webhook: Internal notification accepted`-Dispatches,
   keine WS-Listener-Aktivität). Der WS-Pool (`eventsub_ws.py` 808 + `eventsub_ws_pool.py`
   389 Zeilen) ist reiner Fallback ohne Prod-Einsatz und wird **nicht portiert** (YAGNI).
   Die Verarbeitungsschicht bleibt transport-agnostisch (Inbox entkoppelt Empfang von
   Verarbeitung) — ein WS-Adapter kann später ergänzt werden. Siehe ADR 0004.
2. **`exp_sessions`/`exp_snapshots`/`exp_game_transitions` werden dünn mitportiert.**
   Es gibt echte Konsumenten (AI-Stream-Reports lesen das Game-Breakdown aus
   `exp_sessions`; `/twitch/api/v2/exp/game-transitions`). Nur die 4 Write-Hooks,
   keine Erweiterung; Konsolidierung mit dem Haupt-Session-Modell nach dem Cutover
   (eingetragen in `05-cleanup-decisions.md`).

## Invarianten (würde ein naiver Port brechen)

1. `twitch_live_state`: DELETE-before-UPSERT gegen user_id-Drift; Conflict-Key ist
   `twitch_user_id` (nicht login!); leere user_id → Row stumm überspringen.
2. `twitch_stream_sessions`-INSERT hat in Python **keinen** DB-Unique-Guard — nur einen
   In-Memory-Cache (Start-Rehydrierung). Latenter Doppel-Insert-Bug; der Rust-Port fixt
   das bewusst mit einem DB-seitigen Guard.
3. `twitch_stats_tracked`, Follow-/First-Message-Events sind reine INSERTs ohne Dedup —
   Schema-Vertrag halten, Dedup-Verhalten nicht stillschweigend ändern.
4. Guard-`claim` ist das Exactly-once-Primitiv: conditional Upsert
   `WHERE expires_at <= EXCLUDED.updated_at`. TTLs: Message-Dedup 600 s,
   Offline-Throttle 120 s, Business-Effect 7 d.

## Slices

| Slice | Inhalt | Crate/Ort |
|---|---|---|
| 4a | Guard-Store + Processing-Inbox (Store + Runtime-Worker) | `tb-monitoring` (neu) |
| 4b | Write-Core: live_state, sessions (+exp dünn), session_viewers, stats, Telemetrie | `tb-monitoring` |
| 4c | Poll-Loop: Helix-Abgleich, Transitions, Auto-Archiv, Orphan-Cleanup, Capacity-Snapshot | `tb-monitoring` + `tb-transport-twitch` |
| 4d | Bridge-Dispatch-Endpoint (`/eventsub/dispatch`) + Subscription-Lifecycle (Helix, 409-as-success, Cleanup) | `tb-internal-api` + `tb-transport-twitch` |
| 4e | Go-Live-/Offline-Announcements via Master-Broker (Idempotency-Key) | `tb-transport-discord` |
| 4f | Atomarer Cutover (Python-Monitoring AUS, Rust AN) — **user-gated** | Doku in `04-cutover-plan.md` |

Slices 4a–4e laufen ohne Prod-Berührung (Python bleibt alleiniger Live-Writer);
das gesamte Risiko konzentriert sich auf 4f (Wartungsfenster).

## Bewusste Abweichungen vom Python-Original

- Guard-`claim`: kein `DELETE` aller abgelaufenen Rows im Hot-Path (Python macht das bei
  jedem Claim) — Korrektheit liegt im konditionalen Upsert; GC wird ein periodischer Sweep.
- Inbox-Worker: Store-Fehler beim Lease töten den Worker nicht (Python-Task stirbt bei
  DB-Fehler im Lease-Pfad still); Rust loggt, wartet, macht weiter.
- Python-Bug nicht mitportiert: Dead-Letter-Hook referenziert bei kaputtem Payload-JSON
  eine ungebundene Variable (`payload`) → würde den Inbox-Task killen.
- Session-Insert bekommt DB-seitigen Doppel-Insert-Guard (Invariante 2).
- WS-Pool entfällt (Fork 1), `exp_*` nur Write-Hooks (Fork 2).
