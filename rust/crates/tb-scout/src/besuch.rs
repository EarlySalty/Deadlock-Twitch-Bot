//! Besuch-Erkennung: taucht der Owner-Login ([`crate::OWNER_LOGIN`]) im Chat
//! oder in den Präsenz-Ticks eines Kandidaten-Kanals auf, ist die Beziehung
//! bereits persönlich. Rein lesende Erkennung — kein Versand, keine
//! Nachricht, kein LLM; geschrieben wird nur der Status in
//! `twitch_scout_candidates` (INV-06: Zustand nur in der zentralen PG).
//!
//! Abgrenzung: die frühere Trust-Leiter (`recruitment_messaging.rs`) ist
//! deaktiviert; hier wird nicht darauf gebaut und nichts versendet.

use sqlx::PgPool;

use crate::{normalisiere_login, STATUS_PERSOENLICH, STATUS_VORGESCHLAGEN};

/// Entscheider-Kennzeichnung für automatisch erkannte Owner-Besuche.
pub const BESUCH_ACTOR: &str = "besuch-erkennung";

/// Markiert Owner-Besuche: `visited_at` wird bei erstem Nachweis gesetzt
/// (idempotent, `NULL`-Schutz). Nur noch offene Kandidaten (`vorgeschlagen`)
/// wechseln zusätzlich auf `persoenlich`; getroffene Nutzer-Entscheidungen
/// bleiben unangetastet (REQ-05). Liefert die Anzahl angefasster Zeilen.
pub async fn erkenne_besuche(pool: &PgPool) -> Result<usize, sqlx::Error> {
    let Some(owner) = normalisiere_login(crate::OWNER_LOGIN) else {
        return Ok(0);
    };
    let ergebnis = sqlx::query(
        "UPDATE twitch_scout_candidates c \
           SET visited_at = NOW(), \
               status = CASE WHEN c.status = $2 THEN $3 ELSE c.status END, \
               approver = CASE WHEN c.status = $2 THEN $4 ELSE c.approver END, \
               decided_at = CASE WHEN c.status = $2 THEN NOW() ELSE c.decided_at END \
         WHERE c.visited_at IS NULL \
           AND (EXISTS (SELECT 1 FROM twitch_chat_messages m \
                        WHERE LOWER(m.streamer_login) = c.streamer_login \
                          AND LOWER(m.chatter_login) = $1) \
             OR EXISTS (SELECT 1 FROM twitch_viewer_presence_ticks v \
                        WHERE LOWER(v.streamer_login) = c.streamer_login \
                          AND LOWER(v.viewer_login) = $1))",
    )
    .bind(&owner)
    .bind(STATUS_VORGESCHLAGEN)
    .bind(STATUS_PERSOENLICH)
    .bind(BESUCH_ACTOR)
    .execute(pool)
    .await?;
    Ok(ergebnis.rows_affected() as usize)
}
