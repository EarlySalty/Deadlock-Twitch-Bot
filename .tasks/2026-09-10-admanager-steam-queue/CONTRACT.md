# Contract: Werbemanager legt Werbung in die Queue-Phase (Steam-Match-Status)

status: aktiv
datum: 2026-09-10
klasse: mittel
repo: Deadlock-Twitch-Bot

Dieser Contract ist der Maßstab für Implementierung und Merge-Kritiker. Nach dem
Anlegen ist er unveränderlich: der Hook lässt nur noch die `status:`-Zeile und
Anhänge unter `## Amendments` zu.

## Ziel

Die Smart-Strategie des Twitch-Werbemanagers nutzt den Steam-Match-Status des
Streamers, statt nur auf Chat-Ruhe zu achten: Läuft gerade ein Deadlock-Match,
verschiebt der Bot die fällige Twitch-Werbung (Snooze). Ist der Streamer
nachweislich nicht im Match (Queue, Menü), startet der Bot die fällige Werbung
kontrolliert in genau diesem Fenster. Ohne frischen Steam-Status gilt das
heutige Chat-Ruhe-Verhalten unverändert als Fallback. Der Streamer muss dafür
seine Steam-ID hinterlegt haben; das UI zeigt den Zustand und sagt, was fehlt.

Voraussetzung (User, 2026-09-10): „Müssen sie ihr Steam verbunden haben, dass
der Steam-Bot schauen kann, wie der Status ist, ob sie im Match sind oder
nicht, und die Werbezeiten kennen und dann smart anpassen, dass man Werbung
dann hat, wenn es am wenigsten nervt."

## Anforderungen (user-sichtbares Verhalten)

- REQ-01 Werbezeiten-Kenntnis bleibt Basis: Der Worker liest den Twitch-Ad-Plan
  (next_ad_at, snooze_count) wie bisher und handelt im Vorlauf-Fenster vor der
  nächsten geplanten Werbung. An diesem Rahmen ändert sich nichts.
- REQ-02 Match-Status als Entscheidungsquelle: Liest der Worker einen frischen
  Steam-Status (`activity.live_player_state` über die in
  `twitch_engagement_settings.steam_id` hinterlegte Steam-ID, Frische
  `COALESCE(deadlock_updated_at, last_seen_at)`, Schwelle 3 Minuten), gilt:
  `in_match_now_strict = true` → Snooze, solange Snoozes verfügbar sind
  (Grund „in_match"); ohne Snoozes keine Aktion (Grund „in_match_no_snooze").
- REQ-03 Queue-Fenster: Frischer Status mit `in_match_now_strict = false` gilt
  als Werbefenster: Der Bot startet die Werbung selbst (Commercial in der
  eingestellten Dauer), wenn der Mindestabstand (min_interval_minutes) seit der
  letzten Werbung eingehalten ist (Grund „in_queue"); ist der Mindestabstand
  noch nicht erreicht, wird stattdessen gesnoozt, sofern verfügbar (Grund
  „in_queue_cooldown"), sonst keine Aktion.
- REQ-04 Fallback unverändert: Kein Steam-Link, keine steam_id, veralteter
  Status oder DB-Fehler am Lookup → exakt die heutige Chat-Ruhe-Logik
  (quiet_chat_messages / chat_ingest_healthy) in unveränderter Reihenfolge.
- REQ-05 Startschutz bleibt vorgeschaltet: Innerhalb der
  startup_delay_minutes nach Streamstart wird weiter gesnoozt bzw. nichts
  getan, auch bei frischem Queue-Status. Der chat_ingest-Gesundheitscheck
  fegt nur im Fallback (REQ-04); mit frischem Steam-Status ist der
  Match-Status die alleinige Entscheidungsquelle.
- REQ-06 Snooze- und Monitor-Strategie unverändert: „Werbung möglichst
  verschieben" snoozt weiterhin jede fällige Werbung unabhängig vom
  Match-Status; Monitor bleibt rein beobachtend.
- REQ-07 Status im Dashboard: Der Status-Response des Werbemanager-Endpoints
  enthält den Steam-Block (verbunden ja/nein, Zustand „im Match" / „in Queue
  oder Menü" / „nicht in Deadlock" / „zu alt", Held, Stage, Beobachtet-Um).
- REQ-08 UI zeigt den Match-Status: Im Verwaltung-Tab „Werbung" gibt es eine
  Status-Karte mit dem Match-Zustand; ohne hinterlegte Steam-ID erscheint ein
  Hinweis mit Verweis auf den bestehenden Ort der Steam-ID-Pflege
  (KI-Engagement-Bereich). Die Smart-Strategiebeschreibung nennt das
  Queue-Verhalten wahrheitsgemäß. Keine neuen Einstellungs-Felder.
- REQ-09 Doku: Die Streamer-Dokumentation, die den Werbemanager beschreibt,
  nennt das Queue-Verhalten und die Voraussetzung Steam-ID wahrheitsgemäß.

## Invarianten (darf sich nicht ändern)

- INV-01: Kein DB-Schema-Wandel: keine Migration, bestehende Tabellen
  `twitch_ad_manager_*` bleiben unverändert; `twitch_engagement_settings` und
  `activity.live_player_state` werden ausschließlich lesend verwendet.
- INV-02: Monitor/Snooze-Verhalten und alle Helix-Aufrufmuster (Lead-Fenster,
  Acht-Minuten-Sperre, mark_unknown_before_send, Reconcile) bleiben unverändert.
- INV-03: Der Unknown-Fallback ist inhaltlich identisch zur heutigen Logik;
  bestehende decide()-Tests bleiben unverändert grün.
- INV-04: Keine neuen ENV-Flags, Secrets oder `*_ENABLED`-Schalter; der
  Fallback macht den Steam-Ausfall unkritisch, ein weiteres Gate ist unnötig.
- INV-05: Manuelle Aktionen (Snooze/Werbung starten im Dashboard) bleiben
  unverändert.
- INV-06: Keine Änderung an tb-engagement, tb-chat, website/ oder dem
  Chat-Werbe-Modul (faq-werbung.md beschreibt die Chat-Werbung, bleibt unangetastet).
- INV-07: Bestehende Tests werden nicht gelöscht oder abgeschwächt.
- INV-08: Keine Werbeaktion außerhalb des Lead-Fensters; der Bot „holt" Werbung
  nicht nach vorn außerhalb des Fensters vor der fälligen geplanten Werbung.
- INV-09: Der Worker verursacht ohne Match-Status keine zusätzlichen
  Helix-Aufrufe; der Steam-Lookup ist ein lesender DB-Zugriff pro Kanal und Tick.

## Nicht-Ziele

- Kein eigener Match-Poller im Werbemanager; Quelle bleibt die vom Steam-Bot
  gepflegte `activity.live_player_state` bzw. der bestehende Engagement-Poller
  bleibt unberührt.
- Keine Pflege der Steam-ID im Werbemanager-UI (bestehender
  Engagement-Bereich bleibt der einzige Pflegeort).
- Keine Änderung der Chat-Werbung, des Werbefrei-Plans oder der Billing-Logik.
- Keine Garantie „gar keine Werbung": Twitch begrenzt Snoozes (drei je Stream,
  Nachschub zeitgesteuert); der Bot kann Twitch-Zwangswerbung nicht abschalten.

## Erlaubter Änderungsbereich

- rust/crates/tb-analytics/src/ad_manager.rs
- rust/bin/tb-bot/src/ad_manager_wiring.rs
- rust/crates/tb-dashboard-api/src/handlers/ad_manager.rs
- bot/dashboard_v2/src/api/adManager.ts
- bot/dashboard_v2/src/components/verwaltung/AdManagerSection.tsx
- docs/streamer/ (falls dort der Werbemanager beschrieben ist)
- .tasks/2026-09-10-admanager-steam-queue/

## Verbotene Änderungen

- rust/crates/tb-engagement/
- rust/crates/tb-chat/
- rust/migrations/
- website/
- bot/admin_dashboard/
- Lint- und CI-Konfiguration

## Offene Produktfragen

- keine

## Amendments

- 2026-09-10, REQ-03, Commercial im Queue-Fenster auch ohne frischen
  Chat-Ingrest: Der chat_ingest-Gesundheitscheck bleibt im Unknown-Fallback
  und bei Startschutz; mit frischem Steam-Status entscheidet allein der
  Match-Status. Grund: Der Steam-Status ist von der Chat-Pipeline unabhängig;
  genau das ist der Sinn der Erweiterung. entschieden von Orchestrator
- 2026-09-10, REQ-08, erlaubter Bereich erweitert um bot/dashboard_v2/src/components/verwaltung/AIEngagementSection.tsx und bot/dashboard_v2/src/api/engagement.ts: Fuer steamId gibt es nur die API, kein UI; ohne Eingabefeld im bestehenden KI-Engagement-Bereich gibt es keinen Selbsthilfe-Weg fuer die geforderte Steam-Verbindung. entschieden von Orchestrator
- 2026-09-10, erlaubter Bereich erweitert um rust/crates/tb-analytics/tests/: Der isolierte DB-Lauf von ad_manager_store ist vor der Aenderung rot (Migration 20260901100000 alteriert twitch_raw_chat_ingest_health, der Test legt die Basistabelle nicht an); Fix ist ein hermetischer Test-Aufbau, keine Abschwaechung. entschieden von Orchestrator
- 2026-09-10, erlaubter Bereich ergaenzt um bot/dashboard_v2/src/preview/fixtures.ts und bot/dashboard_v2/tests/adManager.test.ts: Der neue Steam-Block ist Teil des Statusvertrags, das Preview-Fixture und die Tests muessen ihn abbilden, sonst waere der Vertrag im Preview und im Test unvollstaendig. entschieden von Orchestrator
