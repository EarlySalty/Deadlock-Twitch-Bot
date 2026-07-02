//! Port des Live-Announcement-Template-Systems (`bot/live_announce/template.py`):
//! Standard-Discord-Embed mit `{platzhalter}`-Rendering, Discord-Limits,
//! Thumbnail-Auflösung und stabilem Cache-Buster.
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

// ── Standard-Konfiguration ───────────────────────────────────────────────────

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

fn should_skip_zero_viewer_field(
    name: &str,
    value: &str,
    context: &BTreeMap<String, String>,
) -> bool {
    context
        .get("viewer_count")
        .is_some_and(|count| count.trim() == "0")
        && name.trim().eq_ignore_ascii_case("viewer")
        && value.trim() == "0"
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
    // Stream-Thumbnail als gemeinsamer Fallback (Python-Legacy `embeds_mixin.py`
    // :704: leeres/abwesendes image_cfg zeigt trotzdem das Stream-Preview).
    let stream_thumbnail = || {
        stream_thumbnail_url(
            context
                .get("stream_thumbnail_url")
                .map(String::as_str)
                .unwrap_or(""),
            &config.image_ratio,
            config.cache_buster,
            now,
            cache_buster_seed,
        )
    };
    let image_url = match config.image_mode.as_str() {
        "stream_thumbnail" => stream_thumbnail(),
        // Custom mit gesetztem Template gewinnt; leeres Template fällt zurück.
        "custom" => {
            let custom = render_placeholders(&config.image_url_template, context);
            if custom.is_empty() {
                stream_thumbnail()
            } else {
                custom
            }
        }
        // image_mode=none/unbekannt: B18-7-Fallback aufs Stream-Thumbnail.
        _ => stream_thumbnail(),
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
            .filter_map(|(name, value, inline)| {
                let name = render_placeholders(name, context);
                let value = render_placeholders(value, context);
                (!should_skip_zero_viewer_field(&name, &value, context)).then(|| {
                    serde_json::json!({
                        "name": name,
                        "value": value,
                        "inline": *inline,
                    })
                })
            })
            .take(MAX_FIELDS)
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
        content: sanitize_live_content(&render_placeholders(
            &sanitize_live_content_template(&config.content_template),
            context,
        ))
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

/// `@everyone`/`@here` neutralisieren (Python `_sanitize_live_content`,
/// `re.sub(r"@everyone", ..., flags=re.IGNORECASE)`).
///
/// Wirkt case-insensitiv über JEDE Schreibweise (`@EveryOne`, `@hErE`, …): ein
/// Zero-Width-Space wird zwischen `@` und das Schlüsselwort gesetzt, sodass
/// Discord den Mention nicht mehr auflöst, die Original-Schreibweise aber
/// erhalten bleibt. Bewusst dependency-frei (manueller ASCII-Scan), da die
/// Suchbegriffe reines ASCII sind.
pub fn sanitize_live_content(content: &str) -> String {
    /// Fügt nach jedem `@<keyword>` (case-insensitiv) ein Zero-Width-Space ein.
    fn neutralize(input: &str, keyword: &str) -> String {
        let bytes = input.as_bytes();
        let kw = keyword.as_bytes();
        let mut out = String::with_capacity(input.len());
        let mut i = 0;
        while i < bytes.len() {
            // Treffer nur, wenn `@` direkt vom Keyword (egal welche Schreibweise)
            // gefolgt wird.
            if bytes[i] == b'@'
                && i + 1 + kw.len() <= bytes.len()
                && bytes[i + 1..i + 1 + kw.len()].eq_ignore_ascii_case(kw)
            {
                out.push('@');
                out.push('\u{200b}');
                // Original-Schreibweise des Keywords beibehalten.
                out.push_str(&input[i + 1..i + 1 + kw.len()]);
                i += 1 + kw.len();
            } else {
                // Einzelnes UTF-8-Zeichen kopieren (Byte-Index ist Char-Grenze,
                // weil wir nur an `@`/ASCII-Treffern voranspringen).
                let ch = input[i..].chars().next().expect("char boundary");
                out.push(ch);
                i += ch.len_utf8();
            }
        }
        out
    }

    let out = neutralize(content, "everyone");
    neutralize(&out, "here")
}

/// Fügt bei Discord-Rollen-Mentions (`<@&123>`) nach `&` ein
/// Zero-Width-Space ein. Nur direkte Rollen-Mentions aus gespeicherten
/// Templates werden verändert; der generierte `{mention_role}`-Platzhalter
/// bleibt erlaubt und wird über Discord `allowed_mentions` begrenzt.
fn sanitize_live_content_template(template: &str) -> String {
    let input = sanitize_live_content(template);
    let bytes = input.as_bytes();
    let mut out = String::with_capacity(input.len());
    let mut i = 0;
    while i < bytes.len() {
        if i + 4 <= bytes.len() && &bytes[i..i + 3] == b"<@&" {
            let mut j = i + 3;
            while j < bytes.len() && bytes[j].is_ascii_digit() {
                j += 1;
            }
            if j > i + 3 && j < bytes.len() && bytes[j] == b'>' {
                out.push_str("<@&\u{200b}");
                out.push_str(&input[i + 3..j]);
                out.push('>');
                i = j + 1;
                continue;
            }
        }

        if let Some(ch) = input[i..].chars().next() {
            out.push(ch);
            i += ch.len_utf8();
        } else {
            break;
        }
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
            user_id: "0".to_string(),
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
    fn default_render_laesst_viewer_feld_bei_null_weg() {
        let config = AnnouncementConfig::default();
        let now = parse_dt_utc("2026-06-09T18:00:00Z").unwrap();
        let stream = StreamSnapshot {
            user_login: "drag".to_string(),
            user_name: "Drag".to_string(),
            title: "Ranked Grind".to_string(),
            game_name: "Deadlock".to_string(),
            viewer_count: 0,
            ..Default::default()
        };
        let ctx = build_context("drag", &stream, "https://www.twitch.tv/drag", "", now, None);
        let rendered = render_announcement(&config, &ctx, now, Some("token-1"));
        let fields = rendered.embed["fields"].as_array().expect("fields array");

        assert!(
            !fields.iter().any(|field| field["name"] == "Viewer"),
            "Viewer-Feld bei 0 nicht rendern"
        );
        assert!(
            fields.iter().any(|field| field["name"] == "Kategorie"),
            "Kategorie-Feld bleibt erhalten"
        );
    }

    /// B18-7 (`embeds_mixin-2`): Steht kein explizites Bild zur Verfügung
    /// (`image_mode != "stream_thumbnail"` und kein Custom-URL), fällt der
    /// Renderer auf das Stream-Thumbnail zurück — wie der Legacy-Pfad in Python
    /// (`embeds_mixin.py`:704, leeres image_cfg → Stream-Preview).
    #[test]
    fn image_mode_none_faellt_auf_stream_thumbnail_zurueck() {
        let now = parse_dt_utc("2026-06-09T18:00:00Z").unwrap();
        let stream = StreamSnapshot {
            user_login: "drag".to_string(),
            user_name: "Drag".to_string(),
            title: "Ranked Grind".to_string(),
            game_name: "Deadlock".to_string(),
            ..Default::default()
        };
        let ctx = build_context(
            "drag",
            &stream,
            "https://www.twitch.tv/drag",
            "",
            now,
            Some("https://cdn/{width}x{height}.jpg"),
        );

        // image_mode="none": Python-Legacy zeigt trotzdem das Stream-Thumbnail.
        let config = AnnouncementConfig {
            image_mode: "none".to_string(),
            cache_buster: false,
            ..AnnouncementConfig::default()
        };
        let rendered = render_announcement(&config, &ctx, now, Some("seed"));
        assert_eq!(
            rendered.embed["image"]["url"], "https://cdn/1280x720.jpg",
            "image_mode=none → Stream-Thumbnail-Fallback"
        );

        // image_mode="custom" mit leerem Template: ebenfalls Fallback.
        let config = AnnouncementConfig {
            image_mode: "custom".to_string(),
            image_url_template: String::new(),
            cache_buster: false,
            ..AnnouncementConfig::default()
        };
        let rendered = render_announcement(&config, &ctx, now, Some("seed"));
        assert_eq!(
            rendered.embed["image"]["url"], "https://cdn/1280x720.jpg",
            "image_mode=custom (leer) → Stream-Thumbnail-Fallback"
        );

        // Explizites Custom-Bild bleibt unangetastet (kein Fallback).
        let config = AnnouncementConfig {
            image_mode: "custom".to_string(),
            image_url_template: "https://custom/banner.png".to_string(),
            cache_buster: false,
            ..AnnouncementConfig::default()
        };
        let rendered = render_announcement(&config, &ctx, now, Some("seed"));
        assert_eq!(rendered.embed["image"]["url"], "https://custom/banner.png");
    }

    /// Fehlt das Stream-Thumbnail komplett, bleibt das Bild leer (kein Fallback
    /// auf einen leeren Wert, der ein kaputtes Embed-Feld erzeugen würde).
    #[test]
    fn fallback_ohne_thumbnail_laesst_image_leer() {
        let now = parse_dt_utc("2026-06-09T18:00:00Z").unwrap();
        let stream = StreamSnapshot {
            user_login: "drag".to_string(),
            ..Default::default()
        };
        let ctx = build_context("drag", &stream, "https://t/drag", "", now, None);
        let config = AnnouncementConfig {
            image_mode: "none".to_string(),
            ..AnnouncementConfig::default()
        };
        let rendered = render_announcement(&config, &ctx, now, Some("seed"));
        assert!(
            rendered.embed.get("image").is_none(),
            "ohne Stream-Thumbnail kein Image-Feld"
        );
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
    fn sanitize_neutralisiert_mixed_case_everyone_here() {
        // P1.16/P1.48: Python `re.IGNORECASE` neutralisiert JEDE Schreibweise.
        let out = sanitize_live_content("@EveryOne @hErE @eVeRyOnE");
        // Kein rohes @everyone/@here (case-insensitiv) darf durchrutschen.
        let lower = out.to_lowercase();
        assert!(
            !lower.contains("@everyone"),
            "rohes @everyone (mixed-case) nicht neutralisiert: {out}"
        );
        assert!(
            !lower.contains("@here"),
            "rohes @here (mixed-case) nicht neutralisiert: {out}"
        );
        // Die Original-Schreibweise bleibt erhalten, nur mit Zero-Width-Joiner.
        assert_eq!(out, "@\u{200b}EveryOne @\u{200b}hErE @\u{200b}eVeRyOnE");
    }

    #[test]
    fn render_neutralisiert_direkte_rollen_mentions_aber_nicht_mention_role() {
        let mut config = AnnouncementConfig {
            content_template: "Ping <@&1234567890> {mention_role} @everyone".to_string(),
            ..AnnouncementConfig::default()
        };
        config.fields.clear();
        let now = parse_dt_utc("2026-06-09T18:00:00Z").unwrap();
        let ctx = ctx_with(&[
            ("mention_role", "<@&42>"),
            ("url", "https://www.twitch.tv/drag"),
        ]);

        let out = render_announcement(&config, &ctx, now, Some("seed")).content;
        assert!(
            !out.contains("<@&1234567890>") && !out.to_lowercase().contains("@everyone"),
            "direkte disallowed Mentions nicht neutralisiert: {out}"
        );
        assert_eq!(out, "Ping <@&\u{200b}1234567890> <@&42> @\u{200b}everyone");
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
