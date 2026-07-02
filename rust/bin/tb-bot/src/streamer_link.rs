//! Automatischer Streamer↔Discord-Abgleich (Rust-Port des Python StreamerLinkMatcher-Cog).
//!
//! Läuft alle 6h als Hintergrund-Task. Wenn keine neuen unverknüpften Partner vorhanden
//! sind, beendet er sich still ohne Discord-Post.
//!
//! Schwellen:
//!   Score ≥ AUTO_THRESHOLD (90) → automatisch verknüpfen + Rolle vergeben
//!   Score ≥ REVIEW_THRESHOLD (70) → "Manual-Link-Prompt" in den Notify-Kanal
//!   darunter → verwerfen, als geprüft markieren
//!
//! State-Datei (JSON) teilt denselben Pfad wie der Python-Cog →
//! nahtlose Übergabe ohne Re-Scan beim Umstieg.

use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};
use tb_analytics::streamer_link::list_unlinked;
use tb_transport_discord::{backend::SendRichMessage, BrokerRelay, DiscordBackend, GuildMember};
use unicode_normalization::UnicodeNormalization;

// ── Konfiguration ────────────────────────────────────────────────────────────

const AUTO_THRESHOLD: i32 = 90;
const REVIEW_THRESHOLD: i32 = 70;
const FUZZY_FLOOR: f64 = 0.62;
const SCAN_INTERVAL: Duration = Duration::from_secs(6 * 60 * 60);

static AFFIXES: &[&str] = &[
    "ttv", "live", "twitch", "stream", "streams", "streamer", "yt", "youtube", "tv", "official",
    "real", "the", "its", "im", "iam", "gg",
];

fn env_u64(name: &str, default: u64) -> u64 {
    std::env::var(name)
        .ok()
        .and_then(|v| v.trim().parse().ok())
        .unwrap_or(default)
}

fn env_bool(name: &str, default: bool) -> bool {
    match std::env::var(name).as_deref() {
        Ok("1") | Ok("true") | Ok("yes") | Ok("on") => true,
        Ok("0") | Ok("false") | Ok("no") | Ok("off") => false,
        _ => default,
    }
}

// ── Konfigurationsstruct ──────────────────────────────────────────────────────

pub struct StreamerLinkConfig {
    pub notify_channel_id: u64,
    pub streamer_role_id: u64,
    pub guild_id: u64,
    pub state_path: PathBuf,
    pub enabled: bool,
}

impl StreamerLinkConfig {
    pub fn from_env() -> Self {
        let state_path = std::env::var("STREAMER_LINK_STATE_PATH")
            .map(PathBuf::from)
            .unwrap_or_else(|_| {
                PathBuf::from(
                    "/home/naniadm/Documents/Deadlock-Bots/data/streamer_link_state.json",
                )
            });
        Self {
            notify_channel_id: env_u64("STREAMER_LINK_NOTIFY_CHANNEL_ID", 1374364800817303632),
            streamer_role_id: env_u64("STREAMER_ROLE_ID", 1313624729466441769),
            guild_id: env_u64("MAIN_GUILD_ID", 1289721245281292288),
            state_path,
            enabled: env_bool("STREAMER_LINK_ENABLED", true),
        }
    }
}

// ── State-Datei ───────────────────────────────────────────────────────────────

#[derive(Debug, Default, Serialize, Deserialize)]
struct LinkState {
    #[serde(default)]
    processed: HashMap<String, serde_json::Value>,
    #[serde(default)]
    pending: serde_json::Value,
    #[serde(default)]
    manual_pending: serde_json::Value,
}

impl LinkState {
    fn load(path: &Path) -> Self {
        let raw = match std::fs::read_to_string(path) {
            Ok(raw) => raw,
            Err(error) => {
                tracing::debug!(
                    %error,
                    path = %path.display(),
                    "streamer_link: State nicht lesbar, nutze Default"
                );
                return Self::default();
            }
        };
        match serde_json::from_str(&raw) {
            Ok(state) => state,
            Err(error) => {
                tracing::warn!(
                    %error,
                    path = %path.display(),
                    "streamer_link: State-JSON nicht lesbar, nutze Default"
                );
                Self::default()
            }
        }
    }

    fn save(&self, path: &Path) {
        let json = match serde_json::to_string(self) {
            Ok(json) => json,
            Err(error) => {
                tracing::warn!(%error, "streamer_link: State konnte nicht serialisiert werden");
                return;
            }
        };
        let tmp = path.with_extension("tmp");
        if let Err(error) = std::fs::write(&tmp, &json) {
            tracing::warn!(
                %error,
                path = %tmp.display(),
                "streamer_link: State-Tempfile konnte nicht geschrieben werden"
            );
            return;
        }
        if let Err(error) = std::fs::rename(&tmp, path) {
            tracing::warn!(
                %error,
                tmp = %tmp.display(),
                path = %path.display(),
                "streamer_link: State-Tempfile konnte nicht ersetzt werden"
            );
        }
    }

    fn is_handled(&self, login: &str) -> bool {
        self.processed.contains_key(login)
    }

    fn mark(&mut self, login: &str, status: &str, extra: serde_json::Value) {
        let mut entry = serde_json::json!({
            "status": status,
            "at": chrono::Utc::now().to_rfc3339(),
        });
        if let (Some(obj), Some(extra_obj)) = (entry.as_object_mut(), extra.as_object()) {
            for (k, v) in extra_obj {
                obj.insert(k.clone(), v.clone());
            }
        }
        self.processed.insert(login.to_string(), entry);
    }
}

// ── Name-Normalisierung ───────────────────────────────────────────────────────

fn leet_replace(c: char) -> char {
    match c {
        '0' => 'o',
        '1' => 'i',
        '3' => 'e',
        '4' => 'a',
        '5' => 's',
        '7' => 't',
        '$' => 's',
        '@' => 'a',
        '8' => 'b',
        _ => c,
    }
}

fn norm_key(value: &str) -> String {
    // Unicode-Normalisierung → ASCII-Transliteration
    let ascii: String = value
        .nfkd()
        .filter(|c| c.is_ascii())
        .flat_map(|c| c.to_lowercase())
        .map(leet_replace)
        .collect();

    // Nur a-z0-9-Tokens
    let tokens: Vec<&str> = {
        let mut start = 0;
        let mut result = Vec::new();
        let bytes = ascii.as_bytes();
        let mut i = 0;
        while i <= bytes.len() {
            let boundary = i == bytes.len()
                || !(bytes[i].is_ascii_alphanumeric());
            if boundary {
                if i > start {
                    result.push(&ascii[start..i]);
                }
                start = i + 1;
            }
            i += 1;
        }
        result
    };

    let kept: Vec<&str> = tokens
        .iter()
        .copied()
        .filter(|t| !AFFIXES.contains(t))
        .collect();

    let used = if kept.is_empty() { &tokens } else { &kept };
    used.join("")
}

fn similarity(a: &str, b: &str) -> f64 {
    if a.is_empty() || b.is_empty() {
        return 0.0;
    }
    if a == b {
        return 1.0;
    }
    strsim::jaro_winkler(a, b)
}

fn fallback_score(ratio: f64, exact_unique: bool) -> i32 {
    if exact_unique && ratio >= 0.999 {
        return 92;
    }
    if ratio >= 0.93 {
        return 80;
    }
    if ratio >= 0.82 {
        return 72;
    }
    (ratio * 70.0).round() as i32
}

// ── Member-Index ─────────────────────────────────────────────────────────────

struct MemberIndex {
    // norm_key → Liste von Members mit dem gleichen Schlüssel
    exact: HashMap<String, Vec<GuildMember>>,
}

impl MemberIndex {
    fn build(members: &[GuildMember]) -> Self {
        let mut exact: HashMap<String, Vec<GuildMember>> = HashMap::new();
        for m in members {
            let keys = [
                norm_key(&m.name),
                m.global_name.as_deref().map(norm_key).unwrap_or_default(),
                m.nick.as_deref().map(norm_key).unwrap_or_default(),
            ];
            let unique_keys: std::collections::HashSet<_> =
                keys.iter().filter(|k| !k.is_empty()).collect();
            for key in unique_keys {
                exact.entry(key.clone()).or_default().push(m.clone());
            }
        }
        Self { exact }
    }

    fn best_match(&self, login_key: &str) -> Option<(GuildMember, f64, bool)> {
        if let Some(members) = self.exact.get(login_key) {
            let exact_unique = members.len() == 1;
            return Some((members[0].clone(), 1.0, exact_unique));
        }
        // Fuzzy-Suche über alle Keys
        let mut best: Option<(GuildMember, f64)> = None;
        for (key, members) in &self.exact {
            let ratio = similarity(login_key, key);
            if ratio > best.as_ref().map(|(_, r)| *r).unwrap_or(0.0) {
                best = Some((members[0].clone(), ratio));
            }
        }
        best.map(|(m, r)| (m, r, false))
    }
}

// ── Broker-Helfer ────────────────────────────────────────────────────────────

async fn link_discord_profile(
    internal_base: &str,
    token: &str,
    login: &str,
    discord_user_id: &str,
    discord_display_name: &str,
) -> Result<(), String> {
    let client = reqwest::Client::new();
    let url = format!(
        "{}/internal/twitch/v1/streamers/{}/discord-profile",
        internal_base, login
    );
    let resp = client
        .post(&url)
        .header("X-Internal-Token", token)
        .json(&serde_json::json!({
            "discord_user_id": discord_user_id,
            "discord_display_name": discord_display_name,
            "mark_member": true,
        }))
        .timeout(Duration::from_secs(10))
        .send()
        .await
        .map_err(|e| e.to_string())?;
    if !resp.status().is_success() {
        return Err(format!("HTTP {}", resp.status()));
    }
    Ok(())
}

async fn grant_role(relay: &BrokerRelay, guild_id: u64, user_id: u64, role_id: u64) -> String {
    match relay
        .add_member_role(guild_id, user_id, role_id, "Streamer-Link Auto-Match")
        .await
    {
        Ok(()) => "Streamer-Rolle vergeben.".to_string(),
        Err(e) => format!("⚠️ Rolle fehlgeschlagen: {e}"),
    }
}

async fn notify_embed(relay: &BrokerRelay, channel_id: u64, title: &str, description: &str, color: u32) {
    let payload = SendRichMessage {
        channel_id: channel_id as i64,
        content: None,
        embed: serde_json::json!({
            "title": title,
            "description": description,
            "color": color,
        }),
        allowed_role_ids: vec![],
        view_spec: None,
    };
    if let Err(e) = relay.send_rich_message(payload).await {
        tracing::warn!("streamer_link: Discord-Notify fehlgeschlagen: {e}");
    }
}

// ── Haupt-Scan-Logik ──────────────────────────────────────────────────────────

async fn run_scan(
    pool: &sqlx::PgPool,
    relay: &BrokerRelay,
    config: &StreamerLinkConfig,
    internal_base: &str,
    token: &str,
    state: &mut LinkState,
) {
    // Kandidaten holen
    let candidates = match list_unlinked(pool).await {
        Ok(c) => c,
        Err(e) => {
            tracing::error!("streamer_link: DB-Fehler beim Kandidaten-Abruf: {e}");
            return;
        }
    };

    // Nur wirklich neue (noch nicht im State)
    let new_candidates: Vec<_> = candidates
        .into_iter()
        .filter(|c| !state.is_handled(&c.twitch_login))
        .collect();

    if new_candidates.is_empty() {
        tracing::debug!("streamer_link: keine neuen Kandidaten – übersprungen");
        return;
    }

    tracing::info!(count = new_candidates.len(), "streamer_link: neue Kandidaten gefunden");

    // Discord-Member-Index aufbauen
    let members = match relay.list_members().await {
        Ok(m) => m,
        Err(e) => {
            tracing::error!("streamer_link: Member-Abruf vom Broker fehlgeschlagen: {e}");
            return;
        }
    };
    let index = MemberIndex::build(&members);

    let mut used_member_ids: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut stats = (0u32, 0u32, 0u32, 0u32); // checked, auto, review, skipped

    for entry in &new_candidates {
        let login = entry.twitch_login.to_lowercase();
        stats.0 += 1;

        let login_key = norm_key(&login);
        if login_key.is_empty() {
            state.mark(&login, "no_match", serde_json::json!({"reason": "leerer Schlüssel"}));
            stats.3 += 1;
            continue;
        }

        let Some((member, ratio, exact_unique)) = index.best_match(&login_key) else {
            state.mark(&login, "no_match", serde_json::json!({"reason": format!("kein Member")}));
            stats.3 += 1;
            notify_embed(
                relay,
                config.notify_channel_id,
                "🔗 Kein Discord-Match",
                &format!(
                    "**Twitch:** `{login}`\nKein Discord-Account automatisch gefunden.\nManuelle Verknüpfung über das Dashboard."
                ),
                0xE67E22,
            )
            .await;
            continue;
        };

        if ratio < FUZZY_FLOOR {
            state.mark(&login, "no_match", serde_json::json!({"reason": format!("Ähnlichkeit {ratio:.2} < {FUZZY_FLOOR}")}));
            stats.3 += 1;
            continue;
        }

        if used_member_ids.contains(&member.id) {
            state.mark(&login, "no_match", serde_json::json!({"reason": "Member-Kollision"}));
            stats.3 += 1;
            continue;
        }

        let score = fallback_score(ratio, exact_unique);
        let display = member
            .global_name
            .as_deref()
            .unwrap_or(&member.name)
            .to_string();

        if score >= AUTO_THRESHOLD {
            match link_discord_profile(internal_base, token, &login, &member.id, &display).await {
                Ok(()) => {
                    let member_id: u64 = member.id.parse().unwrap_or(0);
                    let role_note =
                        grant_role(relay, config.guild_id, member_id, config.streamer_role_id)
                            .await;
                    state.mark(
                        &login,
                        "auto_linked",
                        serde_json::json!({"discord_user_id": member.id, "score": score}),
                    );
                    used_member_ids.insert(member.id.clone());
                    stats.1 += 1;
                    notify_embed(
                        relay,
                        config.notify_channel_id,
                        "✅ Auto-verknüpft",
                        &format!(
                            "**Twitch:** `{login}`\n**Discord:** `{display}` (`{}`)\n**Score:** {score}%\n{role_note}",
                            member.name
                        ),
                        0x2ECC71,
                    )
                    .await;
                }
                Err(e) => {
                    tracing::error!("streamer_link: Auto-Link-Fehler für {login}: {e}");
                    notify_embed(
                        relay,
                        config.notify_channel_id,
                        "⚠️ Streamer-Link Fehler",
                        &format!("Auto-Link für `{login}` → `{display}` fehlgeschlagen: {e}"),
                        0xE74C3C,
                    )
                    .await;
                }
            }
        } else if score >= REVIEW_THRESHOLD {
            state.mark(
                &login,
                "review_posted",
                serde_json::json!({"discord_user_id": member.id, "score": score}),
            );
            used_member_ids.insert(member.id.clone());
            stats.2 += 1;
            notify_embed(
                relay,
                config.notify_channel_id,
                "❓ Möglicher Streamer-Match",
                &format!(
                    "**Twitch:** `{login}`\n**Discord:** `{display}` (`{}`)\n**Score:** {score}%\n\nManuelle Bestätigung: Dashboard → Streamer → Discord verknüpfen.",
                    member.name
                ),
                0xF1C40F,
            )
            .await;
        } else {
            state.mark(
                &login,
                "no_match",
                serde_json::json!({"reason": format!("Score {score} < {REVIEW_THRESHOLD}")}),
            );
            stats.3 += 1;
        }

        tokio::task::yield_now().await;
    }

    state.save(&config.state_path);
    tracing::info!(
        checked = stats.0,
        auto = stats.1,
        review = stats.2,
        skipped = stats.3,
        "streamer_link: Scan abgeschlossen"
    );
}

// ── Hintergrund-Task ──────────────────────────────────────────────────────────

pub async fn streamer_link_task(
    pool: sqlx::PgPool,
    relay: BrokerRelay,
    config: Arc<StreamerLinkConfig>,
    internal_base: String,
    token: String,
) {
    if !config.enabled {
        tracing::info!("streamer_link: deaktiviert (STREAMER_LINK_ENABLED=0)");
        return;
    }

    let mut interval = tokio::time::interval(SCAN_INTERVAL);
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    // Erste Iteration überspringen (sofort beim Start)
    interval.tick().await;

    loop {
        interval.tick().await;
        let mut state = LinkState::load(&config.state_path);
        run_scan(&pool, &relay, &config, &internal_base, &token, &mut state).await;
    }
}
