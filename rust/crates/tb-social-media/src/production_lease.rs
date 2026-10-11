use sqlx::{pool::PoolConnection, PgPool, Postgres};

#[derive(Debug, thiserror::Error)]
pub enum ProductionLeaseError {
    #[error("Die Produktionssperre konnte nicht verwendet werden.")]
    Database(#[from] sqlx::Error),
    #[error("Die Produktionssperre gehört nicht zu dieser Verbindung.")]
    Conflict,
}

pub struct ProductionLease {
    pub(crate) connection: PoolConnection<Postgres>,
    claimed_work: bool,
}

impl ProductionLease {
    pub async fn ensure_held(&mut self) -> Result<(), ProductionLeaseError> {
        let held: bool = sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM pg_locks WHERE locktype = 'advisory' AND pid = pg_backend_pid() AND granted AND classid = ((hashtextextended('social_media_video_production', 0) >> 32) & 4294967295)::oid AND objid = (hashtextextended('social_media_video_production', 0) & 4294967295)::oid AND objsubid = 1)")
            .fetch_one(&mut *self.connection).await?;
        if held {
            Ok(())
        } else {
            Err(ProductionLeaseError::Conflict)
        }
    }

    pub(crate) fn mark_claimed_work(&mut self) {
        self.claimed_work = true;
    }

    pub(crate) fn has_claimed_work(&self) -> bool {
        self.claimed_work
    }

    pub async fn release(mut self) -> Result<(), ProductionLeaseError> {
        let unlocked: Result<bool, sqlx::Error> = sqlx::query_scalar(
            "SELECT pg_advisory_unlock(hashtextextended('social_media_video_production', 0))",
        )
        .fetch_one(&mut *self.connection)
        .await;
        self.connection.close().await?;
        if unlocked? {
            Ok(())
        } else {
            Err(ProductionLeaseError::Conflict)
        }
    }
}

pub async fn acquire_production_lease(
    pool: &PgPool,
) -> Result<Option<ProductionLease>, ProductionLeaseError> {
    let mut connection = pool.acquire().await?;
    connection.close_on_drop();
    let acquired: bool = sqlx::query_scalar(
        "SELECT pg_try_advisory_lock(hashtextextended('social_media_video_production', 0))",
    )
    .fetch_one(&mut *connection)
    .await?;
    Ok(acquired.then_some(ProductionLease {
        connection,
        claimed_work: false,
    }))
}
