use async_trait::async_trait;
use sqlx::PgPool;
use tracing::warn;

const BEWERTUNGS_FENSTER_TAGE: i64 = 14;
const DAUMEN_HOCH: char = '👍';
const DAUMEN_RUNTER: char = '👎';

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Reaktion {
    pub emoji: String,
    pub count: i64,
}

#[derive(Debug)]
pub struct ReaktionsFehler(pub String);

#[async_trait]
pub trait ReaktionsQuelle: Send + Sync {
    async fn reaktionen(
        &self,
        message_id: &str,
    ) -> Result<Option<Vec<Reaktion>>, ReaktionsFehler>;
}

fn urteil(reaktionen: &[Reaktion]) -> Option<&'static str> {
    let hoch = reaktionen
        .iter()
        .any(|r| r.count > 0 && r.emoji.starts_with(DAUMEN_HOCH));
    let runter = reaktionen
        .iter()
        .any(|r| r.count > 0 && r.emoji.starts_with(DAUMEN_RUNTER));
    match (hoch, runter) {
        (true, true) => Some("schlecht"),
        (true, false) => Some("gut"),
        (false, true) => Some("schlecht"),
        (false, false) => None,
    }
}

pub async fn bewerte_offene_karten<Q: ReaktionsQuelle + ?Sized>(pool: &PgPool, quelle: &Q) {
    let offene = match sqlx::query!(
        r#"SELECT id, review_message_id AS "review_message_id!"
             FROM twitch_promo_pitch_log
            WHERE review_message_id IS NOT NULL
              AND bewertung IS NULL
              AND bewertet_at IS NULL
              AND sent_at > NOW() - ($1 || ' days')::interval"#,
        BEWERTUNGS_FENSTER_TAGE.to_string(),
    )
    .fetch_all(pool)
    .await
    {
        Ok(rows) => rows,
        Err(error) => {
            warn!(%error, "pitch-bewertung: offene Karten nicht lesbar");
            return;
        }
    };

    let mut fehler = 0usize;
    for row in offene {
        match quelle.reaktionen(&row.review_message_id.to_string()).await {
            Ok(None) => schliesse_ohne_bewertung(pool, row.id).await,
            Ok(Some(reaktionen)) => {
                if let Some(bewertung) = urteil(&reaktionen) {
                    setze_bewertung(pool, row.id, bewertung).await;
                }
            }
            Err(_) => fehler += 1,
        }
    }
    if fehler > 0 {
        warn!(fehler, "pitch-bewertung: Broker-Reaktionen teilweise nicht abrufbar");
    }
}

async fn setze_bewertung(pool: &PgPool, id: i64, bewertung: &str) {
    if let Err(error) = sqlx::query!(
        "UPDATE twitch_promo_pitch_log SET bewertung = $2, bewertet_at = NOW() WHERE id = $1",
        id,
        bewertung,
    )
    .execute(pool)
    .await
    {
        warn!(%error, id, "pitch-bewertung: Bewertung nicht schreibbar");
    }
}

async fn schliesse_ohne_bewertung(pool: &PgPool, id: i64) {
    if let Err(error) = sqlx::query!(
        "UPDATE twitch_promo_pitch_log SET bewertet_at = NOW() WHERE id = $1",
        id,
    )
    .execute(pool)
    .await
    {
        warn!(%error, id, "pitch-bewertung: geloeschte Karte nicht abschliessbar");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reaktion(emoji: &str, count: i64) -> Reaktion {
        Reaktion {
            emoji: emoji.to_string(),
            count,
        }
    }

    #[test]
    fn urteil_daumen_hoch_ist_gut() {
        assert_eq!(urteil(&[reaktion("👍", 2)]), Some("gut"));
    }

    #[test]
    fn urteil_daumen_hoch_mit_hautton_ist_gut() {
        assert_eq!(urteil(&[reaktion("👍🏽", 1)]), Some("gut"));
    }

    #[test]
    fn urteil_daumen_runter_ist_schlecht() {
        assert_eq!(urteil(&[reaktion("👎", 1)]), Some("schlecht"));
    }

    #[test]
    fn urteil_beides_ist_schlecht() {
        assert_eq!(
            urteil(&[reaktion("👍", 1), reaktion("👎", 1)]),
            Some("schlecht")
        );
    }

    #[test]
    fn urteil_keine_reaktion_bleibt_offen() {
        assert_eq!(urteil(&[]), None);
        assert_eq!(urteil(&[reaktion("❤️", 3)]), None);
        assert_eq!(urteil(&[reaktion("👍", 0)]), None);
    }
}

#[cfg(test)]
mod db_tests {
    use super::*;
    use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
    use std::collections::HashMap;
    use std::str::FromStr;

    struct FakeQuelle(HashMap<String, Result<Option<Vec<Reaktion>>, ()>>);

    #[async_trait]
    impl ReaktionsQuelle for FakeQuelle {
        async fn reaktionen(
            &self,
            message_id: &str,
        ) -> Result<Option<Vec<Reaktion>>, ReaktionsFehler> {
            match self.0.get(message_id) {
                Some(Ok(value)) => Ok(value.clone()),
                Some(Err(())) => Err(ReaktionsFehler("fake".to_string())),
                None => Ok(Some(Vec::new())),
            }
        }
    }

    async fn pool_in_schema(schema: &str) -> Option<PgPool> {
        let dsn = std::env::var("TB_TEST_DATABASE_URL").ok()?;
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
            .options([("search_path", schema)]);
        let pool = PgPoolOptions::new()
            .max_connections(2)
            .connect_with(opts)
            .await
            .unwrap();
        sqlx::query(
            r#"CREATE TABLE twitch_promo_pitch_log (
                id BIGSERIAL PRIMARY KEY,
                channel_login TEXT NOT NULL,
                target_user_id TEXT,
                pfad TEXT NOT NULL,
                occasion TEXT,
                trigger_text TEXT,
                generated_text TEXT,
                reject_reason TEXT,
                sent_at TIMESTAMPTZ,
                review_message_id BIGINT,
                bewertung TEXT,
                bewertet_at TIMESTAMPTZ,
                created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
            )"#,
        )
        .execute(&pool)
        .await
        .unwrap();
        Some(pool)
    }

    async fn insert_karte(pool: &PgPool, message_id: i64) -> i64 {
        sqlx::query_scalar(
            "INSERT INTO twitch_promo_pitch_log
                (channel_login, pfad, generated_text, sent_at, review_message_id)
             VALUES ('kanal', 'anlass', 'antwort', NOW(), $1) RETURNING id",
        )
        .bind(message_id)
        .fetch_one(pool)
        .await
        .unwrap()
    }

    async fn bewertung_von(pool: &PgPool, id: i64) -> (Option<String>, bool) {
        let row = sqlx::query!(
            r#"SELECT bewertung, bewertet_at IS NOT NULL AS "abgeschlossen!"
                 FROM twitch_promo_pitch_log WHERE id = $1"#,
            id,
        )
        .fetch_one(pool)
        .await
        .unwrap();
        (row.bewertung, row.abgeschlossen)
    }

    #[tokio::test]
    async fn bewertet_karten_nach_reaktion() {
        let Some(pool) = pool_in_schema("pitch_bewertung_test").await else {
            eprintln!("SKIP: TB_TEST_DATABASE_URL nicht gesetzt");
            return;
        };
        let gut = insert_karte(&pool, 100).await;
        let schlecht = insert_karte(&pool, 200).await;
        let beides = insert_karte(&pool, 300).await;
        let leer = insert_karte(&pool, 400).await;
        let geloescht = insert_karte(&pool, 500).await;
        let fehler = insert_karte(&pool, 600).await;

        let mut map: HashMap<String, Result<Option<Vec<Reaktion>>, ()>> = HashMap::new();
        map.insert("100".into(), Ok(Some(vec![Reaktion { emoji: "👍".into(), count: 2 }])));
        map.insert("200".into(), Ok(Some(vec![Reaktion { emoji: "👎".into(), count: 1 }])));
        map.insert(
            "300".into(),
            Ok(Some(vec![
                Reaktion { emoji: "👍".into(), count: 1 },
                Reaktion { emoji: "👎".into(), count: 1 },
            ])),
        );
        map.insert("400".into(), Ok(Some(Vec::new())));
        map.insert("500".into(), Ok(None));
        map.insert("600".into(), Err(()));

        bewerte_offene_karten(&pool, &FakeQuelle(map)).await;

        assert_eq!(bewertung_von(&pool, gut).await, (Some("gut".into()), true));
        assert_eq!(bewertung_von(&pool, schlecht).await, (Some("schlecht".into()), true));
        assert_eq!(bewertung_von(&pool, beides).await, (Some("schlecht".into()), true));
        assert_eq!(bewertung_von(&pool, leer).await, (None, false));
        assert_eq!(bewertung_von(&pool, geloescht).await, (None, true));
        assert_eq!(bewertung_von(&pool, fehler).await, (None, false));
    }
}
