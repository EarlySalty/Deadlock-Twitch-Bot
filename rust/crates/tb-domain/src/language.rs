#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum DashboardLanguage {
    #[default]
    De,
    En,
}

impl DashboardLanguage {
    pub const HEADER: &'static str = "x-dashboard-language";

    pub fn from_header_value(value: Option<&str>) -> Self {
        match value.map(str::trim) {
            Some(raw) if raw.eq_ignore_ascii_case("en") => Self::En,
            _ => Self::De,
        }
    }

    pub fn pick<T>(self, de: T, en: T) -> T {
        match self {
            Self::De => de,
            Self::En => en,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::DashboardLanguage;

    #[test]
    fn header_value_defaults_to_german_and_en_selects_english() {
        assert_eq!(
            DashboardLanguage::from_header_value(None),
            DashboardLanguage::De
        );
        assert_eq!(
            DashboardLanguage::from_header_value(Some("de")),
            DashboardLanguage::De
        );
        assert_eq!(
            DashboardLanguage::from_header_value(Some("fr")),
            DashboardLanguage::De
        );
        assert_eq!(
            DashboardLanguage::from_header_value(Some("")),
            DashboardLanguage::De
        );
        assert_eq!(
            DashboardLanguage::from_header_value(Some("en")),
            DashboardLanguage::En
        );
        assert_eq!(
            DashboardLanguage::from_header_value(Some(" EN ")),
            DashboardLanguage::En
        );
    }

    #[test]
    fn pick_returns_the_variant_for_the_language() {
        assert_eq!(DashboardLanguage::De.pick(1, 2), 1);
        assert_eq!(DashboardLanguage::En.pick(1, 2), 2);
    }
}
