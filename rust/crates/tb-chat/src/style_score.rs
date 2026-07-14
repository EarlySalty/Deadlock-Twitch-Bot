use std::collections::{HashMap, HashSet};
use std::sync::OnceLock;

use regex::Regex;
use serde::Serialize;
use sqlx::PgPool;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct StyleBreakdown {
    pub pitch: u8,
    pub campaign: u8,
    pub typo: u8,
    pub bro: u8,
    pub lowercase: u8,
    pub opener: u8,
    pub cosine: u8,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StyleScore {
    pub total: u8,
    pub breakdown: StyleBreakdown,
}

#[derive(Debug, Clone, Default)]
pub struct Centroid {
    vector: HashMap<String, f64>,
    idf: HashMap<String, f64>,
}

const TYPOS: &[&str] = &[
    "denn bot",
    "drinne",
    "dierekt",
    "compeditiv",
    "nolage",
    "cmmunity",
    "sozial media",
    "togehter",
    "aleider",
    "kan ding",
    "broski",
    "homeboy",
];

fn regex(slot: &'static OnceLock<Option<Regex>>, pattern: &str) -> Option<&'static Regex> {
    slot.get_or_init(|| Regex::new(pattern).ok()).as_ref()
}

fn ratio_points(matches: usize, count: usize, multiplier: f64, cap: u8) -> u8 {
    if count == 0 {
        return 0;
    }
    ((matches as f64 / count as f64) * multiplier).min(f64::from(cap)) as u8
}

pub fn score(messages: &[String], crew_centroid: &Centroid) -> StyleScore {
    static PITCH: OnceLock<Option<Regex>> = OnceLock::new();
    static PITCH_REVERSED: OnceLock<Option<Regex>> = OnceLock::new();
    static CAMPAIGN: OnceLock<Option<Regex>> = OnceLock::new();
    static BRO: OnceLock<Option<Regex>> = OnceLock::new();
    static OPENER: OnceLock<Option<Regex>> = OnceLock::new();

    if messages.is_empty() {
        return StyleScore {
            total: 0,
            breakdown: StyleBreakdown {
                pitch: 0,
                campaign: 0,
                typo: 0,
                bro: 0,
                lowercase: 0,
                opener: 0,
                cosine: 0,
            },
        };
    }

    let pitch_matches = regex(
        &PITCH,
        r"(?i)discord\.gg/|(\bdc\b|discord).{0,40}(bock|hast du|kennst|suchen|community|aufbau)",
    )
    .is_some_and(|matcher| messages.iter().any(|message| matcher.is_match(message)))
        || regex(
            &PITCH_REVERSED,
            r"(?i)(bock|hast du|kennst|suchen|community|aufbau).{0,40}(\bdc\b|discord)",
        )
        .is_some_and(|matcher| messages.iter().any(|message| matcher.is_match(message)));
    let pitch = if pitch_matches { 40 } else { 0 };
    let campaign_matches = regex(
        &CAMPAIGN,
        r"(?i)helmbomben|bann?liste|(bot).{0,20}(nani)|warum.{0,20}(gebannt|gebant)",
    )
    .is_some_and(|matcher| messages.iter().any(|message| matcher.is_match(message)));
    let campaign = if campaign_matches { 30 } else { 0 };
    let typo_matches = messages
        .iter()
        .filter(|message| {
            let lower = message.to_lowercase();
            TYPOS.iter().any(|typo| lower.contains(typo))
        })
        .count();
    let typo = ratio_points(typo_matches, messages.len(), 200.0, 20);
    let bro_matches = regex(&BRO, r"(?i)\bbro(s|ski)?\b")
        .map(|matcher| {
            messages
                .iter()
                .filter(|message| matcher.is_match(message))
                .count()
        })
        .unwrap_or(0);
    let bro = ratio_points(bro_matches, messages.len(), 60.0, 10);
    let lowercase_matches = messages
        .iter()
        .filter(|message| message.chars().next().is_some_and(char::is_lowercase))
        .count();
    let lowercase = if (lowercase_matches as f64 / messages.len() as f64) > 0.75 {
        8
    } else {
        0
    };
    let opener_matches = regex(&OPENER, r"(?i)^was geht\b")
        .is_some_and(|matcher| messages.iter().any(|message| matcher.is_match(message)));
    let opener = if opener_matches { 5 } else { 0 };
    let cosine = (160.0 * crew_centroid.similarity(messages)).clamp(0.0, 25.0) as u8;

    let breakdown = StyleBreakdown {
        pitch,
        campaign,
        typo,
        bro,
        lowercase,
        opener,
        cosine,
    };
    let total = [pitch, campaign, typo, bro, lowercase, opener, cosine]
        .into_iter()
        .fold(0u8, u8::saturating_add)
        .min(100);
    StyleScore { total, breakdown }
}

impl Centroid {
    #[cfg(test)]
    fn from_documents(documents: &[Vec<String>]) -> Self {
        Self::from_corpora(documents, documents)
    }

    fn from_corpora(idf_documents: &[Vec<String>], crew_documents: &[Vec<String>]) -> Self {
        let document_count = idf_documents.len() as f64;
        if document_count == 0.0 || crew_documents.is_empty() {
            return Self::default();
        }

        let mut document_frequency: HashMap<String, usize> = HashMap::new();
        for document in idf_documents {
            for gram in trigrams(document).into_keys().collect::<HashSet<_>>() {
                *document_frequency.entry(gram).or_default() += 1;
            }
        }
        let idf = document_frequency
            .into_iter()
            .map(|(gram, frequency)| (gram, (document_count / (1.0 + frequency as f64)).ln()))
            .collect::<HashMap<_, _>>();

        let mut vector: HashMap<String, f64> = HashMap::new();
        for document in crew_documents {
            for (gram, weight) in normalized_vector(document, &idf) {
                *vector.entry(gram).or_default() += weight;
            }
        }
        normalize(&mut vector);
        Self { vector, idf }
    }

    fn similarity(&self, messages: &[String]) -> f64 {
        normalized_vector(messages, &self.idf)
            .into_iter()
            .map(|(gram, weight)| self.vector.get(&gram).copied().unwrap_or(0.0) * weight)
            .sum::<f64>()
            .clamp(0.0, 1.0)
    }
}

fn trigrams(messages: &[String]) -> HashMap<String, usize> {
    let padded = format!(" {} ", messages.join(" ").to_lowercase());
    let chars = padded.chars().collect::<Vec<_>>();
    let mut counts = HashMap::new();
    for window in chars.windows(3) {
        *counts.entry(window.iter().collect()).or_default() += 1;
    }
    counts
}

fn normalized_vector(messages: &[String], idf: &HashMap<String, f64>) -> HashMap<String, f64> {
    let mut vector = trigrams(messages)
        .into_iter()
        .filter_map(|(gram, count)| {
            idf.get(&gram)
                .map(|idf| (gram, (1.0 + (count as f64).ln()) * idf))
        })
        .collect::<HashMap<_, _>>();
    normalize(&mut vector);
    vector
}

fn normalize(vector: &mut HashMap<String, f64>) {
    let norm = vector
        .values()
        .map(|weight| weight * weight)
        .sum::<f64>()
        .sqrt();
    if norm > 0.0 {
        for weight in vector.values_mut() {
            *weight /= norm;
        }
    }
}

pub async fn build_centroid(pool: &PgPool, crew_logins: &[&str]) -> Result<Centroid, sqlx::Error> {
    let idf_documents = sqlx::query_as::<_, (String, Vec<String>)>(
        "SELECT lower(chatter_login), array_agg(content ORDER BY message_ts) \
         FROM twitch_chat_messages \
         WHERE chatter_login IS NOT NULL AND content IS NOT NULL \
         GROUP BY lower(chatter_login) HAVING count(*) >= 5",
    )
    .fetch_all(pool)
    .await?
    .into_iter()
    .map(|(_, messages)| messages)
    .collect::<Vec<_>>();
    let crew_logins = crew_logins
        .iter()
        .map(|login| login.to_lowercase())
        .collect::<Vec<_>>();
    let crew_documents = sqlx::query_as::<_, (String, Vec<String>)>(
        "SELECT lower(chatter_login), array_agg(content ORDER BY message_ts) \
         FROM twitch_chat_messages \
         WHERE lower(chatter_login) = ANY($1) AND content IS NOT NULL \
         GROUP BY lower(chatter_login)",
    )
    .bind(crew_logins)
    .fetch_all(pool)
    .await?
    .into_iter()
    .map(|(_, messages)| messages)
    .collect::<Vec<_>>();
    Ok(Centroid::from_corpora(&idf_documents, &crew_documents))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn messages(lines: &[&str]) -> Vec<String> {
        lines.iter().map(|line| (*line).to_string()).collect()
    }

    fn centroid(lines: &[&str]) -> Centroid {
        let crew = messages(lines);
        let mut idf_documents = vec![crew.clone(), messages(NORMAL)];
        idf_documents.extend([
            messages(&["123 orange violet telescope"]),
            messages(&["456 river mountain bicycle"]),
            messages(&["789 piano window calendar"]),
            messages(&["012 forest lantern kitchen"]),
            messages(&["345 winter coffee airplane"]),
            messages(&["678 garden blanket thunder"]),
            messages(&["901 marble ocean notebook"]),
            messages(&["234 yellow station compass"]),
        ]);
        Centroid::from_corpora(&idf_documents, &[crew])
    }

    const RICKY: &[&str] = &[
        "was geht",
        "hast du schon nen guten dc für deadlock?",
        "willst du auf nen dc wo die leute das spiel ernst nehmen?",
        "ist halt noch am anfang der dc sind jetzt 20 leute nach 3 tagen den der server online ist. haben auch schon sozial media gemacht für den dc",
        "hätte dir dierekt knockdown auf n schädel gehauen",
        "sind im aufbau mit bisschen höheren wehr auf gutes gameplay bro",
        "Sind einfach gesagt nen compeditiv dc",
        "vermitteln viel game nolage usw.",
        "haben auch schon gut viele steamer die auch gerne stream togehter machen",
    ];

    const NORMAL: &[&str] = &[
        "Was geht",
        "Was für nen main hat du",
        "Ich bin champ Magik haha",
        "Entspannt ja ich bin auch kurz vor cele",
        "Ne ich spiel noch white fox,DD,Wanda",
    ];

    #[test]
    fn ricky_korpus_scored_hundert() {
        let result = score(&messages(RICKY), &centroid(RICKY));
        assert_eq!(result.total, 100, "{result:?}");
    }

    #[test]
    fn normaler_chatter_scored_niedrig() {
        let result = score(&messages(NORMAL), &centroid(RICKY));
        assert!(result.total <= 30, "{result:?}");
    }

    #[test]
    fn leere_nachrichten_scored_null() {
        assert_eq!(score(&[], &Centroid::default()).total, 0);
    }

    #[test]
    fn pitch_feature_scored_exakt() {
        assert_eq!(
            score(
                &messages(&["Discord kennst du ne Community?"]),
                &Centroid::default()
            )
            .breakdown
            .pitch,
            40
        );
    }

    #[test]
    fn campaign_feature_scored_exakt() {
        assert_eq!(
            score(&messages(&["warum wurde er gebant?"]), &Centroid::default())
                .breakdown
                .campaign,
            30
        );
    }

    #[test]
    fn typo_feature_scored_exakt() {
        assert_eq!(
            score(&messages(&["Das ist dierekt gut"]), &Centroid::default())
                .breakdown
                .typo,
            20
        );
    }

    #[test]
    fn bro_feature_scored_exakt() {
        assert_eq!(
            score(&messages(&["Okay Bro"]), &Centroid::default())
                .breakdown
                .bro,
            10
        );
    }

    #[test]
    fn lowercase_feature_braucht_mehr_als_75_prozent() {
        let result = score(&messages(&["a", "b", "c", "d", "E"]), &Centroid::default());
        assert_eq!(result.breakdown.lowercase, 8);
        assert_eq!(
            score(&messages(&["a", "b", "c", "D"]), &Centroid::default())
                .breakdown
                .lowercase,
            0
        );
    }

    #[test]
    fn opener_feature_scored_exakt() {
        assert_eq!(
            score(&messages(&["Was geht zusammen"]), &Centroid::default())
                .breakdown
                .opener,
            5
        );
    }

    #[test]
    fn cosine_feature_scored_exakt() {
        let corpus = messages(&["123 alpha beta gamma delta"]);
        let centroid = Centroid::from_documents(std::slice::from_ref(&corpus));
        assert_eq!(score(&corpus, &centroid).breakdown.cosine, 25);
    }

    #[test]
    fn score_ist_deterministisch() {
        let corpus = messages(RICKY);
        let centroid = centroid(RICKY);
        assert_eq!(score(&corpus, &centroid), score(&corpus, &centroid));
    }
}
