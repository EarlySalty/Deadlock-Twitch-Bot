//! YouTube-Metadaten fuer ein archiviertes VOD.
//!
//! Bewusst getrennt vom Uploader: Titel, Beschreibung und Sichtbarkeit sind
//! die einzigen Stellen, an denen ein Archiv-Upload anders aussieht als ein
//! Shorts-Upload, und sie sind ohne Netz pruefbar.

use serde_json::{json, Value};

use crate::config::VodArchiveConfig;
use crate::twitch::vod_url;

const TITEL_MAX: usize = 100;
const BESCHREIBUNG_MAX: usize = 5000;

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
    template
        .replace("{title}", title)
        .replace("{date}", &datum_text)
        .replace("{channel}", channel)
        .replace("{part}", &teil)
        .chars()
        .take(TITEL_MAX)
        .collect()
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
    format!(
        "{title}\n\nTwitch-Stream vom {datum_text}\nOriginal: {}\nLive: https://www.twitch.tv/{channel}",
        vod_url(twitch_id)
    )
    .chars()
    .take(BESCHREIBUNG_MAX)
    .collect()
}

/// Vollstaendiger Metadatenblock fuer die resumable Session.
#[allow(clippy::too_many_arguments)]
pub fn baue_metadaten(
    cfg: &VodArchiveConfig,
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
                &cfg.channel,
                title,
                datum,
                teil_index,
                teil_anzahl,
            ),
            "description": baue_beschreibung(&cfg.channel, title, twitch_id, datum),
            "categoryId": cfg.category_id,
            "tags": ["Twitch", "VOD", cfg.channel.as_str()],
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
        let meta = baue_metadaten(&cfg, "Titel", "v1", datum(), 0, 1, "unlisted");
        assert_eq!(meta["status"]["privacyStatus"], "unlisted");
        assert_eq!(meta["snippet"]["categoryId"], "20");
        assert_eq!(meta["status"]["selfDeclaredMadeForKids"], false);
    }
}
