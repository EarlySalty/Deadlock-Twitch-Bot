//! Dashboard-Konfigurationsschicht des Go-Live-Builders.
//!
//! Port der reinen Helfer aus `bot/dashboard/live/live_announcement_mixin.py`:
//! die UI-seitige Default-Config, rekursives Merging und das Parsen von
//! Config-JSON. Diese Config-Form (UI-Schema) wird später via `to_template_config`
//! auf das gerenderte [`super::template`]-Schema abgebildet.

use serde_json::{json, Value};

/// Maximale Größe einer Preview-Config (Python `_MAX_PREVIEW_CONFIG_CHARS`).
pub const MAX_PREVIEW_CONFIG_CHARS: usize = 50_000;

/// UI-Default-Config (Python `_default_live_announcement_config`).
pub fn default_live_announcement_config() -> Value {
    json!({
        "content": "{rolle}",
        "mentions": { "enabled": true, "role_id": "" },
        "embed": {
            "color": "#9146ff",
            "author": {
                "enabled": true,
                "name": "LIVE: {channel}",
                "icon_mode": "twitch_logo",
                "link_to_channel": true,
            },
            "title": "{channel} ist LIVE in {game}!",
            "title_link_enabled": true,
            "description_mode": "stream_title",
            "description": "{title}",
            "shorten": false,
            "fields": [
                { "name": "Viewer", "value": "{viewer_count}", "inline": true },
                { "name": "Kategorie", "value": "{game}", "inline": true },
            ],
            "thumbnail": { "mode": "none", "custom_url": "" },
            "image": {
                "use_stream_thumbnail": true,
                "custom_url": "",
                "format": "16:9",
                "cache_buster": true,
            },
            "footer": {
                "text": "Auf Twitch ansehen für mehr Action!",
                "icon_mode": "none",
                "timestamp_mode": "started_at",
            },
        },
        "button": { "enabled": true, "label": "Auf Twitch ansehen", "url_template": "{url}" },
        "allowed_editor_role_ids": [],
    })
}

/// Rekursives Merge (Python `_deep_merge`): `dst` tief kopiert, dann je `src`-Key
/// — sind beide Werte Objekte, wird rekursiv gemergt, sonst überschrieben.
pub fn deep_merge(dst: &Value, src: &Value) -> Value {
    let mut out = dst.clone();
    if let (Some(out_map), Some(src_map)) = (out.as_object_mut(), src.as_object()) {
        for (key, value) in src_map {
            let merged = match (out_map.get(key), value) {
                (Some(existing @ Value::Object(_)), Value::Object(_)) => deep_merge(existing, value),
                _ => value.clone(),
            };
            out_map.insert(key.clone(), merged);
        }
    }
    out
}

/// Parst Config-JSON und mergt es über die Default-Config (Python
/// `_parse_config_json`): leer/ungültig/Nicht-Objekt → reine Default-Config.
pub fn parse_config_json(raw: &str) -> Value {
    let text = raw.trim();
    if text.is_empty() {
        return default_live_announcement_config();
    }
    match serde_json::from_str::<Value>(text) {
        Ok(parsed) if parsed.is_object() => deep_merge(&default_live_announcement_config(), &parsed),
        _ => default_live_announcement_config(),
    }
}

fn text(value: Option<&Value>, default: &str) -> String {
    match value {
        Some(Value::String(text)) if !text.trim().is_empty() => text.clone(),
        Some(Value::String(_)) | Some(Value::Null) | None => default.to_string(),
        Some(other) => other.to_string(),
    }
}

fn bool_value(value: Option<&Value>, default: bool) -> bool {
    match value {
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

fn positive_i64(value: Option<&Value>) -> Option<i64> {
    match value {
        Some(Value::Number(n)) => n.as_i64(),
        Some(Value::String(text)) => text.trim().parse::<i64>().ok(),
        _ => None,
    }
    .filter(|id| *id > 0)
}

/// Normalisiert das UI-Schema des Dashboard-Builders auf das Template-Schema.
/// Template-Schema ohne `embed` bleibt unverändert.
pub fn to_template_config(cfg: &Value) -> Value {
    if !cfg.is_object() {
        return json!({});
    }
    if cfg.get("embed").is_none() {
        return cfg.clone();
    }

    let null = Value::Null;
    let embed = cfg.get("embed").filter(|v| v.is_object()).unwrap_or(&null);
    let author = embed.get("author").filter(|v| v.is_object()).unwrap_or(&null);
    let footer = embed.get("footer").filter(|v| v.is_object()).unwrap_or(&null);
    let image = embed.get("image").filter(|v| v.is_object()).unwrap_or(&null);
    let thumbnail = embed.get("thumbnail").filter(|v| v.is_object()).unwrap_or(&null);
    let mentions = cfg.get("mentions").filter(|v| v.is_object()).unwrap_or(&null);
    let button = cfg.get("button").filter(|v| v.is_object()).unwrap_or(&null);

    let fields = embed
        .get("fields")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter(|item| item.is_object())
                .map(|item| {
                    json!({
                        "name_template": text(item.get("name"), ""),
                        "value_template": text(item.get("value"), ""),
                        "inline": bool_value(item.get("inline"), true),
                    })
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    let author_icon = text(author.get("icon_mode"), "none").trim().to_lowercase();
    let author_icon = if author_icon == "twitch_logo" || author_icon == "twitch" {
        "twitch".to_string()
    } else {
        author_icon
    };
    let thumbnail_mode = text(thumbnail.get("mode"), "none").trim().to_lowercase();
    let thumbnail_mode = if thumbnail_mode == "custom_url" {
        "custom".to_string()
    } else {
        thumbnail_mode
    };
    let image_custom_url = text(image.get("custom_url"), "");
    let image_mode = if bool_value(image.get("use_stream_thumbnail"), true) {
        "stream_thumbnail"
    } else if !image_custom_url.trim().is_empty() {
        "custom"
    } else {
        "none"
    };
    let footer_icon = text(footer.get("icon_mode"), "none").trim().to_lowercase();
    let footer_icon = if footer_icon == "twitch_logo" || footer_icon == "twitch" {
        "twitch"
    } else {
        "none"
    };
    let static_ping_role_ids = positive_i64(mentions.get("role_id"))
        .map(|id| vec![id])
        .unwrap_or_default();
    let allowed_editor_role_ids = cfg
        .get("allowed_editor_role_ids")
        .and_then(Value::as_array)
        .map(|items| items.iter().filter_map(|item| positive_i64(Some(item))).collect::<Vec<_>>())
        .unwrap_or_default();

    json!({
        "content_template": text(cfg.get("content"), "").replace("{rolle}", "{mention_role}"),
        "color": embed.get("color").cloned().unwrap_or_else(|| json!("#9146ff")),
        "author": {
            "name_template": text(author.get("name"), "LIVE: {channel}"),
            "icon_mode": author_icon,
            "link_to_stream": bool_value(author.get("link_to_channel"), true),
        },
        "title_template": text(embed.get("title"), "{channel} ist LIVE in {game}!"),
        "title_link_to_stream": bool_value(embed.get("title_link_enabled"), true),
        "description_mode": text(embed.get("description_mode"), "stream_title"),
        "description_template": text(embed.get("description"), "{title}"),
        "short_description": bool_value(embed.get("shorten"), false),
        "fields": fields,
        "images": {
            "thumbnail_mode": thumbnail_mode,
            "thumbnail_url_template": text(thumbnail.get("custom_url"), ""),
            "image_mode": image_mode,
            "image_url_template": image_custom_url,
            "image_ratio": text(image.get("format"), "16:9"),
            "cache_buster": bool_value(image.get("cache_buster"), true),
        },
        "footer": {
            "text_template": text(footer.get("text"), ""),
            "icon_mode": footer_icon,
            "timestamp_mode": text(footer.get("timestamp_mode"), "started_at"),
        },
        "button": {
            "enabled": bool_value(button.get("enabled"), true),
            "label_template": text(button.get("label"), "Auf Twitch ansehen"),
            "url_template": text(button.get("url_template"), "{url}"),
            "force_stream_url": true,
        },
        "mentions": {
            "use_streamer_ping_role": bool_value(mentions.get("enabled"), true),
            "streamer_ping_role_name_template": "{channel} ist live",
            "allowed_editor_role_ids": allowed_editor_role_ids,
            "static_ping_role_ids": static_ping_role_ids,
            "allow_everyone": false,
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_grundwerte() {
        let c = default_live_announcement_config();
        assert_eq!(c["content"], "{rolle}");
        assert_eq!(c["embed"]["color"], "#9146ff");
        assert_eq!(c["embed"]["author"]["name"], "LIVE: {channel}");
        assert_eq!(c["embed"]["fields"].as_array().unwrap().len(), 2);
        assert_eq!(c["button"]["label"], "Auf Twitch ansehen");
        assert_eq!(c["allowed_editor_role_ids"], json!([]));
    }

    #[test]
    fn deep_merge_rekursiv() {
        let dst = json!({ "a": { "x": 1, "y": 2 }, "k": 9 });
        let src = json!({ "a": { "y": 3, "z": 4 }, "b": 5 });
        // a.x bleibt, a.y überschrieben, a.z neu, b neu, k unberührt.
        assert_eq!(
            deep_merge(&dst, &src),
            json!({ "a": { "x": 1, "y": 3, "z": 4 }, "k": 9, "b": 5 })
        );
        // Nicht-Objekt überschreibt Objekt komplett.
        assert_eq!(
            deep_merge(&json!({ "a": { "x": 1 } }), &json!({ "a": 7 })),
            json!({ "a": 7 })
        );
    }

    #[test]
    fn parse_config_json_faelle() {
        // Leer → Default.
        assert_eq!(parse_config_json("   "), default_live_announcement_config());
        // Ungültiges JSON → Default.
        assert_eq!(parse_config_json("{kaputt"), default_live_announcement_config());
        // Nicht-Objekt (Array) → Default.
        assert_eq!(parse_config_json("[1,2]"), default_live_announcement_config());
        // Teil-Override → über Default gemergt.
        let merged = parse_config_json(r##"{"content": "Hallo", "embed": {"color": "#000000"}}"##);
        assert_eq!(merged["content"], "Hallo");
        assert_eq!(merged["embed"]["color"], "#000000");
        // Unberührte Default-Felder bleiben erhalten.
        assert_eq!(merged["embed"]["title"], "{channel} ist LIVE in {game}!");
        assert_eq!(merged["button"]["label"], "Auf Twitch ansehen");
    }
}
