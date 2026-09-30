# Evidence: Bestand Moderation + Dashboard

## Moderationsfunktionen (Pipeline, nur Partner)
- Pipeline-Kern: `rust/crates/tb-chat/src/pipeline.rs:745` `ChatPipeline::process`
- Non-Partner-Abbruch: `rust/crates/tb-chat/src/pipeline.rs:872` (nur Tracking, keine Moderation)
- Schritt 5 Global-Chatter-Ban: `rust/crates/tb-chat/src/pipeline.rs:931` → `execute_auto_ban_with_evidence` → `rust/crates/tb-chat/src/moderation.rs:571` (Delete + Ban)
- Schritt 6 Scam-Pitch: `rust/crates/tb-chat/src/pipeline.rs:986` `scam_pitch.observe`; `PitchDecision` Warn/StrongWarn = nur Warnung+Discord, `StrongTimeout` = Timeout via `handle_strong_timeout`
- Schritt 7 Spam-Score + Auto-Ban: `rust/crates/tb-chat/src/pipeline.rs:1044` `run_spam_check` → `rust/crates/tb-chat/src/spam_filter.rs:742` `evaluate`; `SpamAction::Ban/DeleteOnly/None` `spam_filter.rs:8`
- Schritt 8 Sus-Discord-Invite: `rust/crates/tb-chat/src/pipeline.rs:1068` `handle_sus_invite` → Timeout `pipeline.rs:1184`
- Aktions-Primitive: `rust/crates/tb-chat/src/moderation.rs` `ban_user:269`, `timeout_user:282`, `delete_message:327`, `auto_ban_and_cleanup:549`, `timeout_and_cleanup:711`

## Vorhandene Toggles (verstreut, keine zentrale Tabelle)
- EINZIGER echter Moderations-Toggle: Tabelle `twitch_scam_guard_settings` (`enabled, mode, threshold, suggestion_floor`); Loader `PgScamGuardStore::load_settings` `rust/crates/tb-chat/src/conversation_scam.rs:676`; Struct `GuardSettings` `conversation_scam.rs:120`, Default `conversation_scam.rs:145`
- Feature/Command-Schalter (keine Moderation): `streamer_plans.lurker_tax_enabled` u.a.; `twitch_engagement_settings.output_mode`
- Für Global-Ban / Scam-Pitch / Spam-Auto-Ban / Sus-Invite: KEINE Kanal-Config, hart aktiv sobald Partner

## Backend (tb-dashboard-api)
- Router: `rust/crates/tb-dashboard-api/src/lib.rs:94`, Selbstbedienungs-Block ab `lib.rs:360`
- Auth: `DashboardAuthLevel` `rust/crates/tb-dashboard-api/src/auth/level.rs:52` (Admin/Partner/Unauthenticated); `resolve_login(auth, streamer)` `handlers/scam_guard_settings.rs:36`
- Blaupause-Handler: `rust/crates/tb-dashboard-api/src/handlers/scam_guard_settings.rs` (GET/POST, Upsert)

## Frontend (dashboard_v2)
- App: `bot/dashboard_v2/`, Seite `bot/dashboard_v2/src/pages/Verwaltung.tsx`
- Sektionen: `bot/dashboard_v2/src/components/verwaltung/` (`ScamGuardSection.tsx` = Vorbild, Toggle `ScamGuardSection.tsx:151`)
- API-Clients: `bot/dashboard_v2/src/api/scamGuard.ts` (`BASE = '/twitch/api/v2/streamer/scam-guard'`)

## Einschätzung
Load-Bearing: Pipeline muss per-Kanal-Feature-Config lesen (heute nicht vorhanden, Hot-Path, Cache Pflicht). Schema + Handler + Frontend folgen strikt dem Scam-Guard-Muster. Zentrale Tabelle `twitch_moderation_settings` statt Erweiterung von `streamer_plans`, um die zusammengehörige Gruppe beisammenzuhalten und das Scam-Guard-Muster zu spiegeln.
