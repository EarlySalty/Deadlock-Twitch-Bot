//! Kleine crate-interne Helfer.

/// Maskiert eine Kennung fürs Logging (Python `_mask_log_identifier`): erste und
/// letzte 2 Zeichen, Mitte als `…`. Verhindert volle ID-Disclosure im Log; sehr
/// kurze IDs (≤ 4 Zeichen) werden komplett zu `…`.
pub(crate) fn mask_log_identifier(identifier: &str) -> String {
    let chars: Vec<char> = identifier.chars().collect();
    if chars.len() <= 4 {
        return "…".to_string();
    }
    let head: String = chars.iter().take(2).collect();
    let tail: String = chars
        .iter()
        .rev()
        .take(2)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    format!("{head}…{tail}")
}

#[cfg(test)]
mod tests {
    use super::mask_log_identifier;

    #[test]
    fn maskiert_lang_und_schuetzt_kurz() {
        assert_eq!(mask_log_identifier("123456789"), "12…89");
        assert_eq!(mask_log_identifier("ab"), "…");
    }
}
