//! Partner-Lebenszyklus-Status (DB-Spalte `twitch_partners.status`, text).

/// Status eines Twitch-Partners. `Other` fängt unbekannte DB-Werte robust ab.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PartnerStatus {
    Active,
    Archived,
    Other(String),
}

impl PartnerStatus {
    /// Aus dem rohen DB-Wert (case-insensitiv).
    pub fn from_db(value: &str) -> Self {
        match value.trim().to_ascii_lowercase().as_str() {
            "active" => Self::Active,
            "archived" => Self::Archived,
            other => Self::Other(other.to_string()),
        }
    }

    /// Kanonischer DB-Wert.
    pub fn as_db(&self) -> &str {
        match self {
            Self::Active => "active",
            Self::Archived => "archived",
            Self::Other(s) => s,
        }
    }

    pub fn is_active(&self) -> bool {
        matches!(self, Self::Active)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_values_roundtrip() {
        assert_eq!(PartnerStatus::from_db("active"), PartnerStatus::Active);
        assert_eq!(PartnerStatus::from_db("ARCHIVED"), PartnerStatus::Archived);
        assert!(PartnerStatus::Active.is_active());
        assert_eq!(PartnerStatus::Archived.as_db(), "archived");
    }

    #[test]
    fn unknown_value_is_preserved_not_panicked() {
        let s = PartnerStatus::from_db("frozen");
        assert_eq!(s, PartnerStatus::Other("frozen".to_string()));
        assert_eq!(s.as_db(), "frozen");
        assert!(!s.is_active());
    }
}
