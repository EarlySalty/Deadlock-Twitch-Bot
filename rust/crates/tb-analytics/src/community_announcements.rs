//! Persistente Ankündigungsrotation ausschließlich für den Community-Kanal.
use crate::promo_timers::COMMUNITY_BROADCASTER_ID;
use serde::{Deserialize, Serialize};
use sqlx::{PgPool, Row};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Announcement {
    pub text: String,
    pub enabled: bool,
    pub color: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CommunityAnnouncements {
    pub revision: i64,
    pub enabled: bool,
    pub include_global_event: bool,
    pub entries: Vec<Announcement>,
}

impl CommunityAnnouncements {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.revision < 0 || self.entries.len() > 50 {
            return Err("Es sind höchstens 50 Ankündigungen möglich.");
        }
        for entry in &self.entries {
            if entry.text.trim().is_empty()
                || entry.text.chars().count() > 450
                || entry.text.chars().any(char::is_control)
            {
                return Err("Jeder Text muss zwischen 1 und 450 Zeichen lang sein und darf keine Zeilenumbrüche enthalten.");
            }
            // Nur ein einfacher, optionaler Platzhalter; keine Format-Syntax.
            if entry.text.replace("{invite}", "").contains(['{', '}']) {
                return Err("Als Platzhalter ist nur {invite} erlaubt.");
            }
            if entry.text.matches("{invite}").count() > 1
                || (entry.text.contains("{invite}")
                    && entry.text.replace("{invite}", "").chars().count() > 400)
            {
                return Err("{invite} darf nur einmal vorkommen. Mit Einladungslink sind höchstens 400 weitere Zeichen erlaubt.");
            }
            if !["blue", "green", "orange", "purple", "primary"].contains(&entry.color.as_str()) {
                return Err("Bitte eine gültige Ankündigungsfarbe auswählen.");
            }
        }
        Ok(())
    }
}

pub async fn load(pool: &PgPool) -> Result<CommunityAnnouncements, sqlx::Error> {
    let row = sqlx::query(
        "SELECT revision, settings FROM twitch_community_announcements WHERE broadcaster_id = $1",
    )
    .bind(COMMUNITY_BROADCASTER_ID)
    .fetch_one(pool)
    .await?;
    let mut value: CommunityAnnouncements = serde_json::from_value(row.try_get("settings")?)
        .map_err(|e| sqlx::Error::Decode(Box::new(e)))?;
    value.revision = row.try_get("revision")?;
    value
        .validate()
        .map_err(|e| sqlx::Error::Protocol(e.into()))?;
    Ok(value)
}

/// Optimistischer Schreibschutz: parallele Browser überschreiben keine Änderungen.
pub async fn save(
    pool: &PgPool,
    value: &CommunityAnnouncements,
) -> Result<Option<CommunityAnnouncements>, sqlx::Error> {
    value
        .validate()
        .map_err(|e| sqlx::Error::Protocol(e.into()))?;
    let revision: Option<i64> = sqlx::query_scalar("UPDATE twitch_community_announcements SET settings = $1, revision = revision + 1, updated_at = now() WHERE broadcaster_id = $2 AND revision = $3 RETURNING revision")
        .bind(serde_json::to_value(value).map_err(|e| sqlx::Error::Encode(Box::new(e)))?)
        .bind(COMMUNITY_BROADCASTER_ID).bind(value.revision).fetch_optional(pool).await?;
    Ok(revision.map(|revision| CommunityAnnouncements {
        revision,
        ..value.clone()
    }))
}
