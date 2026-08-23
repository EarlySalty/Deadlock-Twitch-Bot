//! Bildung stabiler Dedupe-Schluessel.
//!
//! Ein Dock verbindet sich nach jedem OBS-Neustart neu und zieht ueber
//! `?seit=<id>` den Nachlauf. Damit dabei keine Dublette entsteht, braucht jedes
//! Ereignis einen Schluessel, der sich aus dem Ereignis selbst ergibt und nicht
//! aus dem Zeitpunkt der Verarbeitung.

use crate::platform::Platform;

/// Trennzeichen zwischen den Bestandteilen des Schluessels.
const TRENNER: char = ':';

/// Baut einen deterministischen Dedupe-Schluessel.
///
/// Aufbau: `<plattform>:<kanal>:<art>:<kennzeichen>`. Gleiches Ereignis ergibt
/// denselben Schluessel, egal wie oft und wann es gebildet wird.
///
/// `kennzeichen` muss innerhalb von Plattform, Kanal und Art eindeutig sein;
/// bei Twitch ist das in der Regel die EventSub-Message-ID oder die Chat-ID.
/// Enthaelt ein Bestandteil selbst ein `:`, wird es maskiert, damit
/// `a:b` + `c` nicht denselben Schluessel ergibt wie `a` + `b:c`.
pub fn dedupe_key(platform: Platform, channel_id: &str, art: &str, kennzeichen: &str) -> String {
    let mut key = String::with_capacity(
        platform.as_str().len() + channel_id.len() + art.len() + kennzeichen.len() + 3,
    );
    key.push_str(platform.as_str());
    key.push(TRENNER);
    maskiert_anhaengen(&mut key, channel_id);
    key.push(TRENNER);
    maskiert_anhaengen(&mut key, art);
    key.push(TRENNER);
    maskiert_anhaengen(&mut key, kennzeichen);
    key
}

/// Haengt einen Bestandteil an und maskiert dabei Trenner und Maskierzeichen.
fn maskiert_anhaengen(ziel: &mut String, teil: &str) {
    for zeichen in teil.chars() {
        match zeichen {
            '\\' => ziel.push_str("\\\\"),
            TRENNER => ziel.push_str("\\:"),
            sonst => ziel.push(sonst),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schluessel_ist_bei_gleicher_eingabe_stabil() {
        let a = dedupe_key(Platform::Twitch, "12345", "follow", "msg-1");
        let b = dedupe_key(Platform::Twitch, "12345", "follow", "msg-1");
        assert_eq!(a, b);
        assert_eq!(a, "twitch:12345:follow:msg-1");
    }

    #[test]
    fn jede_abweichung_ergibt_einen_anderen_schluessel() {
        let basis = dedupe_key(Platform::Twitch, "12345", "follow", "msg-1");
        assert_ne!(
            basis,
            dedupe_key(Platform::Kick, "12345", "follow", "msg-1")
        );
        assert_ne!(
            basis,
            dedupe_key(Platform::Twitch, "99999", "follow", "msg-1")
        );
        assert_ne!(
            basis,
            dedupe_key(Platform::Twitch, "12345", "cheer", "msg-1")
        );
        assert_ne!(
            basis,
            dedupe_key(Platform::Twitch, "12345", "follow", "msg-2")
        );
    }

    #[test]
    fn trenner_im_bestandteil_erzeugt_keine_kollision() {
        let links = dedupe_key(Platform::Twitch, "a:b", "follow", "c");
        let rechts = dedupe_key(Platform::Twitch, "a", "b:follow", "c");
        assert_ne!(links, rechts);
    }
}
