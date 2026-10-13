use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use sqlx::{PgPool, Postgres, Row, Transaction};

use super::skills::{skill_snapshot, SkillId};

const MAX_TEXT_FIELD: usize = 1000;
const MAX_ACTOR: usize = 200;
const MAX_PROPOSALS: i64 = 50;

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum SkillRegistryError {
    #[error("Die Skill-Änderung ist ungültig.")]
    Invalid,
    #[error("Die aktive Skill-Version hat sich geändert.")]
    Conflict,
    #[error("Die Skill-Version wurde nicht gefunden.")]
    NotFound,
    #[error("Die Skill-Daten konnten nicht gelesen oder gespeichert werden.")]
    Storage,
}

impl From<sqlx::Error> for SkillRegistryError {
    fn from(_: sqlx::Error) -> Self {
        Self::Storage
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActiveSkill {
    pub id: SkillId,
    pub version: i32,
    pub sha256: String,
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SkillEvidence {
    pub summary: String,
    pub sample_count: Option<i64>,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DiffKind {
    Added,
    Removed,
    Context,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct DiffLine {
    pub kind: DiffKind,
    pub text: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct SkillVersionView {
    pub version: String,
    pub content_hash: Option<String>,
    pub previous_version: Option<String>,
    pub changed_at: String,
    pub summary: String,
    pub reason: String,
    pub evidence: Option<SkillEvidence>,
    pub diff: Vec<DiffLine>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SkillHistory {
    pub skill_id: String,
    pub name: String,
    pub current_version: String,
    pub versions: Vec<SkillVersionView>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ChangeProposal {
    pub id: String,
    pub title: String,
    pub created_at: String,
    pub summary: String,
    pub reason: String,
    pub status: String,
    pub evidence: Option<String>,
}

#[derive(Debug, Clone)]
pub struct LearnedChange<'a> {
    pub text: &'a str,
    pub summary: &'a str,
    pub reason: &'a str,
    pub evidence: Option<&'a SkillEvidence>,
    pub actor: &'a str,
    pub max_bytes: usize,
}

#[derive(Debug, Clone)]
pub struct NewProposal<'a> {
    pub title: &'a str,
    pub summary: &'a str,
    pub reason: &'a str,
    pub evidence: Option<&'a str>,
}

impl SkillId {
    pub const ALL: [SkillId; 8] = [
        SkillId::DeadlockUnderstanding,
        SkillId::HighlightAnalysis,
        SkillId::EditingStrategy,
        SkillId::VideoLayout,
        SkillId::Subtitles,
        SkillId::Copywriting,
        SkillId::QualityValidation,
        SkillId::FeedbackLearning,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            SkillId::DeadlockUnderstanding => "deadlock_understanding",
            SkillId::HighlightAnalysis => "highlight_analysis",
            SkillId::EditingStrategy => "editing_strategy",
            SkillId::VideoLayout => "video_layout",
            SkillId::Subtitles => "subtitles",
            SkillId::Copywriting => "copywriting",
            SkillId::QualityValidation => "quality_validation",
            SkillId::FeedbackLearning => "feedback_learning",
        }
    }

    pub fn parse(value: &str) -> Option<SkillId> {
        SkillId::ALL.into_iter().find(|id| id.as_str() == value)
    }

    pub fn display_name(self) -> &'static str {
        match self {
            SkillId::DeadlockUnderstanding => "Deadlock-Verständnis",
            SkillId::HighlightAnalysis => "Highlight-Analyse",
            SkillId::EditingStrategy => "Schnittstrategie",
            SkillId::VideoLayout => "Videolayout",
            SkillId::Subtitles => "Untertitel",
            SkillId::Copywriting => "Texte",
            SkillId::QualityValidation => "Qualitätsprüfung",
            SkillId::FeedbackLearning => "Lernen aus Feedback",
        }
    }
}

pub fn sha256_hex(text: &str) -> String {
    Sha256::digest(text.as_bytes())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn bundled(id: SkillId) -> ActiveSkill {
    let snapshot = skill_snapshot(id);
    ActiveSkill {
        id,
        version: 1,
        sha256: snapshot.sha256,
        text: snapshot.text.to_owned(),
    }
}

fn title_prefix(text: &str) -> Option<&str> {
    let heading = text.lines().next()?.strip_prefix("# ")?;
    let title = heading
        .rsplit_once(" v")
        .filter(|(_, version)| !version.is_empty() && version.chars().all(|c| c.is_ascii_digit()))
        .map_or(heading, |(title, _)| title);
    (!title.trim().is_empty()).then_some(title)
}

pub fn validate_candidate(
    id: SkillId,
    text: &str,
    max_bytes: usize,
) -> Result<(), SkillRegistryError> {
    let base = skill_snapshot(id).text;
    let valid = !text.trim().is_empty()
        && text.len() <= max_bytes
        && !text
            .chars()
            .any(|c| c.is_control() && c != '\n' && c != '\t')
        && title_prefix(text).is_some()
        && title_prefix(text) == title_prefix(base);
    if valid {
        Ok(())
    } else {
        Err(SkillRegistryError::Invalid)
    }
}

fn bounded(value: &str, max: usize, allow_empty: bool) -> Result<String, SkillRegistryError> {
    let trimmed = value.trim();
    if (!allow_empty && trimmed.is_empty())
        || trimmed.chars().count() > max
        || trimmed.chars().any(|c| c.is_control() && c != '\n')
    {
        return Err(SkillRegistryError::Invalid);
    }
    Ok(trimmed.to_owned())
}

pub async fn active_skill(
    pool: &PgPool,
    id: SkillId,
    max_bytes: usize,
) -> Result<ActiveSkill, SkillRegistryError> {
    let row = sqlx::query(
        "SELECT v.version, v.sha256, v.content FROM public.social_ai_skill_active a JOIN public.social_ai_skill_versions v ON v.skill_id = a.skill_id AND v.version = a.version WHERE a.skill_id = $1",
    )
    .bind(id.as_str())
    .fetch_optional(pool)
    .await?;
    let Some(row) = row else {
        return Ok(bundled(id));
    };
    let version: i32 = row.try_get("version")?;
    let sha256: String = row.try_get("sha256")?;
    let text: String = row.try_get("content")?;
    if sha256 != sha256_hex(&text) || validate_candidate(id, &text, max_bytes).is_err() {
        tracing::warn!(
            skill = id.as_str(),
            version,
            "Gespeicherte Skill-Version ungültig, gebündelte Version aktiv"
        );
        return Ok(bundled(id));
    }
    Ok(ActiveSkill {
        id,
        version,
        sha256,
        text,
    })
}

async fn lock_skill(
    tx: &mut Transaction<'_, Postgres>,
    id: SkillId,
) -> Result<(), SkillRegistryError> {
    sqlx::query("SELECT pg_advisory_xact_lock(hashtext('social_ai_skill:' || $1))")
        .bind(id.as_str())
        .execute(&mut **tx)
        .await?;
    Ok(())
}

async fn seed_baseline(
    tx: &mut Transaction<'_, Postgres>,
    id: SkillId,
) -> Result<(), SkillRegistryError> {
    let base = bundled(id);
    sqlx::query(
        "INSERT INTO public.social_ai_skill_versions (skill_id, version, previous_version, content, sha256, origin, summary, reason, created_by) VALUES ($1, 1, NULL, $2, $3, 'bundled', 'Mitgelieferte Ausgangsversion', 'Ausgangsstand', 'system') ON CONFLICT DO NOTHING",
    )
    .bind(id.as_str())
    .bind(&base.text)
    .bind(&base.sha256)
    .execute(&mut **tx)
    .await?;
    sqlx::query(
        "INSERT INTO public.social_ai_skill_active (skill_id, version, updated_by) VALUES ($1, 1, 'system') ON CONFLICT DO NOTHING",
    )
    .bind(id.as_str())
    .execute(&mut **tx)
    .await?;
    Ok(())
}

async fn current_version(
    tx: &mut Transaction<'_, Postgres>,
    id: SkillId,
) -> Result<i32, SkillRegistryError> {
    let version: Option<i32> =
        sqlx::query_scalar("SELECT version FROM public.social_ai_skill_active WHERE skill_id = $1")
            .bind(id.as_str())
            .fetch_optional(&mut **tx)
            .await?;
    Ok(version.unwrap_or(1))
}

async fn record_activation(
    tx: &mut Transaction<'_, Postgres>,
    id: SkillId,
    from: i32,
    to: i32,
    kind: &str,
    actor: &str,
    reason: &str,
) -> Result<(), SkillRegistryError> {
    sqlx::query("UPDATE public.social_ai_skill_active SET version = $2, updated_at = now(), updated_by = $3 WHERE skill_id = $1")
        .bind(id.as_str())
        .bind(to)
        .bind(actor)
        .execute(&mut **tx)
        .await?;
    sqlx::query("INSERT INTO public.social_ai_skill_activations (skill_id, from_version, to_version, kind, actor, reason) VALUES ($1, $2, $3, $4, $5, $6)")
        .bind(id.as_str())
        .bind(from)
        .bind(to)
        .bind(kind)
        .bind(actor)
        .bind(reason)
        .execute(&mut **tx)
        .await?;
    Ok(())
}

pub async fn activate_learned(
    pool: &PgPool,
    id: SkillId,
    change: LearnedChange<'_>,
) -> Result<i32, SkillRegistryError> {
    validate_candidate(id, change.text, change.max_bytes)?;
    let summary = bounded(change.summary, MAX_TEXT_FIELD, false)?;
    let reason = bounded(change.reason, MAX_TEXT_FIELD, false)?;
    let actor = bounded(change.actor, MAX_ACTOR, false)?;
    let evidence = match change.evidence {
        Some(evidence) => Some(serde_json::json!({
            "summary": bounded(&evidence.summary, MAX_TEXT_FIELD, false)?,
            "sample_count": evidence.sample_count.filter(|count| *count >= 0),
        })),
        None => None,
    };
    let mut tx = pool.begin().await?;
    lock_skill(&mut tx, id).await?;
    seed_baseline(&mut tx, id).await?;
    let current = current_version(&mut tx, id).await?;
    let sha256 = sha256_hex(change.text);
    let active_sha: String = sqlx::query_scalar(
        "SELECT sha256 FROM public.social_ai_skill_versions WHERE skill_id = $1 AND version = $2",
    )
    .bind(id.as_str())
    .bind(current)
    .fetch_one(&mut *tx)
    .await?;
    if active_sha == sha256 {
        return Err(SkillRegistryError::Invalid);
    }
    let next: i32 = sqlx::query_scalar("SELECT COALESCE(MAX(version), 0) + 1 FROM public.social_ai_skill_versions WHERE skill_id = $1")
        .bind(id.as_str())
        .fetch_one(&mut *tx)
        .await?;
    sqlx::query(
        "INSERT INTO public.social_ai_skill_versions (skill_id, version, previous_version, content, sha256, origin, summary, reason, evidence, created_by) VALUES ($1, $2, $3, $4, $5, 'learned', $6, $7, $8, $9)",
    )
    .bind(id.as_str())
    .bind(next)
    .bind(current)
    .bind(change.text)
    .bind(&sha256)
    .bind(&summary)
    .bind(&reason)
    .bind(evidence)
    .bind(&actor)
    .execute(&mut *tx)
    .await?;
    record_activation(&mut tx, id, current, next, "learned", &actor, &reason).await?;
    tx.commit().await?;
    Ok(next)
}

pub async fn rollback(
    pool: &PgPool,
    id: SkillId,
    expected: i32,
    target: i32,
    reason: &str,
    actor: &str,
) -> Result<i32, SkillRegistryError> {
    let reason = bounded(reason, MAX_TEXT_FIELD, false)?;
    let actor = bounded(actor, MAX_ACTOR, false)?;
    if expected == target || target < 1 {
        return Err(SkillRegistryError::Invalid);
    }
    let mut tx = pool.begin().await?;
    lock_skill(&mut tx, id).await?;
    let current = current_version(&mut tx, id).await?;
    if current != expected {
        return Err(SkillRegistryError::Conflict);
    }
    let exists: Option<i32> = sqlx::query_scalar(
        "SELECT version FROM public.social_ai_skill_versions WHERE skill_id = $1 AND version = $2",
    )
    .bind(id.as_str())
    .bind(target)
    .fetch_optional(&mut *tx)
    .await?;
    if exists.is_none() {
        return Err(SkillRegistryError::NotFound);
    }
    record_activation(&mut tx, id, current, target, "rollback", &actor, &reason).await?;
    tx.commit().await?;
    Ok(target)
}

pub fn line_diff(before: &str, after: &str) -> Vec<DiffLine> {
    let old: Vec<&str> = before.lines().collect();
    let new: Vec<&str> = after.lines().collect();
    let mut table = vec![vec![0usize; new.len() + 1]; old.len() + 1];
    for i in (0..old.len()).rev() {
        for j in (0..new.len()).rev() {
            table[i][j] = if old[i] == new[j] {
                table[i + 1][j + 1] + 1
            } else {
                table[i + 1][j].max(table[i][j + 1])
            };
        }
    }
    let (mut i, mut j) = (0, 0);
    let mut lines = Vec::new();
    let mut push = |kind, text: &str| {
        lines.push(DiffLine {
            kind,
            text: text.to_owned(),
        })
    };
    while i < old.len() || j < new.len() {
        if i < old.len() && j < new.len() && old[i] == new[j] {
            push(DiffKind::Context, old[i]);
            i += 1;
            j += 1;
        } else if j < new.len() && (i == old.len() || table[i][j + 1] >= table[i + 1][j]) {
            push(DiffKind::Added, new[j]);
            j += 1;
        } else {
            push(DiffKind::Removed, old[i]);
            i += 1;
        }
    }
    lines
}

struct StoredVersion {
    version: i32,
    previous_version: Option<i32>,
    content: String,
    sha256: String,
    summary: String,
    reason: String,
    evidence: Option<Value>,
    created_at: DateTime<Utc>,
}

fn bundled_view(id: SkillId) -> SkillVersionView {
    let base = bundled(id);
    SkillVersionView {
        version: "1".into(),
        content_hash: Some(base.sha256),
        previous_version: None,
        changed_at: String::new(),
        summary: "Mitgelieferte Ausgangsversion".into(),
        reason: "Ausgangsstand".into(),
        evidence: None,
        diff: Vec::new(),
    }
}

fn version_view(stored: &StoredVersion, all: &[StoredVersion]) -> SkillVersionView {
    let diff = stored
        .previous_version
        .and_then(|previous| all.iter().find(|candidate| candidate.version == previous))
        .map(|previous| line_diff(&previous.content, &stored.content))
        .unwrap_or_default();
    SkillVersionView {
        version: stored.version.to_string(),
        content_hash: Some(stored.sha256.clone()),
        previous_version: stored.previous_version.map(|v| v.to_string()),
        changed_at: stored.created_at.to_rfc3339(),
        summary: stored.summary.clone(),
        reason: stored.reason.clone(),
        evidence: stored
            .evidence
            .clone()
            .and_then(|value| serde_json::from_value(value).ok()),
        diff,
    }
}

async fn stored_versions(
    pool: &PgPool,
) -> Result<Vec<(String, StoredVersion)>, SkillRegistryError> {
    let rows = sqlx::query(
        "SELECT skill_id, version, previous_version, content, sha256, summary, reason, evidence, created_at FROM public.social_ai_skill_versions ORDER BY skill_id, version DESC",
    )
    .fetch_all(pool)
    .await?;
    rows.iter()
        .map(|row| {
            Ok((
                row.try_get("skill_id")?,
                StoredVersion {
                    version: row.try_get("version")?,
                    previous_version: row.try_get("previous_version")?,
                    content: row.try_get("content")?,
                    sha256: row.try_get("sha256")?,
                    summary: row.try_get("summary")?,
                    reason: row.try_get("reason")?,
                    evidence: row.try_get("evidence")?,
                    created_at: row.try_get("created_at")?,
                },
            ))
        })
        .collect::<Result<_, sqlx::Error>>()
        .map_err(SkillRegistryError::from)
}

pub async fn history(pool: &PgPool) -> Result<Vec<SkillHistory>, SkillRegistryError> {
    let mut stored = stored_versions(pool).await?;
    let active: Vec<(String, i32)> =
        sqlx::query_as("SELECT skill_id, version FROM public.social_ai_skill_active")
            .fetch_all(pool)
            .await?;
    Ok(SkillId::ALL
        .into_iter()
        .map(|id| {
            let versions: Vec<StoredVersion> = {
                let (mine, rest): (Vec<_>, Vec<_>) = std::mem::take(&mut stored)
                    .into_iter()
                    .partition(|(skill, _)| skill == id.as_str());
                stored = rest;
                mine.into_iter().map(|(_, version)| version).collect()
            };
            let current = active
                .iter()
                .find(|(skill, _)| skill == id.as_str())
                .map_or(1, |(_, version)| *version);
            let views = if versions.is_empty() {
                vec![bundled_view(id)]
            } else {
                versions
                    .iter()
                    .map(|version| version_view(version, &versions))
                    .collect()
            };
            SkillHistory {
                skill_id: id.as_str().into(),
                name: id.display_name().into(),
                current_version: current.to_string(),
                versions: views,
            }
        })
        .collect())
}

pub async fn proposals(pool: &PgPool) -> Result<Vec<ChangeProposal>, SkillRegistryError> {
    let rows = sqlx::query(
        "SELECT id, title, summary, reason, evidence, status, created_at FROM public.social_ai_change_proposals ORDER BY created_at DESC, id DESC LIMIT $1",
    )
    .bind(MAX_PROPOSALS)
    .fetch_all(pool)
    .await?;
    rows.iter()
        .map(|row| {
            let id: i64 = row.try_get("id")?;
            let created_at: DateTime<Utc> = row.try_get("created_at")?;
            Ok(ChangeProposal {
                id: id.to_string(),
                title: row.try_get("title")?,
                created_at: created_at.to_rfc3339(),
                summary: row.try_get("summary")?,
                reason: row.try_get("reason")?,
                status: row.try_get("status")?,
                evidence: row.try_get("evidence")?,
            })
        })
        .collect::<Result<_, sqlx::Error>>()
        .map_err(SkillRegistryError::from)
}

pub async fn record_proposal(
    pool: &PgPool,
    proposal: NewProposal<'_>,
) -> Result<i64, SkillRegistryError> {
    let title = bounded(proposal.title, 200, false)?;
    let summary = bounded(proposal.summary, 2000, false)?;
    let reason = bounded(proposal.reason, 2000, false)?;
    let evidence = proposal
        .evidence
        .map(|evidence| bounded(evidence, 2000, true))
        .transpose()?;
    let id: i64 = sqlx::query_scalar(
        "INSERT INTO public.social_ai_change_proposals (title, summary, reason, evidence) VALUES ($1, $2, $3, $4) RETURNING id",
    )
    .bind(title)
    .bind(summary)
    .bind(reason)
    .bind(evidence)
    .fetch_one(pool)
    .await?;
    Ok(id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn candidate_must_keep_skill_heading_and_bounds() {
        let base = skill_snapshot(SkillId::Copywriting).text;
        assert!(validate_candidate(SkillId::Copywriting, base, 4096).is_ok());
        let learned = base.replace("# Copywriting v1", "# Copywriting v2");
        assert!(validate_candidate(SkillId::Copywriting, &learned, 4096).is_ok());
        assert_eq!(
            validate_candidate(SkillId::Copywriting, "# Subtitles v2\nfoo", 4096),
            Err(SkillRegistryError::Invalid)
        );
        assert!(validate_candidate(SkillId::Copywriting, base, 16).is_err());
        assert!(
            validate_candidate(SkillId::Copywriting, &format!("{base}\u{0007}"), 4096).is_err()
        );
    }

    #[test]
    fn line_diff_marks_changes_in_order() {
        let diff = line_diff("a\nb\nc", "a\nx\nc\nd");
        let kinds: Vec<_> = diff
            .iter()
            .map(|line| (line.kind, line.text.as_str()))
            .collect();
        assert_eq!(
            kinds,
            vec![
                (DiffKind::Context, "a"),
                (DiffKind::Added, "x"),
                (DiffKind::Removed, "b"),
                (DiffKind::Context, "c"),
                (DiffKind::Added, "d"),
            ]
        );
    }

    #[test]
    fn skill_ids_round_trip() {
        for id in SkillId::ALL {
            assert_eq!(SkillId::parse(id.as_str()), Some(id));
            assert_eq!(
                serde_json::to_value(id).ok(),
                Some(Value::String(id.as_str().into()))
            );
        }
        assert_eq!(SkillId::parse("unknown"), None);
    }
}
