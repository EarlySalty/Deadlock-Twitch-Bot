use std::collections::HashMap;

use chrono::{DateTime, Utc};
use sqlx::PgPool;
use tb_chat::zuschauer_register::{
    score, MemberIndex, MemberLite, PRIOR_COMMUNITY, PRIOR_PARTNER,
};
use tb_config::Settings;
use tb_transport_discord::BrokerRelay;

const BATCH: usize = 500;

struct Verteilung {
    gesamt: u64,
    mit_discord: u64,
    stufe_neuling: u64,
    stufe_mittel: u64,
    stufe_hoch: u64,
    stufe_sehr_hoch: u64,
}

impl Verteilung {
    fn neu() -> Self {
        Self {
            gesamt: 0,
            mit_discord: 0,
            stufe_neuling: 0,
            stufe_mittel: 0,
            stufe_hoch: 0,
            stufe_sehr_hoch: 0,
        }
    }

    fn zaehle(&mut self, p: f64, hat_discord: bool) {
        self.gesamt += 1;
        if hat_discord {
            self.mit_discord += 1;
        }
        if p < 0.35 {
            self.stufe_neuling += 1;
        } else if p < 0.6 {
            self.stufe_mittel += 1;
        } else if p < 0.85 {
            self.stufe_hoch += 1;
        } else {
            self.stufe_sehr_hoch += 1;
        }
    }
}

#[derive(Default)]
struct Batch {
    uids: Vec<String>,
    logins: Vec<String>,
    discords: Vec<String>,
    ps: Vec<f64>,
    sigs: Vec<String>,
    fpcs: Vec<String>,
    fsas: Vec<String>,
    cas: Vec<DateTime<Utc>>,
}

impl Batch {
    fn len(&self) -> usize {
        self.uids.len()
    }

    fn is_empty(&self) -> bool {
        self.uids.is_empty()
    }

    fn clear(&mut self) {
        self.uids.clear();
        self.logins.clear();
        self.discords.clear();
        self.ps.clear();
        self.sigs.clear();
        self.fpcs.clear();
        self.fsas.clear();
        self.cas.clear();
    }

    async fn schreibe(&self, pool: &PgPool) -> Result<(), sqlx::Error> {
        sqlx::query!(
            r#"INSERT INTO twitch_zuschauer_register
                 (twitch_user_id, twitch_login, discord_user_id, community_probability,
                  signals, first_partner_channel, first_seen_at, computed_at)
               SELECT u, NULLIF(l, ''), NULLIF(d, ''), p, s::jsonb,
                      NULLIF(fpc, ''), NULLIF(fsa, '')::timestamptz, ca
                 FROM UNNEST($1::text[], $2::text[], $3::text[], $4::float8[],
                             $5::text[], $6::text[], $7::text[], $8::timestamptz[])
                      AS t(u, l, d, p, s, fpc, fsa, ca)
               ON CONFLICT (twitch_user_id) DO UPDATE SET
                 twitch_login = COALESCE(
                     EXCLUDED.twitch_login, twitch_zuschauer_register.twitch_login),
                 discord_user_id = COALESCE(
                     EXCLUDED.discord_user_id, twitch_zuschauer_register.discord_user_id),
                 community_probability = EXCLUDED.community_probability,
                 signals = EXCLUDED.signals,
                 first_partner_channel = COALESCE(
                     twitch_zuschauer_register.first_partner_channel,
                     EXCLUDED.first_partner_channel),
                 first_seen_at = COALESCE(
                     twitch_zuschauer_register.first_seen_at,
                     EXCLUDED.first_seen_at),
                 computed_at = EXCLUDED.computed_at"#,
            &self.uids,
            &self.logins,
            &self.discords,
            &self.ps,
            &self.sigs,
            &self.fpcs,
            &self.fsas,
            &self.cas,
        )
        .execute(pool)
        .await?;
        Ok(())
    }
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();
    let mut trocken = false;
    for arg in std::env::args().skip(1) {
        match arg.as_str() {
            "--trocken" => trocken = true,
            other => {
                eprintln!("Unbekanntes Argument: {other}. Erlaubt ist nur --trocken.");
                std::process::exit(2);
            }
        }
    }

    let settings = Settings::from_env().unwrap_or_else(|error| {
        tracing::error!("Konfigurationsfehler: {error}");
        std::process::exit(1);
    });

    let pool = tb_db::connect(&settings.db).await.unwrap_or_else(|error| {
        tracing::error!("DB-Verbindungsfehler: {error}");
        std::process::exit(1);
    });

    let relay = BrokerRelay::new(&settings.broker).unwrap_or_else(|error| {
        tracing::error!("Broker-Konfiguration fehlerhaft: {error}");
        std::process::exit(1);
    });

    let members = relay.list_members().await.unwrap_or_else(|error| {
        tracing::error!("Mitgliederliste nicht erreichbar: {error}");
        std::process::exit(1);
    });

    let lite: Vec<MemberLite> = members
        .into_iter()
        .map(|m| MemberLite {
            id: m.id,
            name: m.name,
            global_name: m.global_name,
            nick: m.nick,
        })
        .collect();
    let index = MemberIndex::build(&lite);

    match run(&pool, &index, trocken).await {
        Ok((verteilung, unaufloesbar)) => {
            tracing::info!(
                trocken,
                eintraege_gesamt = verteilung.gesamt,
                mit_discord = verteilung.mit_discord,
                p_unter_035 = verteilung.stufe_neuling,
                p_035_bis_06 = verteilung.stufe_mittel,
                p_06_bis_085 = verteilung.stufe_hoch,
                p_ueber_085 = verteilung.stufe_sehr_hoch,
                nicht_aufloesbare_logins = unaufloesbar,
                "zuschauer-register-backfill fertig"
            );
        }
        Err(error) => {
            tracing::error!("zuschauer-register-backfill fehlgeschlagen: {error}");
            std::process::exit(1);
        }
    }
}

async fn run(
    pool: &PgPool,
    index: &MemberIndex,
    trocken: bool,
) -> Result<(Verteilung, i64), sqlx::Error> {
    let hard_rows = sqlx::query!(
        r#"SELECT twitch_user_id, discord_user_id AS "discord_user_id!"
             FROM twitch_streamer_identities
            WHERE discord_user_id IS NOT NULL"#
    )
    .fetch_all(pool)
    .await?;
    let hard: HashMap<String, String> = hard_rows
        .into_iter()
        .map(|row| (row.twitch_user_id, row.discord_user_id))
        .collect();

    let rows = sqlx::query!(
        r#"
        WITH alias AS (
            SELECT DISTINCT ON (LOWER(login)) LOWER(login) AS login_l, twitch_user_id
              FROM twitch_login_aliases
             ORDER BY LOWER(login), is_current DESC, last_seen_at DESC
        ),
        events AS (
            SELECT sc.chatter_id, sc.chatter_login, LOWER(sc.streamer_login) AS channel,
                   sc.first_message_at AS ts
              FROM twitch_session_chatters sc
            UNION ALL
            SELECT cm.chatter_id, cm.chatter_login, LOWER(cm.streamer_login) AS channel,
                   cm.message_ts AS ts
              FROM twitch_chat_messages cm
        ),
        resolved AS (
            SELECT COALESCE(e.chatter_id, a.twitch_user_id) AS uid,
                   e.chatter_login, e.channel, e.ts
              FROM events e
              LEFT JOIN alias a ON a.login_l = LOWER(e.chatter_login)
        ),
        valid AS (
            SELECT uid, chatter_login, channel, ts
              FROM resolved
             WHERE uid IS NOT NULL
        ),
        partner AS (
            SELECT LOWER(twitch_login) AS channel
              FROM twitch_streamers_partner_state
             WHERE is_partner_active = 1 AND twitch_login IS NOT NULL
            UNION
            SELECT 'dach_lock'
        ),
        partner_events AS (
            SELECT v.uid, v.channel, v.ts
              FROM valid v
              JOIN partner p ON p.channel = v.channel
        ),
        first_partner AS (
            SELECT DISTINCT ON (uid) uid, channel AS first_partner_channel, ts AS first_seen
              FROM partner_events
             ORDER BY uid, ts ASC
        ),
        community AS (
            SELECT uid, bool_or(channel = 'dach_lock') AS in_community
              FROM partner_events
             GROUP BY uid
        ),
        login_pick AS (
            SELECT DISTINCT ON (uid) uid, chatter_login AS login
              FROM valid
             ORDER BY uid, (chatter_login IS NULL), ts DESC
        )
        SELECT lp.uid AS "uid!",
               lp.login,
               fp.first_partner_channel,
               fp.first_seen,
               COALESCE(c.in_community, false) AS "in_community!"
          FROM login_pick lp
          LEFT JOIN first_partner fp ON fp.uid = lp.uid
          LEFT JOIN community c ON c.uid = lp.uid
        "#
    )
    .fetch_all(pool)
    .await?;

    let unaufloesbar = sqlx::query_scalar!(
        r#"
        WITH alias AS (
            SELECT DISTINCT ON (LOWER(login)) LOWER(login) AS login_l, twitch_user_id
              FROM twitch_login_aliases
             ORDER BY LOWER(login), is_current DESC, last_seen_at DESC
        ),
        events AS (
            SELECT sc.chatter_id, sc.chatter_login FROM twitch_session_chatters sc
            UNION ALL
            SELECT cm.chatter_id, cm.chatter_login FROM twitch_chat_messages cm
        ),
        nur_login AS (
            SELECT LOWER(chatter_login) AS login_l
              FROM events
             WHERE chatter_login IS NOT NULL
             GROUP BY LOWER(chatter_login)
            HAVING bool_and(chatter_id IS NULL)
        )
        SELECT COUNT(*) AS "anzahl!"
          FROM nur_login nl
          LEFT JOIN alias a ON a.login_l = nl.login_l
         WHERE a.twitch_user_id IS NULL
        "#
    )
    .fetch_one(pool)
    .await?;

    let mut verteilung = Verteilung::neu();
    let now = Utc::now();
    let mut batch = Batch::default();

    for row in rows {
        let prior = if row.in_community {
            PRIOR_COMMUNITY
        } else {
            PRIOR_PARTNER
        };
        let login = row.login.unwrap_or_default();
        let hard_id = hard.get(&row.uid).map(|s| s.as_str());
        let (p, discord_id, signals) = score(&login, index, prior, hard_id);

        verteilung.zaehle(p, discord_id.is_some());

        batch.uids.push(row.uid);
        batch.logins.push(login);
        batch.discords.push(discord_id.unwrap_or_default());
        batch.ps.push(p);
        batch
            .sigs
            .push(serde_json::to_string(&signals).unwrap_or_else(|_| "{}".to_string()));
        batch.fpcs.push(row.first_partner_channel.unwrap_or_default());
        batch
            .fsas
            .push(row.first_seen.map(|t| t.to_rfc3339()).unwrap_or_default());
        batch.cas.push(now);

        if batch.len() >= BATCH {
            if !trocken {
                batch.schreibe(pool).await?;
            }
            batch.clear();
        }
    }

    if !batch.is_empty() && !trocken {
        batch.schreibe(pool).await?;
    }

    Ok((verteilung, unaufloesbar))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    use sqlx::postgres::{PgConnectOptions, PgPoolOptions};

    async fn pool_in_schema(name: &str) -> Option<PgPool> {
        let dsn = match std::env::var("TB_TEST_DATABASE_URL") {
            Ok(dsn) => dsn,
            Err(_) => {
                eprintln!("SKIP: TB_TEST_DATABASE_URL nicht gesetzt");
                return None;
            }
        };
        let schema = format!("{name}_{}", std::process::id());
        let admin = PgPoolOptions::new()
            .max_connections(1)
            .connect(&dsn)
            .await
            .unwrap();
        sqlx::query(&format!("DROP SCHEMA IF EXISTS {schema} CASCADE"))
            .execute(&admin)
            .await
            .unwrap();
        sqlx::query(&format!("CREATE SCHEMA {schema}"))
            .execute(&admin)
            .await
            .unwrap();
        admin.close().await;

        let opts = PgConnectOptions::from_str(&dsn)
            .unwrap()
            .options([("search_path", schema.as_str())]);
        let pool = PgPoolOptions::new()
            .max_connections(2)
            .connect_with(opts)
            .await
            .unwrap();
        Some(pool)
    }

    async fn baue_schema(pool: &PgPool) {
        for ddl in [
            "CREATE TABLE twitch_streamer_identities (
                twitch_user_id TEXT NOT NULL,
                twitch_login TEXT NOT NULL,
                discord_user_id TEXT
            )",
            "CREATE TABLE twitch_login_aliases (
                login TEXT NOT NULL,
                twitch_user_id TEXT NOT NULL,
                is_current BOOLEAN NOT NULL DEFAULT false,
                last_seen_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
            )",
            "CREATE TABLE twitch_session_chatters (
                chatter_id TEXT,
                chatter_login TEXT NOT NULL,
                streamer_login TEXT NOT NULL,
                first_message_at TIMESTAMPTZ NOT NULL
            )",
            "CREATE TABLE twitch_chat_messages (
                chatter_id TEXT,
                chatter_login TEXT,
                streamer_login TEXT NOT NULL,
                message_ts TIMESTAMPTZ NOT NULL
            )",
            "CREATE TABLE twitch_streamers_partner_state (
                twitch_login TEXT,
                is_partner_active INTEGER
            )",
            "CREATE TABLE twitch_zuschauer_register (
                twitch_user_id TEXT PRIMARY KEY,
                twitch_login TEXT,
                discord_user_id TEXT,
                community_probability DOUBLE PRECISION NOT NULL,
                signals JSONB NOT NULL DEFAULT '{}'::jsonb,
                first_partner_channel TEXT,
                first_seen_at TIMESTAMPTZ,
                computed_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
            )",
        ] {
            sqlx::query(ddl).execute(pool).await.unwrap();
        }
    }

    async fn fuelle_fixtures(pool: &PgPool) {
        sqlx::query(
            "INSERT INTO twitch_streamers_partner_state (twitch_login, is_partner_active)
             VALUES ('dach_lock', 1), ('partnera', 1), ('randomchan', 0)",
        )
        .execute(pool)
        .await
        .unwrap();

        sqlx::query(
            "INSERT INTO twitch_streamer_identities (twitch_user_id, twitch_login, discord_user_id)
             VALUES ('hard1', 'hartuser', 'd-hard')",
        )
        .execute(pool)
        .await
        .unwrap();

        sqlx::query(
            "INSERT INTO twitch_login_aliases (login, twitch_user_id, is_current)
             VALUES ('aliasuser', 'u_alias', true)",
        )
        .execute(pool)
        .await
        .unwrap();

        sqlx::query(
            "INSERT INTO twitch_session_chatters
                 (chatter_id, chatter_login, streamer_login, first_message_at)
             VALUES
                 ('u_new', 'earlysalty', 'partnera', '2026-08-01T10:00:00Z'),
                 ('u_low', 'voelligfremd', 'partnera', '2026-08-01T11:00:00Z'),
                 ('u_comm', 'someone', 'dach_lock', '2026-08-01T12:00:00Z'),
                 ('hard1', 'hartuser', 'partnera', '2026-08-01T13:00:00Z'),
                 ('u_offchan', 'offperson', 'randomchan', '2026-08-01T14:00:00Z'),
                 (NULL, 'aliasuser', 'partnera', '2026-08-01T15:00:00Z'),
                 (NULL, 'ghost', 'partnera', '2026-08-01T16:00:00Z')",
        )
        .execute(pool)
        .await
        .unwrap();
    }

    fn test_index() -> MemberIndex {
        MemberIndex::build(&[MemberLite {
            id: "disc-early".to_string(),
            name: "earlysalty".to_string(),
            global_name: None,
            nick: None,
        }])
    }

    async fn register_count(pool: &PgPool) -> i64 {
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM twitch_zuschauer_register")
            .fetch_one(pool)
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn trockenlauf_rechnet_ohne_zu_schreiben() {
        let Some(pool) = pool_in_schema("tb_zuschauer_backfill_trocken").await else {
            return;
        };
        baue_schema(&pool).await;
        fuelle_fixtures(&pool).await;

        let index = test_index();
        let (verteilung, unaufloesbar) = run(&pool, &index, true).await.unwrap();

        assert_eq!(verteilung.gesamt, 6, "sechs aufloesbare IDs erwartet");
        assert_eq!(verteilung.mit_discord, 2, "u_new und hard1 tragen Discord");
        assert_eq!(verteilung.stufe_neuling, 3, "u_low, u_alias, u_offchan");
        assert_eq!(verteilung.stufe_mittel, 0);
        assert_eq!(verteilung.stufe_hoch, 1, "u_comm ueber dach_lock-Prior");
        assert_eq!(verteilung.stufe_sehr_hoch, 2, "u_new Namenstreffer, hard1 hart");
        assert_eq!(unaufloesbar, 1, "ghost ist nicht aufloesbar");

        assert_eq!(register_count(&pool).await, 0, "Trockenlauf schreibt nichts");
    }

    #[tokio::test]
    async fn schreiblauf_ist_idempotent_und_haelt_first_seen() {
        let Some(pool) = pool_in_schema("tb_zuschauer_backfill_schreib").await else {
            return;
        };
        baue_schema(&pool).await;
        fuelle_fixtures(&pool).await;

        let index = test_index();
        run(&pool, &index, false).await.unwrap();
        assert_eq!(register_count(&pool).await, 6);

        let discord: Option<String> = sqlx::query_scalar(
            "SELECT discord_user_id FROM twitch_zuschauer_register WHERE twitch_user_id = 'u_new'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(discord.as_deref(), Some("disc-early"));

        let first_seen: DateTime<Utc> = sqlx::query_scalar(
            "SELECT first_seen_at FROM twitch_zuschauer_register WHERE twitch_user_id = 'u_new'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();

        sqlx::query("DELETE FROM twitch_session_chatters WHERE chatter_id = 'u_new'")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query(
            "INSERT INTO twitch_chat_messages (chatter_id, chatter_login, streamer_login, message_ts)
             VALUES ('u_new', 'earlysalty', 'partnera', '2026-09-01T10:00:00Z')",
        )
        .execute(&pool)
        .await
        .unwrap();

        let leerer_index = MemberIndex::build(&[]);
        run(&pool, &leerer_index, false).await.unwrap();
        assert_eq!(register_count(&pool).await, 6, "zweiter Lauf legt nichts doppelt an");

        let first_seen_2: DateTime<Utc> = sqlx::query_scalar(
            "SELECT first_seen_at FROM twitch_zuschauer_register WHERE twitch_user_id = 'u_new'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(
            first_seen, first_seen_2,
            "erstes Auftauchen bleibt trotz spaeterem Ersatz-Event stehen"
        );

        let discord_2: Option<String> = sqlx::query_scalar(
            "SELECT discord_user_id FROM twitch_zuschauer_register WHERE twitch_user_id = 'u_new'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(
            discord_2.as_deref(),
            Some("disc-early"),
            "bekannte Discord-Zuordnung bleibt, auch wenn der Index sie nicht mehr kennt"
        );

        let offchan_seen: Option<DateTime<Utc>> = sqlx::query_scalar(
            "SELECT first_seen_at FROM twitch_zuschauer_register WHERE twitch_user_id = 'u_offchan'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert!(offchan_seen.is_none(), "ohne Partnerkanal kein erstes Auftauchen");
    }
}
