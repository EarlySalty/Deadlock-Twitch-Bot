use std::collections::HashMap;

use sqlx::{FromRow, PgPool};
use tb_domain::network::{GuildMembership, GuildSettings, StreamerNetwork, COMMUNITY_GUILD_ID};

#[derive(FromRow)]
struct GuildRow {
    guild_id: i64,
    language: String,
    cross_server_raids: bool,
    promo_invite_url: Option<String>,
    installed: bool,
}

impl From<GuildRow> for GuildSettings {
    fn from(row: GuildRow) -> Self {
        Self {
            guild_id: row.guild_id,
            language: row.language,
            cross_server_raids: row.cross_server_raids,
            promo_invite_url: row.promo_invite_url,
            installed: row.installed,
        }
    }
}

#[derive(FromRow)]
struct MembershipRow {
    twitch_user_id: String,
    broadcaster_language: Option<String>,
    guild_id: Option<i64>,
    active: Option<bool>,
    language: Option<String>,
    cross_server_raids: Option<bool>,
    promo_invite_url: Option<String>,
    installed: Option<bool>,
}

impl MembershipRow {
    fn membership(&self) -> Option<GuildMembership> {
        Some(GuildMembership {
            guild: GuildSettings {
                guild_id: self.guild_id?,
                language: self.language.clone()?,
                cross_server_raids: self.cross_server_raids?,
                promo_invite_url: self.promo_invite_url.clone(),
                installed: self.installed?,
            },
            active: self.active?,
        })
    }
}

fn uninstalled_community() -> GuildSettings {
    GuildSettings {
        installed: false,
        ..GuildSettings::community_default()
    }
}

fn unavailable_network() -> StreamerNetwork {
    StreamerNetwork::community(&uninstalled_community())
}

pub async fn community_settings(pool: &PgPool) -> Result<GuildSettings, sqlx::Error> {
    let row = sqlx::query_as::<_, GuildRow>(
        "SELECT g.guild_id, g.language, g.cross_server_raids, g.promo_invite_url, \
                (g.status = 'active' AND g.removed_at IS NULL) AS installed \
           FROM discord_guilds g \
          WHERE g.guild_id = $1",
    )
    .bind(COMMUNITY_GUILD_ID)
    .fetch_optional(pool)
    .await?;
    Ok(row
        .map(GuildSettings::from)
        .unwrap_or_else(uninstalled_community))
}

pub async fn load_networks(
    pool: &PgPool,
    twitch_user_ids: &[String],
) -> Result<HashMap<String, StreamerNetwork>, sqlx::Error> {
    let ids: Vec<String> = twitch_user_ids
        .iter()
        .map(|id| id.trim().to_string())
        .filter(|id| !id.is_empty())
        .collect();
    if ids.is_empty() {
        return Ok(HashMap::new());
    }
    let community = community_settings(pool).await?;
    let rows = sqlx::query_as::<_, MembershipRow>(
        "WITH ids AS ( \
             SELECT DISTINCT id AS twitch_user_id FROM UNNEST($1::text[]) AS u(id) \
         ) \
         SELECT ids.twitch_user_id, \
                i.broadcaster_language, \
                sg.guild_id, \
                sg.active, \
                g.language, \
                g.cross_server_raids, \
                g.promo_invite_url, \
                (g.status = 'active' AND g.removed_at IS NULL) AS installed \
           FROM ids \
           LEFT JOIN twitch_streamer_identities i ON i.twitch_user_id = ids.twitch_user_id \
           LEFT JOIN streamer_guilds sg ON sg.twitch_user_id = ids.twitch_user_id \
           LEFT JOIN discord_guilds g ON g.guild_id = sg.guild_id \
          ORDER BY ids.twitch_user_id, sg.joined_at, sg.guild_id",
    )
    .bind(&ids)
    .fetch_all(pool)
    .await?;

    let mut grouped: HashMap<String, (Vec<GuildMembership>, Option<String>)> = HashMap::new();
    for row in rows {
        let membership = row.membership();
        let entry = grouped
            .entry(row.twitch_user_id.clone())
            .or_insert_with(|| (Vec::new(), None));
        if entry.1.is_none() {
            entry.1 = row.broadcaster_language;
        }
        if let Some(membership) = membership {
            entry.0.push(membership);
        }
    }
    Ok(grouped
        .into_iter()
        .map(|(id, (memberships, language))| {
            let memberships = if memberships.is_empty() {
                vec![GuildMembership {
                    guild: community.clone(),
                    active: false,
                }]
            } else {
                memberships
            };
            (id, StreamerNetwork::new(memberships, language, &community))
        })
        .collect())
}

pub async fn load_network(
    pool: &PgPool,
    twitch_user_id: &str,
) -> Result<StreamerNetwork, sqlx::Error> {
    let id = twitch_user_id.trim().to_string();
    let mut networks = load_networks(pool, std::slice::from_ref(&id)).await?;
    match networks.remove(&id) {
        Some(network) => Ok(network),
        None => Ok(unavailable_network()),
    }
}

pub async fn twitch_user_id_for_login(
    pool: &PgPool,
    login: &str,
) -> Result<Option<String>, sqlx::Error> {
    let login = login.trim().to_lowercase();
    if login.is_empty() {
        return Ok(None);
    }
    sqlx::query_scalar::<_, String>(
        "SELECT twitch_user_id FROM twitch_streamer_identities \
          WHERE LOWER(twitch_login) = $1 \
          ORDER BY twitch_user_id \
          LIMIT 1",
    )
    .bind(&login)
    .fetch_optional(pool)
    .await
}

pub async fn load_network_by_login(
    pool: &PgPool,
    login: &str,
) -> Result<StreamerNetwork, sqlx::Error> {
    match twitch_user_id_for_login(pool, login).await? {
        Some(id) => load_network(pool, &id).await,
        None => Ok(unavailable_network()),
    }
}

pub async fn record_broadcaster_languages(
    pool: &PgPool,
    observed: &[(String, String)],
) -> Result<u64, sqlx::Error> {
    let mut latest: HashMap<String, String> = HashMap::new();
    for (id, language) in observed {
        let id = id.trim();
        let language = language.trim().to_lowercase();
        if id.is_empty() || language.is_empty() {
            continue;
        }
        latest.insert(id.to_string(), language);
    }
    if latest.is_empty() {
        return Ok(0);
    }
    let (ids, languages): (Vec<String>, Vec<String>) = latest.into_iter().unzip();
    let result = sqlx::query(
        "UPDATE twitch_streamer_identities i \
            SET broadcaster_language = x.language, \
                broadcaster_language_updated_at = now() \
           FROM UNNEST($1::text[], $2::text[]) AS x(twitch_user_id, language) \
          WHERE i.twitch_user_id = x.twitch_user_id \
            AND i.broadcaster_language IS DISTINCT FROM x.language",
    )
    .bind(&ids)
    .bind(&languages)
    .execute(pool)
    .await?;
    Ok(result.rows_affected())
}
