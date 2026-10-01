use sqlx::PgPool;

use tb_analytics::bekannte_bots::KNOWN_CHAT_BOTS;

pub(crate) fn push_normalized_login(logins: &mut Vec<String>, raw: &str) {
    for part in raw.split(|c: char| c == ',' || c == ';' || c.is_whitespace()) {
        let login = part.trim().trim_start_matches('@').to_lowercase();
        if !login.is_empty() && !logins.contains(&login) {
            logins.push(login);
        }
    }
}

fn dynamic_bot_logins_from_env() -> Vec<String> {
    let mut logins = Vec::new();
    if let Ok(config) = tb_config::runtime::settings() {
        for login in &config.dashboard.options.excluded_bot_logins {
            push_normalized_login(&mut logins, login);
        }
    }
    logins
}

fn dynamic_bot_user_ids_from_env() -> Vec<String> {
    let mut ids = Vec::new();
    if let Ok(config) = tb_config::runtime::settings() {
        ids.push(config.twitch.bot_user_id.clone());
        for value in &config.dashboard.options.additional_bot_user_ids {
            for part in value.split(|c: char| c == ',' || c == ';' || c.is_whitespace()) {
                let id = part.trim().to_string();
                if !id.is_empty() && !ids.contains(&id) {
                    ids.push(id);
                }
            }
        }
    }
    ids
}

async fn dynamic_bot_logins_from_db(pool: &PgPool) -> Vec<String> {
    let user_ids = dynamic_bot_user_ids_from_env();
    if user_ids.is_empty() {
        return Vec::new();
    }
    match sqlx::query_scalar!(
        r#"SELECT DISTINCT LOWER(TRIM(login)) AS "login!"
           FROM (
               SELECT twitch_login AS login FROM twitch_streamers WHERE twitch_user_id = ANY($1)
               UNION
               SELECT twitch_login AS login FROM twitch_streamer_identities WHERE twitch_user_id = ANY($1)
               UNION
               SELECT twitch_login AS login FROM twitch_user_profile WHERE twitch_user_id = ANY($1)
           ) resolved
           WHERE TRIM(COALESCE(login, '')) <> ''"#,
        &user_ids
    )
    .fetch_all(pool)
    .await
    {
        Ok(rows) => rows,
        Err(e) => {
            tracing::debug!(error = %e, "viewer dynamic bot-login DB-Resolve fehlgeschlagen");
            Vec::new()
        }
    }
}

pub(crate) fn viewer_exclusion_logins_from_dynamic(
    streamer: &str,
    dynamic_logins: &[String],
) -> Vec<String> {
    let mut logins: Vec<String> = KNOWN_CHAT_BOTS.iter().map(|s| s.to_string()).collect();
    for login in dynamic_bot_logins_from_env() {
        push_normalized_login(&mut logins, &login);
    }
    for login in dynamic_logins {
        push_normalized_login(&mut logins, login);
    }
    let own = streamer.to_lowercase();
    if !own.is_empty() && !logins.contains(&own) {
        logins.push(own);
    }
    logins
}

pub(crate) async fn viewer_exclusion_logins(pool: &PgPool, streamer: &str) -> Vec<String> {
    let dynamic_logins = dynamic_bot_logins_from_db(pool).await;
    viewer_exclusion_logins_from_dynamic(streamer, &dynamic_logins)
}

pub(crate) fn is_known_or_dynamic_excluded(login: &str, excluded_logins: &[String]) -> bool {
    let login = login.trim().to_lowercase();
    !login.is_empty() && excluded_logins.iter().any(|excluded| excluded == &login)
}
