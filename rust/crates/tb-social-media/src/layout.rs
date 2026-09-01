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

use crate::approval::{invalidate_clips_for_content_change, ContentMutationError};

/// Breite des fertigen Hochformat-Frames (9:16).
pub const TARGET_WIDTH: i64 = 1080;
/// Höhe des fertigen Hochformat-Frames (9:16).
pub const TARGET_HEIGHT: i64 = 1920;

/// Kantenlänge der Cam-Kachel im PiP-Standard.
const DEFAULT_PIP_SIZE: i64 = 320;
/// Abstand der Cam-Kachel zum Frame-Rand im PiP-Standard.
const DEFAULT_PIP_INSET: i64 = 48;

/// Cam-Kachel rechts oben: der PiP-Standard, wie er vor der Umstellung fest im
/// Renderer stand.
pub const DEFAULT_PIP_TILE: LayoutBox = LayoutBox {
    x: TARGET_WIDTH - DEFAULT_PIP_SIZE - DEFAULT_PIP_INSET,
    y: DEFAULT_PIP_INSET,
    w: DEFAULT_PIP_SIZE,
    h: DEFAULT_PIP_SIZE,
};

#[derive(Debug, thiserror::Error)]
#[error("{0}")]
pub struct LayoutValidationError(pub String);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LayoutMutationOutcome {
    pub changed: bool,
    pub pending_stopped: i64,
}

#[derive(Debug, thiserror::Error)]
pub enum LayoutMutationError {
    #[error("{0} Upload(s) laufen bereits")]
    UploadRunning(i64),
    #[error("Eine betroffene Vorschau wird gerade erstellt")]
    PreparationRunning,
    #[error(transparent)]
    Db(#[from] sqlx::Error),
}

#[derive(Debug, thiserror::Error)]
pub enum EffectiveLayoutError {
    #[error("Clip wurde nicht gefunden")]
    ClipNotFound,
    #[error("Gespeichertes Clip-Layout ist ungültig")]
    InvalidClipLayout,
    #[error("Gespeichertes Kanal-Layout ist ungültig")]
    InvalidStreamerLayout,
    #[error(transparent)]
    Db(#[from] sqlx::Error),
}

impl From<ContentMutationError> for LayoutMutationError {
    fn from(error: ContentMutationError) -> Self {
        match error {
            ContentMutationError::UploadRunning(count) => Self::UploadRunning(count),
            ContentMutationError::Db(error) => Self::Db(error),
        }
    }
}

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

    /// Zielrechteck, garantiert innerhalb des Hochformat-Frames und mit geraden
    /// Kantenlängen. Der Renderer ruft das defensiv auf: ein Altlayout darf
    /// keinen Overlay ausserhalb des Bildes erzeugen, und ungerade Maße lassen
    /// libx264 mit yuv420p abbrechen (`scale=421:561`).
    pub fn clamped_to_target(self) -> Self {
        let inside = self.clamped_within(TARGET_WIDTH, TARGET_HEIGHT);
        Self {
            w: even_size(inside.w),
            h: even_size(inside.h),
            ..inside
        }
    }

    fn to_json(self) -> Value {
        json!({ "x": self.x, "y": self.y, "w": self.w, "h": self.h })
    }
}

/// Rundet eine Kantenlänge auf einen geraden Wert ab (Minimum 2).
fn even_size(value: i64) -> i64 {
    (value - value.rem_euclid(2)).max(2)
}

/// Bringt ein gespeichertes `cam_position` in den Zielframe.
///
/// Sonderfall PiP: vor diesem Umbau war `cam_position` im PiP-Modus weder im
/// Dashboard editierbar noch im Renderer wirksam (die Kachel stand fest auf
/// 320x320 mit Rand 48). Gespeichert wurde trotzdem der Stacked-Default
/// `{0,0,1080,540}`. Ein frameweiter Wert ist dort also keine Entscheidung des
/// Nutzers, sondern Altlast; würde man ihn übernehmen, klebte plötzlich eine
/// 1080x540 große Cam oben links im Bild. Deshalb zurück auf den PiP-Standard.
///
/// Im Stacked-Modus war der Wert echt gesetzt (die Streifenhöhe war editierbar
/// und wirksam) und bleibt unangetastet.
fn normalize_stored_cam_position(cam_position: LayoutBox, mode: &str) -> LayoutBox {
    if mode == "pip" && cam_position.w >= TARGET_WIDTH {
        return DEFAULT_PIP_TILE;
    }
    cam_position.clamped_to_target()
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
        let cam_position = match cam_position_policy {
            CamPositionPolicy::Strict => cam_position,
            CamPositionPolicy::Clamp => normalize_stored_cam_position(cam_position, &resolved_mode),
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
        self.game_crop.validate_within(
            self.source.width,
            self.source.height,
            "game_crop",
            "source",
        )?;
        self.cam_crop.validate_within(
            self.source.width,
            self.source.height,
            "cam_crop",
            "source",
        )?;
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
        cam_position: DEFAULT_PIP_TILE,
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
) -> Result<LayoutMutationOutcome, LayoutMutationError> {
    let normalized = login.trim().to_lowercase();
    if normalized.is_empty() {
        return Ok(LayoutMutationOutcome {
            changed: false,
            pending_stopped: 0,
        });
    }
    let updated_by = updated_by.map(str::trim).filter(|s| !s.is_empty());
    let payload =
        serde_json::to_string(&layout.to_layout_json()).unwrap_or_else(|_| "{}".to_string());
    let mut transaction = pool.begin().await?;
    // Auch beim ersten INSERT serialisiert das Advisory-Lock konkurrierende
    // Änderungen desselben Streamers; dort existiert noch keine Zeile für ein
    // normales FOR UPDATE.
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1, 0))")
        .bind(&normalized)
        .execute(transaction.as_mut())
        .await?;
    let unchanged: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM social_media_streamer_layout \
         WHERE LOWER(streamer_login) = LOWER($1) \
           AND layout_json IS NOT DISTINCT FROM $2::text::jsonb \
           AND cam_enabled IS NOT DISTINCT FROM $3 \
           AND mode IS NOT DISTINCT FROM $4 FOR UPDATE)",
    )
    .bind(&normalized)
    .bind(&payload)
    .bind(layout.cam_enabled)
    .bind(&layout.mode)
    .fetch_one(transaction.as_mut())
    .await?;
    if unchanged {
        transaction.commit().await?;
        return Ok(LayoutMutationOutcome {
            changed: false,
            pending_stopped: 0,
        });
    }

    let affected: Vec<i64> = sqlx::query_scalar(
        "SELECT id FROM twitch_clips_social_media \
         WHERE LOWER(streamer_login) = LOWER($1) AND layout_override_json IS NULL \
         ORDER BY id",
    )
    .bind(&normalized)
    .fetch_all(transaction.as_mut())
    .await?;
    ensure_no_active_preparation(transaction.as_mut(), &affected).await?;
    let invalidated =
        invalidate_clips_for_content_change(transaction.as_mut(), &affected, "layout_changed")
            .await?;

    sqlx::query(
        "INSERT INTO social_media_streamer_layout \
            (streamer_login, layout_json, cam_enabled, mode, updated_at, updated_by) \
         VALUES ($1, $2::text::jsonb, $3, $4, CURRENT_TIMESTAMP, $5) \
         ON CONFLICT (streamer_login) DO UPDATE \
            SET layout_json = EXCLUDED.layout_json, cam_enabled = EXCLUDED.cam_enabled, \
                mode = EXCLUDED.mode, updated_at = CURRENT_TIMESTAMP, updated_by = EXCLUDED.updated_by",
    )
    .bind(&normalized)
    .bind(&payload)
    .bind(layout.cam_enabled)
    .bind(&layout.mode)
    .bind(updated_by)
    .execute(transaction.as_mut())
    .await?;
    reset_preparations_after_layout_change(transaction.as_mut(), &affected).await?;
    transaction.commit().await?;
    Ok(LayoutMutationOutcome {
        changed: true,
        pending_stopped: invalidated.pending_stopped,
    })
}

/// Effektives Layout eines Clips: Override > Streamer-Default > globaler Default.
pub async fn get_clip_effective_layout(
    pool: &PgPool,
    clip_db_id: impl Into<i64>,
) -> StreamerLayout {
    let clip_db_id = clip_db_id.into();
    match get_clip_effective_layout_checked(pool, clip_db_id).await {
        Ok(layout) => layout,
        Err(error) => {
            tracing::warn!(
                clip_db_id,
                error_kind = match error {
                    EffectiveLayoutError::ClipNotFound => "clip_not_found",
                    EffectiveLayoutError::InvalidClipLayout => "invalid_clip_layout",
                    EffectiveLayoutError::InvalidStreamerLayout => "invalid_streamer_layout",
                    EffectiveLayoutError::Db(_) => "database_failed",
                },
                "Effektives Clip-Layout konnte nicht geladen werden"
            );
            default_streamer_layout()
        }
    }
}

/// Fail-closed-Variante für alle rendernden Pfade. Ein DB-Ausfall oder ein
/// beschädigtes gespeichertes Layout darf nie still den globalen Standard
/// rendern und anschließend als geprüft freigabefähig werden.
pub async fn get_clip_effective_layout_checked(
    pool: &PgPool,
    clip_db_id: impl Into<i64>,
) -> Result<StreamerLayout, EffectiveLayoutError> {
    let clip_db_id = clip_db_id.into();
    type EffectiveLayoutRow = (
        Option<String>,
        String,
        Option<String>,
        Option<bool>,
        Option<String>,
    );
    let row: Option<EffectiveLayoutRow> = sqlx::query_as(
        "SELECT c.layout_override_json::text AS override_json, c.streamer_login, \
                l.layout_json::text AS streamer_layout_json, l.cam_enabled, l.mode \
           FROM twitch_clips_social_media c \
           LEFT JOIN social_media_streamer_layout l \
             ON LOWER(l.streamer_login) = LOWER(c.streamer_login) \
          WHERE c.id = $1 LIMIT 1",
    )
    .bind(clip_db_id)
    .fetch_optional(pool)
    .await?;

    let Some(row) = row else {
        return Err(EffectiveLayoutError::ClipNotFound);
    };
    let (override_json, _streamer_login, streamer_json, streamer_cam, streamer_mode) = row;

    if let Some(raw) = override_json.filter(|s| !s.is_empty()) {
        let payload = decode_layout_json(&raw).ok_or(EffectiveLayoutError::InvalidClipLayout)?;
        return StreamerLayout::from_stored_value(&payload, None, None)
            .map_err(|_| EffectiveLayoutError::InvalidClipLayout);
    }
    if let Some(raw) = streamer_json.filter(|s| !s.is_empty()) {
        let payload =
            decode_layout_json(&raw).ok_or(EffectiveLayoutError::InvalidStreamerLayout)?;
        let cam = streamer_cam.unwrap_or(true);
        let mode = streamer_mode.as_deref().unwrap_or("pip");
        return StreamerLayout::from_stored_value(&payload, Some(cam), Some(mode))
            .map_err(|_| EffectiveLayoutError::InvalidStreamerLayout);
    }
    Ok(default_streamer_layout())
}

/// Setzt (oder löscht mit `None`) das Clip-spezifische Layout-Override.
pub async fn set_clip_layout_override(
    pool: &PgPool,
    clip_db_id: impl Into<i64>,
    layout: Option<&StreamerLayout>,
) -> Result<LayoutMutationOutcome, LayoutMutationError> {
    let clip_db_id = clip_db_id.into();
    let payload = layout
        .map(|l| serde_json::to_string(&l.to_override_json()).unwrap_or_else(|_| "{}".to_string()));
    let mut transaction = pool.begin().await?;
    let unchanged: Option<bool> = sqlx::query_scalar(
        "SELECT layout_override_json IS NOT DISTINCT FROM $2::text::jsonb \
         FROM twitch_clips_social_media WHERE id = $1",
    )
    .bind(clip_db_id)
    .bind(payload.as_deref())
    .fetch_optional(transaction.as_mut())
    .await?;
    if unchanged.unwrap_or(true) {
        transaction.commit().await?;
        return Ok(LayoutMutationOutcome {
            changed: false,
            pending_stopped: 0,
        });
    }

    ensure_no_active_preparation(transaction.as_mut(), &[clip_db_id]).await?;
    let unchanged_after_lock: Option<bool> = sqlx::query_scalar(
        "SELECT layout_override_json IS NOT DISTINCT FROM $2::text::jsonb \
         FROM twitch_clips_social_media WHERE id = $1 FOR UPDATE",
    )
    .bind(clip_db_id)
    .bind(payload.as_deref())
    .fetch_optional(transaction.as_mut())
    .await?;
    if unchanged_after_lock.unwrap_or(true) {
        transaction.commit().await?;
        return Ok(LayoutMutationOutcome {
            changed: false,
            pending_stopped: 0,
        });
    }
    let invalidated =
        invalidate_clips_for_content_change(transaction.as_mut(), &[clip_db_id], "layout_changed")
            .await?;
    sqlx::query(
        "UPDATE twitch_clips_social_media SET layout_override_json = $2::text::jsonb WHERE id = $1",
    )
    .bind(clip_db_id)
    .bind(payload.as_deref())
    .execute(transaction.as_mut())
    .await?;
    reset_preparations_after_layout_change(transaction.as_mut(), &[clip_db_id]).await?;
    transaction.commit().await?;
    Ok(LayoutMutationOutcome {
        changed: true,
        pending_stopped: invalidated.pending_stopped,
    })
}

async fn ensure_no_active_preparation(
    connection: &mut sqlx::PgConnection,
    clip_db_ids: &[i64],
) -> Result<(), LayoutMutationError> {
    if clip_db_ids.is_empty() {
        return Ok(());
    }
    let active: Vec<i64> = sqlx::query_scalar(
        "SELECT clip_db_id FROM social_media_clip_preparation \
         WHERE clip_db_id = ANY($1) AND state IN ('materializing', 'rendering') \
           AND updated_at >= CURRENT_TIMESTAMP - INTERVAL '30 minutes' \
         ORDER BY clip_db_id FOR UPDATE",
    )
    .bind(clip_db_ids)
    .fetch_all(&mut *connection)
    .await?;
    if active.is_empty() {
        Ok(())
    } else {
        Err(LayoutMutationError::PreparationRunning)
    }
}

async fn reset_preparations_after_layout_change(
    connection: &mut sqlx::PgConnection,
    clip_db_ids: &[i64],
) -> Result<(), sqlx::Error> {
    if clip_db_ids.is_empty() {
        return Ok(());
    }
    sqlx::query(
        "UPDATE social_media_clip_preparation SET state = 'pending', \
         lease_token = NULL, \
         error_code = NULL, error_message = NULL, completed_at = NULL, \
         requested_at = CURRENT_TIMESTAMP, updated_at = CURRENT_TIMESTAMP \
         WHERE clip_db_id = ANY($1)",
    )
    .bind(clip_db_ids)
    .execute(&mut *connection)
    .await?;
    Ok(())
}

/// Stellt dynamische Vererbung des Streamer-Defaults sicher. Neue Clips bleiben
/// bewusst ohne Override; ein alter automatisch kopierter Snapshot wird nur
/// dann entfernt, wenn er dem aktuellen Streamer-Default exakt entspricht.
/// Davon abweichende, echte Individualanpassungen bleiben erhalten.
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
    sqlx::query(
        "UPDATE twitch_clips_social_media \
            SET layout_override_json = NULL \
          WHERE id = $2 AND layout_override_json = $1::text::jsonb",
    )
    .bind(&payload)
    .bind(clip_db_id)
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

    /// Exakt der Stand der 57 Zeilen in `twitch_clips_social_media`
    /// (`layout_override_json`) vor der Umstellung.
    fn live_altlayout() -> Value {
        serde_json::from_str(
            r#"{"version":1,
                "source":{"width":1920,"height":1080},
                "game_crop":{"x":0,"y":0,"w":1080,"h":1080},
                "cam_crop":{"x":1500,"y":50,"w":380,"h":380},
                "cam_position":{"x":0,"y":0,"w":1080,"h":540},
                "cam_enabled":true,
                "mode":"pip"}"#,
        )
        .unwrap()
    }

    #[test]
    fn altes_pip_layout_faellt_auf_die_kachel_zurueck() {
        // Im PiP-Modus war cam_position vorher weder editierbar noch wirksam.
        // Ein frameweiter Wert ist Altlast, keine Nutzerentscheidung: sonst
        // klebt ab dem Deploy eine 1080x540-Cam oben links im Bild.
        let layout = StreamerLayout::from_stored_value(&live_altlayout(), None, None).unwrap();
        assert_eq!(layout.mode, "pip");
        assert_eq!(layout.cam_position, DEFAULT_PIP_TILE);
        assert_eq!(layout, default_streamer_layout());

        // Auch wenn mode aus der Spalte kommt statt aus dem JSON.
        let layout =
            StreamerLayout::from_stored_value(&live_altlayout(), Some(true), Some("PIP")).unwrap();
        assert_eq!(layout.cam_position, DEFAULT_PIP_TILE);

        // Stacked bleibt unangetastet: dort war der Wert echt gesetzt.
        let mut stacked = live_altlayout();
        stacked["mode"] = json!("stacked");
        let layout = StreamerLayout::from_stored_value(&stacked, None, None).unwrap();
        assert_eq!(
            layout.cam_position,
            LayoutBox {
                x: 0,
                y: 0,
                w: 1080,
                h: 540
            }
        );

        // Ein echter, schmaler PiP-Wert kommt unveraendert durch.
        let mut echt = live_altlayout();
        echt["cam_position"] = json!({"x": 40, "y": 900, "w": 300, "h": 300});
        let layout = StreamerLayout::from_stored_value(&echt, None, None).unwrap();
        assert_eq!(
            layout.cam_position,
            LayoutBox {
                x: 40,
                y: 900,
                w: 300,
                h: 300
            }
        );

        // Der strenge Pfad (neue Eingaben) fasst nichts an.
        let layout = StreamerLayout::from_value(&live_altlayout(), None, None).unwrap();
        assert_eq!(
            layout.cam_position,
            LayoutBox {
                x: 0,
                y: 0,
                w: 1080,
                h: 540
            }
        );
    }

    #[test]
    fn cam_position_bekommt_gerade_kantenlaengen() {
        // yuv420p vertraegt keine ungeraden Chroma-Maße: scale=421:561 laesst
        // libx264 abbrechen.
        assert_eq!(
            LayoutBox {
                x: 100,
                y: 100,
                w: 321,
                h: 201
            }
            .clamped_to_target(),
            LayoutBox {
                x: 100,
                y: 100,
                w: 320,
                h: 200
            }
        );
        // Gerade Werte bleiben, wie sie sind.
        assert_eq!(DEFAULT_PIP_TILE.clamped_to_target(), DEFAULT_PIP_TILE);
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
        assert!(StreamerLayout::from_value(&alt, None, Some("stacked")).is_err());

        // Beim Lesen aus der DB darf dasselbe Layout nicht verloren gehen.
        // (Stacked, damit hier das Clampen greift und nicht die PiP-Altlast-Regel.)
        let layout = StreamerLayout::from_stored_value(&alt, None, Some("stacked"))
            .expect("gespeichertes Altlayout bleibt lesbar");
        assert_eq!(
            layout.cam_position,
            LayoutBox {
                x: 0,
                y: 700,
                w: 1080,
                h: 540
            }
        );
        // Die Crops bleiben unberuehrt.
        assert_eq!(
            layout.game_crop,
            LayoutBox {
                x: 420,
                y: 0,
                w: 1080,
                h: 1080
            }
        );
        assert_eq!(
            layout.cam_crop,
            LayoutBox {
                x: 1500,
                y: 50,
                w: 380,
                h: 380
            }
        );

        // Auch zu hohe Werte werden gestutzt statt abgelehnt.
        let mut zu_hoch = alt.clone();
        zu_hoch["cam_position"] = json!({"x": 900, "y": 1800, "w": 600, "h": 400});
        let layout = StreamerLayout::from_stored_value(&zu_hoch, None, None).unwrap();
        assert_eq!(
            layout.cam_position,
            LayoutBox {
                x: 480,
                y: 1520,
                w: 600,
                h: 400
            }
        );

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
        let dsn = crate::test_support::test_dsn()?;
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
            "CREATE TABLE twitch_clips_social_media (id SERIAL PRIMARY KEY, streamer_login TEXT, status TEXT DEFAULT 'pending', layout_override_json JSONB)",
            "CREATE TABLE social_media_streamer_layout (streamer_login TEXT PRIMARY KEY, layout_json JSONB NOT NULL, cam_enabled BOOLEAN NOT NULL DEFAULT TRUE, mode TEXT NOT NULL DEFAULT 'pip', updated_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP, updated_by TEXT)",
            "CREATE TABLE social_media_clip_preparation (clip_db_id BIGINT PRIMARY KEY, state TEXT NOT NULL DEFAULT 'pending', lease_token TEXT, error_code TEXT, error_message TEXT, completed_at TIMESTAMPTZ, requested_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP, updated_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP)",
            "CREATE TABLE social_media_clip_approval (clip_db_id INTEGER PRIMARY KEY, state TEXT NOT NULL DEFAULT 'awaiting_approval', approved_platforms JSONB NOT NULL DEFAULT '[]'::jsonb, approver_user_id TEXT, decided_at TIMESTAMPTZ, letzter_nachreih_versuch TIMESTAMPTZ, approved_render_fingerprint TEXT)",
            "CREATE TABLE twitch_clips_upload_queue (id BIGSERIAL PRIMARY KEY, clip_id BIGINT NOT NULL, status TEXT NOT NULL DEFAULT 'pending', last_error TEXT, last_attempt_at TIMESTAMPTZ, provider_started_at TIMESTAMPTZ, provider_lease_token TEXT, provider_external_id TEXT, provider_accepted_at TIMESTAMPTZ)",
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

        // Clip ohne Override erbt den Streamer-Default dynamisch.
        let clip: i32 = sqlx::query_scalar(
            "INSERT INTO twitch_clips_social_media (streamer_login) VALUES ('nani') RETURNING id",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        apply_default_layout(&pool, clip, "nani").await.unwrap();
        let eff = get_clip_effective_layout(&pool, clip).await;
        assert_eq!(eff.mode, "pip");
        let stored_override: Option<serde_json::Value> = sqlx::query_scalar(
            "SELECT layout_override_json FROM twitch_clips_social_media WHERE id = $1",
        )
        .bind(clip)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert!(stored_override.is_none());

        // Eine spätere Kanaländerung wirkt sofort, solange kein echtes
        // Clip-Override gesetzt wurde.
        let mut other = default_streamer_layout();
        other.mode = "stacked".into();
        upsert_streamer_layout(&pool, "nani", &other, None)
            .await
            .unwrap();
        apply_default_layout(&pool, clip, "nani").await.unwrap();
        assert_eq!(get_clip_effective_layout(&pool, clip).await.mode, "stacked");

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
    async fn layoutaenderung_invalidiert_vorschau_freigabe_und_wartende_queue() {
        let Some(pool) = make_pool("t_sm_layout_invalidation").await else {
            return;
        };
        let clip: i32 = sqlx::query_scalar(
            "INSERT INTO twitch_clips_social_media (streamer_login, status) \
             VALUES ('nani', 'approved') RETURNING id",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO social_media_clip_preparation (clip_db_id, state) \
             VALUES ($1, 'preview_ready')",
        )
        .bind(i64::from(clip))
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO social_media_clip_approval (clip_db_id, state, approved_platforms) \
             VALUES ($1, 'approved', '[\"tiktok\"]'::jsonb)",
        )
        .bind(clip)
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO twitch_clips_upload_queue (clip_id, status) VALUES ($1, 'pending')",
        )
        .bind(i64::from(clip))
        .execute(&pool)
        .await
        .unwrap();

        let mut changed_layout = default_streamer_layout();
        changed_layout.mode = "stacked".to_string();
        let outcome = set_clip_layout_override(&pool, clip, Some(&changed_layout))
            .await
            .unwrap();
        assert!(outcome.changed);
        assert_eq!(outcome.pending_stopped, 1);
        let preparation: String = sqlx::query_scalar(
            "SELECT state FROM social_media_clip_preparation WHERE clip_db_id = $1",
        )
        .bind(i64::from(clip))
        .fetch_one(&pool)
        .await
        .unwrap();
        let approval: (String, serde_json::Value) = sqlx::query_as(
            "SELECT state, approved_platforms FROM social_media_clip_approval WHERE clip_db_id = $1",
        )
        .bind(clip)
        .fetch_one(&pool)
        .await
        .unwrap();
        let queue: (String, Option<String>) = sqlx::query_as(
            "SELECT status, last_error FROM twitch_clips_upload_queue WHERE clip_id = $1",
        )
        .bind(i64::from(clip))
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(preparation, "pending");
        assert_eq!(approval.0, "editing");
        assert_eq!(approval.1, json!([]));
        assert_eq!(queue.0, "failed");
        assert_eq!(queue.1.as_deref(), Some("layout_changed"));

        let unchanged = set_clip_layout_override(&pool, clip, Some(&changed_layout))
            .await
            .unwrap();
        assert!(!unchanged.changed);
        assert_eq!(unchanged.pending_stopped, 0);
    }

    #[tokio::test]
    async fn layoutaenderung_lehnt_aktive_vorbereitung_und_upload_ab() {
        let Some(pool) = make_pool("t_sm_layout_active_conflicts").await else {
            return;
        };
        let clip_rendering: i32 = sqlx::query_scalar(
            "INSERT INTO twitch_clips_social_media (streamer_login) VALUES ('nani') RETURNING id",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO social_media_clip_preparation (clip_db_id, state) \
             VALUES ($1, 'rendering')",
        )
        .bind(i64::from(clip_rendering))
        .execute(&pool)
        .await
        .unwrap();
        let mut changed_layout = default_streamer_layout();
        changed_layout.mode = "stacked".to_string();
        assert!(matches!(
            set_clip_layout_override(&pool, clip_rendering, Some(&changed_layout)).await,
            Err(LayoutMutationError::PreparationRunning)
        ));
        let stored: Option<serde_json::Value> = sqlx::query_scalar(
            "SELECT layout_override_json FROM twitch_clips_social_media WHERE id = $1",
        )
        .bind(clip_rendering)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert!(stored.is_none());

        let clip_uploading: i32 = sqlx::query_scalar(
            "INSERT INTO twitch_clips_social_media (streamer_login) VALUES ('nani') RETURNING id",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO twitch_clips_upload_queue (clip_id, status) VALUES ($1, 'processing')",
        )
        .bind(i64::from(clip_uploading))
        .execute(&pool)
        .await
        .unwrap();
        assert!(matches!(
            set_clip_layout_override(&pool, clip_uploading, Some(&changed_layout)).await,
            Err(LayoutMutationError::UploadRunning(1))
        ));
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
        assert_eq!(
            got.game_crop,
            LayoutBox {
                x: 420,
                y: 0,
                w: 1080,
                h: 1080
            }
        );
        assert_eq!(
            got.cam_position,
            LayoutBox {
                x: 0,
                y: 0,
                w: 1080,
                h: 540
            }
        );

        // Auch der Clip-Pfad (Override) darf daran nicht scheitern.
        let clip: i32 = sqlx::query_scalar(
            "INSERT INTO twitch_clips_social_media (streamer_login) VALUES ('alt') RETURNING id",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        let eff = get_clip_effective_layout(&pool, clip).await;
        assert_eq!(
            eff.cam_position,
            LayoutBox {
                x: 0,
                y: 0,
                w: 1080,
                h: 540
            }
        );
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
