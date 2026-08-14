//! Laufzeit-Konfiguration des VOD-Archivs.
//!
//! Alles, was den Ort der Werkzeuge, die Grenzen eines Laufs und die Kadenz
//! betrifft, kommt aus der Umgebung. Ob das Archiv ueberhaupt laeuft und wie
//! sichtbar die Uploads sind, steht dagegen in `social_media_settings` und
//! damit im Dashboard: das sind Entscheidungen des Betreibers, keine
//! Betriebsparameter.

use std::path::PathBuf;
use std::time::Duration;

/// YouTube nimmt maximal 12 Stunden. Mit Puffer wird frueher geschnitten,
/// damit eine ungenaue Laengenmessung nicht kurz vor Schluss den Upload kippt.
pub const MAX_PART_SECONDS: i64 = 11 * 3600 + 30 * 60;

/// Eigener Kanal. Der VOD-Export fremder Kanaele haengt an anderer Stelle
/// (`tb-highlight::vod_export`) und geht bewusst nicht hier durch.
pub const DEFAULT_CHANNEL: &str = "earlysalty";

#[derive(Debug, Clone)]
pub struct VodArchiveConfig {
    pub channel: String,
    /// Wurzel fuer die heruntergeladenen Dateien.
    pub download_dir: PathBuf,
    /// Downloads kosten kein API-Kontingent, nur Zeit und Platte, deshalb ein
    /// eigenes, hoeheres Limit als beim Upload.
    pub max_downloads_per_run: usize,
    /// Ein Upload kostet 1600 der 10000 Einheiten Tageskontingent. Zwei Laeufe
    /// mit je zwei Uploads bleiben mit 6400 sicher unter der Grenze und lassen
    /// Luft fuer die uebrige Social-Media-Pipeline.
    pub max_uploads_per_run: usize,
    /// Untergrenze freier Plattenplatz in Gigabyte.
    pub min_free_gb: u64,
    /// Lokale Dateien nach dieser Frist loeschen. 0 heisst: nie loeschen, das
    /// lokale Archiv ist der eigentliche Verlustschutz.
    pub keep_local_days: i64,
    /// Optionale Bandbreitenbremse fuer yt-dlp, etwa "5M".
    pub rate_limit: Option<String>,
    pub yt_dlp: PathBuf,
    pub ffmpeg: PathBuf,
    pub ffprobe: PathBuf,
    /// Harte Zeitgrenze fuer einen einzelnen yt-dlp-Download.
    pub download_timeout: Duration,
    /// Abstand zwischen zwei Laeufen. Default 12 Stunden, also zweimal taeglich.
    pub interval: Duration,
    /// Optionale Ziel-Playlist auf YouTube.
    pub playlist_id: Option<String>,
    pub category_id: String,
    pub title_template: String,
}

impl Default for VodArchiveConfig {
    fn default() -> Self {
        Self {
            channel: DEFAULT_CHANNEL.to_string(),
            download_dir: PathBuf::from("data/vod-archive"),
            max_downloads_per_run: 6,
            max_uploads_per_run: 2,
            min_free_gb: 80,
            keep_local_days: 0,
            rate_limit: None,
            yt_dlp: PathBuf::from("yt-dlp"),
            ffmpeg: PathBuf::from("ffmpeg"),
            ffprobe: PathBuf::from("ffprobe"),
            download_timeout: Duration::from_secs(21_600),
            interval: Duration::from_secs(12 * 3600),
            playlist_id: None,
            category_id: "20".to_string(),
            title_template: "{title} [{date}]{part}".to_string(),
        }
    }
}

impl VodArchiveConfig {
    pub fn from_env() -> Self {
        Self::load(&|name| std::env::var(name).ok())
    }

    /// Wie [`Self::from_env`], aber mit austauschbarer Quelle, damit die
    /// Auswertung ohne echte Umgebungsvariablen pruefbar bleibt.
    pub fn load(source: &dyn Fn(&str) -> Option<String>) -> Self {
        let default = Self::default();
        let text = |name: &str| {
            source(name)
                .map(|value| value.trim().to_string())
                .filter(|value| !value.is_empty())
        };
        let zahl = |name: &str, fallback: u64| -> u64 {
            match text(name).map(|value| value.parse::<u64>()) {
                Some(Ok(value)) => value,
                Some(Err(_)) => {
                    tracing::warn!(variable = name, "keine gueltige Zahl, Default greift");
                    fallback
                }
                None => fallback,
            }
        };

        Self {
            channel: text("TB_VOD_ARCHIVE_CHANNEL").unwrap_or(default.channel),
            download_dir: text("TB_VOD_ARCHIVE_DIR")
                .map(PathBuf::from)
                .unwrap_or(default.download_dir),
            max_downloads_per_run: zahl(
                "TB_VOD_ARCHIVE_MAX_DOWNLOADS",
                default.max_downloads_per_run as u64,
            ) as usize,
            max_uploads_per_run: zahl(
                "TB_VOD_ARCHIVE_MAX_UPLOADS",
                default.max_uploads_per_run as u64,
            ) as usize,
            min_free_gb: zahl("TB_VOD_ARCHIVE_MIN_FREE_GB", default.min_free_gb),
            keep_local_days: zahl(
                "TB_VOD_ARCHIVE_KEEP_LOCAL_DAYS",
                default.keep_local_days as u64,
            ) as i64,
            rate_limit: text("TB_VOD_ARCHIVE_RATE_LIMIT"),
            // yt-dlp wird im Bot bereits zentral aufgeloest und von dort
            // hereingereicht; diese Variable ist nur der Notausgang.
            yt_dlp: text("YT_DLP_PATH")
                .map(PathBuf::from)
                .unwrap_or(default.yt_dlp),
            ffmpeg: text("TB_VOD_ARCHIVE_FFMPEG")
                .map(PathBuf::from)
                .unwrap_or(default.ffmpeg),
            ffprobe: text("TB_VOD_ARCHIVE_FFPROBE")
                .map(PathBuf::from)
                .unwrap_or(default.ffprobe),
            download_timeout: Duration::from_secs(zahl(
                "TB_VOD_ARCHIVE_DOWNLOAD_TIMEOUT_SECS",
                default.download_timeout.as_secs(),
            )),
            interval: Duration::from_secs(
                zahl(
                    "TB_VOD_ARCHIVE_INTERVAL_HOURS",
                    default.interval.as_secs() / 3600,
                )
                .max(1)
                    * 3600,
            ),
            playlist_id: text("TB_VOD_ARCHIVE_PLAYLIST_ID"),
            category_id: text("TB_VOD_ARCHIVE_CATEGORY_ID").unwrap_or(default.category_id),
            title_template: text("TB_VOD_ARCHIVE_TITLE_TEMPLATE").unwrap_or(default.title_template),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn quelle(werte: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> {
        let map: HashMap<String, String> = werte
            .iter()
            .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
            .collect();
        move |name: &str| map.get(name).cloned()
    }

    #[test]
    fn leere_umgebung_liefert_defaults() {
        let cfg = VodArchiveConfig::load(&quelle(&[]));
        assert_eq!(cfg.channel, DEFAULT_CHANNEL);
        assert_eq!(cfg.max_uploads_per_run, 2);
        assert_eq!(cfg.interval, Duration::from_secs(12 * 3600));
    }

    #[test]
    fn werte_werden_uebernommen() {
        let cfg = VodArchiveConfig::load(&quelle(&[
            ("TB_VOD_ARCHIVE_CHANNEL", " nani "),
            ("TB_VOD_ARCHIVE_MAX_UPLOADS", "3"),
            ("TB_VOD_ARCHIVE_INTERVAL_HOURS", "6"),
            ("TB_VOD_ARCHIVE_RATE_LIMIT", "5M"),
        ]));
        assert_eq!(cfg.channel, "nani");
        assert_eq!(cfg.max_uploads_per_run, 3);
        assert_eq!(cfg.interval, Duration::from_secs(6 * 3600));
        assert_eq!(cfg.rate_limit.as_deref(), Some("5M"));
    }

    #[test]
    fn unsinn_faellt_auf_default_zurueck() {
        let cfg = VodArchiveConfig::load(&quelle(&[
            ("TB_VOD_ARCHIVE_MAX_UPLOADS", "viele"),
            // Ein Intervall von 0 wuerde den Worker heisslaufen lassen.
            ("TB_VOD_ARCHIVE_INTERVAL_HOURS", "0"),
            ("TB_VOD_ARCHIVE_RATE_LIMIT", "   "),
        ]));
        assert_eq!(cfg.max_uploads_per_run, 2);
        assert_eq!(cfg.interval, Duration::from_secs(3600));
        assert!(cfg.rate_limit.is_none());
    }
}
