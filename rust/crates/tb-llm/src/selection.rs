//! Provider-Auswahl: welches Modell bedient welchen Anwendungsfall.
//!
//! Vorbild ist der Discord-Bot (`dl-ai`): ein Default plus Overrides pro
//! Anwendungsfall, alles über Umgebungsvariablen. So lässt sich ein einzelnes
//! Feature auf einen anderen Anbieter ziehen, ohne den Rest anzufassen.
//!
//! - `TB_LLM_PROVIDER_DEFAULT`: Basis für alles außer den Anthropic-Fällen
//!   in [`ANTHROPIC_USE_CASES`] und den Nur-Fireworks-Fällen in
//!   [`FIREWORKS_ONLY_USE_CASES`]; die ignorieren den globalen Default.
//! - `TB_LLM_PROVIDER_<USE_CASE>` — überschreibt einzeln, z. B.
//!   `TB_LLM_PROVIDER_INVITE_QUESTION=minimax`. Nur so lässt sich ein
//!   Anthropic-Fall auf einen anderen Anbieter legen.
//! - `TB_LLM_MODEL_<USE_CASE>`: überschreibt das Modell eines Anwendungsfalls
//!   beim gewählten Anbieter; das Ausweichglied einer Kette behält sein
//!   eigenes Standardmodell.
//!
//! Ohne jede Konfiguration gilt: Fireworks/DeepSeek, wenn ein Key da ist,
//! sonst MiniMax. Ein unbekannter Name fällt ebenfalls auf diesen Weg zurück
//! und wird geloggt — eine Namensverwechslung darf den Bot nicht stumm
//! schalten. Die Anwendungsfälle in [`ANTHROPIC_USE_CASES`] wählen ohne
//! Konfiguration Anthropic; das sind genau die, die ihn heute schon nutzen.
//!
//! [`endpoint_for`] ist die gemeinsame Basis: Sie liefert Adresse, Modell und
//! Key. Der HTTP-Weg dahinter liegt in [`crate::hub`] — hier steht nur, WOHIN
//! ein Aufruf geht.
//!
//! Alle Anbieter-Konstanten des Bots stehen in dieser Datei. Kopien in den
//! fachlichen Crates sind der Grund, warum ein Anbieterwechsel früher an fünf
//! Stellen passieren musste.

use std::sync::OnceLock;

use tracing::{info, warn};

/// Altvariablen, die seit der Zentralisierung nichts mehr bewirken, mit dem
/// Namen, der sie ersetzt. Wer sie noch setzt, bekommt beim ersten
/// `endpoint_for` eine Warnung statt stiller Wirkungslosigkeit.
const ALTVARIABLEN: &[(&str, &str)] = &[
    ("ENGAGEMENT_MINIMAX_MODEL", "TB_LLM_MODEL_ENGAGEMENT"),
    ("ANTHROPIC_HAIKU_MODEL", "TB_LLM_MODEL_SOCIAL_MEDIA_CLAUDE"),
    ("FIREWORKS_RICKY_REVIEW_MODEL", "TB_LLM_MODEL_RICKY_CREW_REVIEW"),
];

/// Gesetzte Altvariablen mit ihrem neuen Namen.
fn gesetzte_altvariablen() -> Vec<(&'static str, &'static str)> {
    ALTVARIABLEN
        .iter()
        .copied()
        .filter(|(alt, _)| nonempty_env(alt).is_some())
        .collect()
}

/// Warnt genau einmal je Prozess fuer jede gesetzte Altvariable. Liefert die
/// gewarnten Altnamen zurueck (leer bei jedem weiteren Aufruf), damit ein Test
/// die Einmaligkeit pruefen kann.
fn warne_altvariablen() -> &'static [&'static str] {
    static GEWARNT: OnceLock<Vec<&'static str>> = OnceLock::new();
    let mut frisch = false;
    let gewarnt = GEWARNT.get_or_init(|| {
        frisch = true;
        gesetzte_altvariablen()
            .into_iter()
            .map(|(alt, neu)| {
                warn!(
                    alt,
                    neu,
                    "Altvariable ohne Wirkung gesetzt; seit der LLM-Zentralisierung gilt der neue Name"
                );
                alt
            })
            .collect()
    });
    if frisch {
        gewarnt
    } else {
        &[]
    }
}

const PROVIDER_DEFAULT_ENV: &str = "TB_LLM_PROVIDER_DEFAULT";
/// Fireworks-Endpunkt (OpenAI-kompatibel), identisch zum Discord-Bot.
pub const FIREWORKS_BASE_URL: &str = "https://api.fireworks.ai/inference/v1";
/// DeepSeek über Fireworks. Benchmark 11.07.2026: 56/56 auf echten
/// Produktionsfällen (30 obfuskierter Spam, 26 harmlose Viewer-Sätze).
/// Die undatierte Fassung wurde am 2026-08-15 bei Fireworks abgeschaltet
/// (404); Nachfolger derselben Klasse ist die datierte 0731-Fassung,
/// verifiziert per Testcall mit echtem Judge-Prompt (scam, confidence 0.95).
pub const FIREWORKS_DEFAULT_MODEL: &str = "accounts/fireworks/models/deepseek-v4-flash-0731";
/// MiniMax-Endpunkt (OpenAI-kompatibel).
pub const MINIMAX_BASE_URL: &str = "https://api.minimax.io/v1";
/// MiniMax-Modell-Lock.
pub const MINIMAX_DEFAULT_MODEL: &str = "MiniMax-M3";
/// Anthropic-Messages-API.
pub const ANTHROPIC_BASE_URL: &str = "https://api.anthropic.com/v1/messages";
/// Anthropic-Standardmodell (Premium-Pfad).
pub const ANTHROPIC_DEFAULT_MODEL: &str = "claude-opus-4-6";
/// Anthropic-Modell für die Clip-Anreicherung: klein, günstig, schnell.
pub const ANTHROPIC_HAIKU_MODEL: &str = "claude-haiku-4-5-20251001";

/// Anwendungsfälle, die ohne Konfiguration Anthropic wählen, mit optionalem
/// eigenem Standardmodell. Der Rest des Bots läuft über Fireworks/MiniMax.
///
/// Diese Liste ersetzt die früher fest verdrahteten Anthropic-Aufrufe in
/// `claude_chat.rs`, `post_stream.rs` und `llm_dispatch.rs`. Ein Wechsel eines
/// dieser Fälle auf einen anderen Anbieter geht jetzt über
/// `TB_LLM_PROVIDER_<USE_CASE>`, ohne Codeänderung.
pub const ANTHROPIC_USE_CASES: &[(&str, Option<&str>)] = &[
    ("ai_analysis", None),
    ("ai_chat", None),
    ("post_stream_opus", None),
    ("social_media_claude", Some(ANTHROPIC_HAIKU_MODEL)),
];

/// Anwendungsfaelle, die nur auf Fireworks laufen duerfen (fail-closed in
/// `crew_review.rs` und `outreach_shadow.rs`). Wie die Anthropic-Faelle
/// ignorieren sie `TB_LLM_PROVIDER_DEFAULT`: ein globales `minimax` fuer den
/// Rest des Bots wuerde beide Schatten-Reviews sonst still abschalten.
/// Umgelenkt (und damit abgeschaltet) werden sie nur ueber ihre eigene
/// `TB_LLM_PROVIDER_<USE_CASE>`-Variable; das wird beim Aufloesen gewarnt.
pub const FIREWORKS_ONLY_USE_CASES: &[&str] = &["ricky_crew_review", "outreach_shadow"];

/// Adresse, Modell und Key eines Anbieters — alles, was ein
/// OpenAI-kompatibler Call braucht. Der Key wird nie geloggt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LlmEndpoint {
    pub provider: &'static str,
    pub base_url: String,
    pub model: String,
    pub api_key: Option<String>,
}

/// Endpunkt für einen Anwendungsfall (z. B. `"invite_question"`).
pub fn endpoint_for(use_case: &str) -> LlmEndpoint {
    warne_altvariablen();
    let env_name = format!("TB_LLM_PROVIDER_{}", use_case.to_uppercase());
    // Die Anthropic-Anwendungsfaelle sind der Premium-Pfad. Ein globaler
    // `TB_LLM_PROVIDER_DEFAULT` (z. B. "minimax" fuer den Rest des Bots) darf
    // sie nicht stillschweigend auf ein anderes Produkt ziehen; umgelenkt
    // werden sie nur ueber ihre eigene `TB_LLM_PROVIDER_<USE_CASE>`-Variable.
    let eigene_variable = nonempty_env(&env_name);
    let configured = eigene_variable.clone().or_else(|| {
        if anthropic_default_model(use_case).is_some() || ist_nur_fireworks(use_case) {
            None
        } else {
            nonempty_env(PROVIDER_DEFAULT_ENV)
        }
    });
    let mut endpoint = resolve(configured.as_deref(), use_case);
    if ist_nur_fireworks(use_case) && endpoint.provider != "fireworks" {
        // Zwei Ursachen, zwei Meldungen: bewusst umgelenkt oder schlicht
        // kein Fireworks-Schluessel (dann faellt die Auto-Auswahl auf MiniMax).
        if eigene_variable.is_some() {
            warn!(
                use_case,
                provider = endpoint.provider,
                variable = %env_name,
                "Nur-Fireworks-Anwendungsfall per eigener Variable auf anderen Anbieter gelenkt; der Aufrufer schaltet sich damit ab"
            );
        } else {
            warn!(
                use_case,
                provider = endpoint.provider,
                "Nur-Fireworks-Anwendungsfall ohne Fireworks-Schluessel (FIREWORK_API_KEY); der Aufrufer schaltet sich damit ab"
            );
        }
    }
    apply_model_override(&mut endpoint, use_case);
    endpoint
}

/// Das Modell eines Anwendungsfalls lässt sich unabhängig vom Anbieter
/// umstellen. Diese eine Variable ersetzt die früheren Sonderformen
/// `ENGAGEMENT_MINIMAX_MODEL`, `FIREWORKS_RICKY_REVIEW_MODEL` und
/// `ANTHROPIC_HAIKU_MODEL`. Ein Modellname gehoert zu einem Anbieter; deshalb
/// gilt der Override nur fuer Glieder mit demselben Anbieter wie das
/// bevorzugte, das Ausweichglied behaelt sein Anbieter-Standardmodell.
fn apply_model_override(endpoint: &mut LlmEndpoint, use_case: &str) {
    if let Some(model) = nonempty_env(&format!("TB_LLM_MODEL_{}", use_case.to_uppercase())) {
        endpoint.model = model;
    }
}

/// Endpunkt-Kette für einen Anwendungsfall: bevorzugter Anbieter zuerst, der
/// andere als Ausweichweg. Aufrufer mit Failover (der Spam-Judge) arbeiten die
/// Kette ab, statt beim ersten Fehler aufzugeben. Einträge ohne Key entfallen.
pub fn endpoint_chain(use_case: &str) -> Vec<LlmEndpoint> {
    let primary = endpoint_for(use_case);
    // Anthropic ist der Premium-Pfad: ein stiller Rückfall auf ein kleineres
    // Modell wäre kein Ausweichweg, sondern ein anderes Produkt.
    if primary.provider == "anthropic" {
        return [primary]
            .into_iter()
            .filter(|endpoint| endpoint.api_key.is_some())
            .collect();
    }
    // Das Ausweichglied ist immer der jeweils andere Anbieter und bekommt
    // bewusst keinen Modell-Override: `fireworks` mit `MiniMax-Text-01` waere
    // ein sicher scheiternder Aufruf.
    let fallback = if primary.provider == "fireworks" {
        minimax_endpoint()
    } else {
        fireworks_endpoint()
    };
    [primary, fallback]
        .into_iter()
        .filter(|endpoint| endpoint.api_key.is_some())
        .collect()
}

fn ist_nur_fireworks(use_case: &str) -> bool {
    FIREWORKS_ONLY_USE_CASES.contains(&use_case)
}

fn resolve(configured: Option<&str>, use_case: &str) -> LlmEndpoint {
    match configured.map(str::trim).map(str::to_lowercase).as_deref() {
        Some("minimax") => minimax_endpoint(),
        Some("fireworks") | Some("deepseek") => fireworks_endpoint(),
        Some("anthropic") | Some("claude") => anthropic_endpoint(use_case),
        Some(other) => {
            warn!(
                provider = other,
                use_case, "unbekannter LLM-Provider konfiguriert, nutze Auto-Auswahl"
            );
            auto(use_case)
        }
        None => auto(use_case),
    }
}

/// Fireworks, wenn ein Key vorliegt — sonst MiniMax. Ohne diesen Check würde
/// eine fehlende Fireworks-Konfiguration erst beim ersten Call auffallen.
fn auto(use_case: &str) -> LlmEndpoint {
    if anthropic_default_model(use_case).is_some() {
        let anthropic = anthropic_endpoint(use_case);
        info!(use_case, model = %anthropic.model, "LLM-Provider: Anthropic");
        return anthropic;
    }
    let fireworks = fireworks_endpoint();
    if fireworks.api_key.is_some() {
        info!(
            use_case,
            model = %fireworks.model,
            "LLM-Provider: Fireworks"
        );
        return fireworks;
    }
    let fallback = minimax_endpoint();
    info!(use_case, model = %fallback.model, "LLM-Provider: MiniMax");
    fallback
}

/// Adresse der Fireworks-API. Auch der Modell-Resolver fragt hierüber, damit
/// eine umgebogene Basis-URL (Test-Proxy) für beide Wege gilt.
pub(crate) fn fireworks_base_url() -> String {
    nonempty_env("FIREWORK_BASE_URL")
        .or_else(|| nonempty_env("FIREWORKS_BASE_URL"))
        .unwrap_or_else(|| FIREWORKS_BASE_URL.to_string())
}

/// Ein ausdrücklich festgelegtes Modell, falls gesetzt. Diese Festlegung
/// schlägt den Resolver: wer einen Namen einträgt, will genau den.
pub(crate) fn pinned_fireworks_model() -> Option<String> {
    nonempty_env("FIREWORK_MODEL").or_else(|| nonempty_env("FIREWORKS_MODEL"))
}

/// Modellname in der Rangfolge Festlegung, Resolver, einkompilierter Default.
///
/// Der Default ist bewusst der letzte Notnagel: er altert mit dem Binary und
/// war am 15.08.2026 einen ganzen Tag lang falsch, ohne dass es auffiel.
fn fireworks_model() -> String {
    pinned_fireworks_model()
        .or_else(crate::model_resolver::resolved_fireworks_model)
        .unwrap_or_else(|| FIREWORKS_DEFAULT_MODEL.to_string())
}

fn fireworks_endpoint() -> LlmEndpoint {
    LlmEndpoint {
        provider: "fireworks",
        base_url: fireworks_base_url(),
        model: fireworks_model(),
        api_key: crate::keys::fireworks_api_key(),
    }
}

fn minimax_endpoint() -> LlmEndpoint {
    LlmEndpoint {
        provider: "minimax",
        base_url: nonempty_env("MINIMAX_BASE_URL").unwrap_or_else(|| MINIMAX_BASE_URL.to_string()),
        model: nonempty_env("MINIMAX_MODEL").unwrap_or_else(|| MINIMAX_DEFAULT_MODEL.to_string()),
        api_key: crate::keys::minimax_api_key(),
    }
}

/// Standardmodell eines Anthropic-Anwendungsfalls, falls er eines hat.
/// `Some(None)` gibt es nicht: `None` heißt "kein Anthropic-Anwendungsfall".
fn anthropic_default_model(use_case: &str) -> Option<Option<&'static str>> {
    ANTHROPIC_USE_CASES
        .iter()
        .find(|(name, _)| *name == use_case)
        .map(|(_, model)| *model)
}

/// Anthropic-Endpunkt. Ein Anwendungsfall mit eigenem Standardmodell (die
/// Clip-Anreicherung mit Haiku) ignoriert `ANTHROPIC_MODEL`: sonst zöge ein
/// globaler Opus-Schalter den günstigen Pfad mit hoch.
fn anthropic_endpoint(use_case: &str) -> LlmEndpoint {
    let model = match anthropic_default_model(use_case).flatten() {
        Some(fest) => fest.to_string(),
        None => nonempty_env("ANTHROPIC_MODEL")
            .unwrap_or_else(|| ANTHROPIC_DEFAULT_MODEL.to_string()),
    };
    LlmEndpoint {
        provider: "anthropic",
        base_url: anthropic_messages_url(
            &nonempty_env("ANTHROPIC_BASE_URL").unwrap_or_else(|| ANTHROPIC_BASE_URL.to_string()),
        ),
        model,
        api_key: crate::keys::anthropic_api_key(),
    }
}

/// `ANTHROPIC_BASE_URL` gilt wie im SDK als Host-Wurzel
/// (`https://api.anthropic.com`); der Messages-Pfad wird angehaengt. Steht er
/// schon drin (alte Konfigurationen, Tests mit Mock-Server), bleibt er einfach.
fn anthropic_messages_url(raw: &str) -> String {
    let wurzel = raw.trim().trim_end_matches('/');
    if wurzel.ends_with("/v1/messages") {
        wurzel.to_string()
    } else {
        format!("{wurzel}/v1/messages")
    }
}

fn nonempty_env(var: &str) -> Option<String> {
    std::env::var(var).ok().filter(|v| !v.trim().is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    // Prozessweiter Lock fuer alle Tests dieser Crate: Sowohl die
    // Resolver-Zelle in model_resolver als auch die Umgebungsvariablen sind
    // global, parallel laufende Tests duerfen sie nicht gleichzeitig
    // setzen oder lesen.
    use crate::model_resolver::TEST_LOCK as ENV_LOCK;

    fn clear() {
        for v in [
            PROVIDER_DEFAULT_ENV,
            "ENGAGEMENT_MINIMAX_MODEL",
            "ANTHROPIC_HAIKU_MODEL",
            "FIREWORKS_RICKY_REVIEW_MODEL",
            "TB_LLM_PROVIDER_INVITE_QUESTION",
            "FIREWORK_API_KEY",
            "FIREWORKS_API_KEY",
            "FIREWORK_BASE_URL",
            "FIREWORKS_BASE_URL",
            "FIREWORKS_MODEL",
            "FIREWORK_MODEL",
            "MINIMAX_API_KEY",
            "MINIMAX_TOKEN_PLAN_KEY",
            "MINIMAX_MODEL",
            "MINMAX",
            "ANTHROPIC_API_KEY",
            "ANTHROPIC_MODEL",
            "ANTHROPIC_BASE_URL",
            "TB_LLM_PROVIDER_AI_ANALYSIS",
            "TB_LLM_MODEL_TITLE_AI",
            "TB_LLM_MODEL_SPAM_JUDGE",
            "TB_LLM_PROVIDER_OUTREACH_SHADOW",
        ] {
            std::env::remove_var(v);
        }
    }

    #[test]
    fn use_case_override_schlaegt_default() {
        let _g = ENV_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        clear();
        std::env::set_var(PROVIDER_DEFAULT_ENV, "minimax");
        std::env::set_var("TB_LLM_PROVIDER_INVITE_QUESTION", "fireworks");

        assert_eq!(endpoint_for("invite_question").provider, "fireworks");
        assert_eq!(endpoint_for("title_ai").provider, "minimax");
        clear();
    }

    #[test]
    fn ohne_konfiguration_entscheidet_der_key() {
        let _g = ENV_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        clear();
        assert_eq!(endpoint_for("default").provider, "minimax");

        std::env::set_var("FIREWORK_API_KEY", "k");
        let endpoint = endpoint_for("default");
        assert_eq!(endpoint.provider, "fireworks");
        assert_eq!(endpoint.model, FIREWORKS_DEFAULT_MODEL);
        assert_eq!(endpoint.base_url, FIREWORKS_BASE_URL);
        clear();
    }

    #[test]
    fn deepseek_ist_ein_alias_fuer_fireworks() {
        let _g = ENV_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        clear();
        std::env::set_var(PROVIDER_DEFAULT_ENV, "deepseek");
        assert_eq!(endpoint_for("default").provider, "fireworks");
        clear();
    }

    #[test]
    fn modell_ist_ueberschreibbar() {
        let _g = ENV_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        clear();
        std::env::set_var(PROVIDER_DEFAULT_ENV, "fireworks");
        std::env::set_var("FIREWORKS_MODEL", "accounts/fireworks/models/eigenes");
        assert_eq!(
            endpoint_for("default").model,
            "accounts/fireworks/models/eigenes"
        );
        clear();
    }

    #[test]
    fn fireworks_base_url_bevorzugt_singular_alias() {
        let _g = ENV_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        clear();
        std::env::set_var(PROVIDER_DEFAULT_ENV, "fireworks");
        std::env::set_var("FIREWORK_BASE_URL", "https://singular.example/v1");
        std::env::set_var("FIREWORKS_BASE_URL", "https://plural.example/v1");
        std::env::set_var("FIREWORK_MODEL", "singular-model");
        std::env::set_var("FIREWORKS_MODEL", "plural-model");

        let endpoint = endpoint_for("default");
        assert_eq!(endpoint.base_url, "https://singular.example/v1");
        assert_eq!(endpoint.model, "singular-model");
        clear();
    }

    #[test]
    fn kette_stellt_den_gewaehlten_anbieter_nach_vorn() {
        let _g = ENV_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        clear();
        // Nur MiniMax konfiguriert: kein Fireworks-Eintrag ohne Key.
        std::env::set_var("MINIMAX_API_KEY", "m");
        let chain = endpoint_chain("default");
        assert_eq!(chain.len(), 1);
        assert_eq!(chain[0].provider, "minimax");

        // Beide Keys: Fireworks führt, MiniMax bleibt als Ausweichweg.
        std::env::set_var("FIREWORK_API_KEY", "f");
        let chain = endpoint_chain("default");
        assert_eq!(chain.len(), 2);
        assert_eq!(chain[0].provider, "fireworks");
        assert_eq!(chain[1].provider, "minimax");

        // Umgeschaltet: MiniMax führt, Fireworks weicht aus.
        std::env::set_var(PROVIDER_DEFAULT_ENV, "minimax");
        let chain = endpoint_chain("default");
        assert_eq!(chain[0].provider, "minimax");
        assert_eq!(chain[1].provider, "fireworks");
        clear();
    }

    #[test]
    fn anthropic_anwendungsfaelle_waehlen_ohne_konfiguration_anthropic() {
        let _g = ENV_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        clear();
        std::env::set_var("ANTHROPIC_API_KEY", "a");

        let opus = endpoint_for("ai_analysis");
        assert_eq!(opus.provider, "anthropic");
        assert_eq!(opus.model, ANTHROPIC_DEFAULT_MODEL);

        // Die Clip-Anreicherung hat ihr eigenes, kleines Modell und darf nicht
        // von einem globalen ANTHROPIC_MODEL nach oben gezogen werden.
        std::env::set_var("ANTHROPIC_MODEL", "claude-riesig");
        assert_eq!(endpoint_for("ai_analysis").model, "claude-riesig");
        assert_eq!(
            endpoint_for("social_media_claude").model,
            ANTHROPIC_HAIKU_MODEL
        );

        // Ein Anthropic-Fall bekommt keinen stillen Rueckfall auf ein
        // kleineres Modell.
        std::env::set_var("FIREWORK_API_KEY", "f");
        let kette = endpoint_chain("ai_analysis");
        assert_eq!(kette.len(), 1);
        assert_eq!(kette[0].provider, "anthropic");
        clear();
    }

    #[test]
    fn modell_je_anwendungsfall_schlaegt_den_anbieter_default() {
        let _g = ENV_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        clear();
        std::env::set_var("FIREWORK_API_KEY", "f");
        std::env::set_var("TB_LLM_MODEL_TITLE_AI", "accounts/fireworks/models/klein");

        assert_eq!(
            endpoint_for("title_ai").model,
            "accounts/fireworks/models/klein"
        );
        // Andere Anwendungsfaelle bleiben unberuehrt.
        assert_eq!(endpoint_for("spam_judge").model, FIREWORKS_DEFAULT_MODEL);
        clear();
    }

    #[test]
    fn provider_default_zieht_anthropic_faelle_nicht_um() {
        let _g = ENV_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        clear();
        std::env::set_var("ANTHROPIC_API_KEY", "a");
        std::env::set_var("MINIMAX_API_KEY", "m");
        std::env::set_var(PROVIDER_DEFAULT_ENV, "minimax");

        // Der globale Default greift fuer den Rest des Bots ...
        assert_eq!(endpoint_for("title_ai").provider, "minimax");
        // ... laesst die Premium-Faelle aber in Ruhe.
        for (use_case, _) in ANTHROPIC_USE_CASES {
            assert_eq!(endpoint_for(use_case).provider, "anthropic", "{use_case}");
        }

        // Nur die eigene Variable lenkt einen Anthropic-Fall um.
        std::env::set_var("TB_LLM_PROVIDER_AI_ANALYSIS", "minimax");
        assert_eq!(endpoint_for("ai_analysis").provider, "minimax");
        assert_eq!(endpoint_for("ai_chat").provider, "anthropic");
        clear();
    }

    #[test]
    fn provider_default_zieht_nur_fireworks_faelle_nicht_um() {
        let _g = ENV_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        clear();
        std::env::set_var("FIREWORK_API_KEY", "f");
        std::env::set_var("MINIMAX_API_KEY", "m");
        std::env::set_var(PROVIDER_DEFAULT_ENV, "minimax");

        assert_eq!(endpoint_for("title_ai").provider, "minimax");
        for use_case in FIREWORKS_ONLY_USE_CASES {
            assert_eq!(endpoint_for(use_case).provider, "fireworks", "{use_case}");
        }

        // Nur die eigene Variable lenkt um (und schaltet den Aufrufer ab).
        std::env::set_var("TB_LLM_PROVIDER_OUTREACH_SHADOW", "minimax");
        assert_eq!(endpoint_for("outreach_shadow").provider, "minimax");
        assert_eq!(endpoint_for("ricky_crew_review").provider, "fireworks");
        clear();
    }

    #[test]
    fn modell_je_anwendungsfall_laesst_das_ausweichglied_in_ruhe() {
        let _g = ENV_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        clear();
        std::env::set_var("FIREWORK_API_KEY", "f");
        std::env::set_var("MINIMAX_API_KEY", "m");
        std::env::set_var("TB_LLM_MODEL_SPAM_JUDGE", "eigenes-fireworks-modell");

        let kette = endpoint_chain("spam_judge");
        assert_eq!(kette.len(), 2);
        assert_eq!(kette[0].provider, "fireworks");
        assert_eq!(kette[0].model, "eigenes-fireworks-modell");
        // Das MiniMax-Glied behaelt sein eigenes Modell; ein Fireworks-Name
        // an der MiniMax-Adresse waere ein sicherer Modellfehler.
        assert_eq!(kette[1].provider, "minimax");
        assert_eq!(kette[1].model, MINIMAX_DEFAULT_MODEL);
        clear();
    }

    #[test]
    fn altvariablen_werden_einmalig_gewarnt() {
        let _g = ENV_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        clear();
        // Die Warnung laeuft einmal je Prozess; deshalb muss dieser Test der
        // erste Aufrufer sein, der sie ausloest. Andere Tests im selben
        // Prozess koennen sie schon verbraucht haben, dann bleibt die Liste
        // leer, und wir pruefen nur die Einmaligkeit.
        std::env::set_var("ENGAGEMENT_MINIMAX_MODEL", "MiniMax-Text-01");
        std::env::set_var("FIREWORKS_RICKY_REVIEW_MODEL", "x");
        // Erkennung ist deterministisch pruefbar ...
        let gesetzt = gesetzte_altvariablen();
        assert_eq!(
            gesetzt,
            vec![
                ("ENGAGEMENT_MINIMAX_MODEL", "TB_LLM_MODEL_ENGAGEMENT"),
                ("FIREWORKS_RICKY_REVIEW_MODEL", "TB_LLM_MODEL_RICKY_CREW_REVIEW"),
            ]
        );
        // ... die Warnung selbst laeuft einmal je Prozess.
        let erste = warne_altvariablen();
        if !erste.is_empty() {
            assert!(erste.contains(&"ENGAGEMENT_MINIMAX_MODEL"));
            assert!(erste.contains(&"FIREWORKS_RICKY_REVIEW_MODEL"));
        }
        assert!(warne_altvariablen().is_empty(), "zweiter Aufruf warnt nicht erneut");
        std::env::remove_var("ENGAGEMENT_MINIMAX_MODEL");
        std::env::remove_var("FIREWORKS_RICKY_REVIEW_MODEL");
        clear();
    }

    #[test]
    fn anthropic_base_url_ist_host_wurzel_oder_voller_pfad() {
        assert_eq!(
            anthropic_messages_url("https://api.anthropic.com"),
            "https://api.anthropic.com/v1/messages"
        );
        assert_eq!(
            anthropic_messages_url("https://proxy.example/"),
            "https://proxy.example/v1/messages"
        );
        assert_eq!(
            anthropic_messages_url("https://api.anthropic.com/v1/messages"),
            "https://api.anthropic.com/v1/messages"
        );
        assert_eq!(
            anthropic_messages_url("https://api.anthropic.com/v1/messages/"),
            "https://api.anthropic.com/v1/messages"
        );

        let _g = ENV_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        clear();
        std::env::set_var("ANTHROPIC_API_KEY", "a");
        std::env::set_var("ANTHROPIC_BASE_URL", "http://127.0.0.1:9/");
        assert_eq!(endpoint_for("ai_chat").base_url, "http://127.0.0.1:9/v1/messages");
        std::env::set_var("ANTHROPIC_BASE_URL", "http://127.0.0.1:9/v1/messages");
        assert_eq!(endpoint_for("ai_chat").base_url, "http://127.0.0.1:9/v1/messages");
        clear();
    }

    #[test]
    fn unbekannter_name_faellt_zurueck_statt_zu_scheitern() {
        let _g = ENV_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        clear();
        std::env::set_var(PROVIDER_DEFAULT_ENV, "gibtsnicht");
        assert_eq!(endpoint_for("default").provider, "minimax");
        clear();
    }
}
