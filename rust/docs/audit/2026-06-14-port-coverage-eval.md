# Port-Vollständigkeits-Evaluierung Twitch-Bot (Python→Rust) — 2026-06-14

> Multi-Agent-Evaluierung (27 Opus-Agenten, find→adversarial-verify→Synthese) über 13
> Subsysteme. Was ist nativ, was läuft via Python-Proxy (8765/8779), was ist tot/Stub,
> welche stillen Regressionen bestehen. Interne Doku — kein Changelog.

Ich erstelle den Bericht direkt aus den verifizierten Subsystem-Evaluierungen. Die Daten sind vollständig und durch adversarische Verdicts geprüft — keine weiteren Tool-Calls nötig.

# Was fehlt nach dem Umschreiben — Twitch-Bot Python→Rust

## 1. Gesamtbild

Die Migration ist **infrastrukturell weit, funktional gespalten**. Nativ und produktiv live laufen: der EventSub-Webhook-Empfang/Dispatch, Monitoring/Go-Live-Verarbeitung (Poll-Loop selbst aber default aus), der Chat-Bot inkl. Moderation (seit 12.6. geflippt), der Raid-Kern (Auto-Raid/!raid/Score/Blacklist-Guard/OAuth), die read-only Analytics-Charts und die geschäftskritischen internen API-Routen. **Drei große Funktionsblöcke sind aber nicht nativ**: das gesamte AI-/Coaching-/Post-Stream-/Admin-Write-Dashboard (~24 Routen, läuft via Proxy→Python 8765), der komplette Social-Media-Upload/Approval/Transkriptions-Stack (~13k LOC, gar nicht portiert, läuft im Python-Bot-Prozess), und der Engagement-KI-Kern (MiniMax-Stammgast) plus Highlight-Clipper plus Voice-Reaction. Der Strangler-Proxy fängt alle HTTP-Routen sauber ab, **aber zwei Subsysteme sind durch den Chat-Flip jetzt real tot**: der Engagement-KI-Stammgast schweigt (kein Proxy für EventSub-Chat) und `!title` antwortet nicht mehr. Dazu kommen mehrere stille Feld-Regressionen in portiertem Code (Raid-Arrival-Nachrichten gedroppt, first_message confirmed_first_ever, discord-profile Rollen-Sync, /stats-EventSub-Sektion leer).

## 2. Statustabelle

| Subsystem | port_status | Kurzbefund |
|---|---|---|
| Chat (Commands/Mod/Notif) | native_partial | Voll portiert + LIVE; Einzel-Commands `!title`/`!lurkersteuer_off` fehlen, einige stille Reply-Diffs |
| Monitoring (Go-Live/EventSub) | native_partial | Empfang/Verarbeitung nativ; Telemetrie-/Moderator-Sub-Anlage nur Python; 1 echte first_message-Regression |
| Raid (Auto/OAuth/Recruit) | native_partial | Kern nativ; **Arrival-Followup-Nachrichten + Externe-Recruitment-Blacklist still gedroppt** (kein Proxy) |
| Analytics + Dashboard-v2 | proxied_to_python | ~37 read-Routen nativ; AI/Coaching/Post-Stream/~24 Admin-Routen via Proxy→8765 |
| Social Media (Clips/Upload) | native_partial | Nur Clip-Fetcher (default AUS); Upload/Approval/Whisper/OAuth gar nicht portiert |
| Engagement (MiniMax-KI) | native_partial | Ränder nativ; **KI-Kern tot, weil Chat geflippt** (No-op, kein Proxy) |
| Community / Voice-Reaction | not_ported | Voice-Reaction-Stack abwesend; default-OFF auch in Python; bewusst aufgeschoben |
| Highlight-Clipper | not_ported | Komplett Python (ungated, läuft); 0 Rust-Bausteine, kein Proxy |
| Title-Generator (!title) | proxied_to_python | Dashboard-Weg via Proxy; **Chat-`!title` tot unter Takeover** |
| Entitlements/Billing | proxied_to_python | Read+Trial-Self-Claim nativ; gesamter Geld-/Schreib-Pfad via Proxy; Trial-Auto-Grant fehlt nativ |
| Live-Announce | native_partial | Voll portiert, aber Poll-Loop default AUS → Python besitzt es; 2 Latenz-Lücken beim Flip |
| Interne API (8776) | native_partial | Kern nativ; mehrere Stubs schatten Proxy aus (503), discord-profile Rollen-Sync verloren |
| Coaching-Audit (CLI) | not_ported | Admin-CLI, nie im Runtime; 0% portiert, kein User-Impact |

## 3. Was noch FEHLT — priorisiert

### (A) Ganz nicht-nativ / nur via Python-Proxy (Migration unvollständig, läuft aber)

| Subsystem | Item | Datei:Zeile | User-Impact | Severity |
|---|---|---|---|---|
| Analytics-Dashboard | AI-Routen (ai/analysis, ai/chat, ai/history) | proxied; py `api_ai.py:167,1001,1087` | AI-Coach-Chat tot bei Proxy-Aus | high |
| Analytics-Dashboard | Coaching-Route (CoachingEngine, 9 Bereiche) | proxied; py `api_insights.py:963` | Coaching-Tab leer bei Proxy-Aus | high |
| Analytics-Dashboard | Post-Stream-Reports + Background-Generierung | proxied; py `api_post_stream.py:1060ff,379,544` | Reports+A/B nur Python; Hintergrund-Gen nur im Python-Prozess | high |
| Analytics-Dashboard | ~24 Admin-Write-Routen (config, affiliates, audit-log, announcements, legal, oauth-scopes, query) | proxied; py `api_admin.py:678-734` | Großteil Admin-Panel bricht bei Python-Down | high |
| Social Media | Upload-Worker (YT/TikTok/IG, 9:16-Transcode) | gar nicht; py `upload_worker.py:28-152` | Kein Upload bei reinem Rust-Betrieb | critical¹ |
| Social Media | Approval-Flow (Pending-DM, Freigabe→Queue) | gar nicht; py `approval_worker.py:18-88` | Keine Approval-DMs ohne Python | critical¹ |
| Social Media | Whisper-Transkription (3 Engines) + Enrichment-LLM + Retention + OAuth-Refresh | gar nicht; py `transcription/`, `enrichment.py`, `oauth_manager.py` | Komplett Python-abhängig | high |
| Entitlements/Billing | Stripe-Webhook-Eingang (Signatur/Idempotenz/Plan-Sync) | proxied; py `routes_billing.py:132-229` | Abos bei Proxy-Aus nie aktiviert/gekündigt | high |
| Entitlements/Billing | Affiliate-Provision 30% + Gutschrift-PDF + 6h-Loop | proxied/gar nicht; py `affiliate_mixin.py:1352`, `gutschrift.py` | Keine Provisionen/Gutschriften ohne Python | high |
| Highlight-Clipper | Gesamtes Auto-Highlight-Subsystem (boon-Demo-Analyse, VOD-Suche, yt-dlp/ffmpeg, Discord-Versand) | gar nicht; py `bot/highlight_clipper/` (11 Module, 1544 LOC) | Feature verschwindet bei Python-Cog-Abschaltung | high |
| Voice-Reaction | Scheduler+ConversationBrain+Audio-Capture+Sales-Webhook | gar nicht; py `voice_reaction/scheduler.py:113` | Akquise-Feature, default-OFF auch in Python | low |
| Title-Generator | knowledge_job/insight_job (Hintergrund) | gar nicht; py `knowledge_job.py:75`, `insight_job.py:65` | Datengrundlage nur Python | medium |

¹ critical bezieht sich auf Migrationsgrad, **kein aktueller User-Bruch** — Python-Bot trägt die Last.

### (B) Tot / No-op / Stub-gibt-false (User-sichtbar kaputt oder Feature weg)

| Subsystem | Item | Datei:Zeile | User-Impact | Severity |
|---|---|---|---|---|
| Engagement | MiniMax-KI-Stammgast antwortet nicht mehr (Pipeline-Schritt 11 No-op) | `tb-chat/src/pipeline.rs:527` | **KI antwortet GAR NICHT im Chat — Chat ist geflippt, kein Proxy** | critical |
| Engagement | Threads/Beziehungsführung + Lurker-Signal still | `threads.py`/`lurker_signal.py` ohne Rust-Port | Langzeitgedächtnis/Follow-ups weg unter Takeover | high |
| Engagement | Super-Mod-Toggle tot (NoopSuperMod → false) | `tb-bot/src/chat_wiring.rs:685-693` | Super-Mod ohne Twitch-Mod kann Engagement nicht togglen | high |
| Title-Generator | Chat-`!title`/`!titel` ohne jede Antwort | `tb-chat/src/commands.rs:339` | Mod/Broadcaster bekommt null Reaktion unter Takeover | medium² |
| Interne API | chat-action 503 (Stub schattet Proxy aus) | `handlers/python_stubs.rs:58-71` | Admin-Chat-Aktion via interne API schlägt hart fehl | medium |
| Interne API | raid/requirements 503 (Stub statt Proxy) | `handlers/python_stubs.rs:85-103` | Raid-Onboarding-Discord-DM wird nicht versendet | medium |
| Interne API | verify clear/failed = 503 (Partner-Departnering nativ nicht möglich) | `handlers/streamers.rs:399-405` | Departnering über interne API blockiert | medium |

² severity vom Verdict auf medium korrigiert (mod-only, low-traffic, Dashboard-Alternative lebt).

### (C) Stille Regressionen in portiertem Code (Feld/Edge verloren)

| Subsystem | Item | Datei:Zeile | User-Impact | Severity |
|---|---|---|---|---|
| Raid | Arrival-Followup-Flags berechnet, aber NIE konsumiert: Partner-Raid-Nachricht, Recruitment-Nachricht, Externe-Recruitment-Persist+Auto-Blacklist (Schwelle 4) | `arrival_confirmation.rs:432-436` berechnet, `raid_arrival_wiring.rs:256-370` droppt sie | Partner-Dank/Shoutout fehlt; Recruitment-Funnel tot; kein Auto-Blacklist gegen Raid-Spam | high |
| Monitoring | first_message setzt `confirmed_first_ever=TRUE` nicht | `telemetry.rs:384-417` vs py `eventsub_mixin.py:2461-2469` | Analytics-Feld verliert Daten, still | medium |
| Interne API | discord-profile: reiner DB-Write ohne Helix-Lookup + Discord-Rollen-Sync | `handlers/streamers_crud.rs:592-651` vs py `streamer_admin_mixin.py:212-288` | Rollen-Zuweisung+twitch_user_id-Auflösung entfällt | medium |
| Interne API | /stats EventSub-Sektion hardcoded leer (eventsub_stats=None) | `tb-bot/src/main.rs:682` | Dashboard ohne EventSub-Sub-Metriken | medium |
| Monitoring | Offline-Throttle VOR Engagement-Auto-Off (Python danach) | `handlers.rs:236-252` vs py `eventsub_mixin.py:1861-1869` | Bei 2. Offline in 120s wird Engagement-Off übersprungen | low |
| Entitlements/Billing | Trial-Auto-Grant 24h nach first_login fehlt nativ (nur Self-Claim portiert) | `tb-analytics/src/trial.rs` vs py `billing_mixin.py:1110-1168` | Passive Neu-Streamer bekommen Trial nicht automatisch (nur im nativen Pfad) | medium |
| Entitlements/Billing | current_period_end ohne Spalten-Fallback (Python fail-open) | `plan.rs:222-241` | Auf Alt-Schema fail-closed statt Plan liefern | low |
| Social Media | Clip-Fetcher Partner-Selektion über Roh-Tabelle statt `is_partner_active`-View | `clip/repository.rs:20-33` vs py `clip_fetcher.py:80-87` | Andere Streamer-Menge — folgenlos solange Fetcher aus | low |
| Live-Announce | Retry rendert aktuellen Tick statt Erstversuch-Payload | `announce/sink.rs:197-238` vs py `monitoring.py:382-469` | Embed-Inhalt kann abweichen (Idempotenz bleibt); nur bei aktivem Rust-Pfad | low |
| Live-Announce | Auto-Erstellung Live-Ping-Rolle nicht portiert | py `embeds_mixin.py:505+` | Partner ohne vorab gesetzte Rolle kein Ping; nur bei Flip | medium |
| Chat | `!raid_enable` sendet "Kontaktiere Admin" statt OAuth-Link | `commands.rs:589-593` vs py `commands.py:94` | Kein klickbarer Auth-Link | low |
| Chat | Unberechtigte Mod-Commands still geschluckt (kein Ablehnungs-Reply) | `commands.rs:275-308` vs py `commands.py:626` | Keine Rückmeldung bei fehlender Berechtigung | low |

## 4. Top-Prioritäten (User-Impact × Aufwand)

1. **Engagement-KI-Kern reaktivieren oder explizit entscheiden** (critical, mittlerer Aufwand) — Der KI-Stammgast schweigt seit dem Chat-Flip komplett. Pipeline-Schritt 11 ist No-op, kein Proxy fängt EventSub-Chat ab. Entweder Kern portieren (MiniMax-Pipeline + Threads + Persona) oder bewusst als deferred kommunizieren. **Höchster echter User-Impact heute.**

2. **Raid-Arrival-Followup-Nachrichten verdrahten** (high, geringer Aufwand) — Die 4 Entscheidungs-Flags sind bereits berechnet (`arrival_confirmation.rs:432-436`), nur der Sink (`raid_arrival_wiring.rs:256`) konsumiert sie nicht. Partner-Dank-Nachricht, Recruitment-Funnel und Externe-Auto-Blacklist (Schwelle 4) sind still tot. **Bestes Aufwand/Nutzen-Verhältnis** — Flags existieren, nur Sink-Hooks + Send-Pfad fehlen.

3. **Super-Mod-Toggle nativ implementieren** (high, sehr geringer Aufwand) — NoopSuperMod (`chat_wiring.rs:685-693`) durch echte DB-Query auf `twitch_admin_roles role='super_mod'` ersetzen. Eng begrenzt, aber konkrete Berechtigungs-Regression unter dem heute aktiven Chat-Pfad.

4. **first_message `confirmed_first_ever`-Update nachziehen** (medium, trivial) — `telemetry.rs:384-417` um das fehlende UPDATE im selben TX ergänzen. Stiller Analytics-Datenverlust, der heute im nativen Receiver aktiv ist. Ein-Statement-Fix.

5. **discord-profile Seiteneffekte wiederherstellen** (medium, mittlerer Aufwand) — Nativer Handler macht nur DB-Write; Helix-Lookup (twitch_user_id) und Discord-Rollen-Sync (`sync_streamer_role`) fehlen. Admin-Flow degradiert still auf reinen DB-Zustand.

6. **`!title` Chat-Command portieren** (medium, gering) — MiniMax-M3 ist in Rust schon da (`scam_pitch.rs:127`), nur der Title-Use-Case fehlt. `TitlePort`-Trait andocken (`commands.rs:1224`). Mod-only/low-traffic, aber sichtbar tot.

7. **Telemetrie-/Moderator-Sub-Anlage nativ portieren** (high, mittel/hoch — aber zukünftig) — Kein heutiger Defekt, aber harter Cutover-Blocker: bei Python-Abschaltung fielen alle Telemetrie-Subs UND der channel.moderate-Blacklist-Raid-Guard aus. Muss VOR jedem Python-Cutover stehen.

8. **`/stats` EventSub-Sektion befüllen** (medium, gering) — `main.rs:682` eventsub_stats=None durch echte native Quelle ersetzen; Admin-Dashboard zeigt sonst still leere Sub-Metriken.

## 5. Ehrliche Abgrenzung

**Bewusst zurückgestellt / unkritisch (kein Fix nötig jetzt):**
- **Coaching-Audit-CLI**: Admin-Forensik-Tool, war nie im Bot-Runtime, kein User-Impact. 0% portiert, aber korrekt low.
- **Voice-Reaction**: default-OFF auch in Python (`scheduler.py:80-81`), dokumentiert bis Phase 6g aufgeschoben. Severity vom Verdict medium→low korrigiert.
- **Highlight-Clipper**: läuft ungated voll auf Python, Rust nicht beteiligt — Migrations-Risiko (high) aber **null aktueller Funktionsverlust**.
- **Social-Media-Stack**: critical bezieht sich rein auf Migrationsgrad; User merkt nichts, solange Python-Bot mitläuft.
- **Live-Announce + Monitoring-Poll-Loop**: nativ fertig portiert, aber per Flag aus (TB_MONITORING_POLL_ENABLED default false) — Python besitzt es exklusiv, kein User-Impact. Beide vom Verdict high→medium korrigiert.
- **Dashboard-Proxy-Routen (AI/Coaching/Admin)**: funktionieren transparent via Proxy. "Unvollständig", nicht "kaputt".
- **EventSub-WebSocket-Transport**: bewusst nicht portiert (ADR 0004, Webhook-only). WS-Capacity-Felder immer 0 — by design.

**Von der Verifikation als Falsch-Positiv verworfen:**
- **Chat `!invite` "stoppt Pipeline"** — FALSCH: `handle()`-Return wird bei `pipeline.rs:546` verworfen, Commands laufen zuletzt, nichts wird gestoppt. Gültig bleibt nur der schmale `is_deadlock_live`-Gate-Unterschied.
- **Monitoring channel.raid "nicht durable"** — FALSCH-POSITIV: Pythons Webhook fuhr raid ebenfalls inline (delivery_mode="inline"); die durable Inbox-Enqueue gilt nur im nicht-portierten WS-Pfad. Rust ist bei Core-Typen sogar durabler.
- **Monitoring Offline-Ordering / Global-Ban-Sweep** — Sweep-Anteil FALSCH: beide planen den Sweep nach dem Throttle (Parität). Nur Engagement-Auto-Off ist echte (kleine) Regression.
- **Raid `/raid/requirements` "läuft via Proxy"** — Eval-Behauptung korrigiert: Route ist an nativen 503-Stub gebunden, NICHT geproxyt (`lib.rs:143` ist stale Doc-Kommentar). Real: dead/noop, nicht proxied.
- **Community Admin-Streamer-Mutationen "via Python-Proxy"** — FALSCH für die HTTP-Schicht: add/remove/list sind nativ (`lib.rs:221-226`). Nur die discord.py-Cog-Command-Schicht ist unportiert.
- **Interne API discord-FLAG Rollen-Sync** — FALSCH-POSITIV: Python-flag-Callback ist reiner DB-Write, kein Rollen-Sync; Rust verhaltensgleich. (Nur discord-PROFILE hat die echte Regression.)
- **Billing fehlende Snapshot-Felder (status/manual_override)** — kein Regressions-FP, aber zu Recht low: die native auth-status-Route hat volle Parität mit Python (`api_v2.py:195-207` emittiert dieselben 7 Felder); die reichen Felder speisen nur proxied Endpoints.
- **Social-Media `days=7`-Filter** — KEINE Regression: bereits in Python tot (`clip_manager.py:148-153` setzt keinen started_at-Filter), Rust verhaltensgleich.
- **Live-Announce channel_avatar leer** — Gleichstand, kein Verlust: in beiden Implementierungen praktisch leer (Helix /streams liefert profile_image_url nicht).
