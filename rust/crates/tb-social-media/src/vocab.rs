//! Deadlock-Vokabular für die Transkript-Korrektur (Port von
//! `bot/social_media/transcription/vocab.py`).
//!
//! CRUD über `deadlock_vocab` (term/canonical/category/source/aliases/weight).
//! `aliases` ist JSONB → via `::text` gelesen und `$N::jsonb` geschrieben.

use sqlx::PgPool;

/// Erlaubte Kategorien (deadlock_vocab_category_chk).
pub const ALLOWED_CATEGORIES: [&str; 4] = ["hero", "item", "ability", "slang"];
/// Erlaubte Quellen (deadlock_vocab_source_chk).
pub const ALLOWED_SOURCES: [&str; 2] = ["deadlock_api", "manual"];

/// Validierungs-/DB-Fehler beim Vokabular.
#[derive(Debug, thiserror::Error)]
pub enum VocabError {
    #[error("term is required")]
    TermRequired,
    #[error("canonical is required")]
    CanonicalRequired,
    #[error("invalid category: {0}")]
    InvalidCategory(String),
    #[error("invalid source: {0}")]
    InvalidSource(String),
    #[error("db: {0}")]
    Db(#[from] sqlx::Error),
}

/// Ein Vokabular-Eintrag.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VocabEntry {
    pub term: String,
    pub canonical: String,
    pub category: String,
    pub source: String,
    pub aliases: Vec<String>,
    pub weight: i32,
    pub updated_at: Option<String>,
}

fn normalize_term(term: &str) -> Result<String, VocabError> {
    let v = term.trim().to_lowercase();
    if v.is_empty() {
        return Err(VocabError::TermRequired);
    }
    Ok(v)
}

/// Trimmt + dedupliziert (case-insensitiv, Original-Casing behalten).
fn normalize_aliases(aliases: &[String]) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();
    for raw in aliases {
        let token = raw.trim();
        if token.is_empty() {
            continue;
        }
        if seen.insert(token.to_lowercase()) {
            out.push(token.to_string());
        }
    }
    out
}

fn validate_category(category: &str) -> Result<String, VocabError> {
    let v = category.trim().to_lowercase();
    if ALLOWED_CATEGORIES.contains(&v.as_str()) {
        Ok(v)
    } else {
        Err(VocabError::InvalidCategory(category.to_string()))
    }
}

fn validate_source(source: &str) -> Result<String, VocabError> {
    let v = {
        let t = source.trim().to_lowercase();
        if t.is_empty() { "manual".to_string() } else { t }
    };
    if ALLOWED_SOURCES.contains(&v.as_str()) {
        Ok(v)
    } else {
        Err(VocabError::InvalidSource(source.to_string()))
    }
}

/// JSON-Array-Text → Vec<String> (fehlertolerant wie Python `_decode_aliases`).
fn decode_aliases(raw: Option<&str>) -> Vec<String> {
    let Some(text) = raw.filter(|s| !s.is_empty()) else {
        return Vec::new();
    };
    match serde_json::from_str::<serde_json::Value>(text) {
        Ok(serde_json::Value::Array(a)) => a
            .iter()
            .filter_map(|v| match v {
                serde_json::Value::String(s) => Some(s.clone()),
                serde_json::Value::Null => None,
                other => Some(other.to_string()),
            })
            .collect(),
        _ => Vec::new(),
    }
}

type Row = (String, String, String, String, Option<String>, i32, Option<String>);

fn row_to_entry(r: Row) -> VocabEntry {
    VocabEntry {
        term: r.0,
        canonical: r.1,
        category: r.2,
        source: r.3,
        aliases: decode_aliases(r.4.as_deref()),
        weight: if r.5 < 1 { 1 } else { r.5 },
        updated_at: r.6,
    }
}

const SELECT_COLS: &str =
    "term, canonical, category, source, aliases::text, weight, updated_at::text";

/// Listet Einträge mit optionalem Filter + Pagination → (Einträge, Gesamtzahl).
pub async fn list_vocab(
    pool: &PgPool,
    category: Option<&str>,
    query: Option<&str>,
    limit: i64,
    offset: i64,
) -> (Vec<VocabEntry>, i64) {
    let limit = limit.clamp(1, 500);
    let offset = offset.max(0);
    let like = query.map(|q| format!("%{}%", q.to_lowercase()));

    let total: i64 = {
        let mut qb = sqlx::QueryBuilder::<sqlx::Postgres>::new("SELECT COUNT(*) FROM deadlock_vocab WHERE 1=1");
        if let Some(cat) = category {
            qb.push(" AND LOWER(category) = LOWER(").push_bind(cat).push(")");
        }
        if let Some(l) = &like {
            qb.push(" AND (LOWER(term) LIKE ").push_bind(l).push(" OR LOWER(canonical) LIKE ").push_bind(l).push(")");
        }
        qb.build_query_scalar().fetch_one(pool).await.unwrap_or(0)
    };

    let mut qb = sqlx::QueryBuilder::<sqlx::Postgres>::new(&format!("SELECT {SELECT_COLS} FROM deadlock_vocab WHERE 1=1"));
    if let Some(cat) = category {
        qb.push(" AND LOWER(category) = LOWER(").push_bind(cat).push(")");
    }
    if let Some(l) = &like {
        qb.push(" AND (LOWER(term) LIKE ").push_bind(l).push(" OR LOWER(canonical) LIKE ").push_bind(l).push(")");
    }
    qb.push(" ORDER BY weight DESC, canonical ASC LIMIT ").push_bind(limit).push(" OFFSET ").push_bind(offset);
    let rows: Vec<Row> = qb.build_query_as().fetch_all(pool).await.unwrap_or_default();
    (rows.into_iter().map(row_to_entry).collect(), total)
}

/// Einzelnen Eintrag nach (normalisiertem) Term.
pub async fn get_vocab_entry(pool: &PgPool, term: &str) -> Option<VocabEntry> {
    let normalized = normalize_term(term).ok()?;
    let row: Option<Row> = sqlx::query_as(&format!(
        "SELECT {SELECT_COLS} FROM deadlock_vocab WHERE term = $1 LIMIT 1"
    ))
    .bind(&normalized)
    .fetch_optional(pool)
    .await
    .ok()
    .flatten();
    row.map(row_to_entry)
}

/// Upsert eines Eintrags (validiert; Python `upsert_vocab_entry`).
#[allow(clippy::too_many_arguments)]
pub async fn upsert_vocab_entry(
    pool: &PgPool,
    term: &str,
    canonical: &str,
    category: &str,
    source: &str,
    aliases: &[String],
    weight: i32,
) -> Result<VocabEntry, VocabError> {
    let normalized_term = normalize_term(term)?;
    let canonical_value = canonical.trim().to_string();
    if canonical_value.is_empty() {
        return Err(VocabError::CanonicalRequired);
    }
    let cat = validate_category(category)?;
    let src = validate_source(source)?;
    let alias_list = normalize_aliases(aliases);
    let weight_value = weight.max(1);
    let aliases_json = serde_json::to_string(&alias_list).unwrap_or_else(|_| "[]".to_string());

    sqlx::query(
        "INSERT INTO deadlock_vocab (term, canonical, category, source, aliases, weight, updated_at) \
         VALUES ($1, $2, $3, $4, $5::jsonb, $6, CURRENT_TIMESTAMP) \
         ON CONFLICT (term) DO UPDATE SET \
             canonical = EXCLUDED.canonical, category = EXCLUDED.category, \
             source = EXCLUDED.source, aliases = EXCLUDED.aliases, \
             weight = EXCLUDED.weight, updated_at = CURRENT_TIMESTAMP",
    )
    .bind(&normalized_term)
    .bind(&canonical_value)
    .bind(&cat)
    .bind(&src)
    .bind(&aliases_json)
    .bind(weight_value)
    .execute(pool)
    .await?;

    Ok(get_vocab_entry(pool, &normalized_term).await.unwrap_or(VocabEntry {
        term: normalized_term,
        canonical: canonical_value,
        category: cat,
        source: src,
        aliases: alias_list,
        weight: weight_value,
        updated_at: None,
    }))
}

/// Bulk-Upsert → (geschrieben, übersprungen).
pub async fn bulk_upsert_vocab_entries(pool: &PgPool, entries: &[VocabEntry]) -> (usize, usize) {
    let (mut written, mut skipped) = (0, 0);
    for e in entries {
        match upsert_vocab_entry(pool, &e.term, &e.canonical, &e.category, &e.source, &e.aliases, e.weight).await {
            Ok(_) => written += 1,
            Err(error) => {
                tracing::warn!(term = %e.term, %error, "Vocab-Upsert fehlgeschlagen");
                skipped += 1;
            }
        }
    }
    (written, skipped)
}

/// Löscht einen Eintrag; `true` wenn etwas gelöscht wurde.
pub async fn delete_vocab_entry(pool: &PgPool, term: &str) -> Result<bool, VocabError> {
    let normalized = normalize_term(term)?;
    let result = sqlx::query("DELETE FROM deadlock_vocab WHERE term = $1")
        .bind(&normalized)
        .execute(pool)
        .await?;
    Ok(result.rows_affected() > 0)
}

/// Alle Einträge (sortiert weight DESC, canonical ASC). Fehler → leer (Python
/// `load_all_vocab_safe`).
pub async fn load_all_vocab(pool: &PgPool) -> Vec<VocabEntry> {
    let rows: Vec<Row> = sqlx::query_as(&format!(
        "SELECT {SELECT_COLS} FROM deadlock_vocab ORDER BY weight DESC, canonical ASC"
    ))
    .fetch_all(pool)
    .await
    .unwrap_or_default();
    rows.into_iter().map(row_to_entry).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
    use std::str::FromStr;

    #[test]
    fn validatoren_und_aliases() {
        assert!(normalize_term("  ").is_err());
        assert_eq!(normalize_term("  Haze ").unwrap(), "haze");
        assert!(validate_category("hero").is_ok());
        assert!(validate_category("quatsch").is_err());
        assert_eq!(validate_source("").unwrap(), "manual"); // leer → manual
        assert!(validate_source("deadlock_api").is_ok());
        assert!(validate_source("evil").is_err());
        // Dedup case-insensitiv, Original-Casing.
        let a = normalize_aliases(&["Haze".into(), " haze ".into(), "".into(), "Geist".into()]);
        assert_eq!(a, vec!["Haze".to_string(), "Geist".to_string()]);
        // Decode tolerant.
        assert_eq!(decode_aliases(Some("[\"a\",\"b\"]")), vec!["a", "b"]);
        assert_eq!(decode_aliases(Some("kaputt")), Vec::<String>::new());
        assert_eq!(decode_aliases(None), Vec::<String>::new());
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
            "CREATE TABLE deadlock_vocab (term TEXT PRIMARY KEY, canonical TEXT NOT NULL, \
             category TEXT NOT NULL, source TEXT NOT NULL DEFAULT 'manual', \
             aliases JSONB NOT NULL DEFAULT '[]'::JSONB, weight INTEGER NOT NULL DEFAULT 1, \
             updated_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP)",
        )
        .execute(&pool).await.unwrap();
        Some(pool)
    }

    #[tokio::test]
    async fn crud_roundtrip() {
        let Some(pool) = make_pool("t_sm_vocab").await else { return };
        // Upsert + get.
        let e = upsert_vocab_entry(&pool, "Haze", "Haze", "hero", "manual", &["Häze".into()], 5).await.unwrap();
        assert_eq!(e.term, "haze");
        assert_eq!(e.weight, 5);
        assert_eq!(e.aliases, vec!["Häze".to_string()]);
        let got = get_vocab_entry(&pool, "HAZE").await.unwrap();
        assert_eq!(got.canonical, "Haze");
        assert_eq!(got.aliases, vec!["Häze".to_string()]);

        // Upsert überschreibt.
        upsert_vocab_entry(&pool, "haze", "Haze (Hero)", "hero", "deadlock_api", &[], 9).await.unwrap();
        assert_eq!(get_vocab_entry(&pool, "haze").await.unwrap().canonical, "Haze (Hero)");

        // Zweiter Eintrag + list/filter.
        upsert_vocab_entry(&pool, "trophy", "Trophy Collector", "item", "manual", &[], 3).await.unwrap();
        let (all, total) = list_vocab(&pool, None, None, 200, 0).await;
        assert_eq!(total, 2);
        assert_eq!(all.len(), 2);
        assert_eq!(all[0].term, "haze"); // weight 9 > 3 → zuerst
        let (heroes, h_total) = list_vocab(&pool, Some("hero"), None, 200, 0).await;
        assert_eq!(h_total, 1);
        assert_eq!(heroes[0].term, "haze");
        let (search, _) = list_vocab(&pool, None, Some("trophy"), 200, 0).await;
        assert_eq!(search.len(), 1);

        // Ungültige Kategorie → Fehler, kein Insert.
        assert!(upsert_vocab_entry(&pool, "x", "X", "blah", "manual", &[], 1).await.is_err());

        // Delete.
        assert!(delete_vocab_entry(&pool, "trophy").await.unwrap());
        assert!(!delete_vocab_entry(&pool, "trophy").await.unwrap()); // schon weg
        assert_eq!(load_all_vocab(&pool).await.len(), 1);
    }
}
