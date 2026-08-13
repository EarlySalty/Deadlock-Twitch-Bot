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

use crate::minimax_chat::{strip_think, EngagementMinimaxClient, PersonaMode};

/// So viele jüngste Anker in den Prompt.
const MAX_ANCHORS: i64 = 5;
/// So viele Anker insgesamt behalten (Pruning).
const KEEP_ANCHORS: i64 = 30;
/// So viele jüngste Turns die Reflexion ansieht.
const REFLECT_TURNS: i64 = 40;
/// Darunter lohnt sich Reflexion nicht.
const REFLECT_MIN_TURNS: usize = 8;
/// Maximale Anker-Länge.
const ANCHOR_MAX_LEN: usize = 220;

const ANCHOR_SYS: &str = "Du bist eine feste Twitch-Chat-Persönlichkeit (ein Deadlock-Stammgast). \
Antworte knapp und nur mit dem Verlangten.";

/// Vorspann: betont, dass dies INNERER Geschmack ist, nicht der Chat-Ton.
const SOUL_INTRO: &str = "Noch was zu dir — aber WICHTIG: das hier ist dein INNERER Geschmack und dein \
Gedächtnis, nicht dein Chat-Ton. Zieh daraus deine Meinung, aber bleib im Chat \
trocken und knapp wie immer. Kipp diese Begeisterung NICHT 1:1 raus, kein Gehype, \
kein Schwall — eine ruhige, beiläufige Zeile reicht. Beziehe dich nur auf Helden/\
Abilities, die hier vorkommen.";

/// Vorspann im Neuling-Modus: derselbe Zweck, aber ohne die Einladung, daraus
/// eine Meinung zu ziehen — die hat ein Neuling nicht.
const SOUL_INTRO_ROOKIE: &str = "Noch was zu dir — aber WICHTIG: das hier ist dein Gedächtnis, nicht dein \
Chat-Ton und erst recht kein Fachwissen. Es sind Sachen, die dir als Neuling \
aufgefallen sind. Greif sie höchstens beiläufig auf, wenn es gerade passt, und \
bleib im Chat kurz und normal. Leite daraus KEINE Einschätzung zum Spiel ab und \
tu nicht so, als würdest du dadurch etwas verstehen.";

/// Baut das Soul-Extension-Fragment aus Hero-Takes + Ankern (reiner Port der
/// Assembly in `get_soul_extension_fragment`). Leer, wenn nichts da.
pub fn build_soul_fragment(takes: Option<&str>, anchors: &[String]) -> String {
    build_soul_fragment_for(takes, anchors, PersonaMode::from_env())
}

/// Wie [`build_soul_fragment`], mit explizitem Persona-Modus. Im
/// Neuling-Modus fallen die Hero-Vorlieben weg: kuratierte Hero-Meinungen sind
/// genau das, was ein Neuling nicht hat.
pub fn build_soul_fragment_for(
    takes: Option<&str>,
    anchors: &[String],
    persona: PersonaMode,
) -> String {
    let rookie = persona == PersonaMode::Rookie;
    let takes = takes.filter(|t| !t.is_empty()).filter(|_| !rookie);
    if takes.is_none() && anchors.is_empty() {
        return String::new();
    }
    let intro = if rookie { SOUL_INTRO_ROOKIE } else { SOUL_INTRO };
    let mut parts: Vec<String> = vec![intro.to_string()];
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

/// Baut das Reflexions-Transkript: `who: content` (who = "ich" für den Bot,
/// sonst Login/„jemand"), zusammengefügt, auf die letzten 4000 Zeichen gekürzt.
fn build_transcript(rows: &[(String, Option<String>, String)]) -> String {
    let lines: Vec<String> = rows
        .iter()
        .map(|(role, login, content)| {
            let who = if role == "assistant" {
                "ich".to_string()
            } else {
                login.clone().filter(|l| !l.is_empty()).unwrap_or_else(|| "jemand".to_string())
            };
            format!("{who}: {content}")
        })
        .collect();
    let joined = lines.join("\n");
    let len = joined.chars().count();
    if len <= 4000 {
        joined
    } else {
        joined.chars().skip(len - 4000).collect()
    }
}

/// User-Prompt für die Anker-Reflexion (Python `_anchor_user_prompt`).
fn anchor_user_prompt(transcript: &str) -> String {
    format!(
        "Hier ein Ausschnitt aus dem Chat, in dem du grad unterwegs warst ('ich' = du). \
         Ist dir was hängengeblieben — ein geiles Gespräch, ein Running Gag, ein cooler Move \
         den jemand beschrieben hat, oder was Cooles das du entdeckt hast — was DU dir als \
         dieser Typ wirklich merken würdest? Wenn ja, schreib EINE kurze Ich-Notiz an dich \
         selbst (max 1 Satz, locker, wie ein mentaler Merker, kein Namedropping von Usern als \
         Fakt). Wenn nichts wirklich hängengeblieben ist, antworte EXAKT mit: NICHTS\n\nChat:\n{transcript}"
    )
}

/// Nachbearbeitung der Modell-Antwort: `<think>`-Strip, Whitespace + Quotes weg,
/// dann Filter (leer / „NICHTS…" / zu lang / identisch zum letzten Anker) → None.
fn process_anchor_text(raw: &str, last: Option<&str>) -> Option<String> {
    let stripped = strip_think(raw);
    let text = stripped.trim().trim_matches('"').to_string();
    if text.is_empty()
        || text.to_uppercase().starts_with("NICHTS")
        || text.chars().count() > ANCHOR_MAX_LEN
    {
        return None;
    }
    if let Some(last) = last {
        if text.trim().to_lowercase() == last.trim().to_lowercase() {
            return None;
        }
    }
    Some(text)
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
        sqlx::query!(
            "INSERT INTO twitch_engagement_soul (kind, content) VALUES ($1, $2)",
            kind,
            content
        )
            .execute(&self.pool)
            .await?;
        if kind == "anchor" {
            sqlx::query!(
                "DELETE FROM twitch_engagement_soul \
                 WHERE kind = 'anchor' AND id NOT IN (\
                   SELECT id FROM twitch_engagement_soul \
                   WHERE kind = 'anchor' ORDER BY created_at DESC LIMIT $1)",
                KEEP_ANCHORS
            )
            .execute(&self.pool)
            .await?;
        }
        Ok(())
    }

    /// Jüngste Hero-Takes (oder None).
    async fn latest_hero_takes(&self) -> Option<String> {
        sqlx::query_scalar!(
            r#"SELECT content AS "content!" FROM twitch_engagement_soul
             WHERE kind='hero_takes'
             ORDER BY created_at DESC LIMIT 1"#
        )
        .fetch_optional(&self.pool)
        .await
        .ok()
        .flatten()
        .filter(|s| !s.is_empty())
    }

    /// Die `limit` jüngsten Anker.
    async fn recent_anchors(&self, limit: i64) -> Vec<String> {
        sqlx::query_scalar!(
            r#"SELECT content AS "content!" FROM twitch_engagement_soul
             WHERE kind='anchor'
             ORDER BY created_at DESC LIMIT $1"#,
            limit
        )
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

    /// Die `limit` jüngsten Konversations-Turns (role, login, content),
    /// chronologisch (älteste zuerst).
    async fn recent_convo(&self, limit: i64) -> Vec<(String, Option<String>, String)> {
        let rows = sqlx::query!(
            r#"SELECT role AS "role!", twitch_login, content AS "content!"
             FROM twitch_engagement_conversation
             ORDER BY ts DESC LIMIT $1"#,
            limit
        )
        .fetch_all(&self.pool)
        .await
        .unwrap_or_default();
        rows.into_iter()
            .rev()
            .map(|r| (r.role, r.twitch_login, r.content))
            .collect()
    }

    /// Der jüngste Anker (für die Dedup-Prüfung).
    async fn last_anchor(&self) -> Option<String> {
        sqlx::query_scalar!(
            r#"SELECT content AS "content!" FROM twitch_engagement_soul
             WHERE kind='anchor'
             ORDER BY created_at DESC LIMIT 1"#
        )
        .fetch_optional(&self.pool)
        .await
        .ok()
        .flatten()
        .filter(|s| !s.is_empty())
    }

    /// Reflektiert die letzten Chats; speichert einen Anker, wenn etwas
    /// hängenblieb (Python `reflect_and_store_anchor`). Hintergrund-Job, nicht
    /// per-Message. Liefert den neuen Anker oder None.
    pub async fn reflect_and_store_anchor(
        &self,
        minimax: &EngagementMinimaxClient,
    ) -> Option<String> {
        let rows = self.recent_convo(REFLECT_TURNS).await;
        if rows.len() < REFLECT_MIN_TURNS {
            return None;
        }
        // Der Bot war gar nicht aktiv → nichts zu erinnern.
        if !rows.iter().any(|(role, _, _)| role == "assistant") {
            return None;
        }
        let transcript = build_transcript(&rows);
        let raw = minimax
            .raw_completion(ANCHOR_SYS, &anchor_user_prompt(&transcript), 2000, 0.7)
            .await
            .ok()?;
        let last = self.last_anchor().await;
        let text = process_anchor_text(&raw, last.as_deref())?;
        self.store_soul_entry("anchor", &text).await.ok()?;
        tracing::info!(anchor = %text.chars().take(90).collect::<String>(), "SoulAnchor: neuer Anker gespeichert");
        Some(text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
    use std::str::FromStr;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

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

    /// Hero-Takes sind kuratierte Meinungen — genau das, was ein Neuling nicht
    /// hat. Im Rookie-Modus fallen sie weg, die Anker bleiben.
    #[test]
    fn rookie_fragment_laesst_hero_takes_weg() {
        let anchors = vec!["combo war nice".to_string()];
        let rookie = build_soul_fragment_for(Some("mag Haze"), &anchors, PersonaMode::Rookie);
        assert!(!rookie.contains("mag Haze"));
        assert!(!rookie.contains("Hero-Vorlieben"));
        assert!(rookie.contains("- combo war nice"));
        assert!(rookie.contains("kein Fachwissen"));

        // Nur Takes und Rookie-Modus: nichts bleibt übrig.
        assert_eq!(build_soul_fragment_for(Some("mag Haze"), &[], PersonaMode::Rookie), "");

        // Veteran bleibt unberührt.
        let veteran = build_soul_fragment_for(Some("mag Haze"), &anchors, PersonaMode::Veteran);
        assert!(veteran.contains("Deine Hero-Vorlieben:\nmag Haze"));
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
        sqlx::query(
            "CREATE TABLE twitch_engagement_conversation (\
             id BIGSERIAL PRIMARY KEY, channel_login TEXT, role TEXT, twitch_login TEXT, \
             content TEXT, ts TIMESTAMPTZ NOT NULL DEFAULT NOW())",
        )
        .execute(&pool)
        .await
        .unwrap();
        Some(pool)
    }

    #[test]
    fn anchor_text_filter() {
        // NICHTS → None
        assert_eq!(process_anchor_text("NICHTS", None), None);
        // Quotes + Whitespace weg
        assert_eq!(
            process_anchor_text("  \"der move war nice\"  ", None),
            Some("der move war nice".to_string())
        );
        // zu lang → None
        assert_eq!(process_anchor_text(&"a ".repeat(200), None), None);
        // <think> raus
        assert_eq!(
            process_anchor_text("<think>hmm</think> kurzer merker", None),
            Some("kurzer merker".to_string())
        );
        // identisch zum letzten Anker → None
        assert_eq!(process_anchor_text("gleicher anker", Some("Gleicher Anker")), None);
    }

    #[test]
    fn transcript_who_und_kuerzung() {
        let rows = vec![
            ("user".to_string(), Some("chatter".to_string()), "hi".to_string()),
            ("assistant".to_string(), None, "antwort".to_string()),
            ("user".to_string(), None, "ohne login".to_string()),
        ];
        let t = build_transcript(&rows);
        assert!(t.contains("chatter: hi"));
        assert!(t.contains("ich: antwort")); // assistant = ich
        assert!(t.contains("jemand: ohne login")); // kein login → jemand
    }

    #[tokio::test]
    async fn reflect_speichert_anker() {
        let Some(pool) = make_pool("t_eng_soul_reflect").await else { return };
        // 8 Turns inkl. einem Assistant-Turn.
        sqlx::query(
            "INSERT INTO twitch_engagement_conversation (channel_login, role, twitch_login, content) VALUES \
             ('nani','user','c','eins'),('nani','user','c','zwei'),('nani','user','c','drei'), \
             ('nani','assistant',NULL,'meine antwort'),('nani','user','c','fünf'), \
             ('nani','user','c','sechs'),('nani','user','c','sieben'),('nani','user','c','acht')",
        )
        .execute(&pool)
        .await
        .unwrap();

        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "choices": [{"message": {"content": "\"der dive war echt wild\""}}]
            })))
            .mount(&server)
            .await;
        let minimax = EngagementMinimaxClient::new(
            Some("k".to_string()),
            Some(server.uri()),
            Some("m".to_string()),
            None,
        );

        let store = SoulStore::new(pool.clone());
        let anchor = store.reflect_and_store_anchor(&minimax).await;
        assert_eq!(anchor.as_deref(), Some("der dive war echt wild")); // Quotes weg
        // Persistiert → taucht im Fragment auf.
        assert!(store.get_soul_extension_fragment().await.contains("der dive war echt wild"));
    }

    #[tokio::test]
    async fn reflect_zu_wenig_turns_none() {
        let Some(pool) = make_pool("t_eng_soul_reflect_few").await else { return };
        sqlx::query(
            "INSERT INTO twitch_engagement_conversation (channel_login, role, content) VALUES \
             ('nani','assistant','a'),('nani','user','b')",
        )
        .execute(&pool)
        .await
        .unwrap();
        let minimax = EngagementMinimaxClient::new(
            Some("k".to_string()),
            Some("http://127.0.0.1:1".to_string()),
            Some("m".to_string()),
            None,
        );
        // < 8 Turns → None, ohne MiniMax zu rufen.
        assert_eq!(SoulStore::new(pool).reflect_and_store_anchor(&minimax).await, None);
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
