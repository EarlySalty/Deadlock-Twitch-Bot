# 02 — DB-Vertrag (PostgreSQL)

Während der Migration teilen Python und Rust **dieselbe** DB. Das Schema ist ein Vertrag und
wird nicht gebrochen (siehe [`adr/0002`](adr/0002-db-sqlx-refinery-shared-schema.md)). Owner =
Subsystem, das Schema + primären Schreibpfad besitzt; viele Tabellen werden cross-gelesen.

> Inventar aus dem Ist-Zustand-Mapping abgeleitet. **Vor Phase 0** wird es gegen das Prod-Schema
> (read-only) verifiziert und vervollständigt — gilt bis dahin als „belegt, aber zu bestätigen".

## Identität / Partner / Plan

| Tabelle | Owner | Mitnutzer (read) |
|---|---|---|
| `schema_version` | storage | — |
| `twitch_streamers` | storage | chat, monitoring, dashboard, analytics, social-media |
| `twitch_streamer_identities` | storage | monitoring, raid, analytics *(redundanter 2. Identity-Store)* |
| `twitch_partners` | storage | raid, monitoring, analytics, dashboard, internal-api |
| `twitch_partners_all_state` *(View)* | partner_registry | analytics, dashboard |
| `twitch_streamers_partner_state` *(View)* | storage | chat, monitoring, raid, analytics, dashboard, billing, social-media |
| `streamer_plans` | storage | raid, billing, dashboard, analytics, chat |
| `twitch_subscriptions_snapshot` | analytics-core | community-coaching |

## Live / Sessions / Stats

| Tabelle | Owner | Mitnutzer |
|---|---|---|
| `twitch_live_state` | monitoring | chat, raid, dashboard, analytics |
| `twitch_stream_sessions` | monitoring | analytics, dashboard, raid, community |
| `twitch_session_viewers` | monitoring | analytics, dashboard |
| `twitch_session_chatters` | chat (irc-lurker) | monitoring, analytics |
| `twitch_chatter_rollup` | chat | analytics, community |
| `twitch_chat_messages` | chat | analytics, dashboard, community |
| `twitch_chat_word_groups` | analytics-api | — |
| `twitch_viewer_presence_ticks` | monitoring | analytics |
| `twitch_stats_tracked` | monitoring | analytics, dashboard, raid, community *(Time-Series, ohne PK)* |
| `twitch_stats_category` | monitoring | analytics, dashboard, raid, community *(Time-Series, ohne PK)* |
| `twitch_first_message_events` | monitoring | — |
| `twitch_raw_chat_ingest_health` | chat | analytics |
| `twitch_raw_chat_backfill_runs` | chat | — |
| `exp_sessions` / `exp_snapshots` / `exp_game_transitions` | monitoring *(experimentell)* | analytics-api/exp |

## EventSub-Infrastruktur

| Tabelle | Owner |
|---|---|
| `twitch_eventsub_processing_inbox` / `_dead_letter` | monitoring |
| `twitch_eventsub_bridge_outbox` / `_dead_letter` | dashboard_service |
| `eventsub_guard_state` | monitoring |
| `twitch_eventsub_capacity_snapshot` | monitoring |

## Channel-Events (Telemetrie)

| Tabelle | Owner | Mitnutzer |
|---|---|---|
| `twitch_bits_events`, `_hype_train_events`, `_subscription_events`, `_ad_break_events`, `_ban_events`, `_shoutout_events`, `_follow_events`, `_channel_points_events`, `_channel_updates` | analytics-core (Schreiber via EventSub) | analytics-api, dashboard |
| `twitch_ads_schedule_snapshot` | analytics-core | analytics-api |

## Raid

| Tabelle | Owner | Mitnutzer |
|---|---|---|
| `twitch_raid_auth` | raid | analytics, dashboard, internal-api, monitoring *(AES-256-GCM verschlüsselt)* |
| `oauth_state_tokens` | raid | **social-media (geteilt!)** |
| `twitch_partner_raid_scores` / `_score_tracking` | raid | monitoring |
| `twitch_raid_arrival_tracking` | raid | analytics |
| `twitch_raid_history` | raid | analytics, dashboard, monitoring |
| `twitch_raid_blacklist` | raid | chat, monitoring, internal-api, dashboard |
| `twitch_raid_retention` | analytics-core | analytics-api |
| `twitch_raid_disabled_strikes` | raid | — |
| `twitch_auto_raid_pause` | storage | raid |
| `twitch_confirmed_external_recruitment_raids`, `_external_recruitment_blacklist_pending`, `_external_bot_ban_check_pending` | raid | — |
| `twitch_partner_outreach`, `_outreach_conversations`, `_outreach_audit` | raid | — |
| `twitch_token_blacklist` | internal-api (token_error_handler) | raid, analytics |

## Moderation / Global-Ban / Spam

| Tabelle | Owner |
|---|---|
| `twitch_chatter_global_ban` / `_global_ban_applied` | storage (Global-Ban-API) |
| `twitch_global_ban_sweep_due` | chat |
| `twitch_outbound_chat_suppressions` | chat |
| `twitch_auto_learned_spam_patterns` / `_safe_patterns` | chat (spam_ai_review) |
| `twitch_promo_cooldowns` | promo_cooldowns |

## Discord / Invites / Announce / Settings

| Tabelle | Owner |
|---|---|
| `discord_invite_codes` | runtime-cog-discord |
| `twitch_streamer_invites` | storage |
| `twitch_guild_settings` | runtime-cog-discord |
| `twitch_live_announcement_configs` | dashboard/live-announce |
| `twitch_global_promo_modes` | storage |
| `twitch_global_settings` | storage |
| `twitch_link_clicks` | monitoring/dashboard |

## Billing / Affiliate

| Tabelle | Owner |
|---|---|
| `twitch_billing_subscriptions` / `_profiles` / `_events` | billing |
| `affiliate_accounts`, `_pii`, `_streamer_claims`, `_commissions`, `_gutschriften`, `_gutschrift_counter` | billing/affiliate |

## Social-Media

| Tabelle | Owner |
|---|---|
| `twitch_clips_social_media` / `_upload_queue` / `_social_analytics` | social-media |
| `social_media_platform_auth` *(AES-256-GCM)*, `_settings`, `_streamer_layout`, `_clip_enrichment`, `_clip_approval`, `_reauth_notifications`, `_reports` | social-media |
| `clip_templates_global` / `_streamer`, `clip_last_hashtags`, `clip_fetch_history` | social-media |
| `deadlock_vocab` | social-media |

## Dashboard / AI / Sonstiges

| Tabelle | Owner |
|---|---|
| `dashboard_sessions` | storage *(Fernet-verschlüsselt — Migrations-Entscheid offen)* |
| `twitch_self_explainer_log` | dashboard |
| `internal_home_changelog` | analytics *(DDL aktuell in Handlern → in Migration ziehen)* |
| `twitch_stream_ai_reports` / `_report_ratings` / `_report_ab_votes` | analytics-api |
| `ai_analyses` | analytics-api |
| `twitch_roadmap_items` | analytics-api |
| `twitch_engagement_settings` / `twitch_user_engagement_optout` | engagement (von chat genutzt) |

## Bekannte Vertrags-Schulden (Schema halten, sauber dokumentiert)

- **`twitch_stats_tracked` / `_category`**: PK-los, strukturgleich, dupliziert — Kandidat für
  Konsolidierung/Hypertable, aber Schema bleibt während der Migration stabil.
- **`twitch_streamers` vs `twitch_streamer_identities`**: zwei Identity-Stores, redundant.
- **`oauth_state_tokens`**: von **raid und social-media** geteilt — beim getrennten Cutover muss
  der `platform`-Discriminator stabil bleiben, sonst stören sich Rust-raid und Python-social-media.
- **Verschlüsselte Spalten**: `twitch_raid_auth`, `social_media_platform_auth` (AES-256-GCM),
  `dashboard_sessions` (Fernet) — Interop-Klärung vor dem jeweiligen Cutover, siehe
  [`06-open-questions.md`](06-open-questions.md).
