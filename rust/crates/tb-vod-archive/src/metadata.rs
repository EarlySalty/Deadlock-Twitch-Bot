//! YouTube-Metadaten fuer ein archiviertes VOD.
//!
//! Bewusst getrennt vom Uploader: Titel, Beschreibung und Sichtbarkeit sind
//! die einzigen Stellen, an denen ein Archiv-Upload anders aussieht als ein
//! Shorts-Upload, und sie sind ohne Netz pruefbar.

use serde_json::{json, Value};

use crate::config::VodArchiveConfig;
use crate::twitch::vod_url;

const TITEL_MAX: usize = 100;
const BESCHREIBUNG_MAX_BYTES: usize = 5000;

/// YouTube erlaubt keine spitzen Klammern. Das Chat-Herz bleibt als Herz erhalten.
fn bereinige_text(text: &str) -> String {
    text.replace("<3", "♥")
        .chars()
        .filter(|c| *c != '<' && *c != '>')
        .collect()
}

/// Setzt die Titelvorlage ein und kuerzt auf das YouTube-Limit.
pub fn baue_titel(
    template: &str,
    channel: &str,
    title: &str,
    datum: Option<chrono::NaiveDate>,
    teil_index: usize,
    teil_anzahl: usize,
) -> String {
    // Der Teil-Zusatz taucht nur auf, wenn wirklich geschnitten wurde.
    let teil = if teil_anzahl > 1 {
        format!(" (Teil {}/{})", teil_index + 1, teil_anzahl)
    } else {
        String::new()
    };
    let datum_text = datum.map(|d| d.to_string()).unwrap_or_default();
    let titel = template
        .replace("{title}", title)
        .replace("{date}", &datum_text)
        .replace("{channel}", channel)
        .replace("{part}", &teil);
    bereinige_text(&titel).chars().take(TITEL_MAX).collect()
}

pub fn baue_beschreibung(
    channel: &str,
    title: &str,
    twitch_id: &str,
    datum: Option<chrono::NaiveDate>,
) -> String {
    let datum_text = datum
        .map(|d| d.to_string())
        .unwrap_or_else(|| "unbekannt".to_string());
    let mut beschreibung = bereinige_text(&format!(
        "{title}\n\nTwitch-Stream vom {datum_text}\nOriginal: {}\nLive: https://www.twitch.tv/{channel}",
        vod_url(twitch_id)
    ));
    if beschreibung.len() > BESCHREIBUNG_MAX_BYTES {
        let mut ende = BESCHREIBUNG_MAX_BYTES;
        while !beschreibung.is_char_boundary(ende) {
            ende -= 1;
        }
        beschreibung.truncate(ende);
    }
    beschreibung
}

/// Vollstaendiger Metadatenblock fuer die resumable Session.
#[allow(clippy::too_many_arguments)]
pub fn baue_metadaten(
    cfg: &VodArchiveConfig,
    kanal: &str,
    title: &str,
    twitch_id: &str,
    datum: Option<chrono::NaiveDate>,
    teil_index: usize,
    teil_anzahl: usize,
    privacy: &str,
) -> Value {
    json!({
        "snippet": {
            "title": baue_titel(
                &cfg.title_template,
                kanal,
                title,
                datum,
                teil_index,
                teil_anzahl,
            ),
            "description": baue_beschreibung(kanal, title, twitch_id, datum),
            "categoryId": cfg.category_id,
            "tags": ["Twitch", "VOD", kanal],
        },
        "status": {
            "privacyStatus": privacy,
            "selfDeclaredMadeForKids": false,
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn datum() -> Option<chrono::NaiveDate> {
        chrono::NaiveDate::from_ymd_opt(2026, 8, 13)
    }

    #[test]
    fn einteiliges_vod_bekommt_keinen_teil_zusatz() {
        let titel = baue_titel(
            "{title} [{date}]{part}",
            "earlysalty",
            "Deadlock Ranked",
            datum(),
            0,
            1,
        );
        assert_eq!(titel, "Deadlock Ranked [2026-08-13]");
    }

    #[test]
    fn geschnittenes_vod_zaehlt_ab_eins() {
        let titel = baue_titel(
            "{title} [{date}]{part}",
            "earlysalty",
            "Langer Stream",
            datum(),
            1,
            3,
        );
        assert_eq!(titel, "Langer Stream [2026-08-13] (Teil 2/3)");
    }

    #[test]
    fn titel_wird_auf_hundert_zeichen_gekuerzt() {
        let titel = baue_titel("{title}", "earlysalty", &"a".repeat(300), datum(), 0, 1);
        assert_eq!(titel.chars().count(), TITEL_MAX);
    }

    #[test]
    fn metadaten_erhalten_herzen_ohne_verbotene_klammern() {
        let cfg = VodArchiveConfig::default();
        let meta = baue_metadaten(
            &cfg,
            "earlysalty",
            "Deadlock <3 <Ranked>",
            "v42",
            datum(),
            0,
            1,
            "unlisted",
        );
        for feld in ["title", "description"] {
            let text = meta["snippet"][feld].as_str().unwrap();
            assert!(text.contains("Deadlock ♥ Ranked"));
            assert!(!text.contains(['<', '>']));
        }
    }

    #[test]
    fn titelgrenze_gilt_nach_der_herz_umwandlung() {
        let text = format!("{}<3Ende", "a".repeat(98));
        let titel = baue_titel("{title}", "earlysalty", &text, datum(), 0, 1);
        assert_eq!(titel, format!("{}♥E", "a".repeat(98)));
        assert_eq!(titel.chars().count(), TITEL_MAX);
    }

    #[test]
    fn beschreibung_begrenzt_utf8_bytes_nach_der_bereinigung() {
        let titel = format!("{}<3{}", "ä".repeat(2499), "🎮".repeat(100));
        let beschreibung = baue_beschreibung("earlysalty", &titel, "v42", datum());
        assert_eq!(beschreibung, "ä".repeat(2499));
        assert!(beschreibung.len() <= BESCHREIBUNG_MAX_BYTES);
        assert!(!beschreibung.contains(['<', '>']));
    }

    #[test]
    fn fehlendes_datum_bricht_nichts() {
        let titel = baue_titel("{title} [{date}]", "earlysalty", "Ohne", None, 0, 1);
        assert_eq!(titel, "Ohne []");
        let text = baue_beschreibung("earlysalty", "Ohne", "v42", None);
        assert!(text.contains("vom unbekannt"));
        assert!(text.contains("https://www.twitch.tv/videos/42"));
    }

    #[test]
    fn sichtbarkeit_landet_im_status() {
        let cfg = VodArchiveConfig::default();
        let meta = baue_metadaten(&cfg, "earlysalty", "Titel", "v1", datum(), 0, 1, "unlisted");
        assert_eq!(meta["status"]["privacyStatus"], "unlisted");
        assert_eq!(meta["snippet"]["categoryId"], "20");
        assert_eq!(meta["status"]["selfDeclaredMadeForKids"], false);
    }

    #[test]
    fn der_kanal_kommt_vom_streamer_nicht_aus_der_konfiguration() {
        let cfg = VodArchiveConfig::default();
        let meta = baue_metadaten(&cfg, "nani", "Titel", "v1", datum(), 0, 1, "private");
        assert!(meta["snippet"]["description"]
            .as_str()
            .unwrap()
            .contains("https://www.twitch.tv/nani"));
        assert_eq!(meta["snippet"]["tags"][2], "nani");
    }
}
