#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum TextLanguage {
    #[default]
    German,
    English,
}

impl TextLanguage {
    pub fn from_code(code: Option<&str>) -> Self {
        match code.map(str::trim).and_then(|code| code.split('-').next()) {
            Some(code) if code.eq_ignore_ascii_case("en") => Self::English,
            _ => Self::German,
        }
    }

    pub fn choose<'a>(self, german: &'a str, english: &'a str) -> &'a str {
        match self {
            Self::German => german,
            Self::English => english,
        }
    }
}

#[macro_export]
macro_rules! localized_format {
    ($language:expr, $german:literal, $english:literal $(, $($args:tt)*)?) => {
        match $language {
            $crate::language::TextLanguage::German => format!($german $(, $($args)*)?),
            $crate::language::TextLanguage::English => format!($english $(, $($args)*)?),
        }
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_english_codes_select_english() {
        for code in [Some("en"), Some("en-US"), Some(" EN-gb ")] {
            assert_eq!(TextLanguage::from_code(code), TextLanguage::English);
        }
        for code in [None, Some(""), Some("de"), Some("fr"), Some("und")] {
            assert_eq!(TextLanguage::from_code(code), TextLanguage::German);
        }
    }
}
