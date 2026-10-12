//! Werbung nur für den Heimat-Server (Multi-Server-Plan, Paket R, Abschnitt 4.6).
//!
//! Jede Discord-Einladung, die der Bot im Twitch-Chat eines Streamers zeigt,
//! gehört zu dessen Heimat-Server:
//!
//! 1. Persönliche Einladung aus `twitch_streamer_invites`, wenn sie zum
//!    Heimat-Server passt. Auf dem deutschen Server gilt wie bisher jede
//!    (unveränderte Abfrage), auf anderen Servern nur eine mit deren
//!    `guild_id`.
//! 2. Sonst der Rückfall des Heimat-Servers. Für andere Server ist das allein
//!    `discord_guilds.promo_invite_url`; eine globale Ersatz-Einladung gibt es
//!    für sie nicht. Für den deutschen Server bleiben die bisherigen Ketten
//!    unverändert (siehe [`InvitePurpose`]).
//!
//! Kann das Netzwerk nicht geladen werden, gilt der deutsche Server, also das
//! bisherige Verhalten.

use sqlx::PgPool;
use tb_domain::network::GuildSettings;

use crate::promos::DEFAULT_PROMO_DISCORD_INVITE;

/// Wofür eine Einladung gebraucht wird. Bestimmt nur den Rückfall des
/// deutschen Servers, der je Aufrufer bewusst verschieden ist.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InvitePurpose {
    /// Wiederkehrende Chat-Werbung: deutscher Server endet bei Konfiguration,
    /// Server-Einladung, fester Community-Einladung.
    Promo,
    /// Antworten (Invite-Frage, LFG-Pitch, `!invite`): deutscher Server endet
    /// bei der Konfiguration, sonst keine Einladung.
    Reply,
    /// Kanal-Link (`!dldc`): deutscher Server nur mit persönlicher Einladung.
    ChannelLink,
}

/// Schlüssel, mit dem der Aufrufer den Streamer kennt. Die Aufrufer suchen die
/// persönliche Einladung bisher teils per Login, teils per User-ID; das bleibt so.
#[derive(Clone, Copy, Debug)]
pub enum StreamerKey<'a> {
    Login(&'a str),
    UserId(&'a str),
}

/// Gefundene Einladung.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResolvedInvite {
    pub url: String,
    /// `true` für die persönliche Einladung des Streamers.
    pub personal: bool,
}

/// Rückfall des Heimat-Servers, wenn keine persönliche Einladung passt.
/// `community_override` ist `discord.chat.promo_invite` aus der Konfiguration;
/// er gilt nur für den deutschen Server.
pub fn server_invite(
    home: &GuildSettings,
    purpose: InvitePurpose,
    community_override: Option<&str>,
) -> Option<String> {
    if !home.is_community() {
        return home.promo_invite().map(str::to_string);
    }
    let configured = community_override
        .map(str::trim)
        .filter(|value| !value.is_empty());
    match purpose {
        InvitePurpose::Promo => Some(
            configured
                .or_else(|| home.promo_invite())
                .unwrap_or(DEFAULT_PROMO_DISCORD_INVITE)
                .to_string(),
        ),
        InvitePurpose::Reply => configured.map(str::to_string),
        InvitePurpose::ChannelLink => None,
    }
}

/// Löst Einladungen pro Streamer nach Heimat-Server auf.
#[derive(Clone)]
pub struct HomeInviteResolver {
    pool: PgPool,
    community_override: Option<String>,
}

impl HomeInviteResolver {
    pub fn new(pool: PgPool, community_override: Option<String>) -> Self {
        Self {
            pool,
            community_override,
        }
    }

    pub fn pool(&self) -> &PgPool {
        &self.pool
    }

    /// Heimat-Server des Streamers. `None`, wenn er keine aktive
    /// Mitgliedschaft mehr hat (dann wirbt der Bot für keinen Server). Ohne
    /// jede Zeile oder bei Ladefehlern: der deutsche Server.
    pub async fn home_guild(&self, key: StreamerKey<'_>) -> Option<GuildSettings> {
        let loaded = match key {
            StreamerKey::Login(login) => {
                tb_db::network::load_network_by_login(&self.pool, login).await
            }
            StreamerKey::UserId(id) => tb_db::network::load_network(&self.pool, id).await,
        };
        match loaded {
            Ok(network) => network.home_guild().cloned(),
            Err(error) => {
                tracing::warn!(
                    %error,
                    ?key,
                    "Heimat-Server nicht ladbar — deutscher Server wie bisher"
                );
                Some(GuildSettings::community_default())
            }
        }
    }

    /// Persönliche Einladung (`twitch_streamer_invites`), die zum
    /// Heimat-Server passt. Für den deutschen Server läuft die bisherige
    /// Abfrage ohne Server-Filter; für andere Server zählt nur eine Einladung
    /// mit deren `guild_id`. Der Schlüssel wird wie bisher unverändert gebunden.
    pub async fn fitting_personal_invite(
        &self,
        home: &GuildSettings,
        key: StreamerKey<'_>,
    ) -> Result<Option<String>, sqlx::Error> {
        let url: Option<String> = match (key, home.is_community()) {
            (StreamerKey::Login(login), true) => {
                sqlx::query_scalar(
                    "SELECT invite_url FROM twitch_streamer_invites \
                     WHERE LOWER(streamer_login) = $1 LIMIT 1",
                )
                .bind(login)
                .fetch_optional(&self.pool)
                .await?
            }
            (StreamerKey::Login(login), false) => {
                sqlx::query_scalar(
                    "SELECT invite_url FROM twitch_streamer_invites \
                     WHERE LOWER(streamer_login) = $1 AND guild_id = $2 LIMIT 1",
                )
                .bind(login)
                .bind(home.guild_id)
                .fetch_optional(&self.pool)
                .await?
            }
            (StreamerKey::UserId(id), true) => {
                sqlx::query_scalar(
                    "SELECT invite_url FROM twitch_streamer_invites \
                     WHERE twitch_user_id = $1 LIMIT 1",
                )
                .bind(id)
                .fetch_optional(&self.pool)
                .await?
            }
            (StreamerKey::UserId(id), false) => {
                sqlx::query_scalar(
                    "SELECT invite_url FROM twitch_streamer_invites \
                     WHERE twitch_user_id = $1 AND guild_id = $2 LIMIT 1",
                )
                .bind(id)
                .bind(home.guild_id)
                .fetch_optional(&self.pool)
                .await?
            }
        };
        Ok(url.filter(|url| !url.trim().is_empty()))
    }

    /// Rückfall des Heimat-Servers (siehe [`server_invite`]).
    pub fn server_invite(&self, home: &GuildSettings, purpose: InvitePurpose) -> Option<String> {
        server_invite(home, purpose, self.community_override.as_deref())
    }

    /// Vollständige Auflösung: passende persönliche Einladung, sonst der
    /// Rückfall des Heimat-Servers. `None` heißt: keine Einladung zeigen.
    pub async fn resolve(
        &self,
        key: StreamerKey<'_>,
        purpose: InvitePurpose,
    ) -> Result<Option<ResolvedInvite>, sqlx::Error> {
        let Some(home) = self.home_guild(key).await else {
            return Ok(None);
        };
        if let Some(url) = self.fitting_personal_invite(&home, key).await? {
            return Ok(Some(ResolvedInvite {
                url,
                personal: true,
            }));
        }
        Ok(self
            .server_invite(&home, purpose)
            .map(|url| ResolvedInvite {
                url,
                personal: false,
            }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tb_domain::network::COMMUNITY_GUILD_ID;

    fn community(invite: Option<&str>) -> GuildSettings {
        GuildSettings {
            promo_invite_url: invite.map(str::to_string),
            ..GuildSettings::community_default()
        }
    }

    fn foreign(invite: Option<&str>) -> GuildSettings {
        GuildSettings {
            guild_id: 42,
            language: "en".to_string(),
            cross_server_raids: true,
            promo_invite_url: invite.map(str::to_string),
            installed: true,
        }
    }

    #[test]
    fn deutsche_werbung_bleibt_konfiguration_dann_fest() {
        let home = community(Some("https://discord.gg/z5TfVHuQq2"));
        assert_eq!(
            server_invite(
                &home,
                InvitePurpose::Promo,
                Some(" https://discord.gg/conf ")
            ),
            Some("https://discord.gg/conf".to_string())
        );
        assert_eq!(
            server_invite(&home, InvitePurpose::Promo, None),
            Some("https://discord.gg/z5TfVHuQq2".to_string())
        );
        // Ohne Zeile (Standardwerte) bleibt es bei der festen Einladung.
        assert_eq!(
            server_invite(
                &GuildSettings::community_default(),
                InvitePurpose::Promo,
                Some("  ")
            ),
            Some(DEFAULT_PROMO_DISCORD_INVITE.to_string())
        );
        assert_eq!(home.guild_id, COMMUNITY_GUILD_ID);
    }

    #[test]
    fn deutsche_antworten_bleiben_ohne_konfiguration_still() {
        let home = community(Some("https://discord.gg/z5TfVHuQq2"));
        assert_eq!(server_invite(&home, InvitePurpose::Reply, None), None);
        assert_eq!(
            server_invite(&home, InvitePurpose::Reply, Some("https://discord.gg/conf")),
            Some("https://discord.gg/conf".to_string())
        );
        assert_eq!(
            server_invite(
                &home,
                InvitePurpose::ChannelLink,
                Some("https://discord.gg/conf")
            ),
            None
        );
    }

    #[test]
    fn fremder_server_wirbt_nur_mit_eigener_einladung() {
        let home = foreign(Some("https://discord.gg/english"));
        for purpose in [
            InvitePurpose::Promo,
            InvitePurpose::Reply,
            InvitePurpose::ChannelLink,
        ] {
            assert_eq!(
                server_invite(&home, purpose, Some("https://discord.gg/conf")),
                Some("https://discord.gg/english".to_string())
            );
        }
        let ohne = foreign(None);
        assert_eq!(
            server_invite(&ohne, InvitePurpose::Promo, Some("https://discord.gg/conf")),
            None
        );
    }
}

#[cfg(test)]
mod db_tests {
    use std::str::FromStr;

    use sqlx::postgres::{PgConnectOptions, PgPoolOptions};

    use super::*;

    macro_rules! pool_or_skip {
        ($schema:expr) => {{
            let Some(dsn) = std::env::var("TB_TEST_DATABASE_URL").ok() else {
                if std::env::var("TB_TEST_REQUIRE_DB").as_deref() == Ok("1") {
                    panic!("TB_TEST_REQUIRE_DB=1 gesetzt, aber TB_TEST_DATABASE_URL fehlt");
                }
                eprintln!("SKIP: TB_TEST_DATABASE_URL nicht gesetzt");
                return;
            };
            pool_in_schema(&dsn, $schema).await
        }};
    }

    async fn pool_in_schema(dsn: &str, schema: &str) -> PgPool {
        let admin = PgPoolOptions::new()
            .max_connections(1)
            .connect(dsn)
            .await
            .unwrap();
        sqlx::query(crate::test_sql::drop_schema(schema, true))
            .execute(&admin)
            .await
            .unwrap();
        sqlx::query(crate::test_sql::create_schema(schema, false))
            .execute(&admin)
            .await
            .unwrap();
        admin.close().await;
        let opts = PgConnectOptions::from_str(dsn)
            .unwrap()
            .options([("search_path", schema)]);
        PgPoolOptions::new()
            .max_connections(2)
            .connect_with(opts)
            .await
            .unwrap()
    }

    /// Ausschnitt des Prod-Schemas (Migration 20261012120000) mit einem
    /// deutschen und zwei englischen Servern.
    async fn seed(pool: &PgPool) {
        sqlx::raw_sql(
            "CREATE TABLE twitch_streamer_identities (
                twitch_user_id TEXT PRIMARY KEY, twitch_login TEXT NOT NULL,
                broadcaster_language TEXT);
             CREATE TABLE discord_guilds (
                guild_id BIGINT PRIMARY KEY, language TEXT NOT NULL,
                cross_server_raids BOOLEAN NOT NULL DEFAULT TRUE, promo_invite_url TEXT,
                status TEXT NOT NULL DEFAULT 'active', removed_at TIMESTAMPTZ);
             CREATE TABLE streamer_guilds (
                twitch_user_id TEXT NOT NULL, guild_id BIGINT NOT NULL,
                active BOOLEAN NOT NULL DEFAULT TRUE,
                joined_at TIMESTAMPTZ NOT NULL DEFAULT now());
             CREATE TABLE twitch_streamer_invites (
                streamer_login TEXT PRIMARY KEY, twitch_user_id TEXT,
                guild_id BIGINT NOT NULL, invite_url TEXT NOT NULL);
             INSERT INTO discord_guilds (guild_id, language, promo_invite_url) VALUES
                (1289721245281292288, 'de', 'https://discord.gg/z5TfVHuQq2'),
                (42, 'en', 'https://discord.gg/english'),
                (43, 'en', NULL);
             INSERT INTO twitch_streamer_identities (twitch_user_id, twitch_login) VALUES
                ('de1', 'deutsch'), ('de2', 'deutsch_ohne'),
                ('en1', 'english'), ('en2', 'english_alt'), ('en3', 'ohne_einladung');
             INSERT INTO streamer_guilds (twitch_user_id, guild_id) VALUES
                ('en1', 42), ('en2', 42), ('en3', 43);
             INSERT INTO twitch_streamer_invites VALUES
                ('deutsch', 'de1', 1289721245281292288, 'https://discord.gg/persoenlich'),
                ('english', 'en1', 42, 'https://discord.gg/eigene'),
                -- Alte Einladung des deutschen Servers eines inzwischen englischen Streamers.
                ('english_alt', 'en2', 1289721245281292288, 'https://discord.gg/deutschalt');",
        )
        .execute(pool)
        .await
        .unwrap();
    }

    async fn url(
        resolver: &HomeInviteResolver,
        key: StreamerKey<'_>,
        purpose: InvitePurpose,
    ) -> Option<(String, bool)> {
        resolver
            .resolve(key, purpose)
            .await
            .unwrap()
            .map(|invite| (invite.url, invite.personal))
    }

    #[tokio::test]
    async fn deutscher_server_behaelt_die_bisherigen_ketten() {
        let pool = pool_or_skip!("home_invite_deutsch");
        seed(&pool).await;
        let ohne_konfig = HomeInviteResolver::new(pool.clone(), None);
        let mit_konfig =
            HomeInviteResolver::new(pool.clone(), Some("https://discord.gg/konfig".to_string()));

        assert_eq!(
            url(
                &ohne_konfig,
                StreamerKey::Login("deutsch"),
                InvitePurpose::Reply
            )
            .await,
            Some(("https://discord.gg/persoenlich".to_string(), true))
        );
        assert_eq!(
            url(
                &ohne_konfig,
                StreamerKey::UserId("de1"),
                InvitePurpose::ChannelLink
            )
            .await,
            Some(("https://discord.gg/persoenlich".to_string(), true))
        );
        // Ohne persönliche Einladung: Werbung → Konfiguration bzw. feste Einladung,
        // Antworten → Konfiguration oder still, Kanal-Link → still.
        assert_eq!(
            url(
                &ohne_konfig,
                StreamerKey::Login("deutsch_ohne"),
                InvitePurpose::Promo
            )
            .await,
            Some((DEFAULT_PROMO_DISCORD_INVITE.to_string(), false))
        );
        assert_eq!(
            url(
                &mit_konfig,
                StreamerKey::Login("deutsch_ohne"),
                InvitePurpose::Promo
            )
            .await,
            Some(("https://discord.gg/konfig".to_string(), false))
        );
        assert_eq!(
            url(
                &ohne_konfig,
                StreamerKey::Login("deutsch_ohne"),
                InvitePurpose::Reply
            )
            .await,
            None
        );
        assert_eq!(
            url(
                &mit_konfig,
                StreamerKey::UserId("de2"),
                InvitePurpose::Reply
            )
            .await,
            Some(("https://discord.gg/konfig".to_string(), false))
        );
        assert_eq!(
            url(
                &mit_konfig,
                StreamerKey::UserId("de2"),
                InvitePurpose::ChannelLink
            )
            .await,
            None
        );
        // Unbekannter Kanal: deutscher Server wie bisher.
        assert_eq!(
            url(
                &ohne_konfig,
                StreamerKey::Login("fremd"),
                InvitePurpose::Promo
            )
            .await,
            Some((DEFAULT_PROMO_DISCORD_INVITE.to_string(), false))
        );
    }

    #[tokio::test]
    async fn fremder_server_wirbt_nur_fuer_sich() {
        let pool = pool_or_skip!("home_invite_fremd");
        seed(&pool).await;
        let resolver =
            HomeInviteResolver::new(pool.clone(), Some("https://discord.gg/konfig".to_string()));

        assert_eq!(
            url(
                &resolver,
                StreamerKey::Login("english"),
                InvitePurpose::Promo
            )
            .await,
            Some(("https://discord.gg/eigene".to_string(), true))
        );
        // Persönliche Einladung des deutschen Servers passt nicht mehr.
        for purpose in [
            InvitePurpose::Promo,
            InvitePurpose::Reply,
            InvitePurpose::ChannelLink,
        ] {
            assert_eq!(
                url(&resolver, StreamerKey::UserId("en2"), purpose).await,
                Some(("https://discord.gg/english".to_string(), false))
            );
        }
        // Server ohne Einladung: keine Werbung, auch nicht die globale.
        assert_eq!(
            url(
                &resolver,
                StreamerKey::Login("ohne_einladung"),
                InvitePurpose::Promo
            )
            .await,
            None
        );
    }

    #[tokio::test]
    async fn ohne_guild_tabellen_gilt_der_deutsche_server() {
        let pool = pool_or_skip!("home_invite_ohne_tabellen");
        sqlx::raw_sql(
            "CREATE TABLE twitch_streamer_invites (streamer_login TEXT, invite_url TEXT);
             INSERT INTO twitch_streamer_invites VALUES ('kanal', 'https://discord.gg/kanal');",
        )
        .execute(&pool)
        .await
        .unwrap();
        let resolver = HomeInviteResolver::new(pool.clone(), None);
        assert_eq!(
            url(&resolver, StreamerKey::Login("kanal"), InvitePurpose::Promo).await,
            Some(("https://discord.gg/kanal".to_string(), true))
        );
        assert_eq!(
            url(
                &resolver,
                StreamerKey::Login("anders"),
                InvitePurpose::Promo
            )
            .await,
            Some((DEFAULT_PROMO_DISCORD_INVITE.to_string(), false))
        );
    }
}
