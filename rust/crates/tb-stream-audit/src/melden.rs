//! Zustellung des Berichts als Discord-DM ueber den Master-Broker.
//!
//! Der Broker ist der einzige Weg, auf dem dieser Dienst Discord erreicht: Er
//! haelt den Bot-Token, dieser Dienst nicht. `POST /internal/master/v1/discord/send-dm`
//! mit `user_id` und `content`, authentifiziert per `X-Internal-Token`.
//!
//! Empfaenger ist der Admin, nicht der Streamer. Ein Coaching-Audit ist eine
//! private Einschaetzung ueber eine Person; sie geht an den, der sie angefordert
//! hat, und nicht automatisch an den Bewerteten.

use serde::Serialize;

/// Standard-Empfaenger, uebernommen aus `DEFAULT_ADMIN_DISCORD_USER_ID` der
/// Python-Fassung.
pub const STANDARD_EMPFAENGER: u64 = 662995601738170389;

/// Kopfzeile, mit der der Broker eine doppelte Zustellung erkennt.
pub const IDEMPOTENZ_KOPF: &str = "X-Idempotency-Key";

pub const BROKER_DM_PFAD: &str = "/internal/master/v1/discord/send-dm";

/// Discord kappt Nachrichten bei 2000 Zeichen. Eine abgeschnittene Fundliste
/// liest sich wie eine vollstaendige, deshalb wird hier bewusst gekuerzt und
/// die Kuerzung benannt.
pub const DISCORD_GRENZE: usize = 1900;

/// Der Broker nimmt `user_id` als Zahl. Als Zeichenkette weist er die
/// Anfrage ab - der Fund waere dann dreimal erfolglos wiederholt und
/// danach still verloren gewesen.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct DmAnfrage {
    pub user_id: u64,
    pub content: String,
}

/// Broker-Basis-URL nach derselben Kette wie der Rest des Bots:
/// `MASTER_BROKER_BASE_URL`, sonst `MASTER_BROKER_HOST`/`PORT` mit
/// `127.0.0.1:8770`.
pub fn broker_basis_url() -> String {
    if let Ok(url) = std::env::var("MASTER_BROKER_BASE_URL") {
        let url = url.trim().trim_end_matches('/');
        if !url.is_empty() {
            return url.to_owned();
        }
    }
    let host = std::env::var("MASTER_BROKER_HOST")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| "127.0.0.1".to_owned());
    let port = std::env::var("MASTER_BROKER_PORT")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| "8770".to_owned());
    format!("http://{host}:{port}")
}

/// Token nach derselben Fallback-Kette wie `self_explainer_log`.
pub fn broker_token() -> Option<String> {
    for name in [
        "MASTER_BROKER_TOKEN",
        "MAIN_BOT_INTERNAL_TOKEN",
        "TWITCH_INTERNAL_API_TOKEN",
    ] {
        if let Ok(wert) = std::env::var(name) {
            let wert = wert.trim().to_owned();
            if !wert.is_empty() {
                return Some(wert);
            }
        }
    }
    None
}

/// Empfaenger aus `STREAM_AUDIT_DISCORD_USER_ID`, sonst der Standard.
///
/// Ein unlesbarer Wert faellt auf den Standard zurueck: lieber die DM an den
/// bekannten Admin als gar keine.
pub fn empfaenger() -> u64 {
    std::env::var("STREAM_AUDIT_DISCORD_USER_ID")
        .ok()
        .and_then(|s| s.trim().parse::<u64>().ok())
        .unwrap_or(STANDARD_EMPFAENGER)
}

/// Kuerzt auf die Discord-Grenze und sagt es, statt still abzuschneiden.
pub fn kuerzen(text: &str) -> String {
    if text.chars().count() <= DISCORD_GRENZE {
        return text.to_owned();
    }
    let hinweis = "\n… gekuerzt, vollstaendig im Bericht";
    let platz = DISCORD_GRENZE.saturating_sub(hinweis.chars().count());
    let gekuerzt: String = text.chars().take(platz).collect();
    format!("{gekuerzt}{hinweis}")
}

pub fn anfrage(user_id: u64, content: &str) -> DmAnfrage {
    DmAnfrage {
        user_id,
        content: kuerzen(content),
    }
}

/// Stabiler Schluessel je Bericht.
///
/// Faellt die Antwort des Brokers in eine Zeitueberschreitung, obwohl die DM
/// ankam, wird der Block erneut ausgewertet und die DM erneut geschickt. Mit
/// demselben Schluessel erkennt der Broker die Wiederholung.
pub fn idempotenz_schluessel(lauf_id: &str) -> String {
    format!("stream-audit:{lauf_id}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kurzer_text_bleibt_unveraendert() {
        assert_eq!(kuerzen("kurz"), "kurz");
    }

    #[test]
    fn langer_text_wird_gekuerzt_und_benennt_die_kuerzung() {
        let lang = "a".repeat(DISCORD_GRENZE + 500);
        let gekuerzt = kuerzen(&lang);
        assert!(gekuerzt.chars().count() <= DISCORD_GRENZE);
        assert!(gekuerzt.ends_with("… gekuerzt, vollstaendig im Bericht"));
    }

    #[test]
    fn kuerzung_zaehlt_zeichen_nicht_bytes() {
        // Umlaute sind zwei Bytes; eine Byte-Grenze wuerde hier panicken.
        let lang = "ä".repeat(DISCORD_GRENZE + 100);
        let gekuerzt = kuerzen(&lang);
        assert!(gekuerzt.chars().count() <= DISCORD_GRENZE);
    }

    #[test]
    fn anfrage_traegt_empfaenger_und_gekuerzten_inhalt() {
        let a = anfrage(123, "hallo");
        assert_eq!(a.user_id, 123);
        assert_eq!(a.content, "hallo");
    }

    #[test]
    fn user_id_geht_als_zahl_ueber_die_leitung() {
        // Der Broker-Endpunkt erwartet eine Zahl; als Zeichenkette lehnt er ab.
        let json = serde_json::to_string(&anfrage(662995601738170389, "x")).expect("JSON");
        assert!(
            json.contains("\"user_id\":662995601738170389"),
            "unerwartet: {json}"
        );
    }

    #[test]
    fn idempotenz_schluessel_haengt_am_lauf() {
        assert_eq!(
            idempotenz_schluessel("20260813-ricky-1"),
            "stream-audit:20260813-ricky-1"
        );
        assert_ne!(
            idempotenz_schluessel("a"),
            idempotenz_schluessel("b"),
            "zwei Berichte duerfen nicht denselben Schluessel bekommen"
        );
    }

    #[test]
    fn broker_url_nutzt_host_und_port_wenn_keine_basis_gesetzt() {
        temp_env(&[("MASTER_BROKER_BASE_URL", None)], || {
            let url = broker_basis_url();
            assert!(url.starts_with("http://"), "unerwartet: {url}");
        });
    }

    #[test]
    fn broker_url_ohne_abschliessenden_schraegstrich() {
        temp_env(
            &[("MASTER_BROKER_BASE_URL", Some("http://127.0.0.1:9999/"))],
            || assert_eq!(broker_basis_url(), "http://127.0.0.1:9999"),
        );
    }

    #[test]
    fn leere_basis_url_faellt_auf_host_port_zurueck() {
        temp_env(
            &[
                ("MASTER_BROKER_BASE_URL", Some("   ")),
                ("MASTER_BROKER_HOST", Some("10.0.0.5")),
                ("MASTER_BROKER_PORT", Some("8123")),
            ],
            || assert_eq!(broker_basis_url(), "http://10.0.0.5:8123"),
        );
    }

    #[test]
    fn empfaenger_kann_ueberschrieben_werden() {
        temp_env(&[("STREAM_AUDIT_DISCORD_USER_ID", Some("42"))], || {
            assert_eq!(empfaenger(), 42)
        });
        temp_env(&[("STREAM_AUDIT_DISCORD_USER_ID", None)], || {
            assert_eq!(empfaenger(), STANDARD_EMPFAENGER)
        });
        // Unlesbarer Wert: lieber die DM an den bekannten Admin als keine.
        temp_env(&[("STREAM_AUDIT_DISCORD_USER_ID", Some("kein-id"))], || {
            assert_eq!(empfaenger(), STANDARD_EMPFAENGER)
        });
    }

    /// Setzt Umgebungsvariablen fuer die Dauer des Aufrufs und stellt sie danach
    /// wieder her. Tests laufen im selben Prozess, deshalb serialisiert.
    fn temp_env(paare: &[(&str, Option<&str>)], f: impl FnOnce()) {
        use std::sync::Mutex;
        static SPERRE: Mutex<()> = Mutex::new(());
        let _wache = SPERRE.lock().unwrap_or_else(|e| e.into_inner());
        let vorher: Vec<_> = paare
            .iter()
            .map(|(k, _)| (*k, std::env::var(k).ok()))
            .collect();
        for (k, v) in paare {
            match v {
                Some(wert) => std::env::set_var(k, wert),
                None => std::env::remove_var(k),
            }
        }
        f();
        for (k, v) in vorher {
            match v {
                Some(wert) => std::env::set_var(k, wert),
                None => std::env::remove_var(k),
            }
        }
    }
}
