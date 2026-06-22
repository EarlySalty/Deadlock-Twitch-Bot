//! Helix-Chatters-Poller (#11, P2.64/P2.61/P1.23).
//!
//! Port von `bot/analytics/mixin.py` (`collect_chatters_data`,
//! `_poll_chatters_single`, `_attempt_bot_moderator_self_heal`). Pro 30s-Tick
//! werden alle live Streamer über `GET /chat/chatters` abgefragt und **alle**
//! Anwesenden — inklusive stiller Lurker — in `twitch_session_chatters`,
//! `twitch_chatter_rollup` und `twitch_viewer_presence_ticks` gespiegelt.
//!
//! Das Token-Plumbing (Bot-Token, Streamer-OAuth, Mod-Self-Heal) wird per Trait
//! aus dem Binary injiziert; `tb-monitoring` kennt keinen konkreten Token-Store.
//! Der Helix-Call selbst läuft hinter dem [`ChattersFetcher`]-Seam, damit Tests
//! ohne Netz eine Chatter-Liste einspeisen können.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use chrono::{DateTime, SubsecRound, Utc};
use sqlx::PgPool;
use tb_chat::WHITELISTED_BOTS;
use tb_transport_twitch::HelixError;
use tokio::sync::Mutex;

use crate::record_presence_ticks;
use crate::subscriptions::ModeratorProvisioner;

/// Cooldown nach fehlgeschlagenem Mod-Self-Heal (Python `_mod_retry_cooldown`,
/// 10 Minuten). Solange aktiv wird kein erneuter Heal versucht.
const SELF_HEAL_COOLDOWN: Duration = Duration::from_secs(600);


// ---------------------------------------------------------------------------
// Injizierte Ports (Implementierung lebt im Composition-Root / Binary)
// ---------------------------------------------------------------------------

/// Bot-Token-Kontext (Python `BotTokenManager`). Self-Exclude über `bot_login`.
#[async_trait]
pub trait BotChatterAuth: Send + Sync {
    /// Aktueller Bot-Access-Token (`BotTokenManager::access_token`).
    async fn bot_token(&self) -> Option<String>;
    /// Bot-User-ID — dient als `moderator_id` im Helix-Call.
    async fn bot_user_id(&self) -> Option<String>;
    /// Bot-Login (kleingeschrieben) für den Self-Exclude-Filter.
    async fn bot_login(&self) -> Option<String>;
    /// `true`, wenn `moderator:read:chatters` gewährt ist. Wie Python gilt eine
    /// **leere** Scope-Liste als „noch nicht geladen → erlaubt".
    async fn has_chatters_scope(&self) -> bool;
}

/// Streamer-OAuth-Token-Fallback (Python `TokenProvider::get_valid_token`).
/// `Some` ⇔ der Streamer hat Raid aktiviert (raid_enabled-gated).
#[async_trait]
pub trait StreamerTokenSource: Send + Sync {
    async fn streamer_token(&self, twitch_user_id: &str) -> Option<String>;
}

/// Helix-`GET /chat/chatters`-Seam. Reale Impl wrappt `HelixClient::get_chatters`;
/// Tests injizieren ein Fake, damit kein HTTP in die Suite kommt.
#[async_trait]
pub trait ChattersFetcher: Send + Sync {
    /// Liefert die Chatter eines Kanals als `(user_login, user_id)`-Paare oder
    /// den Helix-Fehler (insb. [`HelixError::NotModerator`] = 403). Eine leere
    /// `user_id` (`""`) wird vom Aufrufer zu `None` normalisiert.
    async fn fetch_chatters(
        &self,
        broadcaster_id: &str,
        moderator_id: &str,
        token: &str,
    ) -> Result<Vec<(String, Option<String>)>, HelixError>;
}

// ---------------------------------------------------------------------------
// Roster
// ---------------------------------------------------------------------------

/// Ein live Streamer aus dem Roster (`twitch_live_state` ⋈ Partner-State).
#[derive(Debug, Clone)]
pub struct LiveStreamer {
    pub twitch_user_id: String,
    pub streamer_login: String,
    pub active_session_id: i64,
    /// `is_partner_active == 1` gatet ausschließlich den Mod-Self-Heal.
    pub is_partner_active: bool,
}

/// Lädt alle live Streamer mit aktiver Session (Python `collect_chatters_data`-
/// Roster). `streamer_login` wird normalisiert (`lower().trim()`).
pub async fn load_live_roster(pool: &PgPool) -> Result<Vec<LiveStreamer>, sqlx::Error> {
    let rows: Vec<(String, String, i64, i32)> = sqlx::query_as(
        "SELECT ls.twitch_user_id, ls.streamer_login, ls.active_session_id, \
                COALESCE(ps.is_partner_active, 0) AS is_partner_active \
         FROM twitch_live_state ls \
         LEFT JOIN twitch_streamers_partner_state ps \
                ON LOWER(ps.twitch_login) = LOWER(ls.streamer_login) \
         WHERE ls.is_live = 1 AND ls.active_session_id IS NOT NULL",
    )
    .fetch_all(pool)
    .await?;

    Ok(rows
        .into_iter()
        .map(
            |(twitch_user_id, streamer_login, active_session_id, partner)| LiveStreamer {
                twitch_user_id,
                streamer_login: normalize_login(&streamer_login),
                active_session_id,
                is_partner_active: partner == 1,
            },
        )
        .collect())
}

// ---------------------------------------------------------------------------
// Self-Heal-Cooldown
// ---------------------------------------------------------------------------

/// Per-Kanal-Cooldown-Tracker für den Mod-Self-Heal (geteilt über Ticks hinweg).
#[derive(Clone, Default)]
pub struct SelfHealCooldowns {
    inner: Arc<Mutex<HashMap<String, Instant>>>,
}

impl SelfHealCooldowns {
    pub fn new() -> Self {
        Self::default()
    }

    /// `true`, wenn der Kanal aktuell im Cooldown ist (kein Heal versuchen).
    async fn is_cooling(&self, key: &str) -> bool {
        let guard = self.inner.lock().await;
        guard.get(key).is_some_and(|until| Instant::now() < *until)
    }

    async fn set_cooldown(&self, key: &str) {
        let mut guard = self.inner.lock().await;
        guard.insert(key.to_string(), Instant::now() + SELF_HEAL_COOLDOWN);
    }

    async fn clear(&self, key: &str) {
        let mut guard = self.inner.lock().await;
        guard.remove(key);
    }
}

// ---------------------------------------------------------------------------
// Counters / Stats
// ---------------------------------------------------------------------------

/// Aggregierte Counter eines Collect-Zyklus (Observability, Python-Parität).
#[derive(Debug, Default, Clone)]
pub struct CycleStats {
    pub live_streamers: usize,
    pub bot_path_attempt: u64,
    pub bot_path_success: u64,
    pub bot_path_failure: u64,
    pub fallback_to_streamer_token: u64,
    pub self_heal_success: u64,
    pub self_heal_failure: u64,
    pub chatters_written: u64,
    pub lurkers_new: u64,
}

/// Ergebnis eines Pro-Streamer-Polls: die Chatter als `(login, user_id)`-Paare
/// (bereits roh, ungefiltert).
#[derive(Debug, Default)]
struct PollResult {
    chatters: Vec<(String, Option<String>)>,
    succeeded: bool,
}

// ---------------------------------------------------------------------------
// Pro-Streamer-Poll (Token-Reihenfolge + Self-Heal)
// ---------------------------------------------------------------------------

/// Pollt einen einzelnen Streamer (Python `_poll_chatters_single`, P2.64).
///
/// Token-Reihenfolge exakt wie Python:
/// 1. Bot-Pfad (falls Bot-Token + `moderator:read:chatters`-Scope vorhanden);
///    403 → Self-Heal, bei Erfolg genau EIN Retry desselben Calls.
/// 2. Streamer-OAuth-Fallback NUR wenn Bot-Pfad nicht erfolgreich, leere
///    Chatter-Liste und ein Streamer-Token (= raid_enabled) vorhanden ist.
#[allow(clippy::too_many_arguments)]
async fn poll_streamer_once(
    streamer: &LiveStreamer,
    bot_token: Option<&str>,
    bot_user_id: Option<&str>,
    streamer_token: Option<&str>,
    fetcher: &dyn ChattersFetcher,
    provisioner: &dyn ModeratorProvisioner,
    cooldowns: &SelfHealCooldowns,
    stats: &mut CycleStats,
) -> PollResult {
    let mut result = PollResult::default();

    // 1) Bot-Pfad.
    if let (Some(token), Some(mod_id)) = (bot_token, bot_user_id) {
        stats.bot_path_attempt += 1;
        match fetcher
            .fetch_chatters(&streamer.twitch_user_id, mod_id, token)
            .await
        {
            Ok(chatters) => {
                stats.bot_path_success += 1;
                result.chatters = chatters;
                result.succeeded = true;
                return result;
            }
            Err(HelixError::NotModerator) => {
                // Self-Heal + genau EIN Retry bei Erfolg.
                let healed = attempt_self_heal(streamer, provisioner, cooldowns, stats).await;
                if healed {
                    match fetcher
                        .fetch_chatters(&streamer.twitch_user_id, mod_id, token)
                        .await
                    {
                        Ok(chatters) => {
                            stats.bot_path_success += 1;
                            result.chatters = chatters;
                            result.succeeded = true;
                            return result;
                        }
                        Err(err) => {
                            stats.bot_path_failure += 1;
                            tracing::warn!(
                                channel = %streamer.streamer_login,
                                error = %err,
                                "chatters: Bot-Retry nach Self-Heal fehlgeschlagen"
                            );
                        }
                    }
                } else {
                    stats.bot_path_failure += 1;
                }
            }
            Err(err) => {
                stats.bot_path_failure += 1;
                tracing::warn!(
                    channel = %streamer.streamer_login,
                    error = %err,
                    "chatters: Bot-Pfad fehlgeschlagen"
                );
            }
        }
    }

    // 2) Streamer-OAuth-Fallback (nur wenn Bot-Pfad nicht erfolgreich + leer).
    if !result.succeeded && result.chatters.is_empty() {
        if let Some(token) = streamer_token {
            stats.fallback_to_streamer_token += 1;
            match fetcher
                .fetch_chatters(&streamer.twitch_user_id, &streamer.twitch_user_id, token)
                .await
            {
                Ok(chatters) => {
                    result.chatters = chatters;
                    result.succeeded = true;
                }
                Err(err) => {
                    tracing::warn!(
                        channel = %streamer.streamer_login,
                        error = %err,
                        "chatters: Streamer-Token-Fallback fehlgeschlagen"
                    );
                }
            }
        }
    }

    result
}

/// Mod-Self-Heal (Python `_attempt_bot_moderator_self_heal`, P2.61).
/// Gatet durch Cooldown + Partner-Status. Liefert `true` bei erfolgreichem
/// Re-Modding (Trigger für den genau-einen Bot-Retry).
async fn attempt_self_heal(
    streamer: &LiveStreamer,
    provisioner: &dyn ModeratorProvisioner,
    cooldowns: &SelfHealCooldowns,
    stats: &mut CycleStats,
) -> bool {
    let key = self_heal_key(&streamer.streamer_login);

    if cooldowns.is_cooling(&key).await {
        return false;
    }
    if !streamer.is_partner_active {
        return false;
    }

    let healed = provisioner
        .ensure_bot_is_mod(&streamer.twitch_user_id, &streamer.streamer_login)
        .await;
    if healed {
        cooldowns.clear(&key).await;
        stats.self_heal_success += 1;
    } else {
        cooldowns.set_cooldown(&key).await;
        stats.self_heal_failure += 1;
    }
    healed
}

// ---------------------------------------------------------------------------
// Batch-Write pro Streamer
// ---------------------------------------------------------------------------

/// Schreibt die Chatter eines Streamers in alle drei Tabellen (Reihenfolge
/// zwingend, Python `_persist_chatters`). Bot- und Self-Logins werden vorher
/// gefiltert. Jeder Chatter ist ein `(login, user_id)`-Paar; eine fehlende
/// `user_id` (`None`) wird als `NULL` gebunden. Liefert
/// `(geschriebene_chatter, neue_lurker)`.
pub async fn record_chatters_for_streamer(
    pool: &PgPool,
    streamer: &LiveStreamer,
    raw_chatters: &[(String, Option<String>)],
    bot_login: Option<&str>,
    tick_at: DateTime<Utc>,
) -> Result<(u64, u64), sqlx::Error> {
    let viewers = filter_viewers(raw_chatters, bot_login);
    if viewers.is_empty() {
        return Ok((0, 0));
    }
    let logins: Vec<String> = viewers.iter().map(|(login, _)| login.clone()).collect();

    // 1) Pre-Read: wer ist im Rollup des Streamers bereits bekannt?
    let seen_before: HashSet<String> = sqlx::query_scalar::<_, String>(
        "SELECT chatter_login FROM twitch_chatter_rollup \
         WHERE streamer_login = $1 AND chatter_login = ANY($2)",
    )
    .bind(&streamer.streamer_login)
    .bind(&logins)
    .fetch_all(pool)
    .await?
    .into_iter()
    .collect();

    let mut tx = pool.begin().await?;
    let mut new_lurkers = 0u64;

    for (login, chatter_id) in &viewers {
        let is_first_time = !seen_before.contains(login);
        if is_first_time {
            new_lurkers += 1;
        }

        // 2) session_chatters — Conflict aktualisiert NUR last_seen_at
        //    (chatter_id wie Python NICHT überschrieben).
        sqlx::query(
            "INSERT INTO twitch_session_chatters \
             (session_id, streamer_login, chatter_login, chatter_id, first_message_at, \
              messages, is_first_time_streamer, seen_via_chatters_api, last_seen_at) \
             VALUES ($1, $2, $3, $4, $5, 0, $6, TRUE, $5) \
             ON CONFLICT (session_id, chatter_login) \
             DO UPDATE SET last_seen_at = EXCLUDED.last_seen_at",
        )
        .bind(streamer.active_session_id)
        .bind(&streamer.streamer_login)
        .bind(login)
        .bind(chatter_id)
        .bind(tick_at)
        .bind(is_first_time)
        .execute(&mut *tx)
        .await?;

        // 3) rollup — total_messages/total_sessions NIE inkrementieren;
        //    chatter_id per COALESCE nachtragen (bestehende ID gewinnt, Python).
        sqlx::query(
            "INSERT INTO twitch_chatter_rollup \
             (streamer_login, chatter_login, chatter_id, first_seen_at, last_seen_at, \
              total_messages, total_sessions) \
             VALUES ($1, $2, $3, $4, $4, 0, 1) \
             ON CONFLICT (streamer_login, chatter_login) DO UPDATE SET \
               last_seen_at = EXCLUDED.last_seen_at, \
               chatter_id = COALESCE(twitch_chatter_rollup.chatter_id, EXCLUDED.chatter_id)",
        )
        .bind(&streamer.streamer_login)
        .bind(login)
        .bind(chatter_id)
        .bind(tick_at)
        .execute(&mut *tx)
        .await?;
    }

    tx.commit().await?;

    // 4) presence_ticks (fertige, idempotente fn aus irc_lurker.rs).
    //    presence_ticks hat keine chatter_id-Spalte → nur die Logins.
    record_presence_ticks(
        pool,
        streamer.active_session_id,
        &streamer.streamer_login,
        &logins,
        tick_at,
    )
    .await;

    Ok((viewers.len() as u64, new_lurkers))
}

/// Filtert Bot- und Self-Logins, normalisiert Logins + dedupliziert (erstes
/// Vorkommen gewinnt). Leere `user_id` (`""`) wird zu `None`.
fn filter_viewers(
    raw: &[(String, Option<String>)],
    bot_login: Option<&str>,
) -> Vec<(String, Option<String>)> {
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for (login, user_id) in raw {
        let login = normalize_login(login);
        if login.is_empty() {
            continue;
        }
        if is_known_chat_bot(&login) {
            continue;
        }
        if bot_login.is_some_and(|b| b == login) {
            continue;
        }
        if seen.insert(login.clone()) {
            let user_id = user_id
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string);
            out.push((login, user_id));
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Collect-Zyklus (Roster + Concurrency + Writes)
// ---------------------------------------------------------------------------

/// Bündelt die injizierten Abhängigkeiten eines Collect-Zyklus.
pub struct ChattersCollector {
    pub pool: PgPool,
    pub auth: Arc<dyn BotChatterAuth>,
    pub streamer_tokens: Arc<dyn StreamerTokenSource>,
    pub fetcher: Arc<dyn ChattersFetcher>,
    pub provisioner: Arc<dyn ModeratorProvisioner>,
    pub cooldowns: SelfHealCooldowns,
}

impl ChattersCollector {
    /// Führt EINEN Collect-Zyklus aus: Roster laden, alle Streamer nebenläufig
    /// pollen, Ergebnisse sammeln, dann sequenziell schreiben. Fehler werden
    /// geloggt; der Loop läuft weiter. Liefert die Zyklus-Stats.
    pub async fn run_cycle(&self) -> CycleStats {
        let mut stats = CycleStats::default();

        let roster = match load_live_roster(&self.pool).await {
            Ok(r) => r,
            Err(err) => {
                tracing::error!(error = %err, "chatters: Roster-Query fehlgeschlagen");
                return stats;
            }
        };
        stats.live_streamers = roster.len();
        if roster.is_empty() {
            return stats;
        }

        // Gemeinsamer Sekunden-Timestamp pro Zyklus → idempotent gg. Doppellauf.
        let tick_at = Utc::now().trunc_subsecs(0);

        // Bot-Kontext einmal pro Zyklus auflösen.
        let bot_token = self.auth.bot_token().await;
        let bot_user_id = self.auth.bot_user_id().await;
        let bot_login = self.auth.bot_login().await.map(|l| normalize_login(&l));
        let has_scope = self.auth.has_chatters_scope().await;
        let bot_usable = bot_token.is_some() && bot_user_id.is_some() && has_scope;

        // Streamer sequenziell pollen + schreiben. Self-Heal-Cooldown und
        // ChattersFetcher hängen an `&self` (kein `'static`), und ohne
        // `futures`-Crate (keine neue Dep erlaubt) wäre echte buffer_unordered-
        // Nebenläufigkeit nicht idiomatisch baubar; bei den real wenigen live
        // Streamern pro 30s-Tick ist die Reihenfolge unkritisch. Der gemeinsame
        // `tick_at` hält das Resultat idempotent. (Abweichung von Spec §4.3.)
        for streamer in &roster {
            let streamer_token = self
                .streamer_tokens
                .streamer_token(&streamer.twitch_user_id)
                .await;

            let result = poll_streamer_once(
                streamer,
                if bot_usable { bot_token.as_deref() } else { None },
                if bot_usable { bot_user_id.as_deref() } else { None },
                streamer_token.as_deref(),
                self.fetcher.as_ref(),
                self.provisioner.as_ref(),
                &self.cooldowns,
                &mut stats,
            )
            .await;

            if !result.succeeded {
                continue;
            }
            match record_chatters_for_streamer(
                &self.pool,
                streamer,
                &result.chatters,
                bot_login.as_deref(),
                tick_at,
            )
            .await
            {
                Ok((written, lurkers)) => {
                    stats.chatters_written += written;
                    stats.lurkers_new += lurkers;
                }
                Err(err) => tracing::error!(
                    channel = %streamer.streamer_login,
                    error = %err,
                    "chatters: Write-Transaktion fehlgeschlagen"
                ),
            }
        }

        stats
    }
}

// ---------------------------------------------------------------------------
// Helfer
// ---------------------------------------------------------------------------

/// `lower().trim()` — die kanonische Login-Normalisierung dieses Subsystems.
fn normalize_login(login: &str) -> String {
    login.trim().to_lowercase()
}

/// Self-Heal-Key: zusätzlich führendes `#` strippen (Footgun #1).
fn self_heal_key(login: &str) -> String {
    normalize_login(login).trim_start_matches('#').to_string()
}

/// Known-Chat-Bot-Check über die geteilte `WHITELISTED_BOTS`-Liste (tb-chat).
/// `login` muss bereits normalisiert sein.
fn is_known_chat_bot(login: &str) -> bool {
    WHITELISTED_BOTS.contains(&login)
}

/// Test-Seam um die private [`poll_streamer_once`]: liefert `(succeeded, chatters)`
/// und macht die Token-Reihenfolge/Self-Heal-Logik ohne Netz prüfbar.
#[allow(clippy::too_many_arguments)]
pub async fn poll_streamer_once_for_test(
    streamer: &LiveStreamer,
    bot_token: Option<&str>,
    bot_user_id: Option<&str>,
    streamer_token: Option<&str>,
    fetcher: &dyn ChattersFetcher,
    provisioner: &dyn ModeratorProvisioner,
    cooldowns: &SelfHealCooldowns,
    stats: &mut CycleStats,
) -> (bool, Vec<(String, Option<String>)>) {
    let result = poll_streamer_once(
        streamer,
        bot_token,
        bot_user_id,
        streamer_token,
        fetcher,
        provisioner,
        cooldowns,
        stats,
    )
    .await;
    (result.succeeded, result.chatters)
}
