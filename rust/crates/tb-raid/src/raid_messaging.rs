//! Pure Textbausteine für Raid-Arrival-Nachrichten (B3 2d, inhaltlicher Teil).
//!
//! Faithful-Port der Templates aus `bot/raid/services/partner_raid_delivery.py`.
//! Reine Präsentationslogik ohne Seiteneffekte — der Versand (Chat-Send,
//! Delay, Suppression, `received_raid_count`-Quelle) ist davon getrennt und
//! folgt als eigene Slice, sobald der Chat-Send-Pfad in den Arrival-Sink
//! eingefädelt ist.
//!
//! Bewusst NICHT enthalten: die Recruitment-Nachrichten an externe
//! Nicht-Partner-Channels — deren Versand ist im nativen Helix-Modell nicht
//! möglich (Bot ist dort nicht autorisiert; der Python-Pfad nutzt IRC-Join).

/// Singular/Plural für die Viewer-Anzahl (Python `_viewer_word`).
fn viewer_word(viewer_count: i32) -> &'static str {
    if viewer_count == 1 {
        "Viewer"
    } else {
        "Viewern"
    }
}

/// Dank-/Einordnungs-Nachricht an einen Partner, der gerade aus dem Netzwerk
/// geraidet wurde (Python `PartnerRaidDeliveryPlanner` Z. 155). Geht an den
/// Ziel-Channel (`to_broadcaster_id`).
///
/// `received_raid_count` ist die laufende Nummer dieses Netzwerk-Raids für das
/// Ziel (vom Aufrufer geliefert; Python klemmt < 1 auf 1).
pub fn build_partner_raid_message(
    from_login: &str,
    target_login: &str,
    viewer_count: i32,
    received_raid_count: i64,
) -> String {
    let count = received_raid_count.max(1);
    format!(
        "Hey @{target_login}! 🎮 @{from_login} hat dich gerade mit {viewer_count} {word} \
         geraidet. Das ist dein Raid Nr. {count} aus dem Deadlock Streamer-Netzwerk. ❤️",
        word = viewer_word(viewer_count),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn viewer_word_singular_vs_plural() {
        assert_eq!(viewer_word(1), "Viewer");
        assert_eq!(viewer_word(0), "Viewern");
        assert_eq!(viewer_word(2), "Viewern");
        assert_eq!(viewer_word(42), "Viewern");
    }

    #[test]
    fn partner_raid_message_matches_python_template() {
        let msg = build_partner_raid_message("raider", "victim", 12, 3);
        assert_eq!(
            msg,
            "Hey @victim! 🎮 @raider hat dich gerade mit 12 Viewern geraidet. \
             Das ist dein Raid Nr. 3 aus dem Deadlock Streamer-Netzwerk. ❤️"
        );
    }

    #[test]
    fn partner_raid_message_singular_viewer_and_count_floor() {
        // 1 Viewer → Singular; received_raid_count < 1 wird auf 1 geklemmt.
        let msg = build_partner_raid_message("a", "b", 1, 0);
        assert!(msg.contains("mit 1 Viewer geraidet"));
        assert!(msg.contains("dein Raid Nr. 1 "));
    }
}
