//! Löst den aktuellen Modellnamen einer Modellfamilie beim Anbieter auf.
//!
//! Anlass: Fireworks hat `deepseek-v4-flash` am 15.08.2026 ersatzlos
//! abgeschaltet. Der Endpunkt antwortete 404, der Conversation-Scam-Judge fiel
//! fail-safe auf `unsure` zurück und hat einen ganzen Tag lang niemanden mehr
//! gebannt — ohne dass jemand etwas gemerkt hätte. Ein hartcodierter
//! Modellname ist deshalb ein Ablaufdatum, das keiner im Kalender stehen hat.
//!
//! Der Resolver fragt `GET /v1/models`, nimmt alle Modelle, deren Name mit der
//! konfigurierten Familie beginnt (`deepseek-v4-flash` findet also auch
//! `deepseek-v4-flash-0731`), und wählt die neueste Fassung nach dem
//! Anbieter-Zeitstempel. Das Ergebnis liegt im Prozess und zusätzlich in
//! Postgres, damit ein Neustart ohne API-Zugriff nicht auf dem alten
//! Kompilat-Default landet.
//!
//! # Rangfolge
//!
//! 1. `FIREWORK_MODEL` / `FIREWORKS_MODEL` — eine ausdrückliche Festlegung
//!    schlägt den Resolver immer. Wer ein bestimmtes Modell will, bekommt es.
//! 2. Aufgelöster Wert aus diesem Modul (frisch oder aus der DB geladen).
//! 3. Der einkompilierte Default als letzter Notnagel.
//!
//! # Selbstheilung
//!
//! Läuft ein Call trotzdem in ein 404, ruft der Aufrufer
//! [`invalidate_and_refresh`]: der zwischengespeicherte Wert fliegt raus, die
//! Liste wird einmal neu geholt, und der Aufrufer wiederholt den Call mit dem
//! frischen Namen. Erst wenn auch das scheitert, gilt der Anbieter als tot.

use std::sync::{OnceLock, RwLock};
use std::time::Duration;

use sqlx::PgPool;
use tracing::{debug, info, warn};

/// Anbieter-Kennung in `llm_model_cache`. Aktuell hat nur Fireworks eine
/// Modellliste, die sich unter uns wegdreht; MiniMax und Anthropic führen
/// stabile Namen und brauchen den Resolver nicht.
pub const PROVIDER_FIREWORKS: &str = "fireworks";

/// Modellfamilie, die aufgelöst wird. Alles, was mit diesem Pfad beginnt,
/// zählt als dieselbe Familie — die datierten Fassungen hängen ihr Datum
/// hinten an (`…-flash`, `…-flash-0731`).
pub const FIREWORKS_FAMILY: &str = "accounts/fireworks/models/deepseek-v4-flash";

/// Wie lange ein aufgelöster Name als frisch gilt. Der Anbieter dreht seine
/// Modelle in Monaten, nicht in Stunden; täglich nachsehen reicht und hält den
/// Zusatzverkehr bei einem Request pro Tag.
const REFRESH_INTERVAL: Duration = Duration::from_secs(24 * 60 * 60);

/// Timeout des Listen-Calls. Der Resolver darf den Bot-Start nicht aufhalten:
/// lieber der Wert aus der DB als eine Minute Warten.
const LIST_TIMEOUT: Duration = Duration::from_secs(10);

/// Prozessweit aufgelöster Fireworks-Modellname. `None` heißt „noch nicht
/// aufgelöst" — dann greift der einkompilierte Default.
fn cell() -> &'static RwLock<Option<String>> {
    static CELL: OnceLock<RwLock<Option<String>>> = OnceLock::new();
    CELL.get_or_init(|| RwLock::new(None))
}

/// Der aufgelöste Name, falls vorhanden. Synchron und billig — genau das,
/// was [`crate::selection`] beim Bauen eines Endpunkts braucht.
pub fn resolved_fireworks_model() -> Option<String> {
    cell().read().ok().and_then(|guard| guard.clone())
}

fn store(model: &str) {
    if let Ok(mut guard) = cell().write() {
        *guard = Some(model.to_string());
    }
}

/// Setzt den zwischengespeicherten Namen zurück. Nach einem 404 ist der Wert
/// nachweislich falsch, und der einkompilierte Default ist dann die bessere
/// Ausgangslage als ein Name, den der Anbieter gerade abgelehnt hat.
pub fn invalidate() {
    if let Ok(mut guard) = cell().write() {
        *guard = None;
    }
}

/// Ein Eintrag aus der Modellliste des Anbieters.
#[derive(Debug, Clone, PartialEq, Eq)]
struct ModelEntry {
    id: String,
    created: Option<i64>,
}

/// Wählt aus einer Modellliste die neueste Fassung der Familie.
///
/// Sortiert nach Anbieter-Zeitstempel, bei Gleichstand (oder fehlendem
/// Zeitstempel) nach Name absteigend — die datierten Namen sortieren dann
/// lexikografisch richtig, weil das Datum als `MMTT` hinten steht.
///
/// Reine Funktion, damit die Auswahlregel ohne Netz testbar bleibt.
fn pick_newest(entries: &[ModelEntry], family: &str) -> Option<String> {
    entries
        .iter()
        .filter(|entry| entry.id.starts_with(family))
        .max_by(|a, b| {
            a.created
                .cmp(&b.created)
                .then_with(|| a.id.as_str().cmp(b.id.as_str()))
        })
        .map(|entry| entry.id.clone())
}

/// Liest die Modellliste des Anbieters. Fehler sind hier kein Drama: der
/// Aufrufer fällt auf DB oder Default zurück.
async fn fetch_models(base_url: &str, api_key: &str) -> Result<Vec<ModelEntry>, String> {
    let url = format!("{}/models", base_url.trim_end_matches('/'));
    let client = reqwest::Client::builder()
        .timeout(LIST_TIMEOUT)
        .build()
        .map_err(|err| format!("HTTP-Client: {err}"))?;
    let response = client
        .get(&url)
        .bearer_auth(api_key)
        .send()
        .await
        .map_err(|err| format!("Modellliste nicht erreichbar: {err}"))?;
    if !response.status().is_success() {
        return Err(format!("Modellliste antwortet {}", response.status()));
    }
    let body: serde_json::Value = response
        .json()
        .await
        .map_err(|err| format!("Modellliste unlesbar: {err}"))?;
    Ok(parse_models(&body))
}

/// Zerlegt die Antwort in Einträge. Getrennt vom HTTP-Teil, damit das Format
/// ohne Netz geprüft werden kann.
fn parse_models(body: &serde_json::Value) -> Vec<ModelEntry> {
    body.get("data")
        .and_then(|data| data.as_array())
        .map(|items| {
            items
                .iter()
                .filter_map(|item| {
                    let id = item.get("id")?.as_str()?.to_string();
                    Some(ModelEntry {
                        id,
                        created: item.get("created").and_then(|c| c.as_i64()),
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

async fn load_from_db(pool: &PgPool, provider: &str, family: &str) -> Option<(String, i64)> {
    let row: Option<(String, Option<i64>, i64)> = sqlx::query_as(
        "SELECT model, model_created, \
         EXTRACT(EPOCH FROM (now() - resolved_at))::BIGINT AS alter_sekunden \
         FROM llm_model_cache WHERE provider = $1 AND family = $2",
    )
    .bind(provider)
    .bind(family)
    .fetch_optional(pool)
    .await
    .map_err(|err| {
        // Eine klemmende DB darf die Modellauswahl nicht kippen.
        warn!(%err, "llm_model_cache nicht lesbar, nutze Anbieter-Antwort");
        err
    })
    .ok()
    .flatten();
    row.map(|(model, _created, alter)| (model, alter))
}

async fn save_to_db(
    pool: &PgPool,
    provider: &str,
    family: &str,
    model: &str,
    created: Option<i64>,
) {
    let result = sqlx::query(
        "INSERT INTO llm_model_cache (provider, family, model, model_created, resolved_at) \
         VALUES ($1, $2, $3, $4, now()) \
         ON CONFLICT (provider, family) DO UPDATE \
         SET model = EXCLUDED.model, \
             model_created = EXCLUDED.model_created, \
             resolved_at = now()",
    )
    .bind(provider)
    .bind(family)
    .bind(model)
    .bind(created)
    .execute(pool)
    .await;
    if let Err(err) = result {
        warn!(%err, "llm_model_cache nicht schreibbar — Auswahl gilt trotzdem");
    }
}

/// Löst die Fireworks-Familie einmal auf und legt das Ergebnis im Prozess und
/// in der DB ab. Liefert den gewählten Namen, oder `None`, wenn weder Anbieter
/// noch DB etwas hergeben.
///
/// `pool` ist optional, damit der Resolver auch ohne Datenbank läuft (Tests,
/// Werkzeuge). Ohne Pool fehlt nur die Überlebensfähigkeit über Neustarts.
pub async fn refresh_fireworks(pool: Option<&PgPool>) -> Option<String> {
    // Eine ausdrückliche Festlegung macht jede Auflösung überflüssig.
    if let Some(pinned) = pinned_model() {
        debug!(model = %pinned, "Fireworks-Modell ist festgelegt, kein Resolver-Lauf");
        return Some(pinned);
    }

    let api_key = crate::keys::fireworks_api_key()?;
    let base_url = crate::selection::fireworks_base_url();

    match fetch_models(&base_url, &api_key).await {
        Ok(entries) => {
            let created = entries
                .iter()
                .filter(|entry| entry.id.starts_with(FIREWORKS_FAMILY))
                .filter_map(|entry| entry.created)
                .max();
            match pick_newest(&entries, FIREWORKS_FAMILY) {
                Some(model) => {
                    let vorher = resolved_fireworks_model();
                    if vorher.as_deref() != Some(model.as_str()) {
                        info!(
                            model = %model,
                            family = FIREWORKS_FAMILY,
                            vorher = vorher.as_deref().unwrap_or("—"),
                            "Fireworks-Modell aufgeloest"
                        );
                    }
                    store(&model);
                    if let Some(pool) = pool {
                        save_to_db(pool, PROVIDER_FIREWORKS, FIREWORKS_FAMILY, &model, created)
                            .await;
                    }
                    Some(model)
                }
                None => {
                    warn!(
                        family = FIREWORKS_FAMILY,
                        "Anbieter kennt kein Modell dieser Familie mehr — pruefe den Familiennamen"
                    );
                    fallback_from_db(pool).await
                }
            }
        }
        Err(err) => {
            warn!(%err, "Modellliste nicht abrufbar, nutze letzten bekannten Stand");
            fallback_from_db(pool).await
        }
    }
}

async fn fallback_from_db(pool: Option<&PgPool>) -> Option<String> {
    let pool = pool?;
    let (model, alter) = load_from_db(pool, PROVIDER_FIREWORKS, FIREWORKS_FAMILY).await?;
    info!(
        model = %model,
        alter_tage = alter / 86_400,
        "Fireworks-Modell aus dem Cache uebernommen"
    );
    store(&model);
    Some(model)
}

/// Nach einem 404: Wert verwerfen, einmal neu auflösen. Liefert den neuen
/// Namen, wenn er sich vom abgelehnten unterscheidet — nur dann lohnt der
/// Wiederholungsversuch.
pub async fn invalidate_and_refresh(pool: Option<&PgPool>, abgelehnt: &str) -> Option<String> {
    warn!(
        model = %abgelehnt,
        "Anbieter kennt das Modell nicht mehr (404) — loese neu auf"
    );
    invalidate();
    let neu = refresh_fireworks(pool).await?;
    if neu == abgelehnt {
        warn!(
            model = %neu,
            "Aufloesung liefert dasselbe abgelehnte Modell — kein Wiederholungsversuch"
        );
        return None;
    }
    Some(neu)
}

/// Startet den Hintergrund-Refresh. Läuft sofort einmal und danach täglich.
/// Der zurückgegebene Task hängt am Tokio-Runtime des Aufrufers; wer ihn
/// fallen lässt, hat trotzdem die erste Auflösung.
pub fn spawn_refresh_loop(pool: Option<PgPool>) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        loop {
            refresh_fireworks(pool.as_ref()).await;
            tokio::time::sleep(REFRESH_INTERVAL).await;
        }
    })
}

/// Eine ausdrücklich gesetzte Modell-Variable, falls vorhanden.
fn pinned_model() -> Option<String> {
    crate::selection::pinned_fireworks_model()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(id: &str, created: Option<i64>) -> ModelEntry {
        ModelEntry {
            id: id.to_string(),
            created,
        }
    }

    #[test]
    fn neueste_fassung_gewinnt_nach_zeitstempel() {
        let entries = vec![
            entry("accounts/fireworks/models/deepseek-v4-flash", Some(1_000)),
            entry(
                "accounts/fireworks/models/deepseek-v4-flash-0731",
                Some(2_000),
            ),
            entry("accounts/fireworks/models/deepseek-v4-pro", Some(9_000)),
        ];
        assert_eq!(
            pick_newest(&entries, FIREWORKS_FAMILY).as_deref(),
            Some("accounts/fireworks/models/deepseek-v4-flash-0731")
        );
    }

    #[test]
    fn pro_gehoert_nicht_zur_flash_familie() {
        // Der Praefix-Filter darf nicht versehentlich die teure Pro-Reihe
        // einsammeln — die kostet ein Vielfaches pro Call.
        let entries = vec![entry(
            "accounts/fireworks/models/deepseek-v4-pro-0813",
            Some(9_999),
        )];
        assert_eq!(pick_newest(&entries, FIREWORKS_FAMILY), None);
    }

    #[test]
    fn ohne_zeitstempel_entscheidet_der_name() {
        let entries = vec![
            entry("accounts/fireworks/models/deepseek-v4-flash", None),
            entry("accounts/fireworks/models/deepseek-v4-flash-0731", None),
            entry("accounts/fireworks/models/deepseek-v4-flash-0612", None),
        ];
        assert_eq!(
            pick_newest(&entries, FIREWORKS_FAMILY).as_deref(),
            Some("accounts/fireworks/models/deepseek-v4-flash-0731")
        );
    }

    #[test]
    fn leere_liste_liefert_nichts() {
        assert_eq!(pick_newest(&[], FIREWORKS_FAMILY), None);
    }

    #[test]
    fn antwort_ohne_created_wird_trotzdem_gelesen() {
        let body = serde_json::json!({
            "data": [
                {"id": "accounts/fireworks/models/deepseek-v4-flash-0731", "created": 1_753_920_000_i64},
                {"id": "accounts/fireworks/models/kimi-k3"},
                {"object": "model"}
            ]
        });
        let entries = parse_models(&body);
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[1].created, None);
        assert_eq!(
            pick_newest(&entries, FIREWORKS_FAMILY).as_deref(),
            Some("accounts/fireworks/models/deepseek-v4-flash-0731")
        );
    }

    #[test]
    fn muell_antwort_kippt_nichts() {
        assert!(parse_models(&serde_json::json!({"error": "kaputt"})).is_empty());
        assert!(parse_models(&serde_json::json!([])).is_empty());
    }

    /// Fragt die echte Modellliste ab. Braucht einen gültigen
    /// `FIREWORK_API_KEY`, läuft deshalb nur auf Zuruf:
    /// `cargo test -p tb-llm -- --ignored aufloesung_gegen_echten_anbieter`
    #[tokio::test]
    #[ignore = "benötigt produktiven Fireworks-Key"]
    async fn aufloesung_gegen_echten_anbieter() {
        // Eine gesetzte Festlegung wuerde den Resolver ueberspringen und den
        // Test wertlos machen.
        std::env::remove_var("FIREWORK_MODEL");
        std::env::remove_var("FIREWORKS_MODEL");

        let model = refresh_fireworks(None)
            .await
            .expect("Anbieter liefert ein Modell der Familie");
        assert!(
            model.starts_with(FIREWORKS_FAMILY),
            "aufgeloest wurde {model}, erwartet war ein Modell der Familie {FIREWORKS_FAMILY}"
        );
        assert_eq!(resolved_fireworks_model().as_deref(), Some(model.as_str()));
        println!("aufgeloest: {model}");
    }
}
