#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum UiLang {
    De,
    En,
}

pub const COMMUNITY_GUILD_ID: u64 = 1_289_721_245_281_292_288;

#[derive(Debug, Clone, Copy, Default)]
pub struct UiLangHints<'a> {
    pub guild_id: Option<u64>,
    pub guild_language: Option<&'a str>,
    pub stream_language: Option<&'a str>,
}

impl UiLang {
    pub fn from_code(code: Option<&str>) -> Self {
        let Some(raw) = code.map(str::trim).filter(|s| !s.is_empty()) else {
            return Self::De;
        };
        let lower = raw.to_ascii_lowercase();
        let primary = lower.split(['-', '_']).next().unwrap_or(lower.as_str());
        match primary {
            "en" => Self::En,
            _ => Self::De,
        }
    }

    pub fn resolve(hints: UiLangHints<'_>) -> Self {
        if hints.guild_id == Some(COMMUNITY_GUILD_ID) {
            return Self::De;
        }
        if let Some(code) = hints
            .guild_language
            .map(str::trim)
            .filter(|code| !code.is_empty())
        {
            return Self::from_code(Some(code));
        }
        Self::from_code(hints.stream_language)
    }

    pub fn pick<'a>(self, de: &'a str, en: &'a str) -> &'a str {
        match self {
            Self::De => de,
            Self::En => en,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_and_german_codes_stay_german() {
        assert_eq!(UiLang::from_code(None), UiLang::De);
        assert_eq!(UiLang::from_code(Some("")), UiLang::De);
        assert_eq!(UiLang::from_code(Some("de")), UiLang::De);
        assert_eq!(UiLang::from_code(Some("de-DE")), UiLang::De);
        assert_eq!(UiLang::from_code(Some("und")), UiLang::De);
        assert_eq!(UiLang::from_code(Some("fr")), UiLang::De);
    }

    #[test]
    fn english_codes_select_english() {
        assert_eq!(UiLang::from_code(Some("en")), UiLang::En);
        assert_eq!(UiLang::from_code(Some("EN")), UiLang::En);
        assert_eq!(UiLang::from_code(Some("en-gb")), UiLang::En);
        assert_eq!(UiLang::from_code(Some("en_US")), UiLang::En);
    }

    #[test]
    fn community_guild_stays_german() {
        let lang = UiLang::resolve(UiLangHints {
            guild_id: Some(COMMUNITY_GUILD_ID),
            guild_language: Some("en"),
            stream_language: Some("en"),
        });
        assert_eq!(lang, UiLang::De);
    }

    #[test]
    fn stream_language_used_without_guild_table() {
        let lang = UiLang::resolve(UiLangHints {
            guild_id: None,
            guild_language: None,
            stream_language: Some("en"),
        });
        assert_eq!(lang, UiLang::En);
    }

    #[test]
    fn guild_language_wins_over_stream() {
        let lang = UiLang::resolve(UiLangHints {
            guild_id: Some(1),
            guild_language: Some("en"),
            stream_language: Some("de"),
        });
        assert_eq!(lang, UiLang::En);
    }
}
