//! Vorbereiteter Quelltext für tb-dashboard-api/examples, noch nicht integriert.
//! Derselbe FD-/Config-/Poolpfad wie clip_context_learn; ausschließlich SELECT.

use serde_json::json;
use sqlx::PgPool;

async fn identity(pool: &PgPool) -> Result<(String, String, String), &'static str> {
    sqlx::query_as("SELECT current_user::text,current_database()::text,current_setting('transaction_read_only')")
        .fetch_one(pool)
        .await
        .map_err(|_| "Datenbankidentität konnte nicht geprüft werden.")
}

async fn privilege(pool: &PgPool, table: &str, permission: &str) -> Result<bool, &'static str> {
    sqlx::query_scalar(
        "SELECT COALESCE(has_table_privilege(current_user,to_regclass($1),$2),false)",
    )
    .bind(table)
    .bind(permission)
    .fetch_one(pool)
    .await
    .map_err(|_| "Datenbankrecht konnte nicht geprüft werden.")
}

#[tokio::main]
async fn main() -> Result<(), &'static str> {
    let config = tb_config::file::ConfigArguments::parse(std::env::args_os().skip(1))
        .map_err(|_| "Normale Konfigurationsargumente sind ungültig.")?;
    let snapshot = tb_config::BotConfigSnapshot::load(&config.path)
        .map_err(|_| "Normale Dienstkonfiguration fehlt oder ist ungültig.")?;
    if config.remaining.len() != 2 || config.remaining[0] != "--uplink-config" {
        return Err("Aufruf: --config <Datei> --uplink-config <Datei>");
    }
    let uplink = tb_dashboard_api::uplink_config::load_arguments(config.remaining)
        .await?
        .ok_or("Die bestehende FD-Konfiguration fehlt.")?;
    tb_dashboard_api::uplink_config::install(uplink)?;
    let runtime = tb_dashboard_api::uplink_config::clip_context_runtime(&snapshot).await?;
    let read = identity(&runtime.read_pool).await?;
    let write = identity(&runtime.write_pool).await?;
    let mut valid = read.1 == "twitch_analytics"
        && write.1 == read.1
        && write.0 == read.0
        && read.2 == "on"
        && write.2 == "off";
    let mut rights = Vec::new();
    for table in [
        "public.twitch_clips_social_media",
        "public.twitch_clip_command_events",
        "public.twitch_clip_context_runs",
        "public.twitch_clip_context_seconds",
        "public.twitch_clip_cut_templates",
        "public.twitch_chat_messages",
    ] {
        let allowed = privilege(&runtime.read_pool, table, "SELECT").await?;
        valid &= allowed;
        rights.push(json!({"pool":"read","table":table,"permission":"SELECT","allowed":allowed}));
    }
    // Exakt die Mutationen des begrenzten Laufs ohne --backfill.
    // Diese drei Tabellen haben ausschließlich natürliche Schlüssel, keine Sequenzen.
    for (table, permissions) in [
        (
            "public.twitch_clip_context_runs",
            &["SELECT", "INSERT", "UPDATE"][..],
        ),
        (
            "public.twitch_clip_context_seconds",
            &["SELECT", "INSERT", "DELETE"][..],
        ),
        (
            "public.twitch_clip_cut_templates",
            &["SELECT", "INSERT", "UPDATE"][..],
        ),
    ] {
        for permission in permissions {
            let allowed = privilege(&runtime.write_pool, table, permission).await?;
            valid &= allowed;
            rights.push(
                json!({"pool":"write","table":table,"permission":permission,"allowed":allowed}),
            );
        }
    }
    let report = json!({
        "read":{"role":read.0,"database":read.1,"transaction_read_only":read.2},
        "write":{"role":write.0,"database":write.1,"transaction_read_only":write.2},
        "rights":rights,"contract_satisfied":valid
    });
    println!(
        "{}",
        serde_json::to_string(&report).map_err(|_| "Status konnte nicht ausgegeben werden.")?
    );
    if !valid {
        return Err("Die tatsächlichen CLI-Verbindungen erfüllen den Lese-/Schreibvertrag nicht.");
    }
    Ok(())
}
