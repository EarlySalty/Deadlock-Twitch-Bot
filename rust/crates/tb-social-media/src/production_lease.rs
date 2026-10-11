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
