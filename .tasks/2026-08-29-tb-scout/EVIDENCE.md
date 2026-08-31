# Evidence: tb-scout

status: aktiv
datum: 2026-08-29

Analoge Implementierungen und Schnittstellen (Mindestens 3, alle als pfad:zeile):

1. Approval-Zustandsmaschine (Vorlage für `twitch_scout_candidates`):
   `rust/crates/tb-social-media/src/approval.rs:3-19` — Zustände
   awaiting_approval → approved/skipped, approver_user_id, decided_at;
   Schema-Bezug `rust/crates/tb-dashboard-api/src/handlers/social_media.rs:4038`.
2. Admin-Handler-Muster mit Auth und CSRF:
   `rust/crates/tb-dashboard-api/src/handlers/admin_research.rs:408` (handler)
   und `:482` (suggestions_handler), Routen-Anmeldung
   `rust/crates/tb-dashboard-api/src/lib.rs:819-824`; CSRF-Beispiel für
   POST-Handler: `rust/crates/tb-dashboard-api/src/handlers/admin_partner_signup_block.rs`
   (require_admin_before_csrf plus csrf_protect, siehe
   Docs/workspace/community-site.md).
3. Kandidaten-Erkennung mit Gates (Vorlage für die Scout-Query):
   `rust/bin/tb-bot/src/partner_recruit.rs:48-93` (detect_recruit_candidates:
   Partner-Ausschluss per NOT EXISTS, Cooldown-Ausschluss,
   Identitäten-Ausschluss) und `:95-141` (Tages-/Tick-Deckel, Enqueue).
4. Sperrlisten-Prüfung: `rust/crates/tb-raid/src/raid_blacklist.rs:26-63`
   (is_blacklisted, is_hard_banned fail-closed);
   `rust/crates/tb-analytics/src/partner_signup_block.rs:382-398` (check).
5. Testmuster gegen echte PG: `rust/bin/tb-bot/src/partner_recruit.rs:418`
   (setup mit Schema, seed-Helper :456, Verhaltenstests :469, :493);
   `rust/crates/tb-dashboard-api/src/handlers/admin_research.rs:562-650`
   (pool_or_skip, request-Helper).
6. Frontend-Tab im bestehenden Dashboard:
   `bot/admin_dashboard/src/pages/community/Research.tsx:213-222` (useQuery),
   `:290` (Vorschläge-Tabelle), `bot/admin_dashboard/src/api/client.ts:1333-1341`.
