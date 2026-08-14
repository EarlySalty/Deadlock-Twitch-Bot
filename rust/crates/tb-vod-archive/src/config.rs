//! Laufzeit-Konfiguration des VOD-Archivs.
//!
//! Alles, was den Ort der Werkzeuge, die Grenzen eines Laufs und die Kadenz
//! betrifft, kommt aus der Umgebung. Welche Kanaele ueberhaupt archiviert
//! werden und wie sichtbar die Uploads sind, steht dagegen je Streamer in
//! `social_media_vod_archive` und damit im Dashboard: das sind Entscheidungen
//! der Streamer, keine Betriebsparameter. Einen Kanal aus der Umgebung gibt es
//! deshalb nicht mehr.

use std::path::{Path, PathBuf};
use std::time::Duration;

/// YouTube nimmt maximal 12 Stunden. Mit Puffer wird frueher geschnitten,
/// damit eine ungenaue Laengenmessung nicht kurz vor Schluss den Upload kippt.
pub const MAX_PART_SECONDS: i64 = 11 * 3600 + 30 * 60;

#[derive(Debug, Clone)]
pub struct VodArchiveConfig {
    /// Wurzel fuer die heruntergeladenen Dateien; je Streamer entsteht darin
    /// ein eigenes Unterverzeichnis.
    pub download_dir: PathBuf,
    /// Downloads kosten kein API-Kontingent, nur Zeit und Platte, deshalb ein
    /// eigenes, hoeheres Limit als beim Upload. Gilt fuer den ganzen Lauf,
    /// nicht je Streamer: Zeit und Platte teilen sich alle.
    pub max_downloads_per_run: usize,
    /// Ein Upload kostet 1600 der 10000 Einheiten Tageskontingent. Zwei Laeufe
    /// mit je zwei Uploads bleiben mit 6400 sicher unter der Grenze und lassen
    /// Luft fuer die uebrige Social-Media-Pipeline. Das Kontingent haengt am
    /// Google-Projekt, nicht am Nutzertoken, deshalb gilt auch diese Grenze
    /// ueber alle Streamer zusammen.
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

    /// Ablage eines Streamers. Je Kanal ein eigenes Unterverzeichnis, damit
    /// zwei Streamer sich weder Dateien noch das Aufraeumen teilen.
    pub fn verzeichnis_fuer(&self, streamer_login: &str) -> PathBuf {
        self.download_dir.join(sicherer_ordnername(streamer_login))
    }
}

/// Ein Twitch-Login besteht aus Buchstaben, Ziffern und Unterstrich. Alles
/// andere kommt nicht aus Twitch, sondern aus einer falsch gefuellten Zeile,
/// und darf sich nicht als `..` durch das Dateisystem schreiben.
fn sicherer_ordnername(streamer_login: &str) -> String {
    let sauber: String = streamer_login
        .trim()
        .to_lowercase()
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect();
    if sauber.is_empty() {
        "unbekannt".to_string()
    } else {
        sauber
    }
}

/// Freier Platz zaehlt fuer die gemeinsame Wurzel, nicht je Streamer: es ist
/// dieselbe Platte.
pub fn wurzel_oder_elternteil(pfad: &Path) -> PathBuf {
    if pfad.exists() {
        pfad.to_path_buf()
    } else {
        // Vor dem ersten Lauf gibt es das Verzeichnis noch nicht.
        pfad.parent()
            .unwrap_or_else(|| Path::new("."))
            .to_path_buf()
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
        assert_eq!(cfg.max_uploads_per_run, 2);
        assert_eq!(cfg.interval, Duration::from_secs(12 * 3600));
    }

    #[test]
    fn werte_werden_uebernommen() {
        let cfg = VodArchiveConfig::load(&quelle(&[
            ("TB_VOD_ARCHIVE_MAX_UPLOADS", "3"),
            ("TB_VOD_ARCHIVE_INTERVAL_HOURS", "6"),
            ("TB_VOD_ARCHIVE_RATE_LIMIT", "5M"),
        ]));
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

    #[test]
    fn jeder_streamer_bekommt_ein_eigenes_verzeichnis() {
        let cfg = VodArchiveConfig::load(&quelle(&[("TB_VOD_ARCHIVE_DIR", "/archiv")]));
        assert_eq!(
            cfg.verzeichnis_fuer("EarlySalty"),
            PathBuf::from("/archiv/earlysalty")
        );
        // Ein Login kann keine Pfadtrenner enthalten; kommt trotzdem einer an,
        // darf er nicht aus der Wurzel herausfuehren.
        assert_eq!(
            cfg.verzeichnis_fuer("../../etc"),
            PathBuf::from("/archiv/______etc")
        );
        assert_eq!(
            cfg.verzeichnis_fuer("  "),
            PathBuf::from("/archiv/unbekannt")
        );
    }
}
