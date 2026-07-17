//! Twitch-Login-Normalisierung — Port von `bot/core/twitch_login.py`.
//!
//! Pure Domänenlogik ohne externe Crates: manuelles Percent-Decode,
//! `urlsplit`-treues Host/Path-Splitting und der `^[a-z0-9_]{3,25}$`-Check.
//! Akzeptiert bloße Logins (`@Name`) und Twitch-Profil-URLs
//! (`https://twitch.tv/name`) und liefert den kanonischen lowercase-Login.

/// Reservierte erste Pfadsegmente von twitch.tv, die keine Kanäle sind.
const RESERVED_SEGMENTS: &[&str] = &[
    "clip",
    "clips",
    "dashboard",
    "directory",
    "downloads",
    "friends",
    "inventory",
    "jobs",
    "login",
    "messages",
    "p",
    "payments",
    "search",
    "settings",
    "signup",
    "subscriptions",
    "turbo",
    "videos",
    "wallet",
];
const TWITCH_HOST_SUFFIX: &str = "twitch.tv";

/// Normalisiert einen Twitch-Login oder eine Twitch-Profil-URL auf den
/// kanonischen Login (lowercase). `None` wenn ungültig.
///
/// Parität-treu zu Pythons `normalize_twitch_login` (`bot/core/twitch_login.py`):
/// Percent-Decode → `@`/Whitespace-Strip → optional URL-Auflösung (Host muss
/// `twitch.tv` oder `*.twitch.tv` sein, erstes Pfadsegment darf nicht reserviert
/// sein) → Regex-Validierung.
pub fn normalize_twitch_login(raw: &str) -> Option<String> {
    let decoded = percent_decode_lossy(raw);
    let value = decoded.trim();
    if value.is_empty() {
        return None;
    }
    // lstrip("@") + strip()
    let value = value.trim_start_matches('@').trim();
    let lowered = value.to_lowercase();

    let mut login = value.to_string();
    if lowered.contains("://") || lowered.contains(TWITCH_HOST_SUFFIX) {
        let candidate = if value.contains("://") {
            value.to_string()
        } else {
            format!("https://{value}")
        };
        let (host, path) = split_netloc_path(&candidate);
        let host = host.trim().to_lowercase();
        if !host.is_empty()
            && host != TWITCH_HOST_SUFFIX
            && !host.ends_with(&format!(".{TWITCH_HOST_SUFFIX}"))
        {
            return None;
        }
        // Erstes nicht-leeres Pfadsegment (Python: `segments[0]`).
        let segment = path.split('/').find(|s| !s.is_empty())?;
        if RESERVED_SEGMENTS.contains(&segment.to_lowercase().as_str()) {
            return None;
        }
        login = segment.to_string();
    }

    let login = login.trim().trim_start_matches('@').to_lowercase();
    if is_valid_login(&login) {
        Some(login)
    } else {
        None
    }
}

/// `^[a-z0-9_]{3,25}$` (Login ist hier bereits lowercase). Da gültige Zeichen
/// rein ASCII sind, ist Byte- == Zeichenlänge sobald `all()` zutrifft.
fn is_valid_login(s: &str) -> bool {
    (3..=25).contains(&s.len())
        && s.bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
}

/// Minimaler `urllib.parse.unquote`-Äquivalent: `%XX` → Byte, dann UTF-8 (lossy,
/// d.h. ungültige Sequenzen → U+FFFD wie Pythons `errors="replace"`).
fn percent_decode_lossy(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let (Some(h), Some(l)) = (hex_val(bytes[i + 1]), hex_val(bytes[i + 2])) {
                out.push((h << 4) | l);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn hex_val(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

/// Minimaler `urlsplit`-Äquivalent für die Felder `netloc` und `path`.
/// Genügt für unsere Eingaben (immer mit Scheme `xxx://`); pathologische
/// Eingaben fallen ohnehin durch die spätere Login-Regex — exakt wie bei Python.
fn split_netloc_path(candidate: &str) -> (String, String) {
    let mut rest = candidate;
    // Scheme bis erstes ':' abtrennen, wenn erstes Zeichen ein Buchstabe ist
    // und alle Scheme-Zeichen gültig sind (Python `urlsplit`-Verhalten).
    if let Some(colon) = candidate.find(':') {
        let scheme = &candidate[..colon];
        let first_alpha = scheme
            .as_bytes()
            .first()
            .is_some_and(|b| b.is_ascii_alphabetic());
        if colon > 0 && first_alpha && scheme.bytes().all(is_scheme_char) {
            rest = &candidate[colon + 1..];
        }
    }
    let mut netloc = "";
    if let Some(after) = rest.strip_prefix("//") {
        let end = after.find(['/', '?', '#']).unwrap_or(after.len());
        netloc = &after[..end];
        rest = &after[end..];
    }
    let path_end = rest.find(['?', '#']).unwrap_or(rest.len());
    (netloc.to_string(), rest[..path_end].to_string())
}

fn is_scheme_char(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'+' || b == b'-' || b == b'.'
}

#[cfg(test)]
mod tests {
    use super::normalize_twitch_login as n;

    #[test]
    fn plain_login_lowercased() {
        assert_eq!(n("DragScope").as_deref(), Some("dragscope"));
    }
    #[test]
    fn at_prefix_stripped() {
        assert_eq!(n("@Foo_Bar").as_deref(), Some("foo_bar"));
    }
    #[test]
    fn whitespace_trimmed() {
        assert_eq!(n("  helmi  ").as_deref(), Some("helmi"));
    }
    #[test]
    fn url_full_scheme() {
        assert_eq!(n("https://twitch.tv/Name").as_deref(), Some("name"));
    }
    #[test]
    fn url_no_scheme_prepended() {
        assert_eq!(n("twitch.tv/Name").as_deref(), Some("name"));
    }
    #[test]
    fn url_www_subdomain_ok() {
        assert_eq!(n("https://www.twitch.tv/Name").as_deref(), Some("name"));
    }
    #[test]
    fn url_query_stripped() {
        assert_eq!(n("https://twitch.tv/name?foo=1").as_deref(), Some("name"));
    }
    #[test]
    fn reserved_segment_rejected() {
        assert_eq!(n("https://twitch.tv/videos"), None);
    }
    #[test]
    fn wrong_host_rejected() {
        assert_eq!(n("https://evil.com/foo"), None);
    }
    #[test]
    fn host_lookalike_rejected() {
        assert_eq!(n("https://nottwitch.tv.evil.com/foo"), None);
    }
    #[test]
    fn url_without_segment_rejected() {
        assert_eq!(n("https://twitch.tv/"), None);
    }
    #[test]
    fn too_short_rejected() {
        assert_eq!(n("ab"), None);
    }
    #[test]
    fn too_long_rejected() {
        assert_eq!(n("a".repeat(26).as_str()), None);
    }
    #[test]
    fn invalid_char_rejected() {
        assert_eq!(n("bad-name"), None);
    }
    #[test]
    fn whitespace_only_rejected() {
        assert_eq!(n("   "), None);
    }
    #[test]
    fn percent_encoded_at_decoded() {
        assert_eq!(n("%40Helmi").as_deref(), Some("helmi"));
    }
    #[test]
    fn min_length_three_ok() {
        assert_eq!(n("abc").as_deref(), Some("abc"));
    }
}
