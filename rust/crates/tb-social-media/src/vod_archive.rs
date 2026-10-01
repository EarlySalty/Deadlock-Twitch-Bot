//! VOD-Archiv-Einstellung je Streamer.
//!
//! Bewusst nicht in [`crate::settings`]: dort liegen die globalen
//! Key/Value-Schalter des Moduls, hier haengt jede Zeile an genau einem
//! Streamer. Das Muster ist dasselbe wie bei [`crate::layout`] und damit bei
//! `social_media_streamer_layout`: Login als Schluessel, Fremdschluessel auf
//! `twitch_streamers`, `updated_by` fuer die Nachvollziehbarkeit.
//!
//! Vorher war das ein globaler Schalter samt fest verdrahtetem Kanal. Ein
//! freigeschalteter Partner haette damit die Archivierung eines fremden Kanals
//! umgelegt.
//!
//! Die Abfragen laufen ueber das Laufzeit-API von sqlx statt ueber die Makros,
//! wie im Rest des VOD-Archivs: das haelt neue Tabellen vom Offline-Cache
//! unabhaengig.

use sqlx::{PgPool, Row};

/// Gueltige Sichtbarkeiten fuer den VOD-Upload. Solange das Google-Projekt
/// nicht auditiert ist, erzwingt YouTube ohnehin `private` und setzt alles
/// andere still zurueck; die Wahl bleibt trotzdem hier, damit sie nach dem
/// Audit ohne Codeaenderung greift.
pub const VOD_ARCHIVE_PRIVACY_VALUES: [&str; 3] = ["private", "unlisted", "public"];
pub const DEFAULT_VOD_ARCHIVE_PRIVACY: &str = "private";

/// Einstellung eines Streamers: laeuft das Archiv, und wie sichtbar sind die
/// Uploads auf seinem YouTube-Kanal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VodArchiveSettings {
    pub streamer_login: String,
    pub twitch_user_id: Option<String>,
    pub enabled: bool,
    pub privacy: String,
}

impl VodArchiveSettings {
    /// Default fuer einen Streamer ohne Eintrag: aus und privat.
    pub fn aus(streamer_login: &str) -> Self {
        Self {
            streamer_login: streamer_login.to_lowercase(),
            twitch_user_id: None,
            enabled: false,
            privacy: DEFAULT_VOD_ARCHIVE_PRIVACY.to_string(),
        }
    }
}

/// Liest die Einstellung eines Streamers. Fehlender Eintrag und unbekannte
/// Sichtbarkeit fallen auf „aus, privat" zurueck statt zu scheitern.
pub async fn get_vod_archive_settings(pool: &PgPool, twitch_user_id: &str) -> VodArchiveSettings {
    let login = twitch_user_id.to_string();
    if login.is_empty() {
        return VodArchiveSettings::aus("");
    }
    let row = sqlx::query(
        "SELECT streamer_login, twitch_user_id, enabled, privacy FROM social_media_vod_archive \
         WHERE twitch_user_id = $1",
    )
    .bind(&login)
    .fetch_optional(pool)
    .await
    .ok()
    .flatten();
    match row {
        Some(row) => {
            let privacy: String = row.get("privacy");
            VodArchiveSettings {
                streamer_login: row.get("streamer_login"),
                twitch_user_id: row.get("twitch_user_id"),
                enabled: row.get("enabled"),
                privacy: if VOD_ARCHIVE_PRIVACY_VALUES.contains(&privacy.as_str()) {
                    privacy
                } else {
                    DEFAULT_VOD_ARCHIVE_PRIVACY.to_string()
                },
            }
        }
        None => {
            let display = sqlx::query_scalar::<_, String>(
                "SELECT twitch_login FROM twitch_streamers WHERE twitch_user_id = $1",
            )
            .bind(&login)
            .fetch_optional(pool)
            .await
            .ok()
            .flatten();
            let mut defaults = VodArchiveSettings::aus(display.as_deref().unwrap_or(""));
            defaults.twitch_user_id = Some(login);
            defaults
        }
    }
}

/// Setzt die Einstellung eines Streamers. Ungueltige Sichtbarkeit wird
/// abgewiesen, damit kein Tippfehler stillschweigend ein VOD oeffentlich stellt.
pub async fn set_vod_archive_settings(
    pool: &PgPool,
    values: &VodArchiveSettings,
    updated_by: Option<&str>,
) -> Result<VodArchiveSettings, sqlx::Error> {
    if !VOD_ARCHIVE_PRIVACY_VALUES.contains(&values.privacy.as_str()) {
        return Err(sqlx::Error::Protocol(format!(
            "unbekannte Sichtbarkeit: {}",
            values.privacy
        )));
    }
    let login = values.twitch_user_id.as_deref().unwrap_or("").to_string();
    if login.is_empty() || !login.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(sqlx::Error::Protocol("twitch_user_id fehlt".to_string()));
    }
    let updated_by = updated_by.map(str::trim).filter(|s| !s.is_empty());
    sqlx::query(
        "INSERT INTO social_media_vod_archive (streamer_login, twitch_user_id, enabled, privacy, updated_at, updated_by) \
         SELECT twitch_login, twitch_user_id, $2, $3, CURRENT_TIMESTAMP, $4 FROM twitch_streamers WHERE twitch_user_id = $1 \
         ON CONFLICT (twitch_user_id) WHERE twitch_user_id IS NOT NULL DO UPDATE SET streamer_login = EXCLUDED.streamer_login, \
             enabled = EXCLUDED.enabled, \
             privacy = EXCLUDED.privacy, \
             updated_at = CURRENT_TIMESTAMP, \
             updated_by = EXCLUDED.updated_by",
    )
    .bind(&login)
    .bind(values.enabled)
    .bind(&values.privacy)
    .bind(updated_by)
    .execute(pool)
    .await?;
    Ok(get_vod_archive_settings(pool, &login).await)
}

/// Alle Streamer mit eingeschaltetem Archiv, alphabetisch. Der Worker
/// iteriert genau darueber, statt einen festen Kanal zu kennen.
pub async fn aktive_vod_archive_streamer(
    pool: &PgPool,
) -> Result<Vec<VodArchiveSettings>, sqlx::Error> {
    let rows = sqlx::query(
        "SELECT source.twitch_login AS streamer_login, settings.twitch_user_id, settings.enabled, settings.privacy \
         FROM social_media_vod_archive settings JOIN twitch_streamers source ON source.twitch_user_id = settings.twitch_user_id \
         WHERE settings.enabled AND settings.twitch_user_id ~ '^[0-9]+$' ORDER BY source.twitch_login ASC",
    )
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(|row| {
            let privacy: String = row.get("privacy");
            VodArchiveSettings {
                streamer_login: row.get::<String, _>("streamer_login").to_lowercase(),
                twitch_user_id: row.get("twitch_user_id"),
                enabled: row.get("enabled"),
                privacy: if VOD_ARCHIVE_PRIVACY_VALUES.contains(&privacy.as_str()) {
                    privacy
                } else {
                    DEFAULT_VOD_ARCHIVE_PRIVACY.to_string()
                },
            }
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
    use std::str::FromStr;

    async fn make_pool(schema: &str) -> Option<PgPool> {
        let dsn = crate::test_support::test_dsn()?;
        let admin = PgPoolOptions::new()
            .max_connections(1)
            .connect(&dsn)
            .await
            .unwrap();
        sqlx::query(crate::test_sql::drop_schema(schema, true))
            .execute(&admin)
            .await
            .unwrap();
        sqlx::query(crate::test_sql::create_schema(schema, false))
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
            "CREATE TABLE social_media_vod_archive (twitch_user_id TEXT DEFAULT '42', streamer_login TEXT PRIMARY KEY, \
             enabled BOOLEAN NOT NULL DEFAULT FALSE, privacy TEXT NOT NULL DEFAULT 'private', \
             updated_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP, updated_by TEXT)",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query("CREATE TABLE twitch_streamers (twitch_login TEXT, twitch_user_id TEXT)")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("INSERT INTO twitch_streamers VALUES ('earlysalty','42'),('nani','99')")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("CREATE UNIQUE INDEX fixture_identity ON social_media_vod_archive (twitch_user_id) WHERE twitch_user_id IS NOT NULL").execute(&pool).await.unwrap();
        Some(pool)
    }

    #[tokio::test]
    async fn einstellung_haengt_am_streamer() {
        let Some(pool) = make_pool("t_sm_vod_archive").await else {
            return;
        };
        // Ohne Eintrag ist das Archiv aus, egal fuer welchen Kanal.
        let defaults = get_vod_archive_settings(&pool, "42").await;
        assert_eq!(defaults.streamer_login, "earlysalty");
        assert_eq!(defaults.twitch_user_id.as_deref(), Some("42"));
        assert!(!defaults.enabled);
        assert_eq!(defaults.privacy, "private");

        set_vod_archive_settings(
            &pool,
            &VodArchiveSettings {
                streamer_login: "EarlySalty".to_string(),
                twitch_user_id: Some("42".to_string()),
                enabled: true,
                privacy: "unlisted".to_string(),
            },
            Some("42"),
        )
        .await
        .unwrap();

        let gelesen = get_vod_archive_settings(&pool, "42").await;
        assert!(gelesen.enabled);
        assert_eq!(gelesen.privacy, "unlisted");
        // Der zweite Kanal bleibt davon unberuehrt.
        assert!(!get_vod_archive_settings(&pool, "99").await.enabled);

        // Nur eingeschaltete Kanaele tauchen beim Worker auf.
        set_vod_archive_settings(
            &pool,
            &VodArchiveSettings {
                streamer_login: "nani".to_string(),
                twitch_user_id: Some("99".to_string()),
                enabled: false,
                privacy: "private".to_string(),
            },
            None,
        )
        .await
        .unwrap();
        let aktiv = aktive_vod_archive_streamer(&pool).await.unwrap();
        assert_eq!(aktiv.len(), 1);
        assert_eq!(aktiv[0].streamer_login, "earlysalty");
        sqlx::query(
            "UPDATE twitch_streamers SET twitch_login = 'renamed' WHERE twitch_user_id = '42';",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query("INSERT INTO twitch_streamers VALUES ('earlysalty', '100')")
            .execute(&pool)
            .await
            .unwrap();
        let aktiv = aktive_vod_archive_streamer(&pool).await.unwrap();
        assert_eq!(aktiv.len(), 1);
        assert_eq!(aktiv[0].streamer_login, "renamed");
        assert_eq!(aktiv[0].twitch_user_id.as_deref(), Some("42"));
    }

    #[tokio::test]
    async fn unbekannte_sichtbarkeit_wird_abgewiesen() {
        let Some(pool) = make_pool("t_sm_vod_archive_privacy").await else {
            return;
        };
        let fehler = set_vod_archive_settings(
            &pool,
            &VodArchiveSettings {
                streamer_login: "earlysalty".to_string(),
                twitch_user_id: Some("42".to_string()),
                enabled: true,
                privacy: "weltweit".to_string(),
            },
            None,
        )
        .await;
        assert!(fehler.is_err());
        // Und nichts davon ist in der Tabelle gelandet.
        assert!(!get_vod_archive_settings(&pool, "42").await.enabled);
    }
}
