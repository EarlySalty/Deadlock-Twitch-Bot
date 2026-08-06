//! Signup-Block: Streamer, die nicht ins Partnerprogramm aufgenommen werden.
//!
//! Eigenständiger Zustand, bewusst getrennt von der Raid-Blacklist
//! (Raid-Ziel-Auswahl), vom Opt-out des Streamers und von der technischen
//! Zwangspause. Hier liegen nur die reinen Typen und der user-sichtbare
//! Absagetext; der DB-Zugriff liegt in `tb-raid` bzw. `tb-internal-api`.

/// Interner Grund-Präfix, mit dem ein Signup-Block seine Spur in
/// `twitch_raid_blacklist` markiert. Nur so markierte Raid-Einträge werden beim
/// Aufheben des Signup-Blocks wieder entfernt; fremde Gründe (Bot-Ban,
/// 4-Raid-Schwelle) bleiben unangetastet.
pub const RAID_BLACKLIST_REASON_PREFIX: &str = "signup_block:";

/// Grund-Marker, mit dem der Promotion-Pfad einen Signup-Block nach oben meldet.
pub const PROMOTE_BLOCK_REASON: &str = "signup_blocked";

/// Titel der Absage. User-sichtbar.
pub const SIGNUP_BLOCK_TITLE: &str = "Aufnahme ins Partnerprogramm nicht möglich";

/// Default-Absagetext. User-sichtbar. Absätze sind durch Leerzeilen getrennt.
/// Wird durch `public_message` aus der DB überschrieben, wenn dort ein Text steht.
pub const SIGNUP_BLOCK_BODY: &str = "Danke für dein Interesse an unserem Partnerprogramm.\n\nWir suchen Streamer, die uns und unsere Community repräsentieren. Nach interner Abwägung haben wir uns entschieden, dich nicht ins Partnerprogramm aufzunehmen.\n\nDiese Entscheidung wurde von uns getroffen und gilt bis auf Weiteres.";

/// Ein aktiver Signup-Block, wie ihn der Nachschlag liefert.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SignupBlock {
    pub twitch_user_id: String,
    pub twitch_login: String,
    /// Interner Grund. Gehört ins Log, niemals in eine Antwort an den Streamer.
    pub reason: String,
    /// Individueller Absagetext. `None` bedeutet Default aus [`SIGNUP_BLOCK_BODY`].
    pub public_message: Option<String>,
}

impl SignupBlock {
    /// Der Text, den der Streamer zu sehen bekommt.
    pub fn public_text(&self) -> &str {
        match self.public_message.as_deref() {
            Some(text) if !text.trim().is_empty() => text,
            _ => SIGNUP_BLOCK_BODY,
        }
    }

    /// Überschrift zum Absagetext. Konstant — der Grund gehört nicht in den
    /// Titel, weil `reason` intern ist.
    pub fn public_title(&self) -> &'static str {
        SIGNUP_BLOCK_TITLE
    }

    /// Absagetext als HTML-Absätze, Sonderzeichen escaped.
    pub fn public_body_html(&self) -> String {
        paragraphs_to_html(self.public_text())
    }
}

/// Wandelt durch Leerzeilen getrennte Absätze in `<p>`-Blöcke und escaped HTML.
pub fn paragraphs_to_html(text: &str) -> String {
    text.split("\n\n")
        .map(str::trim)
        .filter(|p| !p.is_empty())
        .map(|p| format!("<p>{}</p>", escape_html(p)))
        .collect::<Vec<_>>()
        .join("")
}

/// Minimales HTML-Escaping für Text, der in einen Element-Body geht.
pub fn escape_html(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    for ch in raw.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            '\n' => out.push_str("<br>"),
            _ => out.push(ch),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn block(public_message: Option<&str>) -> SignupBlock {
        SignupBlock {
            twitch_user_id: "173926844".into(),
            twitch_login: "temmiee985".into(),
            reason: "owner_decision:repraesentation".into(),
            public_message: public_message.map(str::to_string),
        }
    }

    #[test]
    fn default_text_wenn_kein_override() {
        assert_eq!(block(None).public_text(), SIGNUP_BLOCK_BODY);
        assert_eq!(block(Some("   ")).public_text(), SIGNUP_BLOCK_BODY);
    }

    #[test]
    fn override_schlaegt_default() {
        assert_eq!(block(Some("Eigener Text.")).public_text(), "Eigener Text.");
    }

    #[test]
    fn default_text_hat_drei_absaetze_und_echte_umlaute() {
        let html = block(None).public_body_html();
        assert_eq!(html.matches("<p>").count(), 3);
        assert!(html.contains("repräsentieren"));
        assert!(html.contains("Danke für dein Interesse"));
        // Der interne Grund darf nie im user-sichtbaren Text auftauchen.
        assert!(!html.contains("owner_decision"));
    }

    #[test]
    fn html_wird_escaped() {
        let html = block(Some("<script>alert(1)</script> & \"Zitat\"")).public_body_html();
        assert!(!html.contains("<script>"));
        assert!(html.contains("&lt;script&gt;"));
        assert!(html.contains("&amp;"));
        assert!(html.contains("&quot;Zitat&quot;"));
    }
}
