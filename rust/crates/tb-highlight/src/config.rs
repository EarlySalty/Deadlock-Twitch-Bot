//! Konfigurationskonstanten des Highlight-Clippers — Port von
//! `bot/highlight_clipper/config.py` (1:1-Werte).

/// Discord-Channel-ID für Highlight-Posts.
pub const HIGHLIGHT_DISCORD_CHANNEL_ID: i64 = 1511060958460776458;
/// Pfad der State-Datei (verarbeitete Matches).
pub const STATE_PATH: &str = "data/highlight_clipper/state.json";
/// Ausgabeverzeichnis der geschnittenen Clips.
pub const CLIPS_DIR: &str = "data/highlight_clipper/clips";
/// Poll-Intervall des Worker-Loops in Sekunden.
pub const POLL_INTERVAL_SECONDS: u64 = 600;
/// Clip-Vorlauf vor der ersten Action.
pub const CLIP_PRE_ROLL_SECONDS: i64 = 6;
/// Clip-Nachlauf nach der letzten Action.
pub const CLIP_POST_ROLL_SECONDS: i64 = 4;
/// Maximale Cliplänge in Sekunden.
pub const MAX_CLIP_SECONDS: i64 = 40;
/// Zusätzliches Padding um das Clip-Fenster.
pub const CLIP_PADDING_SECONDS: i64 = 10;
/// Mindestanzahl Kills für einen Multikill.
pub const MULTIKILL_MIN_KILLS: usize = 2;
/// Maximaler Tickabstand (Sekunden) zwischen Kills eines Multikills.
pub const MULTIKILL_THRESHOLD_SECONDS: i64 = 15;
/// Mindestanzahl Tode für einen Teamfight.
pub const TEAMFIGHT_MIN_KILLS: usize = 4;
/// Maximaler Abstand (Sekunden) zwischen verketteten Teamfight-Toden.
pub const TEAMFIGHT_THRESHOLD_SECONDS: i64 = 15;
/// Pfad zum ffmpeg-Binary.
pub const FFMPEG_PATH: &str = "/usr/bin/ffmpeg";
/// Maximale Discord-Dateigröße in MB.
pub const MAX_DISCORD_FILE_MB: i64 = 24;
