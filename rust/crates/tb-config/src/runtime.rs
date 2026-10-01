//! Die beim Dienststart geprüfte Momentaufnahme; niemals die gespeicherte Datei
//! als bereits aktive Konfiguration ausgeben.

use crate::{
    file::{ErrorKind, FileError},
    BotConfigSnapshot,
};
use std::sync::OnceLock;

static ACTIVE: OnceLock<BotConfigSnapshot> = OnceLock::new();

pub fn install(snapshot: BotConfigSnapshot) -> Result<&'static BotConfigSnapshot, FileError> {
    ACTIVE
        .set(snapshot)
        .map_err(|_| FileError::new(ErrorKind::RestartRequired))?;
    ACTIVE
        .get()
        .ok_or_else(|| FileError::new(ErrorKind::FileUnreadable))
}

pub fn active() -> Option<&'static BotConfigSnapshot> {
    ACTIVE.get()
}

/// Fachverbraucher dürfen keine zweite Betriebsquelle oder Ersatzkonfiguration
/// aufbauen. Fehlender Dienststart wird als Fehler weitergereicht.
pub fn settings() -> Result<&'static crate::global::BotConfig, FileError> {
    active()
        .map(BotConfigSnapshot::settings)
        .ok_or_else(|| FileError::new(ErrorKind::FileUnreadable))
}

/// Existing dashboard consumer names projected from the one validated source.
/// Ordinary settings never come from the secret map; credentials never from TOML.
pub fn dashboard_value(name: &str) -> Result<String, std::env::VarError> {
    let config = settings().map_err(|_| std::env::VarError::NotPresent)?;
    let options = &config.dashboard.options;
    let value = match name {
        "TWITCH_ANALYTICS_DSN" => crate::private::service_secret(name, "dashboard"),
        "TWITCH_INTERNAL_API_BASE_URL" => Some(config.internal_api.client_base_url()),
        "TWITCH_INTERNAL_API_HOST" => Some(config.internal_api.host.to_string()),
        "TWITCH_INTERNAL_API_PORT" => Some(config.internal_api.port.to_string()),
        "TWITCH_INTERNAL_API_ALLOW_NON_LOOPBACK" => {
            Some(config.internal_api.probe_allow_non_loopback.to_string())
        }
        "TB_DASHBOARD_COOKIE_INSECURE" => {
            Some(if options.cookie_insecure { "1" } else { "0" }.into())
        }
        "TWITCH_DASHBOARD_NOAUTH" => Some(options.noauth_readiness.to_string()),
        "TWITCH_RUNTIME_ENFORCE" | "TWITCH_SPLIT_RUNTIME_ENFORCE" => {
            Some(options.runtime_enforce.to_string())
        }
        "TWITCH_RUNTIME_ROLE" | "TWITCH_SPLIT_RUNTIME_ROLE" => Some(options.runtime_role.clone()),
        "TWITCH_RUNTIME_PID_LOCK_DIR" => {
            Some(options.runtime_lock_dir.to_string_lossy().into_owned())
        }
        "TB_DASHBOARD_LEGACY_FALLBACK_URL" => options.legacy_fallback_url.clone(),
        "DDC_PENTEST_DISABLE_RATE_LIMITS" => Some(options.pentest_disable_rate_limits.to_string()),
        "TWITCH_HELIX_BASE_URL" => options.helix_base_url.clone(),
        "TURNIER_INTERNAL_API_BASE_URL" => Some(options.turnier_internal_base_url.clone()),
        "STEAM_BOT_RANK_URL" => Some(options.steam_rank_url.clone()),
        "STEAM_BOT_TITLE_CONTEXT_URL" => Some(options.steam_title_context_url.clone()),
        "STEAM_LINK_START_BASE_URL" => Some(options.steam_link_start_base_url.clone()),
        "DEADLOCK_ASSETS_BASE" => Some(options.deadlock_assets_base_url.clone()),
        "ADMIN_DASHBOARD_DIST_PATH" => Some(options.admin_dist_path.to_string_lossy().into_owned()),
        "DASHBOARD_V2_DIST_PATH" => {
            Some(options.dashboard_dist_path.to_string_lossy().into_owned())
        }
        "WEBSITE_DIST_PATH" => Some(options.website_dist_path.to_string_lossy().into_owned()),
        "TB_LEGAL_PAGES_PATH" => Some(options.legal_pages_path.to_string_lossy().into_owned()),
        "KNOWLEDGE_DIR" => Some(config.knowledge.directory.to_string_lossy().into_owned()),
        "TWITCH_LEGAL_TURNSTILE_SITE_KEY" | "TURNSTILE_SITE_KEY" => {
            Some(options.legal_turnstile_site_key.clone())
        }
        "TWITCH_ADMIN_PUBLIC_URL" | "MASTER_DASHBOARD_PUBLIC_URL" => {
            options.admin_public_url.clone()
        }
        "TWITCH_PUBLIC_DASHBOARD_BASE_URL" | "TWITCH_PUBLIC_URL" | "PUBLIC_URL" => {
            options.public_dashboard_url.clone()
        }
        "SOCIAL_MEDIA_PUBLIC_ORIGIN" => Some(config.media.social_media_public_origin.clone()),
        "YOUTUBE_AUDIT_PASSED" => Some(config.media.youtube_audit_passed.to_string()),
        "FIREWORKS_PRICE_INPUT_PER_1K" => Some(config.media.llm_price_input_per_1k.to_string()),
        "FIREWORKS_PRICE_OUTPUT_PER_1K" => Some(config.media.llm_price_output_per_1k.to_string()),
        "TWITCH_DISCORD_REF_CODE" => Some(options.discord_ref_code.clone()),
        "TWITCH_DEMO_LOGIN_TWITCH_USER_ID" => options.demo_login_twitch_user_id.clone(),
        "TWITCH_DEMO_LOGIN_DISPLAY_NAME" => Some(options.demo_login_display_name.clone()),
        "TWITCH_DEMO_EMBED_ORIGINS" => Some(options.demo_embed_origins.join(",")),
        "TWITCH_AFFILIATE_AUTH_REDIRECT_URI" => options.affiliate_oauth_redirect_uri.clone(),
        "TWITCH_DASHBOARD_AUTH_REDIRECT_URI" => options.oauth_redirect_uri.clone(),
        "TWITCH_DISCORD_OAUTH_BROKER_URL" => Some(options.discord_oauth_broker_url.clone()),
        "STRIPE_PRICE_ID_MAP" | "TWITCH_BILLING_STRIPE_PRICE_ID_MAP" => {
            let prices: std::collections::BTreeMap<_, _> = options
                .stripe_price_ids
                .iter()
                .map(|(plan, ids)| {
                    (
                        plan,
                        ids.cycles()
                            .map(|(cycle, id)| (cycle.to_string(), id))
                            .collect::<std::collections::BTreeMap<_, _>>(),
                    )
                })
                .collect();
            serde_json::to_string(&prices).ok()
        }
        "TWITCH_CLIENT_ID"
        | "TWITCH_BOT_CLIENT_ID"
        | "TWITCH_CLIENT_SECRET"
        | "TWITCH_INTERNAL_API_TOKEN"
        | "TURNIER_INTERNAL_API_TOKEN"
        | "TWITCH_PARTNER_TOKEN"
        | "MASTER_BROKER_TOKEN"
        | "MAIN_BOT_INTERNAL_TOKEN"
        | "DEADLOCK_BRAIN_READONLY_DSN"
        | "DEADLOCK_CENTRAL_DSN"
        | "STRIPE_SECRET_KEY"
        | "TWITCH_BILLING_STRIPE_SECRET_KEY"
        | "STRIPE_WEBHOOK_SECRET"
        | "TWITCH_BILLING_STRIPE_WEBHOOK_SECRET"
        | "STRIPE_CONNECT_CLIENT_ID"
        | "TWITCH_DEMO_LOGIN_USER"
        | "TWITCH_DEMO_LOGIN_PASSWORD_HASH"
        | "TWITCH_LEGAL_TURNSTILE_SECRET_KEY"
        | "TURNSTILE_SECRET_KEY"
        | "TWITCH_LEGAL_GATE_COOKIE_SECRET"
        | "LEGAL_GATE_COOKIE_SECRET" => crate::private::value(name).map(str::to_owned),
        _ => None,
    };
    value
        .filter(|value| !value.trim().is_empty())
        .ok_or(std::env::VarError::NotPresent)
}

/// Existing engagement consumer identifiers; all ordinary values are from TOML.
pub fn engagement_value(name: &str) -> Result<String, std::env::VarError> {
    let config = settings().map_err(|_| std::env::VarError::NotPresent)?;
    let options = &config.engagement;
    let value = match name {
        "VOICE_REACTION_STREAMLINK_BIN" => options.streamlink_binary.clone(),
        "FFMPEG_BIN" => options.ffmpeg_binary.clone(),
        "ENGAGEMENT_PERSONA_MODE" => options.persona_mode.clone(),
        "ENGAGEMENT_STREAM_TRANSCRIPTS_ENABLED" => if options.stream_transcripts_enabled {
            "1"
        } else {
            "0"
        }
        .into(),
        "ENGAGEMENT_MIN_PAUSE_SEC" => options.min_pause_seconds.to_string(),
        "ENGAGEMENT_BURST_LIMIT" => options.burst_limit.to_string(),
        "ENGAGEMENT_BURST_WINDOW_SEC" => options.burst_window_seconds.to_string(),
        "ENGAGEMENT_TRANSCRIPT_CAPTURE_SECONDS" => options.transcript_capture_seconds.to_string(),
        "ENGAGEMENT_TRANSCRIPT_INTERVAL_SECONDS" => options.transcript_interval_seconds.to_string(),
        "ENGAGEMENT_TRANSCRIPT_QUALITY" => options.transcript_quality.clone(),
        "ENGAGEMENT_TRANSCRIPT_PROMPT_MAX_CHARS" => options.transcript_prompt_max_chars.to_string(),
        "ENGAGEMENT_TRANSCRIPT_CONTEXT_MINUTES" => options.transcript_context_minutes.to_string(),
        "ENGAGEMENT_TRANSCRIPT_CONTEXT_LIMIT" => options.transcript_context_limit.to_string(),
        "ENGAGEMENT_TRANSCRIPT_RETENTION_MINUTES" => {
            options.transcript_retention_minutes.to_string()
        }
        "ENGAGEMENT_TRANSCRIPT_KEEP_PER_CHANNEL" => options.transcript_keep_per_channel.to_string(),
        "ENGAGEMENT_LEARN_ENABLED" => if options.learn_enabled { "1" } else { "0" }.into(),
        "ENGAGEMENT_LEARN_LOGIN" => options.learn_login.clone(),
        "ENGAGEMENT_LEARN_HOT_MINUTES" => options.learn_hot_minutes.to_string(),
        "ENGAGEMENT_LEARN_CAPTURE_SECONDS" => options.learn_capture_seconds.to_string(),
        "ENGAGEMENT_LEARN_MAX_CHANNELS" => options.learn_max_channels.to_string(),
        "ENGAGEMENT_LEARN_IDLE_CHANNELS" => options.learn_idle_channels.to_string(),
        "ENGAGEMENT_LEARN_RETENTION_HOURS" => options.learn_retention_hours.to_string(),
        "ENGAGEMENT_LEARN_WINDOW_PRE_SECONDS" => options.learn_window_pre_seconds.to_string(),
        "ENGAGEMENT_LEARN_WINDOW_POST_SECONDS" => options.learn_window_post_seconds.to_string(),
        "ENGAGEMENT_LEARN_CHAT_LINES" => options.learn_chat_lines.to_string(),
        "ENGAGEMENT_LEARN_CHAT_MINUTES" => options.learn_chat_minutes.to_string(),
        _ => return Err(std::env::VarError::NotPresent),
    };
    Ok(value)
}

pub fn start(
    arguments: impl IntoIterator<Item = std::ffi::OsString>,
) -> Result<(&'static BotConfigSnapshot, Vec<std::ffi::OsString>), FileError> {
    let arguments = crate::file::ConfigArguments::parse(arguments)?;
    let snapshot = BotConfigSnapshot::load(&arguments.path)?;
    // Quellrelative Betriebspfade prüfen, bevor ein Dienst Clients/Jobs startet.
    snapshot.resolve(&snapshot.settings().discord.streamer_link.state_path)?;
    snapshot.resolve(&snapshot.settings().knowledge.directory)?;
    snapshot.resolve(&snapshot.settings().vod_archive.download_dir)?;
    let bot = &snapshot.settings().bot;
    for path in [
        &bot.chat_review_log_directory,
        &bot.service_warning_log_directory,
    ] {
        snapshot.resolve(path)?;
    }
    for path in [
        &bot.yt_dlp_binary,
        &bot.outreach_yt_dlp_binary,
        &bot.obs_docks_config_path,
    ]
    .into_iter()
    .flatten()
    {
        snapshot.resolve(path)?;
    }
    Ok((install(snapshot)?, arguments.remaining))
}
