//! VOD-Archiv: Twitch-Aufzeichnungen sichern und auf YouTube spiegeln.
//!
//! Twitchs eigener YouTube-Export existiert nicht mehr, und VODs verfallen
//! nach kurzer Zeit. Der Worker holt sie deshalb zweimal taeglich ueber yt-dlp,
//! schneidet zu lange Aufzeichnungen verlustfrei an der 12-Stunden-Grenze von
//! YouTube und schiebt sie ueber den bestehenden YouTube-Uploader hoch.
//!
//! Drei Dinge sind Absicht:
//!
//! * **Je Streamer.** Kanal, Zustand, Ablage und YouTube-Zugang haengen am
//!   Streamer-Login, nicht an einer globalen Einstellung. Ein Upload nutzt nur
//!   die Credentials genau dieses Streamers; einen globalen Rueckfall gibt es
//!   nicht, er wuerde fremde VODs auf den falschen Kanal schieben.
//! * **Lokal zuerst.** Der Download laeuft auch ohne YouTube-Verbindung. Ein
//!   fehlender Login verschiebt nur den Upload; das lokale Archiv ist der
//!   eigentliche Verlustschutz.
//! * **Zustand in Postgres.** Upload-Sitzung und Byte-Position liegen in der
//!   Datenbank, damit ein Abbruch mitten in einem zweistellig grossen Upload
//!   fortgesetzt statt neu begonnen wird.
//!
//! Ein- und ausgeschaltet wird je Streamer ueber das Dashboard (Tabelle
//! `social_media_vod_archive`, siehe `tb_social_media::vod_archive`), nicht
//! ueber die Umgebung. Die Umgebung traegt nur Betriebsparameter, siehe
//! [`config`].
//!
//! Nicht zu verwechseln mit `tb_highlight::vod_export`: das schiebt VODs eines
//! Partnerkanals nach einem Stream in ein Google Drive und loescht sie lokal.

pub mod config;
pub mod error;
pub mod metadata;
pub mod store;
pub mod twitch;
pub mod worker;

pub use config::VodArchiveConfig;
pub use error::VodArchiveError;
pub use worker::VodArchiveWorker;
