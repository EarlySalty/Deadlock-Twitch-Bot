//! Adaptive Channel-Vibe-Sampling (Port von `bot/engagement/persona.py`).
//!
//! Sampelt die letzten ~50 User-Turns aus `twitch_engagement_conversation`,
//! errechnet Sprache (de/en heuristisch), Emoji-Dichte, Caps-Anteil, mittlere
//! Länge und Twitch-Slang — liefert einen Prompt-Baustein. Pro Channel 5min
//! in-memory gecacht.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use regex::Regex;
use sqlx::PgPool;

const GERMAN_MARKERS: &[&str] = &[
    "der", "die", "das", "und", "ist", "nicht", "auch", "auf", "mit", "für", "fuer", "ein",
    "eine", "den", "im", "haben", "sind", "war", "wie", "noch", "schon", "hat", "wird", "halt",
    "ne", "geh", "gleich", "ja", "nein", "aber", "doch", "echt", "krass", "sehr", "mehr", "wenn",
];

const ENGLISH_MARKERS: &[&str] = &[
    "the", "and", "is", "you", "what", "this", "that", "with", "have", "for", "are", "your",
    "they", "from", "just", "like", "but", "out", "yeah", "nah", "now", "really", "more", "if",
    "when",
];

const TWITCH_SLANG: &[&str] = &[
    "kekw", "pog", "pogchamp", "lul", "omegalul", "kappa", "monkas", "jebaited", "sadge", "copium",
    "ratjam", "peped", "5head", "ezclap", "kekwait", "yepw", "pepega", "pepehands", "nyaa",
    "okayeg",
];

const CACHE_TTL: Duration = Duration::from_secs(300);

fn emoji_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"[\x{1F300}-\x{1FAFF}\x{1F600}-\x{1F64F}\x{1F900}-\x{1F9FF}\x{2600}-\x{27BF}]")
            .expect("static regex")
    })
}

fn word_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"[a-zA-ZäöüÄÖÜß]+").expect("static regex"))
}

/// Ergebnis des Vibe-Samplings.
#[derive(Debug, Clone, PartialEq)]
pub struct PersonaSnapshot {
    pub language: String, // 'de' | 'en' | 'mixed'
    pub avg_length_chars: i64,
    pub emoji_density: f64,
    pub caps_ratio: f64,
    pub slang_terms: Vec<String>,
    pub sample_count: i64,
}

impl PersonaSnapshot {
    /// Prompt-Baustein, den die Pipeline an den System-Prompt anhängt.
    pub fn to_prompt_fragment(&self) -> String {
        if self.sample_count == 0 {
            return "Channel-Vibe: noch keine Daten — antworte freundlich-kurz, 1-2 Sätze."
                .to_string();
        }
        let lang_name = match self.language.as_str() {
            "de" => "deutsch",
            "en" => "englisch",
            _ => "deutsch/englisch gemischt",
        };
        let mut bits = vec![format!("dominant {lang_name}")];

        if self.avg_length_chars <= 25 {
            bits.push("sehr kurze Sätze".to_string());
        } else if self.avg_length_chars <= 60 {
            bits.push("mittlere Satzlänge".to_string());
        } else {
            bits.push("längere Sätze".to_string());
        }

        if self.emoji_density >= 0.5 {
            bits.push("hohe Emoji-Dichte".to_string());
        } else if self.emoji_density >= 0.15 {
            bits.push("vereinzelte Emojis".to_string());
        } else {
            bits.push("kaum Emojis".to_string());
        }

        if self.caps_ratio >= 0.35 {
            bits.push("oft GROSSGESCHRIEBEN".to_string());
        }

        if !self.slang_terms.is_empty() {
            let top = self
                .slang_terms
                .iter()
                .take(4)
                .cloned()
                .collect::<Vec<_>>()
                .join(", ");
            bits.push(format!("Twitch-Slang vorhanden ({top})"));
        }

        format!(
            "Channel-Vibe: {}. Spiegele diesen Stil, ohne ihn zu karikieren. \
             Antworten 1-2 Sätze, niemals länger.",
            bits.join(", ")
        )
    }
}

/// Berechnet den [`PersonaSnapshot`] aus den User-Turn-Texten (reiner Port von
/// `_compute`).
pub fn compute(texts: &[String]) -> PersonaSnapshot {
    if texts.is_empty() {
        return PersonaSnapshot {
            language: "mixed".to_string(),
            avg_length_chars: 0,
            emoji_density: 0.0,
            caps_ratio: 0.0,
            slang_terms: Vec::new(),
            sample_count: 0,
        };
    }

    let (mut de_hits, mut en_hits) = (0i64, 0i64);
    let (mut total_letters, mut total_caps) = (0i64, 0i64);
    let (mut total_emojis, mut total_length) = (0i64, 0i64);
    let mut slang_counts: HashMap<String, i64> = HashMap::new();

    for text in texts {
        total_length += text.chars().count() as i64;
        for m in word_re().find_iter(text) {
            let w = m.as_str();
            let wl = w.to_lowercase();
            if GERMAN_MARKERS.contains(&wl.as_str()) {
                de_hits += 1;
            } else if ENGLISH_MARKERS.contains(&wl.as_str()) {
                en_hits += 1;
            }
            if TWITCH_SLANG.contains(&wl.as_str()) {
                *slang_counts.entry(wl).or_insert(0) += 1;
            }
            for c in w.chars() {
                if c.is_alphabetic() {
                    total_letters += 1;
                    if c.is_uppercase() {
                        total_caps += 1;
                    }
                }
            }
        }
        total_emojis += emoji_re().find_iter(text).count() as i64;
    }

    let n = texts.len() as i64;
    let language = if de_hits >= en_hits * 2 && de_hits > 0 {
        "de"
    } else if en_hits >= de_hits * 2 && en_hits > 0 {
        "en"
    } else {
        "mixed"
    };

    let avg_len = total_length / n;
    let emoji_density = total_emojis as f64 / n as f64;
    let caps_ratio = if total_letters > 0 {
        total_caps as f64 / total_letters as f64
    } else {
        0.0
    };
    let mut slang_vec: Vec<(String, i64)> = slang_counts.into_iter().collect();
    slang_vec.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    let slang_terms: Vec<String> = slang_vec.into_iter().take(4).map(|(w, _)| w).collect();

    PersonaSnapshot {
        language: language.to_string(),
        avg_length_chars: avg_len,
        emoji_density,
        caps_ratio,
        slang_terms,
        sample_count: n,
    }
}

/// Channel-Vibe-Provider mit 5min-Cache.
pub struct Persona {
    pool: PgPool,
    cache: Mutex<HashMap<String, (Instant, PersonaSnapshot)>>,
}

impl Persona {
    pub fn new(pool: PgPool) -> Self {
        Self { pool, cache: Mutex::new(HashMap::new()) }
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

    /// [`PersonaSnapshot`] für einen Channel; 5min gecacht.
    pub async fn sample_tone(&self, channel_login: &str, limit: i64) -> PersonaSnapshot {
        {
            let cache = self.cache.lock().unwrap_or_else(|p| p.into_inner());
            if let Some((at, snap)) = cache.get(channel_login) {
                if at.elapsed() < CACHE_TTL {
                    return snap.clone();
                }
            }
        }
        let texts = self.load_user_turns(channel_login, limit).await;
        let snapshot = compute(&texts);
        {
            let mut cache = self.cache.lock().unwrap_or_else(|p| p.into_inner());
            cache.insert(channel_login.to_string(), (Instant::now(), snapshot.clone()));
        }
        snapshot
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
    use std::str::FromStr;

    #[test]
    fn compute_deutsch_caps_slang() {
        let texts = vec![
            "DAS IST KRASS und gut".to_string(), // viele Caps, deutsch
            "ja echt nicht schlecht kekw".to_string(),
            "kekw pog".to_string(),
        ];
        let snap = compute(&texts);
        assert_eq!(snap.language, "de");
        assert_eq!(snap.sample_count, 3);
        assert!(snap.caps_ratio > 0.0);
        assert!(snap.slang_terms.contains(&"kekw".to_string())); // 2x → vorne
        assert_eq!(snap.slang_terms.first().map(String::as_str), Some("kekw"));
    }

    #[test]
    fn compute_leer_und_fragment() {
        let empty = compute(&[]);
        assert_eq!(empty.sample_count, 0);
        assert!(empty.to_prompt_fragment().contains("noch keine Daten"));

        let snap = PersonaSnapshot {
            language: "en".to_string(),
            avg_length_chars: 80,
            emoji_density: 0.6,
            caps_ratio: 0.4,
            slang_terms: vec!["pog".to_string()],
            sample_count: 5,
        };
        let frag = snap.to_prompt_fragment();
        assert!(frag.contains("dominant englisch"));
        assert!(frag.contains("längere Sätze"));
        assert!(frag.contains("hohe Emoji-Dichte"));
        assert!(frag.contains("oft GROSSGESCHRIEBEN"));
        assert!(frag.contains("Twitch-Slang vorhanden (pog)"));
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
    async fn sample_tone_aus_db() {
        let Some(pool) = make_pool("t_eng_persona").await else { return };
        sqlx::query(
            "INSERT INTO twitch_engagement_conversation (channel_login, role, content) VALUES \
             ('nani','user','das ist echt krass und gut'), \
             ('nani','user','ja nicht schlecht'), \
             ('nani','assistant','soll ignoriert werden'), \
             ('other','user','the quick brown fox')",
        )
        .execute(&pool)
        .await
        .unwrap();
        let persona = Persona::new(pool);
        let snap = persona.sample_tone("nani", 50).await;
        // Nur die 2 User-Turns von 'nani' (Assistant + anderer Channel ignoriert).
        assert_eq!(snap.sample_count, 2);
        assert_eq!(snap.language, "de");
    }
}
