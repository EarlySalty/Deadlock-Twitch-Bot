/// Rohdaten eines Twitch-Clips wie von der Helix-API geliefert.
#[derive(Debug, Clone)]
pub struct ClipRecord {
    pub clip_id: String,
    pub clip_url: String,
    pub clip_title: String,
    pub thumbnail_url: Option<String>,
    pub streamer_login: String,
    pub twitch_user_id: String,
    pub created_at: String,
    pub duration_seconds: f64,
    pub view_count: i64,
    pub game_name: Option<String>,
    /// Twitch-Kategorie-ID aus Helix; Grundlage der Kategorie-Zuordnung.
    pub game_id: Option<String>,
}

/// Ergebnis eines Fetch-Laufs für einen einzelnen Streamer.
#[derive(Debug, Default)]
pub struct StreamerFetchResult {
    pub login: String,
    pub clips_found: i32,
    pub clips_new: i32,
    pub duration_ms: i64,
    pub error: Option<String>,
}

/// Aggregierte Statistik nach einem Gesamt-Fetch-Lauf.
#[derive(Debug, Default)]
pub struct FetchStats {
    pub streamers: u32,
    pub clips_total: u32,
    pub clips_new: u32,
    pub errors: u32,
    pub duration_ms: u64,
}
