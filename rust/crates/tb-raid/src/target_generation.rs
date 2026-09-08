//! Zielversionen für den vorhandenen Bot→Uplink-Vertrag. Keine Tokenablage.
//!
//! Ein Trennen reserviert zuerst dauerhaft eine neue Generation. Bestätigtes
//! Relay-DELETE wird nur für genau diese Generation lokal abgeschlossen.
//! Unklare RPC-Antworten bleiben pending; ein erneutes Trennen verwendet
//! dieselbe Generation. Eine ausdrücklich neue Verbindung nutzt nextval und
//! kann deshalb weder durch alte PUTs noch durch alte DELETEs geändert werden.
use chrono::{DateTime, Utc};
use sqlx::{PgPool, Postgres, Transaction};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TargetGeneration {
    pub generation: i64,
    pub enabled: bool,
    pub disconnect_pending: bool,
}
const INVALID: &str = "Zielgeneration oder Plattform-ID ist ungültig";
fn valid(uid: &str, platform: &str) -> Result<(), sqlx::Error> {
    if !crate::auth_writer::valid_uid(uid)
        || !matches!(platform, "twitch" | "kick" | "youtube" | "tiktok")
    {
        return Err(sqlx::Error::Protocol(INVALID.into()));
    }
    Ok(())
}
/// Gemeinsame UID-Sperre, auch für Kick-/YouTube-Callback und Refresh.
/// Reihenfolge: Pool → UID → Generation → Grant. Kein RPC bei gehaltenem
/// Ziel-Fence; Plattform-Refresh bleibt durch seinen Aufrufer zeitbegrenzt.
pub async fn transaction<'a>(
    pool: &'a PgPool,
    uid: &str,
    platform: &str,
) -> Result<Transaction<'a, Postgres>, sqlx::Error> {
    valid(uid, platform)?;
    crate::auth_writer::auth_transaction(pool, uid).await
}
pub async fn snapshot(
    pool: &PgPool,
    uid: &str,
    platform: &str,
) -> Result<TargetGeneration, sqlx::Error> {
    valid(uid, platform)?;
    let row:Option<(i64,bool,bool)>=tokio::time::timeout(std::time::Duration::from_secs(10),sqlx::query_as("SELECT generation,enabled,disconnect_pending FROM uplink_target_generations WHERE twitch_user_id=$1 AND platform=$2").bind(uid).bind(platform).fetch_optional(pool)).await.map_err(|_|sqlx::Error::PoolTimedOut)??;
    Ok(row.map_or(
        TargetGeneration {
            generation: 0,
            enabled: true,
            disconnect_pending: false,
        },
        |(generation, enabled, disconnect_pending)| TargetGeneration {
            generation,
            enabled,
            disconnect_pending,
        },
    ))
}
pub async fn check_current(
    pool: &PgPool,
    uid: &str,
    platform: &str,
    generation: i64,
) -> Result<bool, sqlx::Error> {
    let current = snapshot(pool, uid, platform).await?;
    Ok(current.enabled && !current.disconnect_pending && current.generation == generation)
}
/// Nur in derselben gesperrten Auth-/Plattform-Token-Transaktion aufrufen.
pub async fn activate_callback(
    tx: &mut Transaction<'_, Postgres>,
    uid: &str,
    platform: &str,
    state_created_at: DateTime<Utc>,
) -> Result<i64, sqlx::Error> {
    valid(uid, platform)?;
    let last:Option<Option<DateTime<Utc>>>=sqlx::query_scalar("SELECT last_disconnected_at FROM uplink_target_generations WHERE twitch_user_id=$1 AND platform=$2 FOR UPDATE").bind(uid).bind(platform).fetch_optional(&mut **tx).await?;
    if last.flatten().is_some_and(|at| state_created_at <= at) {
        return Err(sqlx::Error::Protocol(
            "Die Verbindung wurde nach Beginn dieser Anmeldung getrennt".into(),
        ));
    }
    activate(tx, uid, platform).await
}
async fn activate(
    tx: &mut Transaction<'_, Postgres>,
    uid: &str,
    platform: &str,
) -> Result<i64, sqlx::Error> {
    sqlx::query_scalar("INSERT INTO uplink_target_generations(twitch_user_id,platform,generation,enabled) VALUES($1,$2,nextval('uplink_target_generation_seq'),true) ON CONFLICT(twitch_user_id,platform) DO UPDATE SET generation=nextval('uplink_target_generation_seq'),enabled=true,disconnect_pending=false RETURNING generation").bind(uid).bind(platform).fetch_one(&mut **tx).await
}
/// Ein explizit eingegebener neuer Schlüssel ist eine neue Zielverbindung,
/// keine Erteilung zusätzlicher OAuth-Rechte.
pub async fn activate_manual(pool: &PgPool, uid: &str, platform: &str) -> Result<i64, sqlx::Error> {
    valid(uid, platform)?;
    tokio::time::timeout(std::time::Duration::from_secs(15), async {
        let mut tx = crate::auth_writer::auth_transaction(pool, uid).await?;
        let generation = activate(&mut tx, uid, platform).await?;
        tx.commit().await?;
        Ok(generation)
    })
    .await
    .map_err(|_| sqlx::Error::PoolTimedOut)?
}
pub async fn begin_disconnect(
    pool: &PgPool,
    uid: &str,
    platform: &str,
) -> Result<i64, sqlx::Error> {
    valid(uid, platform)?;
    tokio::time::timeout(std::time::Duration::from_secs(15),async{
  let mut tx=crate::auth_writer::auth_transaction(pool,uid).await?;
  let row:Option<(i64,bool)>=sqlx::query_as("SELECT generation,disconnect_pending FROM uplink_target_generations WHERE twitch_user_id=$1 AND platform=$2 FOR UPDATE").bind(uid).bind(platform).fetch_optional(&mut *tx).await?;
  let generation=if let Some((generation,true))=row{generation}else{
   sqlx::query_scalar("INSERT INTO uplink_target_generations(twitch_user_id,platform,generation,enabled,disconnect_pending,last_disconnected_at) VALUES($1,$2,nextval('uplink_target_generation_seq'),false,true,clock_timestamp()) ON CONFLICT(twitch_user_id,platform) DO UPDATE SET generation=nextval('uplink_target_generation_seq'),enabled=false,disconnect_pending=true,last_disconnected_at=GREATEST(uplink_target_generations.last_disconnected_at,clock_timestamp()) RETURNING generation").bind(uid).bind(platform).fetch_one(&mut *tx).await?
  };
  tx.commit().await?;Ok(generation)
 }).await.map_err(|_|sqlx::Error::PoolTimedOut)?
}
/// Gemeinsame Sperre mit Twitch-Callback/Refresh; nach positivem Generation-CAS
/// dürfen hier im selben TX Uplink-Intent oder plattformeigene Tokens enden.
pub async fn disconnect_transaction<'a>(
    pool: &'a PgPool,
    uid: &str,
    platform: &str,
    generation: i64,
) -> Result<Option<Transaction<'a, Postgres>>, sqlx::Error> {
    valid(uid, platform)?;
    let mut tx = crate::auth_writer::auth_transaction(pool, uid).await?;
    let rows=sqlx::query("UPDATE uplink_target_generations SET disconnect_pending=false WHERE twitch_user_id=$1 AND platform=$2 AND generation=$3 AND enabled=false RETURNING generation").bind(uid).bind(platform).bind(generation).fetch_all(&mut *tx).await?;
    if rows.is_empty() {
        return Ok(None);
    }
    Ok(Some(tx))
}
