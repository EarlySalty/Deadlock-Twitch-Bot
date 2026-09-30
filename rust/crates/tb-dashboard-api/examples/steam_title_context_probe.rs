//! Rein lesend: normale --config, vorhandene --uplink-config mit Credential-FD,
//! explizite --discord-id. Kein Dashboardstart, keine DB/Modelle/Twitch-Schreibaufrufe.

#[tokio::main]
async fn main() -> Result<(), &'static str> {
    let (snapshot, remaining) = tb_config::runtime::start(std::env::args_os().skip(1))
        .map_err(|_| "Normale Dienstkonfiguration fehlt oder ist ungültig.")?;
    if remaining.len() != 4 || remaining[0] != "--uplink-config" || remaining[2] != "--discord-id" {
        return Err("Aufruf: --config <Datei> --uplink-config <Datei> --discord-id <ID>");
    }
    let discord_id = remaining[3]
        .to_str()
        .and_then(|value| value.parse::<i64>().ok())
        .filter(|value| *value > 0)
        .ok_or("Eine positive Discord-ID ist erforderlich.")?;
    let runtime = tb_dashboard_api::uplink_config::load_arguments(remaining[..2].iter().cloned())
        .await?
        .ok_or("Die bestehende FD-Konfiguration fehlt.")?;
    let status = runtime
        .steam_title_context_status(
            &snapshot
                .settings()
                .dashboard
                .options
                .steam_title_context_url,
            discord_id,
        )
        .await?;
    println!(
        "{}",
        serde_json::to_string(&status).map_err(|_| "Status konnte nicht ausgegeben werden.")?
    );
    if !status.fresh {
        return Err(
            "Vertrag lesbar, aber Zeitstempel fehlt, liegt in der Zukunft oder ist veraltet.",
        );
    }
    if status.party_size.is_none()
        && status.party_member_count == 0
        && status.voice_member_count == 0
    {
        return Err("Vertrag frisch, aber keine Party- oder Sprachkanaldaten vorhanden.");
    }
    Ok(())
}
