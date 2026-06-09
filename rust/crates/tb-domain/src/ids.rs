//! Domänen-Identifikatoren als Newtypes (kein I/O).

use serde::{Deserialize, Serialize};

/// Twitch-Login (Kanalname, lowercase-Konvention der DB).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct StreamerLogin(pub String);

/// Numerische Twitch-User-ID (in der DB als `text` gespeichert).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TwitchUserId(pub String);

macro_rules! str_newtype {
    ($t:ty) => {
        impl $t {
            pub fn as_str(&self) -> &str {
                &self.0
            }
            pub fn into_inner(self) -> String {
                self.0
            }
        }
        impl From<String> for $t {
            fn from(s: String) -> Self {
                Self(s)
            }
        }
        impl std::fmt::Display for $t {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str(&self.0)
            }
        }
    };
}
str_newtype!(StreamerLogin);
str_newtype!(TwitchUserId);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn newtype_roundtrip() {
        let l = StreamerLogin::from("dragskope".to_string());
        assert_eq!(l.as_str(), "dragskope");
        assert_eq!(l.to_string(), "dragskope");
        assert_eq!(l.into_inner(), "dragskope");
    }
}
