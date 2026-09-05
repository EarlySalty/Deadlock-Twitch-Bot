pub const KNOWN_CHAT_BOTS: &[&str] = &[
    "botrix",
    "deutschedeadlockcommunity",
    "fossabot",
    "kofistreambot",
    "moobot",
    "nightbot",
    "own3d",
    "pretzelrocks",
    "soundalerts",
    "streamelements",
    "streamlabs",
    "wizebot",
];

pub const ANONYM_LOGIN_REGEX_SQL: &str = "^justinfan[0-9]+$";

pub fn ist_anonymer_login(login: &str) -> bool {
    match login.to_ascii_lowercase().strip_prefix("justinfan") {
        Some(rest) => !rest.is_empty() && rest.bytes().all(|b| b.is_ascii_digit()),
        None => false,
    }
}

pub fn ist_ausgeschlossener_login(login: &str) -> bool {
    let login = login.trim().to_lowercase();
    if login.is_empty() {
        return false;
    }
    KNOWN_CHAT_BOTS.contains(&login.as_str()) || ist_anonymer_login(&login)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn liste_ist_alphabetisch_und_ohne_duplikate() {
        let mut sortiert = KNOWN_CHAT_BOTS.to_vec();
        sortiert.sort_unstable();
        sortiert.dedup();
        assert_eq!(sortiert.as_slice(), KNOWN_CHAT_BOTS);
    }

    #[test]
    fn ausgeschlossene_logins() {
        assert!(ist_ausgeschlossener_login("own3d"));
        assert!(ist_ausgeschlossener_login("KofiStreamBot"));
        assert!(ist_ausgeschlossener_login("  StreamElements  "));
        assert!(ist_ausgeschlossener_login("justinfan12345"));
        assert!(ist_ausgeschlossener_login("justinfan99999"));
    }

    #[test]
    fn nicht_ausgeschlossene_logins() {
        assert!(!ist_ausgeschlossener_login("nani"));
        assert!(!ist_ausgeschlossener_login("justinfan"));
        assert!(!ist_ausgeschlossener_login("justinfanx"));
        assert!(!ist_ausgeschlossener_login(""));
        assert!(!ist_ausgeschlossener_login("   "));
    }

    #[test]
    fn anonym_erkennung() {
        assert!(ist_anonymer_login("justinfan1"));
        assert!(ist_anonymer_login("justinfan12345"));
        assert!(!ist_anonymer_login("justinfan"));
        assert!(!ist_anonymer_login("justinfanx"));
        assert!(ist_anonymer_login("Justinfan1"));
        assert!(!ist_anonymer_login("nani"));
    }
}
