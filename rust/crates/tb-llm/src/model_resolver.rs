//! Löst den aktuellen Modellnamen einer Modellfamilie beim Anbieter auf.
//!
//! Anlass: Fireworks hat `deepseek-v4-flash` am 15.08.2026 ersatzlos
//! abgeschaltet. Der Endpunkt antwortete 404, der Conversation-Scam-Judge fiel
//! fail-safe auf `unsure` zurück und hat einen ganzen Tag lang niemanden mehr
//! gebannt — ohne dass jemand etwas gemerkt hätte. Ein hartcodierter
//! Modellname ist deshalb ein Ablaufdatum, das keiner im Kalender stehen hat.
//!
//! Der Resolver fragt `GET /v1/models`, nimmt die Modelle der konfigurierten
//! Familie (`deepseek-v4-flash` und datierte Fassungen wie `…-0731`, nicht
//! `-lite`/`-preview`/`-thinking`) und wählt die neueste Fassung nach dem
//! Namen, mit Anbieter-Zeitstempel nur als Tiebreak. Das Ergebnis liegt im
//! Prozess und zusätzlich in Postgres, damit ein Neustart ohne API-Zugriff
//! nicht auf dem alten Kompilat-Default landet.
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
//! [`invalidate_and_refresh`]: die Liste wird einmal neu geholt, und der
//! Aufrufer wiederholt den Call mit dem frischen Namen. Der alte Cache-Wert
//! bleibt stehen, bis ein neuer feststeht — ein transienter Listen-Fehler
//! darf die Prozess-Zelle nicht für 24 h leeren. Erst wenn auch das
//! scheitert, gilt der Anbieter als tot.

use std::sync::{OnceLock, RwLock};
use std::time::Duration;

use sqlx::PgPool;
use tracing::{debug, info, warn};

/// Pool, den [`spawn_refresh_loop`] einmalig ablegt. Der 404-Pfad darf ihn
/// mitnehmen, auch wenn der Aufrufer selbst keinen Pool in der Hand hat.
static REGISTERED_POOL: OnceLock<PgPool> = OnceLock::new();

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

/// Gemeinsamer Lock für alle Tests dieser Crate, die die prozessweite
/// Resolver-Zelle oder die Umgebungsvariablen dahinter setzen. Rust startet
/// jeden Test in seinem eigenen Thread; ohne diesen Lock würde ein in der
/// Zelle hinterlegter Wert in parallel laufende Tests durchschlagen und
/// deren Erwartung an den einkompilierten Default kippen. Nur unter
/// `cfg(test)` vorhanden, der Produktivcode braucht ihn nicht.
#[cfg(test)]
pub(crate) static TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

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

/// Setzt den zwischengespeicherten Namen zurück. Nur aufrufen, wenn ein
/// neuer Wert feststeht oder der Cache in Tests geleert werden muss. Ein
/// transienter Listen-Fehler darf das nicht tun — sonst fällt der Prozess
/// 24 h auf den einkompilierten Default zurück.
pub fn invalidate() {
    if let Ok(mut guard) = cell().write() {
        *guard = None;
    }
}

/// Pool, den der Startlauf hinterlegt hat. Der 404-Pfad reicht ihn durch,
/// damit der geheilte Name in `llm_model_cache` landet.
pub fn model_cache_pool() -> Option<&'static PgPool> {
    REGISTERED_POOL.get()
}

fn resolve_pool(pool: Option<&PgPool>) -> Option<&PgPool> {
    pool.or_else(|| REGISTERED_POOL.get())
}

/// Ein Eintrag aus der Modellliste des Anbieters.
#[derive(Debug, Clone, PartialEq, Eq)]
struct ModelEntry {
    id: String,
    created: Option<i64>,
}

/// Gehört `id` zur Familie, ohne Varianten wie `-lite`/`-preview`/`-thinking`?
///
/// Erlaubt ist der nackte Familienname und eine rein numerische Datums-
/// Endung (`…-0731`). Alles andere ist eine andere Produktlinie.
fn in_family(id: &str, family: &str) -> bool {
    let Some(rest) = id.strip_prefix(family) else {
        return false;
    };
    if rest.is_empty() {
        return true;
    }
    let Some(suffix) = rest.strip_prefix('-') else {
        return false;
    };
    !suffix.is_empty() && suffix.chars().all(|c| c.is_ascii_digit())
}

/// Wählt aus einer Modellliste die neueste Fassung der Familie.
///
/// Zuerst nach Name absteigend — die datierten Fassungen sortieren
/// lexikografisch richtig, weil das Datum als `MMTT` hinten steht. Der
/// Anbieter-Zeitstempel ist nur Tiebreak. `created = None` darf deshalb
/// nicht gegen einen älteren Eintrag mit Zeitstempel verlieren.
///
/// Reine Funktion, damit die Auswahlregel ohne Netz testbar bleibt.
fn pick_newest(entries: &[ModelEntry], family: &str) -> Option<String> {
    entries
        .iter()
        .filter(|entry| in_family(&entry.id, family))
        .max_by(|a, b| {
            a.id.as_str()
                .cmp(b.id.as_str())
                .then_with(|| a.created.cmp(&b.created))
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

    let pool = resolve_pool(pool);
    let api_key = crate::keys::fireworks_api_key()?;
    let base_url = crate::selection::fireworks_base_url();

    match fetch_models(&base_url, &api_key).await {
        Ok(entries) => {
            let created = entries
                .iter()
                .filter(|entry| in_family(&entry.id, FIREWORKS_FAMILY))
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

/// Nach einem 404: einmal neu auflösen. Liefert den neuen Namen, wenn er
/// sich vom abgelehnten unterscheidet — nur dann lohnt der
/// Wiederholungsversuch. Der alte Cache bleibt stehen, bis `refresh`
/// wirklich etwas Neues ablegt; ein Timeout oder 5xx der Liste darf die
/// Zelle nicht leeren.
pub async fn invalidate_and_refresh(pool: Option<&PgPool>, abgelehnt: &str) -> Option<String> {
    warn!(
        model = %abgelehnt,
        "Anbieter kennt das Modell nicht mehr (404) — loese neu auf"
    );
    let neu = refresh_fireworks(resolve_pool(pool)).await?;
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
/// fallen lässt, hat trotzdem die erste Auflösung. Der Pool bleibt für den
/// 404-Pfad hinterlegt, damit ein geheilter Name die DB erreicht.
pub fn spawn_refresh_loop(pool: Option<PgPool>) -> tokio::task::JoinHandle<()> {
    if let Some(pool) = pool.clone() {
        let _ = REGISTERED_POOL.set(pool);
    }
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
    fn fassung_ohne_zeitstempel_verliert_nicht_gegen_tote() {
        // Die tote Basis hat einen Zeitstempel, die neue datierte Fassung
        // nicht. `None < Some(_)` darf hier nicht die tote gewinnen lassen.
        let entries = vec![
            entry(
                "accounts/fireworks/models/deepseek-v4-flash",
                Some(1_753_000_000),
            ),
            entry("accounts/fireworks/models/deepseek-v4-flash-0731", None),
        ];
        assert_eq!(
            pick_newest(&entries, FIREWORKS_FAMILY).as_deref(),
            Some("accounts/fireworks/models/deepseek-v4-flash-0731")
        );
    }

    #[test]
    fn lite_preview_thinking_gehoeren_nicht_zur_familie() {
        let entries = vec![
            entry(
                "accounts/fireworks/models/deepseek-v4-flash-lite",
                Some(9_999),
            ),
            entry(
                "accounts/fireworks/models/deepseek-v4-flash-preview",
                Some(9_999),
            ),
            entry(
                "accounts/fireworks/models/deepseek-v4-flash-thinking",
                Some(9_999),
            ),
            entry("accounts/fireworks/models/deepseek-v4-flash-0731", Some(1)),
        ];
        assert_eq!(
            pick_newest(&entries, FIREWORKS_FAMILY).as_deref(),
            Some("accounts/fireworks/models/deepseek-v4-flash-0731")
        );
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

    fn clear_resolver_env() {
        for name in [
            "FIREWORK_API_KEY",
            "FIREWORKS_API_KEY",
            "FIREWORK_BASE_URL",
            "FIREWORKS_BASE_URL",
            "FIREWORK_MODEL",
            "FIREWORKS_MODEL",
        ] {
            std::env::remove_var(name);
        }
    }

    #[tokio::test]
    #[allow(clippy::await_holding_lock)]
    async fn transienter_listenfehler_leert_die_zelle_nicht() {
        let _guard = crate::model_resolver::TEST_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        clear_resolver_env();
        let bekannt = "accounts/fireworks/models/deepseek-v4-flash-0731";
        store(bekannt);

        let server = wiremock::MockServer::start().await;
        wiremock::Mock::given(wiremock::matchers::method("GET"))
            .and(wiremock::matchers::path("/models"))
            .respond_with(wiremock::ResponseTemplate::new(503))
            .expect(1)
            .mount(&server)
            .await;

        std::env::set_var("FIREWORK_API_KEY", "test-key");
        std::env::set_var("FIREWORK_BASE_URL", server.uri());

        let neu = invalidate_and_refresh(None, "accounts/fireworks/models/deepseek-v4-flash").await;
        assert!(neu.is_none(), "503 darf keinen neuen Namen liefern");
        assert_eq!(
            resolved_fireworks_model().as_deref(),
            Some(bekannt),
            "transientes Listen-500 darf den letzten guten Stand nicht verwerfen"
        );

        clear_resolver_env();
        invalidate();
    }

    #[tokio::test]
    #[allow(clippy::await_holding_lock)]
    async fn listen_ok_legt_neue_fassung_in_die_zelle() {
        let _guard = crate::model_resolver::TEST_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        clear_resolver_env();
        store("accounts/fireworks/models/deepseek-v4-flash");

        let server = wiremock::MockServer::start().await;
        wiremock::Mock::given(wiremock::matchers::method("GET"))
            .and(wiremock::matchers::path("/models"))
            .respond_with(
                wiremock::ResponseTemplate::new(200).set_body_json(serde_json::json!({
                    "data": [
                        {"id": "accounts/fireworks/models/deepseek-v4-flash", "created": 1_000},
                        {"id": "accounts/fireworks/models/deepseek-v4-flash-0731"}
                    ]
                })),
            )
            .expect(1)
            .mount(&server)
            .await;

        std::env::set_var("FIREWORK_API_KEY", "test-key");
        std::env::set_var("FIREWORK_BASE_URL", server.uri());

        let neu = invalidate_and_refresh(None, "accounts/fireworks/models/deepseek-v4-flash")
            .await
            .expect("Liste liefert die neue Fassung");
        assert_eq!(neu, "accounts/fireworks/models/deepseek-v4-flash-0731");
        assert_eq!(resolved_fireworks_model().as_deref(), Some(neu.as_str()));

        clear_resolver_env();
        invalidate();
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
