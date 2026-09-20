# Twitch: erste Klassifikation der direkten Quellzugriffe

Quelle: reines Quellinventar des Originalkandidaten, keine Betriebsdateien oder Umgebungswerte. Ausgangsinventar 20.09.2026 02:57 Uhr. Dieses Dokument ersetzt keine Werteübernahme. Öffentlich verwendete Client-IDs verbleiben beim bestehenden Infisical-Credential-Paar, um die Auth-Grenze nicht aufzuteilen.

## Zählung

- Betriebswert: in typisierte TOML migrieren: 98
- Infisical-Zugangsdaten/-Identität: 49
- Dynamischer Leser: Aufrufer einzeln auflösen: 80
- Test-DSN, außerhalb Produktivkonfiguration: 6
- OS-Pfad, auf expliziten Startpfad umstellen: 2
- Historischer Secret-Dateipfad: Infisical-Grenze prüfen: 1

Die dynamischen Leser sind ausdrücklich noch nicht abschließend zugeordnet. 534 Schlüsselvorkommen im alten Inventar sind Kandidaten einschließlich Test-/Statuskonstanten; sie dürfen nicht pauschal als 534 Betriebseinstellungen migriert werden.

## Direkte Leser

| Quelle | Schlüssel beziehungsweise Hilfsfunktion | Klasse |
|---|---|---|
| bin/tb-bot/src/auto_raid.rs:36 | TB_AUTO_RAID_OFFLINE_GRACE_SECS | Betriebswert: in typisierte TOML migrieren |
| bin/tb-bot/src/chat_wiring.rs:81 | KNOWLEDGE_DIR | Betriebswert: in typisierte TOML migrieren |
| bin/tb-bot/src/chat_wiring.rs:208 | TWITCH_RAID_REDIRECT_URI | Betriebswert: in typisierte TOML migrieren |
| bin/tb-bot/src/chat_wiring.rs:231 | TWITCH_CLIENT_ID | Infisical-Zugangsdaten/-Identität |
| bin/tb-bot/src/chat_wiring.rs:542 | TB_CHAT_ENABLED | Betriebswert: in typisierte TOML migrieren |
| bin/tb-bot/src/chat_wiring.rs:557 | TWITCH_CLIENT_ID | Infisical-Zugangsdaten/-Identität |
| bin/tb-bot/src/chat_wiring.rs:558 | TWITCH_CLIENT_SECRET | Infisical-Zugangsdaten/-Identität |
| bin/tb-bot/src/chat_wiring.rs:563 | TWITCH_BOT_REFRESH_TOKEN | Infisical-Zugangsdaten/-Identität |
| bin/tb-bot/src/chat_wiring.rs:586 | TWITCH_BOT_TOKEN | Infisical-Zugangsdaten/-Identität |
| bin/tb-bot/src/chat_wiring.rs:820 | TB_CHAT_REVIEW_LOG_DIR | Betriebswert: in typisierte TOML migrieren |
| bin/tb-bot/src/chat_wiring.rs:997 | TWITCH_NOTIFY_CHANNEL_ID | Betriebswert: in typisierte TOML migrieren |
| bin/tb-bot/src/chat_wiring.rs:1572 | TB_GOLIVE_TIPS_ENABLED | Betriebswert: in typisierte TOML migrieren |
| bin/tb-bot/src/chat_wiring.rs:2416 | invite_url | Dynamischer Leser: Aufrufer einzeln auflösen |
| bin/tb-bot/src/chat_wiring.rs:2447 | invite_line | Dynamischer Leser: Aufrufer einzeln auflösen |
| bin/tb-bot/src/chat_wiring.rs:2538 | resolve_invite | Dynamischer Leser: Aufrufer einzeln auflösen |
| bin/tb-bot/src/chat_wiring.rs:3692 | TB_TEST_DATABASE_URL | Test-DSN, außerhalb Produktivkonfiguration |
| bin/tb-bot/src/confirm_resolver.rs:327 | TB_TEST_DATABASE_URL | Test-DSN, außerhalb Produktivkonfiguration |
| bin/tb-bot/src/eventsub_hooks.rs:1610 | TB_TEST_DATABASE_URL | Test-DSN, außerhalb Produktivkonfiguration |
| bin/tb-bot/src/flip_unraid.rs:24 | env_f64 | Dynamischer Leser: Aufrufer einzeln auflösen |
| bin/tb-bot/src/irc_lurker_wiring.rs:20 | TB_IRC_LURKER_ENABLED | Betriebswert: in typisierte TOML migrieren |
| bin/tb-bot/src/main.rs:95 | optional_env_bool | Dynamischer Leser: Aufrufer einzeln auflösen |
| bin/tb-bot/src/main.rs:162 | HOME | OS-Pfad, auf expliziten Startpfad umstellen |
| bin/tb-bot/src/main.rs:163 | YT_DLP_PATH | Betriebswert: in typisierte TOML migrieren |
| bin/tb-bot/src/main.rs:179 | optional_env_u16 | Dynamischer Leser: Aufrufer einzeln auflösen |
| bin/tb-bot/src/main.rs:198 | optional_env_i64 | Dynamischer Leser: Aufrufer einzeln auflösen |
| bin/tb-bot/src/main.rs:218 | optional_env_u64_with_fallback | Dynamischer Leser: Aufrufer einzeln auflösen |
| bin/tb-bot/src/main.rs:239 | optional_env_positive_i64 | Dynamischer Leser: Aufrufer einzeln auflösen |
| bin/tb-bot/src/main.rs:591 | TWITCH_LANGUAGE_FILTERS | Betriebswert: in typisierte TOML migrieren |
| bin/tb-bot/src/main.rs:650 | TWITCH_CLIENT_ID | Infisical-Zugangsdaten/-Identität |
| bin/tb-bot/src/main.rs:651 | TWITCH_CLIENT_SECRET | Infisical-Zugangsdaten/-Identität |
| bin/tb-bot/src/main.rs:676 | TWITCH_TARGET_GAME_NAME | Betriebswert: in typisierte TOML migrieren |
| bin/tb-bot/src/main.rs:763 | TWITCH_WEBHOOK_SECRET | Infisical-Zugangsdaten/-Identität |
| bin/tb-bot/src/main.rs:767 | TWITCH_EVENTSUB_CALLBACK_URL | Betriebswert: in typisierte TOML migrieren |
| bin/tb-bot/src/main.rs:940 | VOD_EXPORT_REMOTE_BASE | Betriebswert: in typisierte TOML migrieren |
| bin/tb-bot/src/main.rs:968 | TWITCH_RAID_REDIRECT_URI | Betriebswert: in typisierte TOML migrieren |
| bin/tb-bot/src/main.rs:975 | TWITCH_CLIENT_ID | Infisical-Zugangsdaten/-Identität |
| bin/tb-bot/src/main.rs:1473 | TWITCH_WEBHOOK_SECRET | Infisical-Zugangsdaten/-Identität |
| bin/tb-bot/src/main.rs:1853 | TWITCH_ALERT_MENTION | Betriebswert: in typisierte TOML migrieren |
| bin/tb-bot/src/main.rs:1854 | TWITCH_DISCORD_REF_CODE | Betriebswert: in typisierte TOML migrieren |
| bin/tb-bot/src/main.rs:1936 | TWITCH_TARGET_GAME_NAME | Betriebswert: in typisierte TOML migrieren |
| bin/tb-bot/src/main.rs:2046 | TB_INTERNAL_API_LEGACY_FALLBACK_URL | Betriebswert: in typisierte TOML migrieren |
| bin/tb-bot/src/main.rs:2231 | TWITCH_RAID_REDIRECT_URI | Betriebswert: in typisierte TOML migrieren |
| bin/tb-bot/src/main.rs:2313 | TWITCH_RAID_REDIRECT_URI | Betriebswert: in typisierte TOML migrieren |
| bin/tb-bot/src/mcp.rs:113 | TB_MCP_HOST | Betriebswert: in typisierte TOML migrieren |
| bin/tb-bot/src/mcp.rs:118 | TB_MCP_PORT | Betriebswert: in typisierte TOML migrieren |
| bin/tb-bot/src/oauth_followups.rs:39 | env_u64 | Dynamischer Leser: Aufrufer einzeln auflösen |
| bin/tb-bot/src/oauth_followups.rs:325 | TWITCH_INTERNAL_API_TOKEN | Infisical-Zugangsdaten/-Identität |
| bin/tb-bot/src/oauth_followups.rs:329 | TB_INTERNAL_API_LEGACY_FALLBACK_URL | Betriebswert: in typisierte TOML migrieren |
| bin/tb-bot/src/oauth_followups.rs:474 | TWITCH_BOT_USER_ID | Betriebswert: in typisierte TOML migrieren |
| bin/tb-bot/src/obs_dock.rs:144 | HOME | OS-Pfad, auf expliziten Startpfad umstellen |
| bin/tb-bot/src/offline_side_effects.rs:97 | TB_TEST_DATABASE_URL | Test-DSN, außerhalb Produktivkonfiguration |
| bin/tb-bot/src/outreach_shadow_wiring.rs:82 | OUTREACH_SHADOW_ENABLED | Betriebswert: in typisierte TOML migrieren |
| bin/tb-bot/src/outreach_shadow_wiring.rs:715 | nonempty_env | Dynamischer Leser: Aufrufer einzeln auflösen |
| bin/tb-bot/src/partner_recruit.rs:617 | TB_TEST_DATABASE_URL | Test-DSN, außerhalb Produktivkonfiguration |
| bin/tb-bot/src/raid_arrival_wiring.rs:1659 | TB_TEST_DATABASE_URL | Test-DSN, außerhalb Produktivkonfiguration |
| bin/tb-bot/src/raid_oauth_impl.rs:765 | TWITCH_INTERNAL_API_ALLOWED_GUILD_IDS | Betriebswert: in typisierte TOML migrieren |
| bin/tb-bot/src/raid_oauth_impl.rs:770 | TWITCH_INTERNAL_API_ALLOWED_CHANNEL_IDS | Betriebswert: in typisierte TOML migrieren |
| bin/tb-bot/src/raid_oauth_impl.rs:775 | TWITCH_INTERNAL_API_ALLOWED_ROLE_IDS | Betriebswert: in typisierte TOML migrieren |
| bin/tb-bot/src/raid_oauth_impl.rs:779 | TWITCH_RAID_SUCCESS_REDIRECT_URL | Betriebswert: in typisierte TOML migrieren |
| bin/tb-bot/src/shadow_review_wiring.rs:43 | ENGAGEMENT_SHADOW_REVIEW_CHANNEL_ID | Betriebswert: in typisierte TOML migrieren |
| bin/tb-bot/src/smalltalk_loop_wiring.rs:118 | SMALLTALK_LOOP_ENABLED | Betriebswert: in typisierte TOML migrieren |
| bin/tb-bot/src/smalltalk_loop_wiring.rs:119 | SMALLTALK_LOOP_LIVE_SEND | Betriebswert: in typisierte TOML migrieren |
| bin/tb-bot/src/streamer_link.rs:32 | env_u64 | Dynamischer Leser: Aufrufer einzeln auflösen |
| bin/tb-bot/src/streamer_link.rs:39 | env_bool | Dynamischer Leser: Aufrufer einzeln auflösen |
| bin/tb-bot/src/streamer_link.rs:58 | STREAMER_LINK_STATE_PATH | Betriebswert: in typisierte TOML migrieren |
| bin/tb-bot/src/token_lifecycle_wiring.rs:64 | env_u64 | Dynamischer Leser: Aufrufer einzeln auflösen |
| bin/tb-dashboard/src/main.rs:37 | optional_env_bool | Dynamischer Leser: Aufrufer einzeln auflösen |
| bin/tb-dashboard/src/main.rs:60 | optional_env_u16 | Dynamischer Leser: Aufrufer einzeln auflösen |
| bin/tb-dashboard/src/main.rs:85 | TWITCH_RUNTIME_ENFORCE | Betriebswert: in typisierte TOML migrieren |
| bin/tb-dashboard/src/main.rs:108 | TWITCH_CLIENT_ID | Infisical-Zugangsdaten/-Identität |
| bin/tb-dashboard/src/main.rs:109 | TWITCH_CLIENT_SECRET | Infisical-Zugangsdaten/-Identität |
| bin/tb-dashboard/src/main.rs:141 | TWITCH_RUNTIME_ROLE | Betriebswert: in typisierte TOML migrieren |
| bin/tb-dashboard/src/main.rs:145 | TWITCH_SPLIT_RUNTIME_ROLE | Betriebswert: in typisierte TOML migrieren |
| bin/tb-dashboard/src/main.rs:255 | TWITCH_RUNTIME_PID_LOCK_DIR | Betriebswert: in typisierte TOML migrieren |
| bin/tb-dashboard/src/main.rs:437 | TB_DASHBOARD_LEGACY_FALLBACK_URL | Betriebswert: in typisierte TOML migrieren |
| bin/tb-stream-audit/src/main.rs:1032 | ENGAGEMENT_STT_BASE_URL | Betriebswert: in typisierte TOML migrieren |
| bin/tb-stream-audit/src/main.rs:1040 | remote_stt_erlaubt | Dynamischer Leser: Aufrufer einzeln auflösen |
| bin/tb-stream-audit/src/main.rs:1062 | TWITCH_CLIENT_ID | Infisical-Zugangsdaten/-Identität |
| bin/tb-stream-audit/src/main.rs:1065 | TWITCH_CLIENT_SECRET | Infisical-Zugangsdaten/-Identität |
| bin/tb-stream-audit/src/main.rs:3700 | STREAM_AUDIT_RCLONE_BIN | Betriebswert: in typisierte TOML migrieren |
| bin/tb-stream-audit/src/main.rs:3712 | STREAM_AUDIT_FFMPEG_BIN | Betriebswert: in typisierte TOML migrieren |
| bin/tb-stream-audit/src/main.rs:3716 | FFMPEG_BIN | Betriebswert: in typisierte TOML migrieren |
| bin/tb-stream-audit/src/main.rs:5066 | VOICE_REACTION_STREAMLINK_BIN | Betriebswert: in typisierte TOML migrieren |
| crates/tb-analytics/src/billing/catalog.rs:450 | first_env | Dynamischer Leser: Aufrufer einzeln auflösen |
| crates/tb-analytics/src/post_stream.rs:1614 | TWITCH_POST_STREAM_REPORTS_ENABLED | Betriebswert: in typisierte TOML migrieren |
| crates/tb-chat/src/chatter_tracking.rs:442 | TB_CHAT_PERSIST_ALL_GAMES | Betriebswert: in typisierte TOML migrieren |
| crates/tb-chat/src/commands.rs:91 | KNOWLEDGE_DIR | Betriebswert: in typisierte TOML migrieren |
| crates/tb-chat/src/lfg_pitch.rs:299 | LFG_PITCH_ENABLED | Betriebswert: in typisierte TOML migrieren |
| crates/tb-chat/src/pipeline.rs:527 | alert_channel_id_from_env | Dynamischer Leser: Aufrufer einzeln auflösen |
| crates/tb-chat/src/scam_pitch.rs:1434 | TWITCH_SERVICE_WARNING_LOG_DIR | Betriebswert: in typisierte TOML migrieren |
| crates/tb-chat/src/secret_sink.rs:176 | non_empty_env | Dynamischer Leser: Aufrufer einzeln auflösen |
| crates/tb-chat/src/stats.rs:392 | STEAM_BOT_RANK_URL | Betriebswert: in typisierte TOML migrieren |
| crates/tb-chat/src/stats.rs:406 | STEAM_BOT_RANK_URL | Betriebswert: in typisierte TOML migrieren |
| crates/tb-chat/src/stats.rs:413 | STEAM_BOT_RANK_URL | Betriebswert: in typisierte TOML migrieren |
| crates/tb-chat/src/stats.rs:420 | STEAM_BOT_RANK_URL | Betriebswert: in typisierte TOML migrieren |
| crates/tb-chat/src/title_ai.rs:650 | ZAI_API_KEY | Infisical-Zugangsdaten/-Identität |
| crates/tb-chat/src/title_ai.rs:654 | ZAI_BASE_URL | Betriebswert: in typisierte TOML migrieren |
| crates/tb-chat/src/title_ai.rs:669 | DDC_PENTEST_DISABLE_RATE_LIMITS | Betriebswert: in typisierte TOML migrieren |
| crates/tb-chat/src/token.rs:88 | TWITCH_BOT_TOKEN | Infisical-Zugangsdaten/-Identität |
| crates/tb-chat/src/token.rs:89 | TWITCH_BOT_REFRESH_TOKEN | Infisical-Zugangsdaten/-Identität |
| crates/tb-chat/src/token.rs:90 | TWITCH_BOT_TOKEN_FILE | Historischer Secret-Dateipfad: Infisical-Grenze prüfen |
| crates/tb-config/src/lib.rs:203 | from_env | Dynamischer Leser: Aufrufer einzeln auflösen |
| crates/tb-crypto/src/field.rs:28 | DB_MASTER_KEY_V1 | Infisical-Zugangsdaten/-Identität |
| crates/tb-dashboard-api/src/ai_state.rs:26 | DDC_PENTEST_DISABLE_RATE_LIMITS | Betriebswert: in typisierte TOML migrieren |
| crates/tb-dashboard-api/src/auth/discord_admin_login.rs:328 | TB_DASHBOARD_COOKIE_INSECURE | Betriebswert: in typisierte TOML migrieren |
| crates/tb-dashboard-api/src/auth/discord_admin_login.rs:1055 | non_empty_env | Dynamischer Leser: Aufrufer einzeln auflösen |
| crates/tb-dashboard-api/src/auth/session.rs:541 | SESSIONS_ENCRYPTION_KEY | Infisical-Zugangsdaten/-Identität |
| crates/tb-dashboard-api/src/handlers/admin_affiliate.rs:44 | env_secret | Dynamischer Leser: Aufrufer einzeln auflösen |
| crates/tb-dashboard-api/src/handlers/admin_chat_action.rs:311 | nonempty_env | Dynamischer Leser: Aufrufer einzeln auflösen |
| crates/tb-dashboard-api/src/handlers/admin_legacy_streamers.rs:348 | TWITCH_CLIENT_ID | Infisical-Zugangsdaten/-Identität |
| crates/tb-dashboard-api/src/handlers/admin_legacy_streamers.rs:351 | TWITCH_CLIENT_SECRET | Infisical-Zugangsdaten/-Identität |
| crates/tb-dashboard-api/src/handlers/admin_partner_signup_block.rs:133 | TWITCH_CLIENT_ID | Infisical-Zugangsdaten/-Identität |
| crates/tb-dashboard-api/src/handlers/admin_partner_signup_block.rs:136 | TWITCH_CLIENT_SECRET | Infisical-Zugangsdaten/-Identität |
| crates/tb-dashboard-api/src/handlers/admin_spa.rs:157 | ADMIN_DASHBOARD_DIST_PATH | Betriebswert: in typisierte TOML migrieren |
| crates/tb-dashboard-api/src/handlers/admin_streamers.rs:774 | TWITCH_INTERNAL_API_TOKEN | Infisical-Zugangsdaten/-Identität |
| crates/tb-dashboard-api/src/handlers/admin_streamers.rs:782 | worker_internal_base_url | Dynamischer Leser: Aufrufer einzeln auflösen |
| crates/tb-dashboard-api/src/handlers/affiliate.rs:82 | TB_DASHBOARD_COOKIE_INSECURE | Betriebswert: in typisierte TOML migrieren |
| crates/tb-dashboard-api/src/handlers/affiliate.rs:1206 | TB_DASHBOARD_COOKIE_INSECURE | Betriebswert: in typisierte TOML migrieren |
| crates/tb-dashboard-api/src/handlers/affiliate.rs:1264 | non_empty_env | Dynamischer Leser: Aufrufer einzeln auflösen |
| crates/tb-dashboard-api/src/handlers/affiliate_portal.rs:135 | TWITCH_DISCORD_REF_CODE | Betriebswert: in typisierte TOML migrieren |
| crates/tb-dashboard-api/src/handlers/auth_login.rs:807 | TB_DASHBOARD_COOKIE_INSECURE | Betriebswert: in typisierte TOML migrieren |
| crates/tb-dashboard-api/src/handlers/auth_login.rs:850 | non_empty_env | Dynamischer Leser: Aufrufer einzeln auflösen |
| crates/tb-dashboard-api/src/handlers/billing_page.rs:115 | resolve_public_origin | Dynamischer Leser: Aufrufer einzeln auflösen |
| crates/tb-dashboard-api/src/handlers/billing_page.rs:140 | non_empty_env | Dynamischer Leser: Aufrufer einzeln auflösen |
| crates/tb-dashboard-api/src/handlers/billing_stripe_sync.rs:342 | STRIPE_WEBHOOK_SECRET | Infisical-Zugangsdaten/-Identität |
| crates/tb-dashboard-api/src/handlers/billing_stripe_sync.rs:346 | TWITCH_BILLING_STRIPE_WEBHOOK_SECRET | Infisical-Zugangsdaten/-Identität |
| crates/tb-dashboard-api/src/handlers/billing_webhook.rs:76 | non_empty_env | Dynamischer Leser: Aufrufer einzeln auflösen |
| crates/tb-dashboard-api/src/handlers/brain_lab.rs:40 | DEADLOCK_BRAIN_READONLY_DSN | Infisical-Zugangsdaten/-Identität |
| crates/tb-dashboard-api/src/handlers/brain_lab.rs:42 | DEADLOCK_CENTRAL_DSN | Infisical-Zugangsdaten/-Identität |
| crates/tb-dashboard-api/src/handlers/caster_overlay.rs:887 | TURNIER_INTERNAL_API_TOKEN | Infisical-Zugangsdaten/-Identität |
| crates/tb-dashboard-api/src/handlers/caster_overlay.rs:897 | TURNIER_INTERNAL_API_BASE_URL | Betriebswert: in typisierte TOML migrieren |
| crates/tb-dashboard-api/src/handlers/community/sources.rs:75 | STEAM_BOT_RANK_URL | Betriebswert: in typisierte TOML migrieren |
| crates/tb-dashboard-api/src/handlers/community/sources.rs:141 | lobbies | Dynamischer Leser: Aufrufer einzeln auflösen |
| crates/tb-dashboard-api/src/handlers/community/sources.rs:143 | MASTER_BROKER_BASE_URL | Betriebswert: in typisierte TOML migrieren |
| crates/tb-dashboard-api/src/handlers/community/sources.rs:144 | MASTER_BROKER_HOST | Betriebswert: in typisierte TOML migrieren |
| crates/tb-dashboard-api/src/handlers/community/sources.rs:145 | MASTER_BROKER_PORT | Betriebswert: in typisierte TOML migrieren |
| crates/tb-dashboard-api/src/handlers/demo.rs:44 | TWITCH_DEMO_EMBED_ORIGINS | Betriebswert: in typisierte TOML migrieren |
| crates/tb-dashboard-api/src/handlers/demo_login.rs:35 | non_empty_env | Dynamischer Leser: Aufrufer einzeln auflösen |
| crates/tb-dashboard-api/src/handlers/demo_login.rs:46 | TB_DASHBOARD_COOKIE_INSECURE | Betriebswert: in typisierte TOML migrieren |
| crates/tb-dashboard-api/src/handlers/health_probe.rs:204 | non_empty_env | Dynamischer Leser: Aufrufer einzeln auflösen |
| crates/tb-dashboard-api/src/handlers/health_probe.rs:485 | TWITCH_ANALYTICS_DSN | Infisical-Zugangsdaten/-Identität |
| crates/tb-dashboard-api/src/handlers/health_probe.rs:486 | DATABASE_URL | Infisical-Zugangsdaten/-Identität |
| crates/tb-dashboard-api/src/handlers/help_page.rs:12 | KNOWLEDGE_DIR | Betriebswert: in typisierte TOML migrieren |
| crates/tb-dashboard-api/src/handlers/internal_home.rs:292 | TWITCH_CLIENT_ID | Infisical-Zugangsdaten/-Identität |
| crates/tb-dashboard-api/src/handlers/internal_home.rs:296 | TWITCH_CLIENT_SECRET | Infisical-Zugangsdaten/-Identität |
| crates/tb-dashboard-api/src/handlers/internal_home.rs:383 | STEAM_LINK_START_BASE_URL | Betriebswert: in typisierte TOML migrieren |
| crates/tb-dashboard-api/src/handlers/legal.rs:608 | TB_LEGAL_PAGES_PATH | Betriebswert: in typisierte TOML migrieren |
| crates/tb-dashboard-api/src/handlers/legal.rs:840 | read | Dynamischer Leser: Aufrufer einzeln auflösen |
| crates/tb-dashboard-api/src/handlers/legal.rs:841 | read | Dynamischer Leser: Aufrufer einzeln auflösen |
| crates/tb-dashboard-api/src/handlers/network.rs:138 | TWITCH_CLIENT_ID | Infisical-Zugangsdaten/-Identität |
| crates/tb-dashboard-api/src/handlers/network.rs:141 | TWITCH_CLIENT_SECRET | Infisical-Zugangsdaten/-Identität |
| crates/tb-dashboard-api/src/handlers/overlay.rs:232 | DEADLOCK_ASSETS_BASE | Betriebswert: in typisierte TOML migrieren |
| crates/tb-dashboard-api/src/handlers/overlay.rs:573 | STEAM_BOT_RANK_URL | Betriebswert: in typisierte TOML migrieren |
| crates/tb-dashboard-api/src/handlers/partner_login.rs:50 | TWITCH_PARTNER_TOKEN | Infisical-Zugangsdaten/-Identität |
| crates/tb-dashboard-api/src/handlers/raid_pages.rs:60 | nonempty_env | Dynamischer Leser: Aufrufer einzeln auflösen |
| crates/tb-dashboard-api/src/handlers/raid_requirements.rs:191 | nonempty_env | Dynamischer Leser: Aufrufer einzeln auflösen |
| crates/tb-dashboard-api/src/handlers/scam_guard_enforce.rs:58 | nonempty_env | Dynamischer Leser: Aufrufer einzeln auflösen |
| crates/tb-dashboard-api/src/handlers/self_explainer.rs:568 | nonempty_env | Dynamischer Leser: Aufrufer einzeln auflösen |
| crates/tb-dashboard-api/src/handlers/social_media.rs:1404 | TWITCH_CLIENT_ID | Infisical-Zugangsdaten/-Identität |
| crates/tb-dashboard-api/src/handlers/social_media.rs:1407 | TWITCH_CLIENT_SECRET | Infisical-Zugangsdaten/-Identität |
| crates/tb-dashboard-api/src/handlers/social_media.rs:3421 | privacy_forced | Dynamischer Leser: Aufrufer einzeln auflösen |
| crates/tb-dashboard-api/src/handlers/social_media.rs:3512 | SOCIAL_MEDIA_PUBLIC_ORIGIN | Betriebswert: in typisierte TOML migrieren |
| crates/tb-dashboard-api/src/handlers/spa.rs:508 | TWITCH_ADMIN_PUBLIC_URL | Betriebswert: in typisierte TOML migrieren |
| crates/tb-dashboard-api/src/handlers/spa.rs:509 | MASTER_DASHBOARD_PUBLIC_URL | Betriebswert: in typisierte TOML migrieren |
| crates/tb-dashboard-api/src/handlers/spa.rs:603 | DASHBOARD_V2_DIST_PATH | Betriebswert: in typisierte TOML migrieren |
| crates/tb-dashboard-api/src/handlers/system/health.rs:98 | TWITCH_ANALYTICS_DSN | Infisical-Zugangsdaten/-Identität |
| crates/tb-dashboard-api/src/handlers/system/health.rs:99 | DATABASE_URL | Infisical-Zugangsdaten/-Identität |
| crates/tb-dashboard-api/src/handlers/system/health.rs:113 | TWITCH_INTERNAL_API_BASE_URL | Betriebswert: in typisierte TOML migrieren |
| crates/tb-dashboard-api/src/handlers/system/health.rs:119 | TWITCH_INTERNAL_API_HOST | Betriebswert: in typisierte TOML migrieren |
| crates/tb-dashboard-api/src/handlers/system/health.rs:123 | TWITCH_INTERNAL_API_PORT | Betriebswert: in typisierte TOML migrieren |
| crates/tb-dashboard-api/src/handlers/system/health.rs:134 | TWITCH_INTERNAL_API_TOKEN | Infisical-Zugangsdaten/-Identität |
| crates/tb-dashboard-api/src/handlers/title.rs:234 | TWITCH_CLIENT_ID | Infisical-Zugangsdaten/-Identität |
| crates/tb-dashboard-api/src/handlers/title.rs:235 | TWITCH_BOT_CLIENT_ID | Infisical-Zugangsdaten/-Identität |
| crates/tb-dashboard-api/src/handlers/title.rs:240 | TWITCH_HELIX_BASE_URL | Betriebswert: in typisierte TOML migrieren |
| crates/tb-dashboard-api/src/handlers/title.rs:634 | settings_handler | Dynamischer Leser: Aufrufer einzeln auflösen |
| crates/tb-dashboard-api/src/handlers/viewer_exclusion.rs:31 | dynamic_bot_logins_from_env | Dynamischer Leser: Aufrufer einzeln auflösen |
| crates/tb-dashboard-api/src/handlers/viewer_exclusion.rs:41 | dynamic_bot_user_ids_from_env | Dynamischer Leser: Aufrufer einzeln auflösen |
| crates/tb-dashboard-api/src/handlers/website.rs:82 | WEBSITE_DIST_PATH | Betriebswert: in typisierte TOML migrieren |
| crates/tb-db/src/retry.rs:189 | env_u32 | Dynamischer Leser: Aufrufer einzeln auflösen |
| crates/tb-db/src/retry.rs:207 | env_secs_f64 | Dynamischer Leser: Aufrufer einzeln auflösen |
| crates/tb-engagement/src/audio_capture.rs:86 | VOICE_REACTION_STREAMLINK_BIN | Betriebswert: in typisierte TOML migrieren |
| crates/tb-engagement/src/background.rs:59 | ENGAGEMENT_STREAM_TRANSCRIPTS_ENABLED | Betriebswert: in typisierte TOML migrieren |
| crates/tb-engagement/src/llm_chat.rs:339 | ENGAGEMENT_PERSONA_MODE | Betriebswert: in typisierte TOML migrieren |
| crates/tb-engagement/src/reaction_learning.rs:136 | env_str | Dynamischer Leser: Aufrufer einzeln auflösen |
| crates/tb-engagement/src/rhythm.rs:112 | env_float | Dynamischer Leser: Aufrufer einzeln auflösen |
| crates/tb-engagement/src/rhythm.rs:119 | env_int | Dynamischer Leser: Aufrufer einzeln auflösen |
| crates/tb-engagement/src/sender_auth.rs:526 | nonempty_env | Dynamischer Leser: Aufrufer einzeln auflösen |
| crates/tb-engagement/src/stream_transcripts.rs:24 | env_int | Dynamischer Leser: Aufrufer einzeln auflösen |
| crates/tb-engagement/src/stream_transcripts.rs:34 | env_float | Dynamischer Leser: Aufrufer einzeln auflösen |
| crates/tb-engagement/src/stream_transcripts.rs:55 | ENGAGEMENT_TRANSCRIPT_QUALITY | Betriebswert: in typisierte TOML migrieren |
| crates/tb-engagement/src/transcribe.rs:392 | nonempty_env | Dynamischer Leser: Aufrufer einzeln auflösen |
| crates/tb-internal-api/src/handlers/chat_command.rs:95 | get_invite_url | Dynamischer Leser: Aufrufer einzeln auflösen |
| crates/tb-internal-api/src/handlers/healthz.rs:108 | TWITCH_ANALYTICS_DSN | Infisical-Zugangsdaten/-Identität |
| crates/tb-internal-api/src/handlers/python_stubs.rs:230 | TWITCH_DASHBOARD_OWNER_DISCORD_ID | Betriebswert: in typisierte TOML migrieren |
| crates/tb-internal-api/src/handlers/self_explainer_log.rs:81 | broker_token | Dynamischer Leser: Aufrufer einzeln auflösen |
| crates/tb-internal-api/src/handlers/self_explainer_log.rs:94 | MASTER_BROKER_BASE_URL | Betriebswert: in typisierte TOML migrieren |
| crates/tb-internal-api/src/handlers/self_explainer_log.rs:99 | MASTER_BROKER_HOST | Betriebswert: in typisierte TOML migrieren |
| crates/tb-internal-api/src/handlers/self_explainer_log.rs:102 | MASTER_BROKER_PORT | Betriebswert: in typisierte TOML migrieren |
| crates/tb-internal-api/src/handlers/streamers.rs:317 | TWITCH_TARGET_GAME_NAME | Betriebswert: in typisierte TOML migrieren |
| crates/tb-internal-api/src/handlers/telemetry_routes.rs:199 | parse_allowlist_ids | Dynamischer Leser: Aufrufer einzeln auflösen |
| crates/tb-internal-api/src/handlers/telemetry_routes.rs:269 | live_active_announcements_handler | Dynamischer Leser: Aufrufer einzeln auflösen |
| crates/tb-internal-api/src/security.rs:414 | nonempty_env | Dynamischer Leser: Aufrufer einzeln auflösen |
| crates/tb-llm/src/keys.rs:7 | nonempty_env | Dynamischer Leser: Aufrufer einzeln auflösen |
| crates/tb-llm/src/ledger.rs:69 | dsn_from_env | Dynamischer Leser: Aufrufer einzeln auflösen |
| crates/tb-llm/src/selection.rs:61 | nonempty_env | Dynamischer Leser: Aufrufer einzeln auflösen |
| crates/tb-load/src/last.rs:174 | prozent_aus_umgebung | Dynamischer Leser: Aufrufer einzeln auflösen |
| crates/tb-load/src/last.rs:194 | u64_aus_umgebung | Dynamischer Leser: Aufrufer einzeln auflösen |
| crates/tb-monitoring/src/inbox_store/retry.rs:135 | env_u32 | Dynamischer Leser: Aufrufer einzeln auflösen |
| crates/tb-monitoring/src/inbox_store/retry.rs:153 | env_secs_f64 | Dynamischer Leser: Aufrufer einzeln auflösen |
| crates/tb-monitoring/src/observability_retention.rs:42 | parse_env_clamped | Dynamischer Leser: Aufrufer einzeln auflösen |
| crates/tb-monitoring/src/scout.rs:352 | TB_SCOUT_ENABLED | Betriebswert: in typisierte TOML migrieren |
| crates/tb-monitoring/src/subscriptions.rs:398 | parse_env_clamped | Dynamischer Leser: Aufrufer einzeln auflösen |
| crates/tb-raid/src/token_lifecycle.rs:59 | BOT_TWITCH_LOGIN | Betriebswert: in typisierte TOML migrieren |
| crates/tb-social-media/src/bin/korpus_ernte.rs:33 | TWITCH_CLIENT_ID | Infisical-Zugangsdaten/-Identität |
| crates/tb-social-media/src/bin/korpus_ernte.rs:34 | TWITCH_CLIENT_SECRET | Infisical-Zugangsdaten/-Identität |
| crates/tb-social-media/src/bin/render_clips.rs:25 | DEADLOCK_CENTRAL_DSN | Infisical-Zugangsdaten/-Identität |
| crates/tb-social-media/src/bin/render_clips.rs:26 | DATABASE_URL | Infisical-Zugangsdaten/-Identität |
| crates/tb-social-media/src/clip/task.rs:46 | TB_CLIP_FETCHER_ENABLED | Betriebswert: in typisierte TOML migrieren |
| crates/tb-social-media/src/llm_dispatch.rs:45 | env_rate | Dynamischer Leser: Aufrufer einzeln auflösen |
| crates/tb-social-media/src/oauth.rs:809 | env_nonempty | Dynamischer Leser: Aufrufer einzeln auflösen |
| crates/tb-stream-audit/src/archiv.rs:58 | min_frei_bytes | Dynamischer Leser: Aufrufer einzeln auflösen |
| crates/tb-stream-audit/src/archiv.rs:93 | archiv_aktiv | Dynamischer Leser: Aufrufer einzeln auflösen |
| crates/tb-stream-audit/src/archiv.rs:99 | archiv_aktiv_fuer | Dynamischer Leser: Aufrufer einzeln auflösen |
| crates/tb-stream-audit/src/archiv.rs:132 | remote_basis | Dynamischer Leser: Aufrufer einzeln auflösen |
| crates/tb-stream-audit/src/config.rs:146 | from_env | Dynamischer Leser: Aufrufer einzeln auflösen |
| crates/tb-stream-audit/src/config.rs:147 | from_env | Dynamischer Leser: Aufrufer einzeln auflösen |
| crates/tb-stream-audit/src/config.rs:153 | from_env | Dynamischer Leser: Aufrufer einzeln auflösen |
| crates/tb-stream-audit/src/config.rs:157 | from_env | Dynamischer Leser: Aufrufer einzeln auflösen |
| crates/tb-stream-audit/src/config.rs:160 | from_env | Dynamischer Leser: Aufrufer einzeln auflösen |
| crates/tb-stream-audit/src/llm.rs:90 | fernes_modell_erlaubt | Dynamischer Leser: Aufrufer einzeln auflösen |
| crates/tb-stream-audit/src/melden.rs:40 | MASTER_BROKER_BASE_URL | Betriebswert: in typisierte TOML migrieren |
| crates/tb-stream-audit/src/melden.rs:46 | MASTER_BROKER_HOST | Betriebswert: in typisierte TOML migrieren |
| crates/tb-stream-audit/src/melden.rs:50 | MASTER_BROKER_PORT | Betriebswert: in typisierte TOML migrieren |
| crates/tb-stream-audit/src/melden.rs:64 | broker_token | Dynamischer Leser: Aufrufer einzeln auflösen |
| crates/tb-stream-audit/src/melden.rs:79 | STREAM_AUDIT_DISCORD_USER_ID | Betriebswert: in typisierte TOML migrieren |
| crates/tb-vod-archive/src/config.rs:75 | from_env | Dynamischer Leser: Aufrufer einzeln auflösen |
