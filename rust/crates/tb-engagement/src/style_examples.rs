//! Konkrete Stil-Beispiele aus echtem Channel-Chat (Few-Shot, Port von
//! `bot/engagement/style_examples.py`).
//!
//! Ergänzt [`crate::persona`]: persona *beschreibt* den Vibe statistisch, dieses
//! Modul *zeigt* dem Modell echte Nachrichten als Stilvorlage („show, don't
//! tell"). Gold-Register (EarlySalty) zuerst, dann channel-eigene Zeilen, dann
//! kuratierte Seeds — so ist der Block auch auf kaltem Channel nie leer. Der
//! Prompt trennt hart Stil von Inhalt: nur Schreibweise nachahmen, NIE die
//! Behauptungen/Spielfakten übernehmen (Fakten bleiben beim Wiki-Grounding).

use std::collections::{HashMap, HashSet};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use sqlx::PgPool;

const POOL_LIMIT: i64 = 120;
const MAX_EXAMPLES: usize = 8;
const GOLD_KEEP: usize = 4;
const MIN_LEN: usize = 8;
const MAX_LEN: usize = 100;
const MAX_SAME_STARTER: usize = 2;
const CACHE_TTL: Duration = Duration::from_secs(600);
/// Ab so vielen brauchbaren gelernten Zeilen ersetzen sie das feste Register.
const MIN_LEARNED_GOLD: usize = 4;
/// So viele gelernte Zeilen werden gezogen (vor der Qualitätsfilterung).
const LEARNED_POOL_LIMIT: i64 = 60;

/// Kuratierter Stil-Fallback (DE/EN gemischt, klein, Slang, kurz) für kalte Channels.
const SEED_EXAMPLES: &[&str] = &[
    "lol was war das für ein dive",
    "brudi warum gehst du da solo rein",
    "der flick war einfach nasty ngl",
    "ok der gap close ist kriminell",
    "no shot dass der das überlebt hat",
    "warum peelt da eigentlich keiner",
    "der hat einfach so locked in aimbot",
    "mach mal mehr seelen die minute",
    "early kills machen nix aus",
    "sheesh die combo war eklig",
    "läuft bei dir heut richtig gut",
    "yo that dive was actually nasty",
    "bro why go solo in there lol",
    "that last fight was so clean",
    "no way he survived that one",
    "the gap close is straight up crime",
    "man this lane is rough rn",
];

/// Gold-Standard (EarlySalty-Register) — IMMER zuerst, damit der Bot kurz/trocken bleibt.
///
/// Handverlesener Startwert. Sobald der Reaktions-Lernmodus genug echte eigene
/// Zeilen gesammelt hat, ersetzen die ihn (siehe [`load_learned_gold`]) — dann
/// steht im Prompt, was wirklich geschrieben wurde, statt was mal für typisch
/// gehalten wurde.
const GOLD_EXAMPLES: &[&str] = &[
    "wilder take",
    "haha legit",
    "alter bitte",
    "echt wild",
    "ngl stimmt",
    "das ist klassiker",
    "no shot den",
    "ich auch",
    "wieder geistig am start ne",
    "der findet das loch eh nicht",
    "der hätte dich da eig wegbügeln müssen",
    "außer du parrierst halt",
    "aber meta ist deutlich angenehmer grade",
    "und die haben noch 2 heal creeps lol",
];

/// Qualitätsfilter für eine Beispiel-Zeile (Länge, hat Leerzeichen, kein Command,
/// kein Link, kein CAPS-Spam).
fn is_good_example(text: &str) -> bool {
    let len = text.chars().count();
    if !(MIN_LEN..=MAX_LEN).contains(&len) {
        return false;
    }
    if !text.contains(' ') {
        return false;
    }
    if matches!(text.chars().next(), Some('!') | Some('/') | Some('.')) {
        return false;
    }
    let low = text.to_lowercase();
    if low.contains("http") || low.contains("www.") {
        return false;
    }
    let letters: Vec<char> = text.chars().filter(|c| c.is_alphabetic()).collect();
    if !letters.is_empty() {
        let caps = letters.iter().filter(|c| c.is_uppercase()).count();
        if caps as f64 / letters.len() as f64 > 0.6 {
            return false;
        }
    }
    true
}

fn normalize_ws(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn starter_of(text: &str) -> String {
    text.split_whitespace()
        .next()
        .map(|w| {
            w.to_lowercase()
                .trim_end_matches(['.', ',', '!', '?'])
                .to_string()
        })
        .unwrap_or_default()
}

/// Wählt repräsentative channel-eigene Beispiele: dedupliziert, max 2 mit
/// gleichem Starter-Wort, nur „gute" Zeilen, bis `max_n`.
fn select_examples(texts: &[String], max_n: usize) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    let mut starter_count: HashMap<String, usize> = HashMap::new();
    for raw in texts {
        let text = normalize_ws(raw);
        if !is_good_example(&text) {
            continue;
        }
        let key = text.to_lowercase();
        if seen.contains(&key) {
            continue;
        }
        let starter = starter_of(&text);
        if *starter_count.get(&starter).unwrap_or(&0) >= MAX_SAME_STARTER {
            continue;
        }
        seen.insert(key);
        *starter_count.entry(starter).or_insert(0) += 1;
        out.push(text);
        if out.len() >= max_n {
            break;
        }
    }
    out
}

/// Setzt die finale Beispiel-Liste zusammen: Gold (erste [`GOLD_KEEP`]) zuerst,
/// dann channel-eigene, dann Seeds — dedupliziert, bis [`MAX_EXAMPLES`].
///
/// Gold sind die gelernten eigenen Zeilen, sobald genug davon brauchbar sind
/// ([`MIN_LEARNED_GOLD`]); darunter bleibt es beim festen Register, weil eine
/// zu dünne Stichprobe den Ton nicht trägt.
fn assemble_examples_with_gold(channel_examples: &[String], learned: &[String]) -> Vec<String> {
    let usable: Vec<String> = learned
        .iter()
        .map(|s| normalize_ws(s))
        .filter(|s| is_good_example(s))
        .collect();
    let gold: Vec<String> = if usable.len() >= MIN_LEARNED_GOLD {
        usable.into_iter().take(GOLD_KEEP).collect()
    } else {
        GOLD_EXAMPLES
            .iter()
            .take(GOLD_KEEP)
            .map(|s| s.to_string())
            .collect()
    };
    let seed: Vec<String> = SEED_EXAMPLES.iter().map(|s| s.to_string()).collect();
    let sources: [&[String]; 3] = [&gold, channel_examples, &seed];

    let mut out: Vec<String> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    for source in sources {
        for raw in source {
            if out.len() >= MAX_EXAMPLES {
                break;
            }
            let cand = normalize_ws(raw);
            if !is_good_example(&cand) {
                continue;
            }
            let key = cand.to_lowercase();
            if seen.contains(&key) {
                continue;
            }
            seen.insert(key);
            out.push(cand);
        }
    }
    out
}

fn build_fragment(examples: &[String]) -> String {
    if examples.is_empty() {
        return String::new();
    }
    let lines = examples
        .iter()
        .map(|e| format!("- {e}"))
        .collect::<Vec<_>>()
        .join("\n");
    format!(
        "So schreiben echte Leute hier — kurz, trocken, mit Banter, oft nur ein paar Wörter. \
         Ahme NUR Schreibweise, Ton und Länge nach (Kleinschreibung/Slang wie üblich, knapp, \
         keine perfekte Grammatik). \
         Den INHALT dieser Beispiele und alle darin enthaltenen Behauptungen oder Spielfakten \
         IGNORIERST du komplett — sie sind reine Stilvorlage, keine Quelle:\n{lines}"
    )
}

/// Rahmt das destillierte Reaktionsprofil ein.
fn build_profile_fragment(profile: &str) -> String {
    format!(
        "Und so entscheidest du, OB du überhaupt schreibst. Das hier ist aus echten \
         beobachteten Reaktionen destilliert, nicht ausgedacht — halt dich daran, \
         besonders an das, worauf NICHT reagiert wird:\n{}",
        profile.trim()
    )
}

/// Few-Shot-Stilblock-Provider mit 10min-Cache.
pub struct StyleExamples {
    pool: PgPool,
    cache: Mutex<HashMap<String, (Instant, String)>>,
}

impl StyleExamples {
    pub fn new(pool: PgPool) -> Self {
        Self {
            pool,
            cache: Mutex::new(HashMap::new()),
        }
    }

    async fn load_user_turns(&self, channel_login: &str, limit: i64) -> Vec<String> {
        sqlx::query_scalar!(
            r#"SELECT content AS "content?" FROM twitch_engagement_conversation
             WHERE channel_login = $1 AND role = 'user'
             ORDER BY ts DESC LIMIT $2"#,
            channel_login,
            limit
        )
        .fetch_all(&self.pool)
        .await
        .unwrap_or_default()
        .into_iter()
        .flatten()
        .filter(|s| !s.is_empty())
        .collect()
    }

    /// Eigene Chat-Zeilen aus dem Reaktions-Lernmodus, jüngste zuerst.
    /// Als `bad` gesichtete Samples bleiben draußen.
    async fn load_learned_gold(&self, limit: i64) -> Vec<String> {
        sqlx::query_scalar!(
            r#"SELECT my_message AS "my_message!"
               FROM twitch_engagement_reaction_samples
               WHERE verdict IS NULL OR verdict <> 'bad'
               ORDER BY message_ts DESC LIMIT $1"#,
            limit
        )
        .fetch_all(&self.pool)
        .await
        .unwrap_or_default()
        .into_iter()
        .filter(|s| !s.is_empty())
        .collect()
    }

    /// Das destillierte Reaktionsprofil, falls eines vorliegt.
    async fn load_reaction_profile(&self) -> Option<String> {
        sqlx::query_scalar!(
            r#"SELECT content AS "content!" FROM twitch_engagement_soul
               WHERE kind = 'reaction_profile'
               ORDER BY created_at DESC LIMIT 1"#
        )
        .fetch_optional(&self.pool)
        .await
        .ok()
        .flatten()
        .filter(|s| !s.trim().is_empty())
    }

    /// Few-Shot-Stilblock pro Channel; 10min gecacht. Nie leer (Gold + Seeds).
    pub async fn build_style_fragment(&self, channel_login: &str) -> String {
        {
            let cache = self.cache.lock().unwrap_or_else(|p| p.into_inner());
            if let Some((at, frag)) = cache.get(channel_login) {
                if at.elapsed() < CACHE_TTL {
                    return frag.clone();
                }
            }
        }
        let texts = self.load_user_turns(channel_login, POOL_LIMIT).await;
        let learned = self.load_learned_gold(LEARNED_POOL_LIMIT).await;
        let examples =
            assemble_examples_with_gold(&select_examples(&texts, MAX_EXAMPLES), &learned);
        let mut fragment = build_fragment(&examples);
        if let Some(profile) = self.load_reaction_profile().await {
            fragment.push_str(&format!("\n\n{}", build_profile_fragment(&profile)));
        }
        {
            let mut cache = self.cache.lock().unwrap_or_else(|p| p.into_inner());
            cache.insert(
                channel_login.to_string(),
                (Instant::now(), fragment.clone()),
            );
        }
        fragment
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
    use std::str::FromStr;

    #[test]
    fn good_example_filter() {
        assert!(is_good_example("der dive war wild")); // ok
        assert!(!is_good_example("gg")); // zu kurz
        assert!(!is_good_example("einzelwort")); // kein Leerzeichen
        assert!(!is_good_example("!clip jetzt")); // Command
        assert!(!is_good_example("schau http://x.de an")); // Link
        assert!(!is_good_example("DAS IST ALLES CAPS")); // CAPS-Spam
    }

    #[test]
    fn select_starter_diversity_und_dedup() {
        let texts = vec![
            "der dive war wild".to_string(),
            "Der dive war wild".to_string(), // Dup (case)
            "der heal war clutch".to_string(),
            "der gap war krass".to_string(), // 3. "der" → raus (max 2)
            "wilder take echt".to_string(),
        ];
        let out = select_examples(&texts, 8);
        // 2x "der" + 1x "wilder" = 3.
        assert_eq!(out.len(), 3);
        assert_eq!(out.iter().filter(|t| t.starts_with("der")).count(), 2);
    }

    #[test]
    fn assemble_gold_zuerst_und_max() {
        let channel = vec!["der dive war komplett wild".to_string()];
        let out = assemble_examples_with_gold(&channel, &[]);
        assert_eq!(out.len(), MAX_EXAMPLES);
        // Erste GOLD_KEEP sind Gold-Zeilen.
        assert_eq!(out[0], "wilder take");
        // Channel-Zeile ist nach den Gold-Zeilen drin.
        assert!(out.contains(&"der dive war komplett wild".to_string()));
    }

    #[test]
    fn gelernte_zeilen_verdraengen_das_feste_register() {
        let channel = vec!["der dive war komplett wild".to_string()];
        let learned: Vec<String> = vec![
            "boah der hat gecampt".to_string(),
            "ne das war luck".to_string(),
            "warum baut der das".to_string(),
            "sowas hab ich nie".to_string(),
        ];
        let out = assemble_examples_with_gold(&channel, &learned);
        assert_eq!(out[0], "boah der hat gecampt", "gelernt steht vorn");
        assert!(
            !out.contains(&"wilder take".to_string()),
            "festes Gold ist raus"
        );
    }

    #[test]
    fn zu_wenige_gelernte_zeilen_lassen_das_register_stehen() {
        // Drei brauchbare Zeilen liegen unter MIN_LEARNED_GOLD.
        let learned: Vec<String> = vec![
            "boah der hat gecampt".into(),
            "ne das war luck".into(),
            "warum baut der das".into(),
        ];
        let out = assemble_examples_with_gold(&[], &learned);
        assert_eq!(out[0], "wilder take");
    }

    #[test]
    fn unbrauchbare_gelernte_zeilen_zaehlen_nicht_mit() {
        // 4 Zeilen, aber nur 2 überstehen den Qualitätsfilter (Command, Link).
        let learned: Vec<String> = vec![
            "boah der hat gecampt".into(),
            "!clip das eben".into(),
            "schau http://x.de an".into(),
            "ne das war luck".into(),
        ];
        let out = assemble_examples_with_gold(&[], &learned);
        assert_eq!(
            out[0], "wilder take",
            "unter der Schwelle bleibt das Register"
        );
    }

    #[test]
    fn profil_fragment_warnt_vor_dem_nicht_reagieren() {
        let frag = build_profile_fragment("  WORAUF: dives  ");
        assert!(frag.contains("WORAUF: dives"));
        assert!(frag.contains("worauf NICHT reagiert wird"));
        assert!(!frag.contains("  WORAUF"), "Whitespace ist getrimmt");
    }

    #[test]
    fn feste_beispiele_erfuellen_den_sanitizer_vertrag() {
        for example in GOLD_EXAMPLES.iter().chain(SEED_EXAMPLES) {
            assert_eq!(
                crate::llm_chat::sanitize_chat_text(example, 120).as_deref(),
                Some(*example),
                "ungueltiges Beispiel: {example}"
            );
        }
    }

    async fn make_pool(schema: &str) -> Option<PgPool> {
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
            "CREATE TABLE twitch_engagement_conversation (\
             id BIGSERIAL PRIMARY KEY, channel_login TEXT, role TEXT, content TEXT, \
             ts TIMESTAMPTZ NOT NULL DEFAULT NOW())",
        )
        .execute(&pool)
        .await
        .unwrap();
        Some(pool)
    }

    #[tokio::test]
    async fn style_fragment_enthaelt_gold_und_channel() {
        let Some(pool) = make_pool("t_eng_style").await else {
            return;
        };
        sqlx::query(
            "INSERT INTO twitch_engagement_conversation (channel_login, role, content) VALUES \
             ('nani','user','der dive war komplett wild heute')",
        )
        .execute(&pool)
        .await
        .unwrap();
        let style = StyleExamples::new(pool);
        let frag = style.build_style_fragment("nani").await;
        assert!(frag.contains("So schreiben echte Leute hier"));
        assert!(frag.contains("- wilder take")); // Gold
        assert!(frag.contains("der dive war komplett wild heute")); // Channel
    }
}
