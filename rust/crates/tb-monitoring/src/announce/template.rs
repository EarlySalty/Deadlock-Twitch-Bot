//! Port des Live-Announcement-Template-Systems (`bot/live_announce/template.py`):
//! per-Streamer konfigurierbare Discord-Embeds mit `{platzhalter}`-Rendering,
//! Discord-Limits, Thumbnail-Auflösung und stabilem Cache-Buster.
//! Reine Logik — kein I/O.

use std::collections::BTreeMap;

use chrono::{DateTime, SecondsFormat, Utc};
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::stream::{parse_dt_utc, StreamSnapshot};

pub const TWITCH_BRAND_COLOR: i64 = 0x9146FF;
pub const TWITCH_ICON_URL: &str =
    "https://static.twitchcdn.net/assets/favicon-32-e29e246c157142c94346.png";
pub const TWITCH_BUTTON_LABEL: &str = "Auf Twitch ansehen";
pub const TWITCH_VOD_BUTTON_LABEL: &str = "VOD anschauen";

const MAX_DESCRIPTION: usize = 4096;
const MAX_FIELDS: usize = 25;

// ── Konfiguration (JSON aus `twitch_live_announcement_configs`) ──────────────

fn s(v: Option<&Value>, default: &str) -> String {
    match v {
        Some(Value::String(text)) => {
            let trimmed = text.trim();
            if trimmed.is_empty() {
                default.to_string()
            } else {
                text.clone()
            }
        }
        Some(Value::Null) | None => default.to_string(),
        Some(other) => other.to_string(),
    }
}

fn b(v: Option<&Value>, default: bool) -> bool {
    match v {
        Some(Value::Bool(value)) => *value,
        Some(Value::Number(n)) => n.as_i64().map(|x| x != 0).unwrap_or(default),
        Some(Value::String(raw)) => match raw.trim().to_lowercase().as_str() {
            "1" | "true" | "yes" | "on" => true,
            "0" | "false" | "no" | "off" => false,
            _ => default,
        },
        _ => default,
    }
}

#[derive(Debug, Clone)]
pub struct AnnouncementConfig {
    pub content_template: String,
    pub color: Value,
    pub author_name_template: String,
    pub author_icon_mode: String,
    pub author_link_to_stream: bool,
    pub title_template: String,
    pub title_link_to_stream: bool,
    pub description_mode: String,
    pub description_template: String,
    pub short_description: bool,
    pub fields: Vec<(String, String, bool)>,
    pub thumbnail_mode: String,
    pub thumbnail_url_template: String,
    pub image_mode: String,
    pub image_url_template: String,
    pub image_ratio: String,
    pub cache_buster: bool,
    pub footer_text_template: String,
    pub footer_icon_mode: String,
    pub footer_timestamp_mode: String,
    pub button_enabled: bool,
    pub button_label_template: String,
    pub use_streamer_ping_role: bool,
    pub static_ping_role_ids: Vec<i64>,
}

impl Default for AnnouncementConfig {
    fn default() -> Self {
        Self {
            content_template: "{mention_role}".to_string(),
            color: Value::from(TWITCH_BRAND_COLOR),
            author_name_template: "LIVE: {channel}".to_string(),
            author_icon_mode: "twitch".to_string(),
            author_link_to_stream: true,
            title_template: "{channel} ist LIVE in {game}!".to_string(),
            title_link_to_stream: true,
            description_mode: "stream_title".to_string(),
            description_template: "{title}".to_string(),
            short_description: false,
            fields: vec![
                ("Viewer".to_string(), "{viewer_count}".to_string(), true),
                ("Kategorie".to_string(), "{game}".to_string(), true),
            ],
            thumbnail_mode: "channel_avatar".to_string(),
            thumbnail_url_template: String::new(),
            image_mode: "stream_thumbnail".to_string(),
            image_url_template: String::new(),
            image_ratio: "16:9".to_string(),
            cache_buster: true,
            footer_text_template: "Auf Twitch ansehen für mehr Action!".to_string(),
            footer_icon_mode: "twitch".to_string(),
            footer_timestamp_mode: "started_at".to_string(),
            button_enabled: true,
            button_label_template: TWITCH_BUTTON_LABEL.to_string(),
            use_streamer_ping_role: true,
            static_ping_role_ids: Vec::new(),
        }
    }
}

impl AnnouncementConfig {
    /// Konfiguration aus dem gespeicherten JSON (fehlende Felder → Defaults,
    /// Python `LiveAnnouncementConfig.from_dict`).
    pub fn from_json(raw: &Value) -> Self {
        let defaults = Self::default();
        let author = raw.get("author").cloned().unwrap_or(Value::Null);
        let images = raw.get("images").cloned().unwrap_or(Value::Null);
        let footer = raw.get("footer").cloned().unwrap_or(Value::Null);
        let button = raw.get("button").cloned().unwrap_or(Value::Null);
        let mentions = raw.get("mentions").cloned().unwrap_or(Value::Null);

        let mut fields: Vec<(String, String, bool)> = Vec::new();
        if let Some(Value::Array(items)) = raw.get("fields") {
            for item in items {
                fields.push((
                    s(item.get("name_template"), "Info"),
                    s(item.get("value_template"), "-"),
                    b(item.get("inline"), true),
                ));
            }
        }
        if fields.is_empty() {
            fields = defaults.fields.clone();
        }
        let static_ping_role_ids = mentions
            .get("static_ping_role_ids")
            .and_then(Value::as_array)
            .map(|items| {
                items
                    .iter()
                    .filter_map(|item| match item {
                        Value::Number(n) => n.as_i64(),
                        Value::String(text) => text.trim().parse::<i64>().ok(),
                        _ => None,
                    })
                    .filter(|id| *id > 0)
                    .collect()
            })
            .unwrap_or_default();

        Self {
            content_template: s(raw.get("content_template"), &defaults.content_template),
            color: raw.get("color").cloned().unwrap_or(defaults.color.clone()),
            author_name_template: s(author.get("name_template"), &defaults.author_name_template),
            author_icon_mode: s(author.get("icon_mode"), &defaults.author_icon_mode).to_lowercase(),
            author_link_to_stream: b(author.get("link_to_stream"), true),
            title_template: s(raw.get("title_template"), &defaults.title_template),
            title_link_to_stream: b(raw.get("title_link_to_stream"), true),
            description_mode: s(raw.get("description_mode"), &defaults.description_mode)
                .to_lowercase(),
            description_template: s(
                raw.get("description_template"),
                &defaults.description_template,
            ),
            short_description: b(raw.get("short_description"), false),
            fields,
            thumbnail_mode: s(images.get("thumbnail_mode"), &defaults.thumbnail_mode)
                .to_lowercase(),
            thumbnail_url_template: s(images.get("thumbnail_url_template"), ""),
            image_mode: s(images.get("image_mode"), &defaults.image_mode).to_lowercase(),
            image_url_template: s(images.get("image_url_template"), ""),
            image_ratio: s(images.get("image_ratio"), &defaults.image_ratio),
            cache_buster: b(images.get("cache_buster"), true),
            footer_text_template: s(footer.get("text_template"), &defaults.footer_text_template),
            footer_icon_mode: s(footer.get("icon_mode"), &defaults.footer_icon_mode).to_lowercase(),
            footer_timestamp_mode: s(
                footer.get("timestamp_mode"),
                &defaults.footer_timestamp_mode,
            )
            .to_lowercase(),
            button_enabled: b(button.get("enabled"), true),
            button_label_template: s(button.get("label_template"), {
                // Legacy-Konfigs nutzen `label` statt `label_template`.
                &s(button.get("label"), &defaults.button_label_template)
            }),
            use_streamer_ping_role: b(mentions.get("use_streamer_ping_role"), true),
            static_ping_role_ids,
        }
    }
}

// ── Kontext + Rendering ───────────────────────────────────────────────────────

/// `{platzhalter}` ersetzen; unbekannte Platzhalter bleiben stehen (Python).
pub fn render_placeholders(template: &str, context: &BTreeMap<String, String>) -> String {
    let mut out = String::with_capacity(template.len());
    let mut rest = template;
    while let Some(start) = rest.find('{') {
        out.push_str(&rest[..start]);
        let tail = &rest[start..];
        if let Some(end) = tail.find('}') {
            let key = &tail[1..end];
            if !key.is_empty() && key.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
                if let Some(value) = context.get(key) {
                    out.push_str(value);
                } else {
                    out.push_str(&tail[..=end]);
                }
                rest = &tail[end + 1..];
                continue;
            }
        }
        out.push('{');
        rest = &tail[1..];
    }
    out.push_str(rest);
    out
}

/// Embed-Farbe parsen (int, `#hex`, `0xhex`, dezimal — Python `parse_embed_color`).
pub fn parse_embed_color(value: &Value) -> i64 {
    let fallback = TWITCH_BRAND_COLOR;
    match value {
        Value::Number(n) => n
            .as_i64()
            .filter(|v| (0..=0xFFFFFF).contains(v))
            .unwrap_or(fallback),
        Value::String(raw) => {
            let mut text = raw.trim().to_lowercase();
            let base = if let Some(stripped) = text.strip_prefix('#') {
                text = stripped.to_string();
                16
            } else if let Some(stripped) = text.strip_prefix("0x") {
                text = stripped.to_string();
                16
            } else if !text.is_empty() && text.chars().all(|c| c.is_ascii_digit()) {
                10
            } else if !text.is_empty() && text.chars().all(|c| c.is_ascii_hexdigit()) {
                16
            } else {
                return fallback;
            };
            i64::from_str_radix(&text, base)
                .ok()
                .filter(|v| (0..=0xFFFFFF).contains(v))
                .unwrap_or(fallback)
        }
        _ => fallback,
    }
}

fn fmt_uptime(started_at: Option<DateTime<Utc>>, now: DateTime<Utc>) -> String {
    let Some(started_at) = started_at else {
        return "0m".to_string();
    };
    let delta = (now - started_at).num_seconds().max(0);
    let hours = delta / 3600;
    let minutes = (delta % 3600) / 60;
    if hours > 0 {
        format!("{hours}h {minutes:02}m")
    } else {
        format!("{minutes}m")
    }
}

fn stable_cache_buster(seed: Option<&str>, now: DateTime<Utc>) -> String {
    match seed.map(str::trim).filter(|s| !s.is_empty()) {
        Some(seed) => {
            let mut hasher = Sha256::new();
            hasher.update(seed.as_bytes());
            hex::encode(hasher.finalize())[..16].to_string()
        }
        None => now.timestamp().to_string(),
    }
}

fn stream_thumbnail_url(
    raw_url: &str,
    ratio: &str,
    cache_buster: bool,
    now: DateTime<Utc>,
    seed: Option<&str>,
) -> String {
    if raw_url.is_empty() {
        return String::new();
    }
    let (width, height) = if ratio == "4:3" {
        (960, 720)
    } else {
        (1280, 720)
    };
    let resolved = raw_url
        .replace("{width}", &width.to_string())
        .replace("{height}", &height.to_string());
    if !cache_buster {
        return resolved;
    }
    let separator = if resolved.contains('?') { '&' } else { '?' };
    format!("{resolved}{separator}cb={}", stable_cache_buster(seed, now))
}

fn shorten_text(text: &str, max_length: usize) -> String {
    if text.chars().count() <= max_length {
        return text.to_string();
    }
    let suffix = "...";
    let keep = max_length.saturating_sub(suffix.len());
    let truncated: String = text.chars().take(keep).collect();
    format!("{truncated}{suffix}")
}

/// Template-Kontext aus dem Stream-Payload (Python `build_template_context`),
/// erweitert um `mention_role`/`rolle` und die Referral-URL.
pub fn build_context(
    login: &str,
    stream: &StreamSnapshot,
    referral_url: &str,
    mention_text: &str,
    now: DateTime<Utc>,
    thumbnail_url: Option<&str>,
) -> BTreeMap<String, String> {
    let channel = if stream.user_name.trim().is_empty() {
        login.to_string()
    } else {
        stream.user_name.clone()
    };
    let started_at = stream.started_at.as_deref().and_then(parse_dt_utc);
    let mut ctx = BTreeMap::new();
    ctx.insert("channel".to_string(), channel);
    ctx.insert("login".to_string(), login.to_lowercase());
    ctx.insert("url".to_string(), referral_url.to_string());
    ctx.insert(
        "title".to_string(),
        Some(stream.title.trim())
            .filter(|t| !t.is_empty())
            .unwrap_or("Live!")
            .to_string(),
    );
    ctx.insert("viewer_count".to_string(), stream.viewer_count.to_string());
    ctx.insert(
        "started_at".to_string(),
        started_at
            .map(|dt| dt.to_rfc3339_opts(SecondsFormat::Secs, false))
            .unwrap_or_default(),
    );
    ctx.insert(
        "language".to_string(),
        Some(stream.language.trim())
            .filter(|l| !l.is_empty())
            .unwrap_or("de")
            .to_string(),
    );
    ctx.insert(
        "tags".to_string(),
        stream
            .tags
            .iter()
            .map(|t| t.trim())
            .filter(|t| !t.is_empty())
            .collect::<Vec<_>>()
            .join(", "),
    );
    ctx.insert("uptime".to_string(), fmt_uptime(started_at, now));
    ctx.insert(
        "game".to_string(),
        Some(stream.game_name.trim())
            .filter(|g| !g.is_empty())
            .unwrap_or("Deadlock")
            .to_string(),
    );
    ctx.insert(
        "stream_thumbnail_url".to_string(),
        thumbnail_url.unwrap_or("").to_string(),
    );
    // Helix /streams liefert kein Profilbild — leer wie im Poll-Pfad von Python.
    ctx.insert("channel_avatar_url".to_string(), String::new());
    ctx.insert(
        "now".to_string(),
        now.to_rfc3339_opts(SecondsFormat::Secs, false),
    );
    ctx.insert("mention_role".to_string(), mention_text.to_string());
    ctx.insert("rolle".to_string(), mention_text.to_string());
    ctx
}

/// Gerendertes Announcement: Content + Discord-Embed-Dict + Button.
#[derive(Debug, Clone)]
pub struct RenderedAnnouncement {
    pub content: String,
    pub embed: Value,
    pub button_label: String,
    pub button_enabled: bool,
}

/// Discord-Embed rendern (Python `render_announcement_payload` — Limits,
/// Bild-Modi, Footer-Timestamp).
pub fn render_announcement(
    config: &AnnouncementConfig,
    context: &BTreeMap<String, String>,
    now: DateTime<Utc>,
    cache_buster_seed: Option<&str>,
) -> RenderedAnnouncement {
    let title = render_placeholders(&config.title_template, context);
    let stream_title = context.get("title").cloned().unwrap_or_default();
    let description_custom = render_placeholders(&config.description_template, context);
    let mut description = match config.description_mode.as_str() {
        "custom" => description_custom,
        "custom_plus_title" => {
            if !description_custom.is_empty() && !stream_title.is_empty() {
                format!("{description_custom}\n\n{stream_title}")
            } else {
                description_custom
            }
        }
        _ => {
            if stream_title.is_empty() {
                description_custom
            } else {
                stream_title.clone()
            }
        }
    };
    if config.short_description {
        description = shorten_text(&description, MAX_DESCRIPTION);
    }

    let author_icon_url = match config.author_icon_mode.as_str() {
        "twitch" => TWITCH_ICON_URL.to_string(),
        "channel_avatar" => context
            .get("channel_avatar_url")
            .cloned()
            .unwrap_or_default(),
        _ => String::new(),
    };
    let thumbnail_url = match config.thumbnail_mode.as_str() {
        "custom" => render_placeholders(&config.thumbnail_url_template, context),
        "channel_avatar" => context
            .get("channel_avatar_url")
            .cloned()
            .unwrap_or_default(),
        _ => String::new(),
    };
    let image_url = match config.image_mode.as_str() {
        "custom" => render_placeholders(&config.image_url_template, context),
        "stream_thumbnail" => stream_thumbnail_url(
            context
                .get("stream_thumbnail_url")
                .map(String::as_str)
                .unwrap_or(""),
            &config.image_ratio,
            config.cache_buster,
            now,
            cache_buster_seed,
        ),
        _ => String::new(),
    };

    let timestamp = match config.footer_timestamp_mode.as_str() {
        "now" => Some(now.to_rfc3339_opts(SecondsFormat::Secs, false)),
        "started_at" => Some(
            context
                .get("started_at")
                .map(String::as_str)
                .filter(|v| !v.is_empty())
                .map(str::to_string)
                .unwrap_or_else(|| now.to_rfc3339_opts(SecondsFormat::Secs, false)),
        ),
        _ => None,
    };

    let url = context.get("url").cloned().unwrap_or_default();
    let mut embed = serde_json::json!({
        "title": title,
        "description": description,
        "color": parse_embed_color(&config.color),
        "author": {
            "name": render_placeholders(&config.author_name_template, context),
        },
        "fields": config
            .fields
            .iter()
            .take(MAX_FIELDS)
            .map(|(name, value, inline)| {
                serde_json::json!({
                    "name": render_placeholders(name, context),
                    "value": render_placeholders(value, context),
                    "inline": inline,
                })
            })
            .collect::<Vec<_>>(),
        "footer": {
            "text": render_placeholders(&config.footer_text_template, context),
        },
    });
    if config.title_link_to_stream && !url.is_empty() {
        embed["url"] = Value::from(url.clone());
    }
    if config.author_link_to_stream && !url.is_empty() {
        embed["author"]["url"] = Value::from(url.clone());
    }
    if !author_icon_url.is_empty() {
        embed["author"]["icon_url"] = Value::from(author_icon_url);
    }
    if config.footer_icon_mode == "twitch" {
        embed["footer"]["icon_url"] = Value::from(TWITCH_ICON_URL);
    }
    if !thumbnail_url.is_empty() {
        embed["thumbnail"] = serde_json::json!({ "url": thumbnail_url });
    }
    if !image_url.is_empty() {
        embed["image"] = serde_json::json!({ "url": image_url });
    }
    if let Some(timestamp) = timestamp {
        embed["timestamp"] = Value::from(timestamp);
    }

    RenderedAnnouncement {
        content: render_placeholders(&config.content_template, context)
            .trim()
            .to_string(),
        embed,
        button_label: {
            let label = render_placeholders(&config.button_label_template, context);
            let label = label.trim();
            if label.is_empty() {
                TWITCH_BUTTON_LABEL.to_string()
            } else {
                label.chars().take(80).collect()
            }
        },
        button_enabled: config.button_enabled,
    }
}

/// `@everyone`/`@here` neutralisieren (Python `_sanitize_live_content`).
pub fn sanitize_live_content(content: &str) -> String {
    let mut out = content.to_string();
    for (needle, replacement) in [
        ("@everyone", "@\u{200b}everyone"),
        ("@Everyone", "@\u{200b}Everyone"),
        ("@EVERYONE", "@\u{200b}EVERYONE"),
        ("@here", "@\u{200b}here"),
        ("@Here", "@\u{200b}Here"),
        ("@HERE", "@\u{200b}HERE"),
    ] {
        out = out.replace(needle, replacement);
    }
    out
}

/// Offline-/VOD-Embed (Python `_build_offline_embed` — gleicher Stil, klar
/// als VOD markiert).
pub fn build_offline_embed(
    display_name: &str,
    last_title: Option<&str>,
    last_game: Option<&str>,
    preview_image_url: Option<&str>,
    target_game: &str,
    now: DateTime<Utc>,
) -> Value {
    let game = last_game
        .map(str::trim)
        .filter(|g| !g.is_empty())
        .unwrap_or(if target_game.is_empty() {
            "Twitch"
        } else {
            target_game
        });
    let description = last_title
        .map(str::trim)
        .filter(|t| !t.is_empty())
        .unwrap_or("Letzten Stream als VOD ansehen.");
    let mut embed = serde_json::json!({
        "title": format!("{display_name} ist OFFLINE"),
        "description": description,
        "color": TWITCH_BRAND_COLOR,
        "timestamp": now.to_rfc3339_opts(SecondsFormat::Secs, false),
        "fields": [
            {"name": "Kategorie", "value": game, "inline": true},
            {"name": "Hinweis", "value": "VOD über den Button abrufen.", "inline": false},
        ],
        "footer": {"text": "Letzten Stream auf Twitch ansehen."},
        "author": {"name": display_name},
    });
    if let Some(url) = preview_image_url.map(str::trim).filter(|u| !u.is_empty()) {
        embed["image"] = serde_json::json!({ "url": url });
    }
    embed
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx_with(entries: &[(&str, &str)]) -> BTreeMap<String, String> {
        entries
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    #[test]
    fn platzhalter_rendering_laesst_unbekannte_stehen() {
        let ctx = ctx_with(&[("channel", "Drag"), ("game", "Deadlock")]);
        assert_eq!(
            render_placeholders("{channel} ist LIVE in {game}! {unbekannt}", &ctx),
            "Drag ist LIVE in Deadlock! {unbekannt}"
        );
    }

    #[test]
    fn farbe_aus_hex_und_fallback() {
        assert_eq!(parse_embed_color(&Value::from("#ff0000")), 0xFF0000);
        assert_eq!(parse_embed_color(&Value::from("0x9146ff")), 0x9146FF);
        assert_eq!(parse_embed_color(&Value::from(123456)), 123456);
        assert_eq!(
            parse_embed_color(&Value::from("quatsch")),
            TWITCH_BRAND_COLOR
        );
        assert_eq!(parse_embed_color(&Value::from(-5)), TWITCH_BRAND_COLOR);
    }

    #[test]
    fn default_render_entspricht_python_struktur() {
        let config = AnnouncementConfig::default();
        let now = parse_dt_utc("2026-06-09T18:00:00Z").unwrap();
        let stream = StreamSnapshot {
            user_login: "drag".to_string(),
            user_name: "Drag".to_string(),
            title: "Ranked Grind".to_string(),
            game_name: "Deadlock".to_string(),
            viewer_count: 42,
            started_at: Some("2026-06-09T17:30:00Z".to_string()),
            ..Default::default()
        };
        let ctx = build_context(
            "drag",
            &stream,
            "https://www.twitch.tv/drag?ref=dc",
            "<@&99>",
            now,
            Some("https://cdn/{width}x{height}.jpg"),
        );
        assert_eq!(ctx["uptime"], "30m");
        let rendered = render_announcement(&config, &ctx, now, Some("token-1"));
        assert_eq!(rendered.content, "<@&99>");
        assert_eq!(rendered.embed["title"], "Drag ist LIVE in Deadlock!");
        assert_eq!(rendered.embed["description"], "Ranked Grind");
        assert_eq!(rendered.embed["url"], "https://www.twitch.tv/drag?ref=dc");
        assert_eq!(rendered.embed["color"], TWITCH_BRAND_COLOR);
        assert_eq!(rendered.embed["fields"][0]["value"], "42");
        assert_eq!(rendered.embed["fields"][1]["value"], "Deadlock");
        let image = rendered.embed["image"]["url"].as_str().unwrap();
        assert!(image.starts_with("https://cdn/1280x720.jpg?cb="), "{image}");
        // Footer-Timestamp = started_at (Default-Modus).
        assert_eq!(rendered.embed["timestamp"], "2026-06-09T17:30:00+00:00");
        assert_eq!(rendered.button_label, "Auf Twitch ansehen");
    }

    #[test]
    fn cache_buster_ist_stabil_pro_seed() {
        let now = Utc::now();
        let a = stream_thumbnail_url(
            "https://x/{width}x{height}.jpg",
            "16:9",
            true,
            now,
            Some("s1"),
        );
        let b = stream_thumbnail_url(
            "https://x/{width}x{height}.jpg",
            "16:9",
            true,
            now,
            Some("s1"),
        );
        assert_eq!(a, b, "gleicher Seed → gleicher Buster (Retry-Stabilität)");
    }

    #[test]
    fn sanitize_neutralisiert_everyone() {
        assert_eq!(
            sanitize_live_content("Hey @everyone und @here!"),
            "Hey @\u{200b}everyone und @\u{200b}here!"
        );
    }

    #[test]
    fn offline_embed_struktur() {
        let now = parse_dt_utc("2026-06-09T18:00:00Z").unwrap();
        let embed = build_offline_embed(
            "Drag",
            Some("Letzter Titel"),
            None,
            Some("https://vod/p.jpg"),
            "Deadlock",
            now,
        );
        assert_eq!(embed["title"], "Drag ist OFFLINE");
        assert_eq!(embed["fields"][0]["value"], "Deadlock");
        assert_eq!(embed["image"]["url"], "https://vod/p.jpg");
    }
}
