//! Layout-Modell + Persistenz (Port von `bot/social_media/layout/`).
//!
//! Beschreibt das Compositing-Layout eines Clips. Zwei Koordinatenräume, die
//! nicht verwechselt werden dürfen:
//!
//! * `game_crop` und `cam_crop` sind Ausschnitte AUS dem Twitch-Bild und liegen
//!   im Quellraum (`source`, üblich 1920x1080).
//! * `cam_position` ist das Zielrechteck IM fertigen Hochformat-Frame
//!   ([`TARGET_WIDTH`] x [`TARGET_HEIGHT`], also 1080x1920).
//!
//! Ein Streamer hat ein Default-Layout in `social_media_streamer_layout`;
//! einzelne Clips können es über `twitch_clips_social_media.layout_override_json`
//! überschreiben.
//!
//! JSON-Schema (gespeichert):
//! ```json
//! { "version": 1, "source": {"width":1920,"height":1080},
//!   "game_crop": {"x":0,"y":0,"w":1080,"h":1080},
//!   "cam_crop": {"x":1500,"y":50,"w":380,"h":380},
//!   "cam_position": {"x":712,"y":48,"w":320,"h":320} }
//! ```
//! Validierung: version==1, x/y>=0, w/h>0, Crops innerhalb `source`,
//! `cam_position` innerhalb des Zielframes, mode ∈ {pip, stacked}.
//!
//! Kompatibilität: Layouts aus der Zeit, als `cam_position` im Quellraum
//! validiert wurde, können Werte tragen, die im Zielframe nicht mehr passen.
//! [`StreamerLayout::from_stored_value`] clampt sie beim Lesen, damit ein
//! bestehendes Layout nicht stillschweigend auf den globalen Default kippt;
//! neue Eingaben über die API laufen weiter streng über
//! [`StreamerLayout::from_value`].

use serde_json::{json, Value};
use sqlx::PgPool;

/// Breite des fertigen Hochformat-Frames (9:16).
pub const TARGET_WIDTH: i64 = 1080;
/// Höhe des fertigen Hochformat-Frames (9:16).
pub const TARGET_HEIGHT: i64 = 1920;

#[derive(Debug, thiserror::Error)]
#[error("{0}")]
pub struct LayoutValidationError(pub String);

fn err(msg: impl Into<String>) -> LayoutValidationError {
    LayoutValidationError(msg.into())
}

/// Eine rechteckige Box (Crop oder Position).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LayoutBox {
    pub x: i64,
    pub y: i64,
    pub w: i64,
    pub h: i64,
}

impl LayoutBox {
    fn from_value(name: &str, payload: Option<&Value>) -> Result<Self, LayoutValidationError> {
        let obj = payload
            .filter(|v| v.is_object())
            .ok_or_else(|| err(format!("{name} must be an object")))?;
        let x = require_int(obj.get("x"), &format!("{name}.x"))?;
        let y = require_int(obj.get("y"), &format!("{name}.y"))?;
        let w = require_int(obj.get("w"), &format!("{name}.w"))?;
        let h = require_int(obj.get("h"), &format!("{name}.h"))?;
        if x < 0 {
            return Err(err(format!("{name}.x must be >= 0")));
        }
        if y < 0 {
            return Err(err(format!("{name}.y must be >= 0")));
        }
        if w <= 0 {
            return Err(err(format!("{name}.w must be > 0")));
        }
        if h <= 0 {
            return Err(err(format!("{name}.h must be > 0")));
        }
        Ok(Self { x, y, w, h })
    }

    /// Prüft, dass die Box vollständig in einem Rahmen liegt. `frame` benennt
    /// den Bezugsrahmen in der Fehlermeldung, damit im Dashboard nicht "source"
    /// steht, wenn der Zielframe gemeint ist.
    fn validate_within(
        &self,
        width: i64,
        height: i64,
        name: &str,
        frame: &str,
    ) -> Result<(), LayoutValidationError> {
        if self.x + self.w > width {
            return Err(err(format!(
                "{name}.x + {name}.w must be <= {frame}.width ({width})"
            )));
        }
        if self.y + self.h > height {
            return Err(err(format!(
                "{name}.y + {name}.h must be <= {frame}.height ({height})"
            )));
        }
        Ok(())
    }

    /// Schiebt und schrumpft die Box in den Rahmen, statt sie abzulehnen.
    /// Nur für gespeicherte Layouts (siehe Modul-Doku), nie für neue Eingaben.
    fn clamped_within(self, width: i64, height: i64) -> Self {
        let w = self.w.min(width).max(1);
        let h = self.h.min(height).max(1);
        Self {
            x: self.x.min(width - w).max(0),
            y: self.y.min(height - h).max(0),
            w,
            h,
        }
    }

    /// Zielrechteck, garantiert innerhalb des Hochformat-Frames. Der Renderer
    /// ruft das defensiv auf, damit ein Altlayout keinen Overlay ausserhalb des
    /// Bildes erzeugt.
    pub fn clamped_to_target(self) -> Self {
        self.clamped_within(TARGET_WIDTH, TARGET_HEIGHT)
    }

    fn to_json(self) -> Value {
        json!({ "x": self.x, "y": self.y, "w": self.w, "h": self.h })
    }
}

/// Quell-Auflösung des Layouts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LayoutSource {
    pub width: i64,
    pub height: i64,
}

impl LayoutSource {
    fn from_value(payload: Option<&Value>) -> Result<Self, LayoutValidationError> {
        let obj = payload
            .filter(|v| v.is_object())
            .ok_or_else(|| err("source must be an object"))?;
        let width = require_int(obj.get("width"), "source.width")?;
        let height = require_int(obj.get("height"), "source.height")?;
        if width <= 0 {
            return Err(err("source.width must be > 0"));
        }
        if height <= 0 {
            return Err(err("source.height must be > 0"));
        }
        Ok(Self { width, height })
    }

    fn to_json(self) -> Value {
        json!({ "width": self.width, "height": self.height })
    }
}

/// Vollständiges Streamer-/Clip-Layout.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StreamerLayout {
    pub version: i64,
    pub source: LayoutSource,
    pub game_crop: LayoutBox,
    pub cam_crop: LayoutBox,
    pub cam_position: LayoutBox,
    pub cam_enabled: bool,
    pub mode: String,
}

/// Wie mit einem `cam_position` umgegangen wird, das nicht in den Zielframe passt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CamPositionPolicy {
    /// Neue Eingaben: ablehnen.
    Strict,
    /// Gespeicherte Layouts: in den Zielframe schieben.
    Clamp,
}

impl StreamerLayout {
    /// Parst + validiert ein Layout aus JSON. `cam_enabled`/`mode` überschreiben
    /// die Werte aus dem Payload (für die getrennt gespeicherten DB-Spalten).
    /// Streng: ein `cam_position` ausserhalb des Zielframes ist ein Fehler.
    pub fn from_value(
        payload: &Value,
        cam_enabled: Option<bool>,
        mode: Option<&str>,
    ) -> Result<Self, LayoutValidationError> {
        Self::parse(payload, cam_enabled, mode, CamPositionPolicy::Strict)
    }

    /// Wie [`Self::from_value`], aber für Layouts aus der DB: `cam_position`
    /// wird in den Zielframe geclampt statt abgelehnt. Altlayouts stammen aus
    /// der Zeit, als `cam_position` gegen die Quellauflösung geprüft wurde;
    /// ohne das Clampen würden sie beim Lesen stumm auf den globalen Default
    /// zurückfallen.
    pub fn from_stored_value(
        payload: &Value,
        cam_enabled: Option<bool>,
        mode: Option<&str>,
    ) -> Result<Self, LayoutValidationError> {
        Self::parse(payload, cam_enabled, mode, CamPositionPolicy::Clamp)
    }

    fn parse(
        payload: &Value,
        cam_enabled: Option<bool>,
        mode: Option<&str>,
        cam_position_policy: CamPositionPolicy,
    ) -> Result<Self, LayoutValidationError> {
        let obj = payload
            .as_object()
            .ok_or_else(|| err("layout must be an object"))?;
        let version = require_int(
            obj.get("version").or(Some(&Value::Number(1.into()))),
            "version",
        )?;
        if version != 1 {
            return Err(err("version must be 1"));
        }
        let source = LayoutSource::from_value(obj.get("source"))?;
        let game_crop = LayoutBox::from_value("game_crop", obj.get("game_crop"))?;
        let cam_crop = LayoutBox::from_value("cam_crop", obj.get("cam_crop"))?;
        let cam_position = LayoutBox::from_value("cam_position", obj.get("cam_position"))?;
        let cam_position = match cam_position_policy {
            CamPositionPolicy::Strict => cam_position,
            CamPositionPolicy::Clamp => cam_position.clamped_to_target(),
        };

        let resolved_mode = match mode {
            Some(m) => m.to_string(),
            None => obj
                .get("mode")
                .and_then(Value::as_str)
                .unwrap_or("pip")
                .to_string(),
        };
        let resolved_mode = resolved_mode.trim().to_lowercase();
        if resolved_mode != "pip" && resolved_mode != "stacked" {
            return Err(err("mode must be one of: pip, stacked"));
        }
        let resolved_cam_enabled = match cam_enabled {
            Some(c) => c,
            None => obj
                .get("cam_enabled")
                .and_then(Value::as_bool)
                .unwrap_or(true),
        };

        let layout = Self {
            version,
            source,
            game_crop,
            cam_crop,
            cam_position,
            cam_enabled: resolved_cam_enabled,
            mode: resolved_mode,
        };
        layout.validate()?;
        Ok(layout)
    }

    fn validate(&self) -> Result<(), LayoutValidationError> {
        // Crops liegen im Twitch-Bild ...
        self.game_crop
            .validate_within(self.source.width, self.source.height, "game_crop", "source")?;
        self.cam_crop
            .validate_within(self.source.width, self.source.height, "cam_crop", "source")?;
        // ... cam_position dagegen im fertigen Hochformat-Frame.
        self.cam_position
            .validate_within(TARGET_WIDTH, TARGET_HEIGHT, "cam_position", "target")?;
        Ok(())
    }

    /// JSON für die `layout_json`-Spalte (ohne cam_enabled/mode — die liegen in
    /// eigenen Spalten).
    pub fn to_layout_json(&self) -> Value {
        json!({
            "version": self.version,
            "source": self.source.to_json(),
            "game_crop": self.game_crop.to_json(),
            "cam_crop": self.cam_crop.to_json(),
            "cam_position": self.cam_position.to_json(),
        })
    }

    /// JSON für die Clip-Override-Spalte (inkl. cam_enabled/mode).
    pub fn to_override_json(&self) -> Value {
        let mut payload = self.to_layout_json();
        payload["cam_enabled"] = json!(self.cam_enabled);
        payload["mode"] = json!(self.mode);
        payload
    }
}

/// Default-Layout: Game formatfüllend, Cam als 320x320-Kachel rechts oben im
/// Hochformat-Frame (1080 - 320 - 48 = 712 / 48 Rand).
pub fn default_streamer_layout() -> StreamerLayout {
    StreamerLayout {
        version: 1,
        source: LayoutSource {
            width: 1920,
            height: 1080,
        },
        game_crop: LayoutBox {
            x: 0,
            y: 0,
            w: 1080,
            h: 1080,
        },
        cam_crop: LayoutBox {
            x: 1500,
            y: 50,
            w: 380,
            h: 380,
        },
        cam_position: LayoutBox {
            x: 712,
            y: 48,
            w: 320,
            h: 320,
        },
        cam_enabled: true,
        mode: "pip".to_string(),
    }
}

/// Spiegelt Pythons `int(value)` mit explizitem bool-Reject.
fn require_int(value: Option<&Value>, field: &str) -> Result<i64, LayoutValidationError> {
    match value {
        Some(Value::Bool(_)) => Err(err(format!("{field} must be an integer"))),
        Some(Value::Number(n)) => n
            .as_i64()
            .or_else(|| n.as_f64().map(|f| f.trunc() as i64))
            .ok_or_else(|| err(format!("{field} must be an integer"))),
        Some(Value::String(s)) => s
            .trim()
            .parse::<i64>()
            .map_err(|_| err(format!("{field} must be an integer"))),
        _ => Err(err(format!("{field} must be an integer"))),
    }
}

fn decode_layout_json(raw: &str) -> Option<Value> {
    serde_json::from_str(raw).ok()
}

/// Default-Layout eines Streamers (`None` wenn keins gesetzt).
pub async fn get_streamer_layout(pool: &PgPool, login: &str) -> Option<StreamerLayout> {
    let normalized = login.trim().to_lowercase();
    if normalized.is_empty() {
        return None;
    }
    let row = sqlx::query!(
        "SELECT layout_json::text AS \"layout_json!\", cam_enabled AS \"cam_enabled!\", mode AS \"mode!\" FROM social_media_streamer_layout \
         WHERE LOWER(streamer_login) = $1 LIMIT 1",
        &normalized
    )
    .fetch_optional(pool)
    .await
    .ok()
    .flatten();
    let row = row?;
    let layout_json = row.layout_json;
    let cam_enabled = row.cam_enabled;
    let mode = row.mode;
    let payload = decode_layout_json(&layout_json)?;
    StreamerLayout::from_stored_value(&payload, Some(cam_enabled), Some(&mode)).ok()
}

/// Schreibt/aktualisiert das Default-Layout eines Streamers.
pub async fn upsert_streamer_layout(
    pool: &PgPool,
    login: &str,
    layout: &StreamerLayout,
    updated_by: Option<&str>,
) -> Result<(), sqlx::Error> {
    let normalized = login.trim().to_lowercase();
    if normalized.is_empty() {
        return Ok(());
    }
    let updated_by = updated_by.map(str::trim).filter(|s| !s.is_empty());
    sqlx::query!(
        "INSERT INTO social_media_streamer_layout \
            (streamer_login, layout_json, cam_enabled, mode, updated_at, updated_by) \
         VALUES ($1, $2::text::jsonb, $3, $4, CURRENT_TIMESTAMP, $5) \
         ON CONFLICT (streamer_login) DO UPDATE \
            SET layout_json = EXCLUDED.layout_json, cam_enabled = EXCLUDED.cam_enabled, \
                mode = EXCLUDED.mode, updated_at = CURRENT_TIMESTAMP, updated_by = EXCLUDED.updated_by",
        &normalized,
        serde_json::to_string(&layout.to_layout_json()).unwrap_or_else(|_| "{}".to_string()),
        layout.cam_enabled,
        &layout.mode,
        updated_by
    )
    .execute(pool)
    .await?;
    Ok(())
}

/// Effektives Layout eines Clips: Override > Streamer-Default > globaler Default.
pub async fn get_clip_effective_layout(
    pool: &PgPool,
    clip_db_id: impl Into<i64>,
) -> StreamerLayout {
    let clip_db_id = clip_db_id.into();
    let row = sqlx::query!(
        "SELECT c.layout_override_json::text AS override_json, c.streamer_login, \
                l.layout_json::text AS streamer_layout_json, l.cam_enabled AS \"cam_enabled?\", l.mode AS \"mode?\" \
           FROM twitch_clips_social_media c \
           LEFT JOIN social_media_streamer_layout l \
             ON LOWER(l.streamer_login) = LOWER(c.streamer_login) \
          WHERE c.id = $1 LIMIT 1",
        clip_db_id
    )
    .fetch_optional(pool)
    .await
    .ok()
    .flatten();

    let Some(row) = row else {
        return default_streamer_layout();
    };
    let override_json = row.override_json;
    let streamer_json = row.streamer_layout_json;
    let streamer_cam = row.cam_enabled;
    let streamer_mode = row.mode;

    if let Some(raw) = override_json.filter(|s| !s.is_empty()) {
        if let Some(payload) = decode_layout_json(&raw) {
            if let Ok(layout) = StreamerLayout::from_stored_value(&payload, None, None) {
                return layout;
            }
        }
    }
    if let Some(raw) = streamer_json.filter(|s| !s.is_empty()) {
        if let Some(payload) = decode_layout_json(&raw) {
            let cam = streamer_cam.unwrap_or(true);
            let mode = streamer_mode.as_deref().unwrap_or("pip");
            if let Ok(layout) = StreamerLayout::from_stored_value(&payload, Some(cam), Some(mode)) {
                return layout;
            }
        }
    }
    default_streamer_layout()
}

/// Setzt (oder löscht mit `None`) das Clip-spezifische Layout-Override.
pub async fn set_clip_layout_override(
    pool: &PgPool,
    clip_db_id: impl Into<i64>,
    layout: Option<&StreamerLayout>,
) -> Result<(), sqlx::Error> {
    let clip_db_id = clip_db_id.into();
    let payload = layout
        .map(|l| serde_json::to_string(&l.to_override_json()).unwrap_or_else(|_| "{}".to_string()));
    sqlx::query!(
        "UPDATE twitch_clips_social_media SET layout_override_json = $1::text::jsonb WHERE id = $2",
        payload.as_deref(),
        clip_db_id
    )
    .execute(pool)
    .await?;
    Ok(())
}

/// Belegt das Clip-Override mit dem Streamer-Default (nur falls noch keins
/// gesetzt — `COALESCE`). Wird beim Registrieren eines Clips aufgerufen.
pub async fn apply_default_layout(
    pool: &PgPool,
    clip_db_id: impl Into<i64>,
    streamer_login: &str,
) -> Result<(), sqlx::Error> {
    let clip_db_id = clip_db_id.into();
    let layout = match get_streamer_layout(pool, streamer_login).await {
        Some(l) => l,
        None => default_streamer_layout(),
    };
    let payload =
        serde_json::to_string(&layout.to_override_json()).unwrap_or_else(|_| "{}".to_string());
    sqlx::query!(
        "UPDATE twitch_clips_social_media \
            SET layout_override_json = COALESCE(layout_override_json, $1::text::jsonb) WHERE id = $2",
        &payload,
        clip_db_id
    )
    .execute(pool)
    .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
    use std::str::FromStr;

    fn valid_payload() -> Value {
        json!({
            "version": 1,
            "source": {"width": 1920, "height": 1080},
            "game_crop": {"x": 0, "y": 0, "w": 1080, "h": 1080},
            "cam_crop": {"x": 1500, "y": 50, "w": 380, "h": 380},
            "cam_position": {"x": 712, "y": 48, "w": 320, "h": 320}
        })
    }

    #[test]
    fn cam_position_gilt_im_zielframe_nicht_in_der_quelle() {
        // Hoher Cam-Streifen: passt in den 1920 hohen Zielframe, nicht in die
        // 1080 hohe Quelle. Muss trotzdem gueltig sein.
        let mut p = valid_payload();
        p["cam_position"] = json!({"x": 0, "y": 0, "w": 1080, "h": 1600});
        let layout = StreamerLayout::from_value(&p, None, None)
            .expect("cam_position gehoert in den Zielframe 1080x1920");
        assert_eq!(layout.cam_position.h, 1600);

        // Breiter als der Zielframe: abgelehnt, obwohl es in die 1920 breite
        // Quelle passen wuerde.
        let mut p = valid_payload();
        p["cam_position"] = json!({"x": 0, "y": 0, "w": 1600, "h": 200});
        assert!(StreamerLayout::from_value(&p, None, None).is_err());

        // Randfall: exakt buendig ist erlaubt.
        let mut p = valid_payload();
        p["cam_position"] = json!({"x": 0, "y": 0, "w": TARGET_WIDTH, "h": TARGET_HEIGHT});
        assert!(StreamerLayout::from_value(&p, None, None).is_ok());
        // Ein Pixel darueber nicht mehr.
        let mut p = valid_payload();
        p["cam_position"] = json!({"x": 1, "y": 0, "w": TARGET_WIDTH, "h": 100});
        assert!(StreamerLayout::from_value(&p, None, None).is_err());
    }

    #[test]
    fn altes_layout_wird_beim_lesen_in_den_zielframe_geclampt() {
        // Genau so liegen Layouts aus der Zeit vor der Umstellung in der DB:
        // cam_position war im Quellkoordinatenraum gemeint.
        let alt: Value = serde_json::from_str(
            r#"{"version":1,
                "source":{"width":1920,"height":1080},
                "game_crop":{"x":420,"y":0,"w":1080,"h":1080},
                "cam_crop":{"x":1500,"y":50,"w":380,"h":380},
                "cam_position":{"x":126,"y":700,"w":1080,"h":540}}"#,
        )
        .unwrap();

        // Neue Eingaben ueber die API werden streng geprueft: 126 + 1080 > 1080.
        assert!(StreamerLayout::from_value(&alt, None, None).is_err());

        // Beim Lesen aus der DB darf dasselbe Layout nicht verloren gehen.
        let layout = StreamerLayout::from_stored_value(&alt, None, None)
            .expect("gespeichertes Altlayout bleibt lesbar");
        assert_eq!(layout.cam_position, LayoutBox { x: 0, y: 700, w: 1080, h: 540 });
        // Die Crops bleiben unberuehrt.
        assert_eq!(layout.game_crop, LayoutBox { x: 420, y: 0, w: 1080, h: 1080 });
        assert_eq!(layout.cam_crop, LayoutBox { x: 1500, y: 50, w: 380, h: 380 });

        // Auch zu hohe Werte werden gestutzt statt abgelehnt.
        let mut zu_hoch = alt.clone();
        zu_hoch["cam_position"] = json!({"x": 900, "y": 1800, "w": 600, "h": 400});
        let layout = StreamerLayout::from_stored_value(&zu_hoch, None, None).unwrap();
        assert_eq!(layout.cam_position, LayoutBox { x: 480, y: 1520, w: 600, h: 400 });

        // Kaputte Struktur bleibt ein Fehler, auch beim Lesen.
        let mut kaputt = alt.clone();
        kaputt["cam_position"] = json!({"x": 0, "y": 0, "w": 0, "h": 100});
        assert!(StreamerLayout::from_stored_value(&kaputt, None, None).is_err());
    }

    #[test]
    fn from_value_roundtrip_und_default() {
        let layout = StreamerLayout::from_value(&valid_payload(), None, None).unwrap();
        assert_eq!(layout, default_streamer_layout());
        // to_layout_json hat KEIN cam_enabled/mode, to_override_json schon.
        let lj = layout.to_layout_json();
        assert!(lj.get("cam_enabled").is_none());
        let oj = layout.to_override_json();
        assert_eq!(oj["cam_enabled"], json!(true));
        assert_eq!(oj["mode"], json!("pip"));
        // Override per Argument schlägt Payload.
        let l2 =
            StreamerLayout::from_value(&valid_payload(), Some(false), Some("STACKED")).unwrap();
        assert!(!l2.cam_enabled);
        assert_eq!(l2.mode, "stacked");
    }

    #[test]
    fn validierungsfehler() {
        // version != 1
        let mut p = valid_payload();
        p["version"] = json!(2);
        assert!(StreamerLayout::from_value(&p, None, None).is_err());
        // bool als Koordinate → Integer-Fehler
        let mut p = valid_payload();
        p["game_crop"]["x"] = json!(true);
        assert!(StreamerLayout::from_value(&p, None, None).is_err());
        // Box ragt über Quelle hinaus
        let mut p = valid_payload();
        p["game_crop"]["w"] = json!(2000);
        assert!(StreamerLayout::from_value(&p, None, None).is_err());
        // ungültiger mode
        assert!(StreamerLayout::from_value(&valid_payload(), None, Some("fullscreen")).is_err());
        // w <= 0
        let mut p = valid_payload();
        p["cam_crop"]["w"] = json!(0);
        assert!(StreamerLayout::from_value(&p, None, None).is_err());
    }

    async fn make_pool(schema: &str) -> Option<PgPool> {
        let dsn = std::env::var("TB_TEST_DATABASE_URL").ok()?;
        let admin = PgPoolOptions::new()
            .max_connections(1)
            .connect(&dsn)
            .await
            .unwrap();
        sqlx::query(&format!("DROP SCHEMA IF EXISTS {schema} CASCADE"))
            .execute(&admin)
            .await
            .unwrap();
        sqlx::query(&format!("CREATE SCHEMA {schema}"))
            .execute(&admin)
            .await
            .unwrap();
        admin.close().await;
        let opts = PgConnectOptions::from_str(&dsn)
            .unwrap()
            .options([("search_path", schema)]);
        let pool = PgPoolOptions::new()
            .max_connections(3)
            .connect_with(opts)
            .await
            .unwrap();
        for ddl in [
            "CREATE TABLE twitch_clips_social_media (id SERIAL PRIMARY KEY, streamer_login TEXT, layout_override_json JSONB)",
            "CREATE TABLE social_media_streamer_layout (streamer_login TEXT PRIMARY KEY, layout_json JSONB NOT NULL, cam_enabled BOOLEAN NOT NULL DEFAULT TRUE, mode TEXT NOT NULL DEFAULT 'pip', updated_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP, updated_by TEXT)",
        ] {
            sqlx::query(ddl).execute(&pool).await.unwrap();
        }
        Some(pool)
    }

    #[tokio::test]
    async fn upsert_get_und_apply_default() {
        let Some(pool) = make_pool("t_sm_layout").await else {
            return;
        };
        // Streamer-Layout mit cam aus, mode stacked.
        let mut custom = default_streamer_layout();
        custom.cam_enabled = false;
        custom.mode = "stacked".into();
        upsert_streamer_layout(&pool, "Nani", &custom, Some("admin"))
            .await
            .unwrap();
        // case-insensitiv lesbar.
        let got = get_streamer_layout(&pool, "nani").await.unwrap();
        assert!(!got.cam_enabled);
        assert_eq!(got.mode, "stacked");
        assert_eq!(got.game_crop, custom.game_crop);
        // Upsert überschreibt.
        upsert_streamer_layout(&pool, "nani", &default_streamer_layout(), None)
            .await
            .unwrap();
        assert!(
            get_streamer_layout(&pool, "nani")
                .await
                .unwrap()
                .cam_enabled
        );

        // Clip ohne Override → apply_default belegt mit Streamer-Default.
        let clip: i32 = sqlx::query_scalar(
            "INSERT INTO twitch_clips_social_media (streamer_login) VALUES ('nani') RETURNING id",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        apply_default_layout(&pool, clip, "nani").await.unwrap();
        let eff = get_clip_effective_layout(&pool, clip).await;
        assert_eq!(eff.mode, "pip"); // Streamer-Default (überschrieben)

        // Zweiter apply_default ändert NICHTS (COALESCE schützt bestehendes Override).
        let mut other = default_streamer_layout();
        other.mode = "stacked".into();
        upsert_streamer_layout(&pool, "nani", &other, None)
            .await
            .unwrap();
        apply_default_layout(&pool, clip, "nani").await.unwrap();
        assert_eq!(get_clip_effective_layout(&pool, clip).await.mode, "pip");

        // Explizites Override schlägt Streamer-Layout.
        let mut ov = default_streamer_layout();
        ov.cam_enabled = false;
        ov.mode = "stacked".into();
        set_clip_layout_override(&pool, clip, Some(&ov))
            .await
            .unwrap();
        let eff = get_clip_effective_layout(&pool, clip).await;
        assert!(!eff.cam_enabled);
        assert_eq!(eff.mode, "stacked");

        // Override löschen → fällt auf Streamer-Layout (stacked) zurück.
        set_clip_layout_override(&pool, clip, None).await.unwrap();
        assert_eq!(get_clip_effective_layout(&pool, clip).await.mode, "stacked");
    }

    #[tokio::test]
    async fn gespeichertes_altlayout_faellt_nicht_auf_den_default_zurueck() {
        let Some(pool) = make_pool("t_sm_layout_alt").await else {
            return;
        };
        // Layout direkt als JSON einspielen, so wie es vor der Umstellung
        // geschrieben wurde (cam_position im Quellraum, x + w > 1080).
        sqlx::query(
            "INSERT INTO social_media_streamer_layout (streamer_login, layout_json, cam_enabled, mode) \
             VALUES ('alt', $1::text::jsonb, TRUE, 'stacked')",
        )
        .bind(
            r#"{"version":1,"source":{"width":1920,"height":1080},
                "game_crop":{"x":420,"y":0,"w":1080,"h":1080},
                "cam_crop":{"x":1500,"y":50,"w":380,"h":380},
                "cam_position":{"x":126,"y":0,"w":1080,"h":540}}"#,
        )
        .execute(&pool)
        .await
        .unwrap();

        let got = get_streamer_layout(&pool, "alt")
            .await
            .expect("Altlayout muss lesbar bleiben, nicht auf den Default kippen");
        assert_eq!(got.mode, "stacked");
        assert_eq!(got.game_crop, LayoutBox { x: 420, y: 0, w: 1080, h: 1080 });
        assert_eq!(got.cam_position, LayoutBox { x: 0, y: 0, w: 1080, h: 540 });

        // Auch der Clip-Pfad (Override) darf daran nicht scheitern.
        let clip: i32 = sqlx::query_scalar(
            "INSERT INTO twitch_clips_social_media (streamer_login) VALUES ('alt') RETURNING id",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        let eff = get_clip_effective_layout(&pool, clip).await;
        assert_eq!(eff.cam_position, LayoutBox { x: 0, y: 0, w: 1080, h: 540 });
        assert_ne!(eff, default_streamer_layout());
    }

    #[tokio::test]
    async fn effective_layout_ohne_clip_und_ohne_streamer() {
        let Some(pool) = make_pool("t_sm_layout_def").await else {
            return;
        };
        // Nicht existierender Clip → globaler Default.
        assert_eq!(
            get_clip_effective_layout(&pool, 999).await,
            default_streamer_layout()
        );
        // Clip ohne Streamer-Layout/Override → globaler Default.
        let clip: i32 = sqlx::query_scalar(
            "INSERT INTO twitch_clips_social_media (streamer_login) VALUES ('ghost') RETURNING id",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(
            get_clip_effective_layout(&pool, clip).await,
            default_streamer_layout()
        );
    }
}
