//! Zusätzliche Antwortsperre, unabhängig von den Moderationsschaltern.
//!
//! Liest ausschließlich vorhandene Identitäts-/Scam-Belege und verwendet den
//! bestehenden Spamfilter. Kein Modell, keine Chat-/Moderations-API, kein
//! Löschen und keine Klassifizierung anhand von Zuschauer-Inaktivität.

use std::sync::atomic::{AtomicBool, Ordering};

use sqlx::PgPool;

use crate::{
    spam_filter::{SpamAction, SpamContext, SpamFilter},
    types::ChatMessageEvent,
};

#[cfg(test)]
pub(crate) const SCAM_FIXTURE: &str = "CREATE TABLE twitch_scam_guard_verdicts (
    id BIGSERIAL PRIMARY KEY, channel_login TEXT NOT NULL, chatter_login TEXT NOT NULL,
    chatter_id TEXT, verdict TEXT NOT NULL, action_taken TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW())";

const BLOCK_QUERY: &str = r#"
SELECT EXISTS (
    SELECT 1 FROM twitch_chat_response_blocks
    WHERE chatter_user_id = $1 AND channel_user_id IN ('', $3) AND active
), COALESCE((
    SELECT verdict = 'scam' AND action_taken IN
        ('watching', 'suggested', 'banned', 'timed_out', 'ban_failed_no_mod')
    FROM twitch_scam_guard_verdicts
    WHERE lower(channel_login) = $4
      AND (chatter_id = $1 OR
           ((chatter_id IS NULL OR trim(chatter_id) = '') AND lower(chatter_login) = $2))
    ORDER BY created_at DESC, id DESC LIMIT 1
), FALSE)
"#;

pub(crate) struct ReactionGuard {
    pool: PgPool,
    unavailable_reported: AtomicBool,
}

impl ReactionGuard {
    pub(crate) fn new(pool: PgPool) -> Self {
        Self {
            pool,
            unavailable_reported: AtomicBool::new(false),
        }
    }

    /// Eine fehlgeschlagene Prüfung erlaubt keine Unterhaltung. Die normale
    /// autorisierte Moderation und der Archivpfad sind davon unabhängig.
    pub(crate) async fn allows(&self, event: &ChatMessageEvent, spam: &SpamFilter) -> bool {
        if event.chatter_user_id.trim().is_empty()
            || event.broadcaster_user_id.trim().is_empty()
            || tb_analytics::bekannte_bots::ist_ausgeschlossener_login(&event.chatter_user_login)
        {
            return false;
        }
        let facts = sqlx::query_as::<_, (bool, bool)>(BLOCK_QUERY)
            .bind(event.chatter_user_id.trim())
            .bind(event.chatter_user_login.trim().to_lowercase())
            .bind(event.broadcaster_user_id.trim())
            .bind(event.broadcaster_user_login.trim().to_lowercase())
            .fetch_one(&self.pool)
            .await;
        let (blocked_identity, scam_verdict) = match facts {
            Ok(facts) => {
                if self.unavailable_reported.swap(false, Ordering::Relaxed) {
                    tracing::info!("Antwortschutz wieder verfügbar");
                }
                facts
            }
            Err(error) => {
                if !self.unavailable_reported.swap(true, Ordering::Relaxed) {
                    tracing::warn!(%error, "Antwortschutz nicht prüfbar; Unterhaltungsantworten bleiben gesperrt");
                }
                return false;
            }
        };
        if blocked_identity || scam_verdict {
            return false;
        }
        // Ein privilegiert bestätigter Safe-Volltext überstimmt nur das
        // Textmuster, niemals eine unabhängig gesetzte Identitätssperre.
        if spam.known_safe_pattern(event.text()).is_some() {
            return true;
        }
        // Kein Alter-/Erstnachrichten-Bonus: neue oder stille Zuschauer sind
        // kein Bot-Beweis. Nur die vorhandenen Textsignale verwenden.
        matches!(
            spam.evaluate(event.text(), &SpamContext::default()).action,
            SpamAction::None
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_postgres::TestPostgres;

    async fn database() -> TestPostgres {
        let db = TestPostgres::start().await;
        sqlx::raw_sql(SCAM_FIXTURE).execute(&db.pool).await.unwrap();
        sqlx::raw_sql(include_str!(
            "../../../migrations/20260918160000_chat_response_guard.sql"
        ))
        .execute(&db.pool)
        .await
        .unwrap();
        db
    }

    fn event() -> ChatMessageEvent {
        ChatMessageEvent {
            chatter_user_id: "101".into(),
            chatter_user_login: "viewer".into(),
            broadcaster_user_id: "202".into(),
            broadcaster_user_login: "channel".into(),
            message: crate::types::ChatMessageBody {
                text: "Hallo, ich bin neu bei Deadlock".into(),
                fragments: vec![],
            },
            ..Default::default()
        }
    }

    fn filter() -> SpamFilter {
        SpamFilter::new(Default::default())
    }

    #[tokio::test]
    async fn normale_und_stille_zuschauer_bekommen_keinen_botverdacht() {
        let db = database().await;
        let guard = ReactionGuard::new(db.pool.clone());
        for text in [
            "Hallo, ich bin neu bei Deadlock",
            "bin nur am lurken",
            "rookie",
            "bitte keine viewbots",
            "!commands",
        ] {
            let mut event = event();
            event.message.text = text.into();
            assert!(guard.allows(&event, &filter()).await, "{text}");
        }
    }

    #[tokio::test]
    async fn bekannte_bots_und_anonyme_identities_bleiben_stumm() {
        let db = database().await;
        let guard = ReactionGuard::new(db.pool.clone());
        for name in tb_analytics::bekannte_bots::KNOWN_CHAT_BOTS
            .iter()
            .copied()
            .chain(["justinfan123", "  NightBot  "])
        {
            let mut event = event();
            event.chatter_user_login = name.into();
            assert!(!guard.allows(&event, &filter()).await, "{name}");
        }
    }

    #[tokio::test]
    async fn bot_id_sperre_ist_reversibel_und_uebersteht_umbenennung() {
        let db = database().await;
        let guard = ReactionGuard::new(db.pool.clone());
        sqlx::query("INSERT INTO twitch_chat_response_blocks(chatter_user_id,kind,reason,source) VALUES ('101','viewer_bot','manuell bestätigt','admin')")
            .execute(&db.pool).await.unwrap();
        let mut evt = event();
        evt.chatter_user_login = "renamed".into();
        assert!(!guard.allows(&evt, &filter()).await);
        evt.chatter_user_id = "303".into();
        assert!(
            guard.allows(&evt, &filter()).await,
            "gleicher Login überträgt keine ID-Sperre"
        );
        sqlx::query("UPDATE twitch_chat_response_blocks SET active=FALSE, updated_at=NOW()")
            .execute(&db.pool)
            .await
            .unwrap();
        assert!(guard.allows(&event(), &filter()).await);
        let count: i64 = sqlx::query_scalar("SELECT count(*) FROM twitch_chat_response_blocks")
            .fetch_one(&db.pool)
            .await
            .unwrap();
        assert_eq!(count, 1, "Aufhebung löscht den Beleg nicht");
    }

    #[tokio::test]
    async fn kanalsperre_wirkt_nicht_in_fremden_kanaelen() {
        let db = database().await;
        let guard = ReactionGuard::new(db.pool.clone());
        sqlx::query("INSERT INTO twitch_chat_response_blocks(chatter_user_id,channel_user_id,kind,reason,source) VALUES ('101','404','suspected','Prüfung offen','admin')")
            .execute(&db.pool).await.unwrap();
        assert!(guard.allows(&event(), &filter()).await);
        let mut evt = event();
        evt.broadcaster_user_id = "404".into();
        assert!(!guard.allows(&evt, &filter()).await);
    }

    async fn verdict(pool: &PgPool, id: Option<&str>, kind: &str, action: &str) {
        sqlx::query("INSERT INTO twitch_scam_guard_verdicts(channel_login,chatter_login,chatter_id,verdict,action_taken) VALUES ('channel','viewer',$1,$2,$3)")
            .bind(id).bind(kind).bind(action).execute(pool).await.unwrap();
    }

    #[tokio::test]
    async fn aktuelles_scamurteil_und_aufhebung_statt_ewiger_altlast() {
        let db = database().await;
        let guard = ReactionGuard::new(db.pool.clone());
        verdict(&db.pool, Some("101"), "scam", "suggested").await;
        assert!(!guard.allows(&event(), &filter()).await);
        sqlx::query("UPDATE twitch_scam_guard_verdicts SET action_taken='overturned'")
            .execute(&db.pool)
            .await
            .unwrap();
        assert!(guard.allows(&event(), &filter()).await);
        verdict(&db.pool, Some("101"), "scam", "watching").await;
        assert!(!guard.allows(&event(), &filter()).await);
        verdict(&db.pool, Some("101"), "safe", "none").await;
        assert!(guard.allows(&event(), &filter()).await);
    }

    #[tokio::test]
    async fn scam_id_hat_vorrang_vor_wiederverwendetem_login() {
        let db = database().await;
        let guard = ReactionGuard::new(db.pool.clone());
        verdict(&db.pool, Some("909"), "scam", "banned").await;
        assert!(guard.allows(&event(), &filter()).await);
        verdict(&db.pool, None, "scam", "suggested").await;
        assert!(
            !guard.allows(&event(), &filter()).await,
            "Legacy-Beleg ohne ID bleibt verwendbar"
        );
    }

    #[tokio::test]
    async fn offensichtliche_viewerwerbung_auch_ohne_moderationsschalter_stumm() {
        let db = database().await;
        let guard = ReactionGuard::new(db.pool.clone());
        let mut evt = event();
        evt.message.text = "Best viewers streamboo.com".into();
        assert!(!guard.allows(&evt, &filter()).await);
    }

    #[tokio::test]
    async fn fehlendes_schema_sperrt_nur_reaktionen_und_erholt_sich() {
        let db = TestPostgres::start().await;
        let guard = ReactionGuard::new(db.pool.clone());
        assert!(!guard.allows(&event(), &filter()).await);
        sqlx::raw_sql(SCAM_FIXTURE).execute(&db.pool).await.unwrap();
        sqlx::raw_sql(include_str!(
            "../../../migrations/20260918160000_chat_response_guard.sql"
        ))
        .execute(&db.pool)
        .await
        .unwrap();
        assert!(guard.allows(&event(), &filter()).await);
    }
}
