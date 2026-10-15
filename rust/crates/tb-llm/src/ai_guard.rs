//! KI-Schalter pro Discord-Server (Multi-Server-Plan, Paket K, E14).
//!
//! Zentraler Guard vor jedem Sprachmodell-Aufruf. Er sitzt im einzigen
//! Eingang ([`crate::complete`]) und nicht bei den Aufrufern: Aufrufer nennen
//! nur den Streamer, auf den sich ein Aufruf bezieht
//! ([`crate::Request::twitch_broadcaster`] oder
//! [`crate::Request::twitch_channel`]); ob die KI für ihn laufen darf,
//! entscheidet allein dieser Guard. Die Regel selbst liegt einmal in der
//! DB-Funktion `twitch_streamer_ai_allowed` (Migration `20261012140000`), die
//! auch globale Sammeljobs zum Filtern ihrer Eingangsdaten nutzen.
//!
//! - Mit Streamerbezug: Server-Zugehörigkeit aus `streamer_guilds` und
//!   `discord_guilds.ai_features` laden. Aus heißt: kein Aufruf, Fehler
//!   [`LlmError::Disabled`]. Die Twitch-User-ID landet im Ledger
//!   (`llm_usage.twitch_user_id`), damit sich jeder Aufruf einem Streamer
//!   zuordnen lässt.
//! - Ohne Streamerbezug, aber mit einem Zweck aus [`STREAMER_PURPOSES`]: Das
//!   ist ein Aufrufer, der den Streamer nicht genannt hat. Solange kein
//!   installierter Server die KI aus hat, gibt es nichts zu schützen und der
//!   Aufruf läuft (heutiger Betrieb). Sobald einer sie aus hat, wird der
//!   Aufruf abgewiesen und gewarnt: lieber ein fehlender Aufruf als KI für
//!   einen fremden Server.
//! - Datenbankfehler sperren (geschlossen). Nur eine Datenbank ohne das
//!   Multi-Server-Schema gilt als „nur der deutsche Server“.
//! - Ohne Ledger-Pool (lokale Mock-Aufrufe in Tests) prüft der Guard nichts;
//!   ein produktiver Aufruf scheitert dann ohnehin am Ledger.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use sqlx::postgres::PgPool;

use crate::hub::{LlmError, Request};

/// Wie lange eine Entscheidung gilt. Ein umgelegter Schalter wirkt spätestens
/// nach dieser Zeit; der Chat fragt so nicht bei jeder Nachricht die DB.
const CACHE_TTL: Duration = Duration::from_secs(60);
/// Obergrenze des Caches; darüber wird er geleert.
const CACHE_MAX: usize = 10_000;

/// Zwecke (Anwendungsfall oder Ledger-Zweck), die sich immer auf genau einen
/// Streamer beziehen. Fehlt bei ihnen der Streamer, greift die Sicherung aus
/// der Modulbeschreibung.
pub const STREAMER_PURPOSES: &[&str] = &[
    // Twitch-Chat (E14): Werbung, Fragen, Spam- und Scam-Prüfung, Smalltalk.
    "promo_pitch",
    "lfg_pitch",
    "invite_question",
    "spam-review",
    "conversation_scam_judge",
    "conversation_scam_followup",
    "engagement",
    "channel_profile",
    "outreach_shadow",
    // Dashboard und Auswertungen eines Kanals.
    "title",
    "title-insight",
    "ai-analysis",
    "dashboard_ai_analysis",
    "ai-chat",
    "dashboard_ai_chat",
    "dashboard_chat_deep_analysis",
    "dashboard-assistent",
    "post-stream-report",
    "post-stream-report-claude",
    "stream_audit",
];

/// Ob der Zweck immer einen Streamerbezug hat. `purpose` ist der Zweck, unter
/// dem der Aufruf verbucht wird (Ledger-Zweck, sonst Anwendungsfall).
pub fn is_streamer_purpose(purpose: &str) -> bool {
    STREAMER_PURPOSES.contains(&purpose)
}

/// Ergebnis der Prüfung für einen Aufruf.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Freigabe {
    /// Twitch-User-ID für das Ledger, sofern bekannt.
    pub twitch_user_id: Option<String>,
}

/// Prüft einen Aufruf. `Err` heißt: nicht senden.
pub(crate) async fn check(
    use_case: &str,
    purpose: &str,
    request: &Request,
) -> Result<Freigabe, LlmError> {
    let id = clean(request.twitch_broadcaster.as_deref());
    let login = clean(request.twitch_channel.as_deref()).map(str::to_ascii_lowercase);
    let Some(pool) = crate::ledger::pool().await else {
        return Ok(Freigabe {
            twitch_user_id: id.map(str::to_string),
        });
    };

    if id.is_none() && login.is_none() {
        if is_streamer_purpose(purpose) && !all_guilds_ai_on(pool).await? {
            tb_observability::warning_budget::warn(
                "ai_guard_ohne_streamer",
                "KI-Aufruf ohne Streamerbezug abgewiesen, weil ein Server die KI aus hat",
            );
            tracing::warn!(
                use_case,
                purpose,
                "KI_GUARD: Aufruf ohne Streamerbezug abgewiesen"
            );
            return Err(LlmError::Disabled(format!(
                "{use_case}: Streamerbezug fehlt, KI-Schalter nicht prüfbar"
            )));
        }
        return Ok(Freigabe {
            twitch_user_id: None,
        });
    }

    let (twitch_user_id, erlaubt) = streamer_verdict(pool, id, login.as_deref()).await?;
    if !erlaubt {
        tracing::debug!(
            use_case,
            purpose,
            twitch_user_id = twitch_user_id.as_deref(),
            "KI_GUARD: KI für den Server des Streamers ausgeschaltet"
        );
        return Err(LlmError::Disabled(
            "KI-Funktionen sind für den Server dieses Streamers ausgeschaltet".into(),
        ));
    }
    Ok(Freigabe { twitch_user_id })
}

/// Ob die KI für diesen Streamer laufen darf, ohne einen Aufruf zu machen.
/// Für Aufrufer, die vor teurer Vorarbeit (Prompt bauen, Daten laden)
/// abbrechen oder Dashboard-Seiten ausblenden wollen. Die Durchsetzung bleibt
/// trotzdem im Eingang. `twitch_user_id` hat Vorrang vor `channel_login`.
pub async fn streamer_ai_allowed(
    twitch_user_id: Option<&str>,
    channel_login: Option<&str>,
) -> Result<bool, LlmError> {
    let id = clean(twitch_user_id);
    let login = clean(channel_login).map(str::to_ascii_lowercase);
    if id.is_none() && login.is_none() {
        return Ok(true);
    }
    let Some(pool) = crate::ledger::pool().await else {
        return Ok(true);
    };
    Ok(streamer_verdict(pool, id, login.as_deref()).await?.1)
}

/// Dieselbe Entscheidung für Eingänge mit eigenem Pool, die nicht über
/// [`crate::complete`] laufen (Social-AI-Editor).
pub(crate) async fn twitch_user_ai_allowed(
    pool: &PgPool,
    twitch_user_id: &str,
) -> Result<bool, LlmError> {
    match clean(Some(twitch_user_id)) {
        Some(id) => Ok(streamer_verdict(pool, Some(id), None).await?.1),
        None => Ok(false),
    }
}

fn clean(value: Option<&str>) -> Option<&str> {
    value.map(str::trim).filter(|value| !value.is_empty())
}

#[derive(Clone)]
struct Eintrag {
    zeit: Instant,
    twitch_user_id: Option<String>,
    erlaubt: bool,
}

fn cache() -> &'static Mutex<HashMap<String, Eintrag>> {
    static CACHE: OnceLock<Mutex<HashMap<String, Eintrag>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

async fn streamer_verdict(
    pool: &PgPool,
    id: Option<&str>,
    login: Option<&str>,
) -> Result<(Option<String>, bool), LlmError> {
    let key = match (id, login) {
        (Some(id), _) => format!("id:{id}"),
        (None, Some(login)) => format!("login:{login}"),
        (None, None) => unreachable!("Aufrufer prüft auf Streamerbezug"),
    };
    let jetzt = Instant::now();
    if let Some(eintrag) = cache()
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .get(&key)
        .filter(|e| jetzt.duration_since(e.zeit) < CACHE_TTL)
        .cloned()
    {
        return Ok((eintrag.twitch_user_id, eintrag.erlaubt));
    }

    let (twitch_user_id, erlaubt) = match load(pool, id, login).await {
        Ok((twitch_user_id, erlaubt)) => {
            (twitch_user_id.or_else(|| id.map(str::to_string)), erlaubt)
        }
        // Ohne Multi-Server-Schema gibt es nur den deutschen Server.
        Err(error) if fehlendes_schema(&error) => (id.map(str::to_string), true),
        Err(error) => {
            tb_observability::warning_budget::warn(
                "ai_guard_db",
                "KI-Schalter nicht lesbar; KI-Aufruf gesperrt",
            );
            tracing::debug!(%error, "KI_GUARD: Abfrage fehlgeschlagen");
            return Err(LlmError::Unavailable(
                "KI-Schalter nicht lesbar; KI-Aufruf gesperrt".into(),
            ));
        }
    };

    let mut cache = cache().lock().unwrap_or_else(|p| p.into_inner());
    if cache.len() >= CACHE_MAX {
        cache.clear();
    }
    cache.insert(
        key,
        Eintrag {
            zeit: jetzt,
            twitch_user_id: twitch_user_id.clone(),
            erlaubt,
        },
    );
    Ok((twitch_user_id, erlaubt))
}

/// Twitch-User-ID (für das Ledger) und Entscheidung der DB-Funktion
/// `twitch_streamer_ai_allowed` (Migration `20261012140000`).
async fn load(
    pool: &PgPool,
    id: Option<&str>,
    login: Option<&str>,
) -> Result<(Option<String>, bool), sqlx::Error> {
    sqlx::query_as::<_, (Option<String>, bool)>(
        "SELECT COALESCE( \
                    $1::text, \
                    (SELECT i.twitch_user_id FROM twitch_streamer_identities i \
                      WHERE LOWER(i.twitch_login) = $2 \
                      ORDER BY i.twitch_user_id LIMIT 1)), \
                twitch_streamer_ai_allowed($1, $2)",
    )
    .bind(id)
    .bind(login)
    .fetch_one(pool)
    .await
}

/// `true`, wenn kein installierter Server die KI aus hat.
async fn all_guilds_ai_on(pool: &PgPool) -> Result<bool, LlmError> {
    static STAND: OnceLock<Mutex<Option<(Instant, bool)>>> = OnceLock::new();
    let stand = STAND.get_or_init(|| Mutex::new(None));
    let jetzt = Instant::now();
    if let Some((zeit, alle_an)) = *stand.lock().unwrap_or_else(|p| p.into_inner()) {
        if jetzt.duration_since(zeit) < CACHE_TTL {
            return Ok(alle_an);
        }
    }
    let alle_an = match sqlx::query_scalar::<_, bool>(
        "SELECT NOT EXISTS ( \
             SELECT 1 FROM discord_guilds \
              WHERE NOT ai_features AND status = 'active' AND removed_at IS NULL \
         )",
    )
    .fetch_one(pool)
    .await
    {
        Ok(alle_an) => alle_an,
        Err(error) if fehlendes_schema(&error) => true,
        Err(error) => {
            tb_observability::warning_budget::warn(
                "ai_guard_db",
                "KI-Schalter nicht lesbar; KI-Aufruf gesperrt",
            );
            tracing::debug!(%error, "KI_GUARD: Serverabfrage fehlgeschlagen");
            return Err(LlmError::Unavailable(
                "KI-Schalter nicht lesbar; KI-Aufruf gesperrt".into(),
            ));
        }
    };
    *stand.lock().unwrap_or_else(|p| p.into_inner()) = Some((jetzt, alle_an));
    Ok(alle_an)
}

/// Postgres `undefined_table` oder `undefined_function`: Migration noch nicht
/// angewandt, es gibt nur den deutschen Server.
fn fehlendes_schema(error: &sqlx::Error) -> bool {
    error
        .as_database_error()
        .and_then(|db| db.code())
        .is_some_and(|code| code == "42P01" || code == "42883")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn streamer_zwecke() {
        assert!(is_streamer_purpose("promo_pitch"));
        assert!(is_streamer_purpose("lfg_pitch"));
        assert!(is_streamer_purpose("spam-review"));
        assert!(!is_streamer_purpose("social-media-fireworks"));
        assert!(!is_streamer_purpose("conversation_scam_guidance"));
        assert!(!is_streamer_purpose("community_sentiment"));
    }

    #[tokio::test]
    async fn ohne_ledger_pool_prueft_der_guard_nichts() {
        let request = Request::prompt("x").twitch_channel("Kanal");
        assert_eq!(
            check("promo_pitch", "promo_pitch", &request).await,
            Ok(Freigabe {
                twitch_user_id: None
            })
        );
        let request = Request::prompt("x").twitch_broadcaster("42");
        assert_eq!(
            check("engagement", "engagement", &request)
                .await
                .map(|f| f.twitch_user_id),
            Ok(Some("42".to_string()))
        );
    }
}
