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
