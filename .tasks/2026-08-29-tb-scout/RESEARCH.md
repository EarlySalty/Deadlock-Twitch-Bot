# Research: tb-scout

status: aktiv
datum: 2026-08-29

## DB-Befunde (Live-Datenbank `twitch_analytics`, gelesen via Infisical-Wrapper, 2026-08-29)

- Erfolgsfmuster Nutzer: 182 bestätigte externe Recruitment-Raids auf 113
  Ziele (2026-03-28 bis 2026-08-28), 177 davon an Kanäle mit unter 10
  Zuschauern, Ø 1,7 bis 5,3 Zuschauer bei den Top-Zielen, wiederholt 3 bis 7
  Raids je Ziel. 12 der 113 Raid-Ziele sind heute Partner (10,6 Prozent).
- Invites: 94 Empfänger in `twitch_streamer_invites`, letzter Versand
  2026-08-28 (aktiv), 66 davon heute Partner. Der Invite-Weg ist der
  tragende Erfolgs_pfad, Raids der Zweitweg.
- Scout-Versuch existierte schon: `twitch_scout_pitch_ledger`, 301 Einträge
  nur am 2026-07-14/15, Trigger `new_streamer|offline_moment|problem_moment`,
  Aktionen `posted` (7), `suppressed_cooldown` (254), `judge_error` (9).
  Danach still. Es gibt `twitch_scout_pitch_blacklist` (Login, Grund).
- Outreach-Bestand: `twitch_partner_outreach` 57 Zeilen (56 "sent"),
  letzter Kontakt 2026-04-29. Tabelle ist text-lastig (Status als text).
- Verdrahtungsbefund: `streamer_dim` hat 0 Zeilen; Joins darauf liefern
  leer. Echter Partner-Bestand: `twitch_partners` (67 Zeilen) und
  `twitch_streamers_partner_state` (60).
- Kandidatenpool "klein + first_seen" ist aus Bestandstabellen ableitbar:
  `twitch_stream_sessions` (Spalten u. a. streamer_login, started_at,
  avg_viewers, language, had_deadlock_in_session, twitch_user_id) und
  `twitch_stats_category` (Ticks; MIN(ts_utc) = first_seen). Beispielabfrage
  lieferte täglich frische Kleinkanäle (2026-08-25 bis 28), 0 bis 7 Ø-Zuschauer.

## Code-Befunde (alle Pfade relativ zu ~/repos/Deadlock-Twitch-Bot)

- Research-Frontend: `bot/admin_dashboard/src/pages/community/Research.tsx`
  (Onboarding-Vorschläge :290, "Analysieren" :324-331, TanStack Query :213-222);
  Route `community/research` in `bot/admin_dashboard/src/App.tsx:69`;
  Client-Funktionen `bot/admin_dashboard/src/api/client.ts:1333-1341`;
  Backend-Routen `rust/crates/tb-dashboard-api/src/lib.rs:819-824` (admin-gated).
- Kandidatenpool heute: `SUGGESTIONS_SQL` in
  `rust/crates/tb-dashboard-api/src/handlers/admin_research.rs:165-191`
  (Quelle `twitch_stats_category`, Sessions on-the-fly per LAG/30-min-Gap,
  nur `MAX(ts_utc) AS last_seen`, Kappung auf 12 Einträge in :547).
  Kein first_seen vorhanden.
- Ansprech-Kette heute: `rust/bin/tb-bot/src/partner_recruit.rs` erkennt
  häufige Streamer (≥4 Tage/28d, ≤40 Ø-Zuschauer) und reiht sie mit Cooldown
  in `twitch_partner_outreach` ein, KEIN kalter Chat-Erstkontakt
  (Dateikopf :1-6); Limits 8/Tag, 3/Tick, 60 s Abstand (:16-22); Enqueue
  `enqueue_partner_outreach` :175, Suppression-Check `source='recruitment'`
  :200-207; Tick-Aufruf `run_partner_recruit` :220.
  Raid-Arrival-Recruitment: `rust/bin/tb-bot/src/raid_arrival_wiring.rs:794-902`
  (Gates :806-829, Versand :902, Bot-Ban-Check :906-910), Planner
  `rust/crates/tb-raid/src/recruitment_messaging.rs:161,292`.
- Sperrlisten: Denylist `rust/crates/tb-analytics/src/partner_signup_block.rs`
  (check :382, add :113, Sync in Raid-Blacklist :188-193); Raid-Blacklist
  `rust/crates/tb-raid/src/raid_blacklist.rs` (is_blacklisted :26-42);
  globaler Ban `is_hard_banned` `raid_blacklist.rs:50-63`.
- Approval-Muster zum Kopieren: `rust/crates/tb-social-media/src/approval.rs:3-19`
  (`awaiting_approval → approved/skipped`, approver_user_id, decided_at,
  Tabelle `social_media_clip_approval`).
- Negativevidenz: keine Freigabe-Tabelle für Recruitment-Ziele, kein
  Safelist-/Waitlist-Baustein im Rust-Stack, keine first_seen-Spalte auf
  Kanalebene, kein szene-stats-Job im Repo (nur Prosa-Treffer).

## Hypothesen (nicht belegt, im Plan nicht als Fakt verwenden)

- Der stiller Scout-Pitch von 2026-07-14 wurde wegen Lautstärke abgestellt
  (254 Cooldown-Suppressionen an einem Tag) — Grund ungeklärt, für Slice 1
  ohne Bedeutung, weil kein Pitch-Pfad gebaut wird.
