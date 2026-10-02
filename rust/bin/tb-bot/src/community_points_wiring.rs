//! Periodischer Aggregationslauf der Community-Punkte (Community-Streamer-
//! Brücke, Paket B). Rechnet im geprüften Bot-TOML-Takt die Tageswerte aus den
//! Rohdaten neu (heute, kurz nach Mitternacht und beim Start auch gestern); die Logik
//! liegt in `tb_analytics::community_points`, hier nur Takt und Logging.

use std::time::Duration;

use sqlx::PgPool;
use tb_analytics::community_points::run_aggregation;

use crate::task_supervisor::TaskSupervisor;

pub fn spawn_community_points_aggregation(
    supervisor: &TaskSupervisor,
    pool: PgPool,
    aggregation_interval_seconds: u64,
) {
    supervisor.spawn("community_points_aggregation", async move {
        let mut tick = tokio::time::interval(Duration::from_secs(aggregation_interval_seconds));
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        let mut first_run = true;
        loop {
            tick.tick().await;
            match run_aggregation(&pool, chrono::Utc::now(), first_run).await {
                Ok(days) => {
                    first_run = false;
                    for (day, stats) in days {
                        tracing::debug!(
                            %day,
                            viewer_rows_changed = stats.viewer_rows_changed,
                            viewer_rows_zeroed = stats.viewer_rows_zeroed,
                            streamer_rows_changed = stats.streamer_rows_changed,
                            streamer_rows_zeroed = stats.streamer_rows_zeroed,
                            discoveries_inserted = stats.discoveries_inserted,
                            "Community-Punkte aggregiert"
                        );
                    }
                }
                Err(error) => {
                    tracing::error!(%error, "Community-Punkte-Aggregation fehlgeschlagen");
                }
            }
        }
    });
}
