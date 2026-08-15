//! Entscheidungsmatrix für den Kanal-Bann des Bots.
//!
//! Die aktive Prüfung über `POST /moderation/moderators` hat `400 user is banned`
//! fälschlich als Bann gewertet. Gegenbeleg: der Bot liest in solchen Kanälen
//! weiter mit und sieht Chatter namentlich. Diese Datei hält die Reihenfolge
//! fest, nach der ein Signal überhaupt ein Zustand werden darf.

/// Harter Beweis, Gegenprobe und alles, was nur ein Verdacht bleibt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BanKlassifikation {
    /// Einziger harter Bann-Beweis: `sender_banned` beim Chat-Senden.
    Gebannt,
    /// Harter Gegenbeweis: der Bot sieht Chatter oder die Moderator-Einsetzung
    /// ist durchgegangen.
    NichtGebannt,
    /// Moderator-Endpunkt mit Ban-Body, ohne Gegenprobe. Kein Zustand.
    Verdacht,
    /// 401 oder abgelaufener Kanal-Token. Getrennt vom Bann.
    AuthFehler,
    /// Nicht genug Information.
    Unklar,
}

impl BanKlassifikation {
    pub fn als_text(self) -> &'static str {
        match self {
            Self::Gebannt => "gebannt",
            Self::NichtGebannt => "nicht_gebannt",
            Self::Verdacht => "verdacht",
            Self::AuthFehler => "auth_fehler",
            Self::Unklar => "unklar",
        }
    }
}

/// Klassifiziert in der Reihenfolge der Akte: erst der Sende-Drop, dann die
/// Chat-Lesbarkeit, dann Auth, zuletzt der Moderator-Body.
pub fn klassifiziere_ban(
    sender_banned: bool,
    chat_sichtbar: bool,
    moderator_ban_body: bool,
    auth_fehler: bool,
) -> BanKlassifikation {
    if sender_banned {
        return BanKlassifikation::Gebannt;
    }
    if chat_sichtbar {
        return BanKlassifikation::NichtGebannt;
    }
    if auth_fehler {
        return BanKlassifikation::AuthFehler;
    }
    if moderator_ban_body {
        return BanKlassifikation::Verdacht;
    }
    BanKlassifikation::Unklar
}

/// Env-Schalter für die Offline-Sendeprobe. Default aus: eine Testzeile im
/// Chat geht nie ungefragt live raus.
pub const SEND_PROBE_ENV: &str = "TB_BOT_BAN_SEND_PROBE";

pub fn send_probe_env_aktiv() -> bool {
    matches!(
        std::env::var(SEND_PROBE_ENV).ok().as_deref().map(str::trim),
        Some("1") | Some("true") | Some("TRUE") | Some("yes")
    )
}

/// Ausgang einer bewusst geschalteten Sendeprobe im Offline-Kanal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SendeprobeErgebnis {
    SenderBanned,
    Zugestellt,
    Timeout,
    KanalLive,
    NichtVerfuegbar,
}

/// Optionale Probe: eine Testzeile senden, solange der Streamer offline ist.
#[async_trait::async_trait]
pub trait OfflineSendeprobe: Send + Sync {
    async fn sendeprobe(&self, twitch_user_id: &str, twitch_login: &str) -> SendeprobeErgebnis;
}

/// Gegenprobe: sieht der Bot im Kanal Chatter, ist er nicht gebannt.
#[async_trait::async_trait]
pub trait ChatSichtbarkeit: Send + Sync {
    async fn sieht_chatter(&self, twitch_user_id: &str, twitch_login: &str) -> bool;
}

/// Admin-Meldung: Moderator-Einsetzung abgelehnt, der Bot liest aber mit.
pub fn admin_kein_mod_aber_sichtbar_text(
    twitch_login: &str,
    twitch_user_id: &str,
) -> (String, String) {
    let title = format!("Kein Moderator in {twitch_login}, kein Bann");
    let description = format!(
        "Die Moderator-Einsetzung in **{twitch_login}** wurde abgelehnt. \
         Ein Bann ist es nicht: der Bot sieht dort weiter Chatter.\n\n\
         Streamer: [{twitch_login}](https://twitch.tv/{twitch_login})\n\
         User ID: `{twitch_user_id}`\n\n\
         Pause, Blacklist und Streamer-Nachricht bleiben aus. \
         Diese Meldung kommt genau einmal pro Vorfall."
    );
    (title, description)
}

/// Admin-Meldung: Moderator-Body klingt nach Bann, Gegenprobe fehlt.
pub fn admin_ban_verdacht_text(twitch_login: &str, twitch_user_id: &str) -> (String, String) {
    let title = format!("Kein Bann-Beweis in {twitch_login}");
    let description = format!(
        "Twitch hat die Moderator-Einsetzung in **{twitch_login}** mit einem \
         Ban-Hinweis abgelehnt. Das allein ist kein Beweis: derselbe Body \
         kommt auch ohne Bann. Sichtbare Chatter gibt es gerade nicht.\n\n\
         Streamer: [{twitch_login}](https://twitch.tv/{twitch_login})\n\
         User ID: `{twitch_user_id}`\n\n\
         Pause, Blacklist und Streamer-Nachricht bleiben aus. Sicher wird es \
         erst bei einem `sender_banned` beim Senden. \
         Diese Meldung kommt genau einmal pro Vorfall."
    );
    (title, description)
}

/// Admin-Meldung: zuvor gemeldeter Zustand ist erledigt.
pub fn admin_zustand_erledigt_text(twitch_login: &str) -> (String, String) {
    let title = format!("Zustand in {twitch_login} erledigt");
    let description = format!(
        "In **{twitch_login}** trägt die Moderator-Einsetzung wieder. \
         Es ist nichts zu tun."
    );
    (title, description)
}

/// Admin-Meldung: eine `bot_banned`-Pause wurde vom Dienst selbst aufgehoben.
pub fn admin_bot_ban_aufgehoben_text(twitch_login: &str) -> (String, String) {
    let title = format!("Pause in {twitch_login} aufgehoben");
    let description = format!(
        "Die Markierung `bot_banned` in **{twitch_login}** ist vom Dienst \
         selbst zurückgenommen: der Bot sieht dort Chatter und ist deshalb \
         nicht gebannt."
    );
    (title, description)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sender_banned_schlaegt_alles() {
        assert_eq!(
            klassifiziere_ban(true, true, true, true),
            BanKlassifikation::Gebannt
        );
    }

    #[test]
    fn sichtbare_chatter_schlagen_moderator_400() {
        assert_eq!(
            klassifiziere_ban(false, true, true, false),
            BanKlassifikation::NichtGebannt
        );
    }

    #[test]
    fn moderator_400_ohne_chatter_ist_verdacht() {
        assert_eq!(
            klassifiziere_ban(false, false, true, false),
            BanKlassifikation::Verdacht
        );
    }

    #[test]
    fn auth_fehler_ist_kein_bann() {
        assert_eq!(
            klassifiziere_ban(false, false, true, true),
            BanKlassifikation::AuthFehler
        );
        assert_eq!(
            klassifiziere_ban(false, false, false, true),
            BanKlassifikation::AuthFehler
        );
    }

    #[test]
    fn ohne_signal_unklar() {
        assert_eq!(
            klassifiziere_ban(false, false, false, false),
            BanKlassifikation::Unklar
        );
    }

    #[test]
    fn send_probe_default_aus() {
        assert!(!send_probe_env_aktiv());
    }

    #[test]
    fn admin_texte_ohne_verbotene_woerter() {
        for text in [
            admin_kein_mod_aber_sichtbar_text("kanal", "1").1,
            admin_ban_verdacht_text("kanal", "1").1,
            admin_zustand_erledigt_text("kanal").1,
            admin_bot_ban_aufgehoben_text("kanal").1,
        ] {
            let lower = text.to_lowercase();
            assert!(!lower.contains("entmoddet"), "{text}");
            assert!(!lower.contains("token"), "{text}");
            assert!(!lower.contains("archiviert"), "{text}");
            assert!(!text.contains('—'), "{text}");
        }
    }
}
