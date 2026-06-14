//! Geschichtete Soul: dynamische Erweiterungen unter dem statischen Kern-Soul
//! (Port von `bot/engagement/soul_store.py`).
//!
//! Der Kern-Soul ist eine Konstante in [`crate::minimax_chat::SOUL`]. Hier kommen
//! die wachsenden Teile dazu, persistiert in `twitch_engagement_soul`:
//! `hero_takes` (kuratierte Hero-Vorlieben = Meinung, nicht Ton) und `anchor`
//! (selbst-gemerkte Notizen). [`SoulStore::get_soul_extension_fragment`] baut
//! daraus EIN Fragment unter den Kern-Soul.
//!
//! Slice 10a (hier): Store + Fragment. Der Reflexions-Job
//! (`reflect_and_store_anchor`, MiniMax) folgt in 10b.

use sqlx::PgPool;

/// So viele jüngste Anker in den Prompt.
const MAX_ANCHORS: i64 = 5;
/// So viele Anker insgesamt behalten (Pruning).
const KEEP_ANCHORS: i64 = 30;

/// Vorspann: betont, dass dies INNERER Geschmack ist, nicht der Chat-Ton.
const SOUL_INTRO: &str = "Noch was zu dir — aber WICHTIG: das hier ist dein INNERER Geschmack und dein \
Gedächtnis, nicht dein Chat-Ton. Zieh daraus deine Meinung, aber bleib im Chat \
trocken und knapp wie immer. Kipp diese Begeisterung NICHT 1:1 raus, kein Gehype, \
kein Schwall — eine ruhige, beiläufige Zeile reicht. Beziehe dich nur auf Helden/\
Abilities, die hier vorkommen.";

/// Baut das Soul-Extension-Fragment aus Hero-Takes + Ankern (reiner Port der
/// Assembly in `get_soul_extension_fragment`). Leer, wenn nichts da.
pub fn build_soul_fragment(takes: Option<&str>, anchors: &[String]) -> String {
    let takes = takes.filter(|t| !t.is_empty());
    if takes.is_none() && anchors.is_empty() {
        return String::new();
    }
    let mut parts: Vec<String> = vec![SOUL_INTRO.to_string()];
    if let Some(t) = takes {
        parts.push(format!("Deine Hero-Vorlieben:\n{t}"));
    }
    if !anchors.is_empty() {
        let lines = anchors.iter().map(|a| format!("- {a}")).collect::<Vec<_>>().join("\n");
        parts.push(format!(
            "Dinge, die dir zuletzt aufgefallen sind oder die du cool fandest \
             (nur beiläufig aufgreifen, wenn's grad passt):\n{lines}"
        ));
    }
    parts.join("\n\n")
}

/// Persistenter Soul-Store (`twitch_engagement_soul`).
pub struct SoulStore {
    pool: PgPool,
}

impl SoulStore {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// Speichert einen Soul-Eintrag. Bei `kind == "anchor"` werden alte Anker auf
    /// die jüngsten [`KEEP_ANCHORS`] gekürzt.
    pub async fn store_soul_entry(&self, kind: &str, content: &str) -> Result<(), sqlx::Error> {
        sqlx::query("INSERT INTO twitch_engagement_soul (kind, content) VALUES ($1, $2)")
            .bind(kind)
            .bind(content)
            .execute(&self.pool)
            .await?;
        if kind == "anchor" {
            sqlx::query(
                "DELETE FROM twitch_engagement_soul \
                 WHERE kind = 'anchor' AND id NOT IN (\
                   SELECT id FROM twitch_engagement_soul \
                   WHERE kind = 'anchor' ORDER BY created_at DESC LIMIT $1)",
            )
            .bind(KEEP_ANCHORS)
            .execute(&self.pool)
            .await?;
        }
        Ok(())
    }

    /// Jüngste Hero-Takes (oder None).
    async fn latest_hero_takes(&self) -> Option<String> {
        sqlx::query_scalar::<_, String>(
            "SELECT content FROM twitch_engagement_soul WHERE kind='hero_takes' \
             ORDER BY created_at DESC LIMIT 1",
        )
        .fetch_optional(&self.pool)
        .await
        .ok()
        .flatten()
        .filter(|s| !s.is_empty())
    }

    /// Die `limit` jüngsten Anker.
    async fn recent_anchors(&self, limit: i64) -> Vec<String> {
        sqlx::query_scalar::<_, String>(
            "SELECT content FROM twitch_engagement_soul WHERE kind='anchor' \
             ORDER BY created_at DESC LIMIT $1",
        )
        .bind(limit)
        .fetch_all(&self.pool)
        .await
        .unwrap_or_default()
        .into_iter()
        .filter(|s| !s.is_empty())
        .collect()
    }

    /// Hero-Takes + jüngste Anker als EIN Fragment unter den Kern-Soul; "" wenn
    /// nichts da.
    pub async fn get_soul_extension_fragment(&self) -> String {
        let takes = self.latest_hero_takes().await;
        let anchors = self.recent_anchors(MAX_ANCHORS).await;
        build_soul_fragment(takes.as_deref(), &anchors)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
    use std::str::FromStr;

    #[test]
    fn fragment_leer_und_teile() {
        assert_eq!(build_soul_fragment(None, &[]), "");
        assert_eq!(build_soul_fragment(Some(""), &[]), ""); // leerer take zählt nicht

        let only_takes = build_soul_fragment(Some("mag Haze"), &[]);
        assert!(only_takes.contains("INNERER Geschmack"));
        assert!(only_takes.contains("Deine Hero-Vorlieben:\nmag Haze"));
        assert!(!only_takes.contains("zuletzt aufgefallen"));

        let both = build_soul_fragment(Some("mag Haze"), &["combo war nice".to_string()]);
        assert!(both.contains("Deine Hero-Vorlieben"));
        assert!(both.contains("- combo war nice"));
    }

    async fn make_pool(schema: &str) -> Option<PgPool> {
        let dsn = std::env::var("TB_TEST_DATABASE_URL").ok()?;
        let admin = PgPoolOptions::new().max_connections(1).connect(&dsn).await.unwrap();
        sqlx::query(&format!("DROP SCHEMA IF EXISTS {schema} CASCADE")).execute(&admin).await.unwrap();
        sqlx::query(&format!("CREATE SCHEMA {schema}")).execute(&admin).await.unwrap();
        admin.close().await;
        let opts = PgConnectOptions::from_str(&dsn).unwrap().options([("search_path", schema)]);
        let pool = PgPoolOptions::new().max_connections(2).connect_with(opts).await.unwrap();
        sqlx::query(
            "CREATE TABLE twitch_engagement_soul (\
             id BIGSERIAL PRIMARY KEY, kind TEXT NOT NULL, content TEXT NOT NULL, \
             created_at TIMESTAMPTZ NOT NULL DEFAULT NOW())",
        )
        .execute(&pool)
        .await
        .unwrap();
        Some(pool)
    }

    #[tokio::test]
    async fn store_und_fragment_aus_db() {
        let Some(pool) = make_pool("t_eng_soul").await else { return };
        let store = SoulStore::new(pool.clone());
        store.store_soul_entry("hero_takes", "Haze ist mein liebling").await.unwrap();
        store.store_soul_entry("anchor", "der dive war wild").await.unwrap();

        let frag = store.get_soul_extension_fragment().await;
        assert!(frag.contains("Haze ist mein liebling"));
        assert!(frag.contains("- der dive war wild"));
    }

    #[tokio::test]
    async fn anchor_pruning_haelt_30() {
        let Some(pool) = make_pool("t_eng_soul_prune").await else { return };
        let store = SoulStore::new(pool.clone());
        // 32 Anker mit aufsteigender created_at, damit das DESC-Pruning deterministisch ist.
        for i in 0..32 {
            sqlx::query(
                "INSERT INTO twitch_engagement_soul (kind, content, created_at) \
                 VALUES ('anchor', $1, NOW() + ($2 || ' seconds')::interval)",
            )
            .bind(format!("anker {i}"))
            .bind(i.to_string())
            .execute(&pool)
            .await
            .unwrap();
        }
        // Ein weiterer Store löst das Pruning aus.
        store.store_soul_entry("anchor", "neuester").await.unwrap();
        let count: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM twitch_engagement_soul WHERE kind='anchor'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(count, KEEP_ANCHORS);
    }
}
