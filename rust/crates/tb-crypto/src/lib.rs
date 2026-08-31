//! Feldverschlüsselung des Rust-Twitch-Bots.
//!
//! Phase 0a: AES-256-GCM-Feldchiffre (`raid_auth`, `social_media`) byte-identisch zum
//! Python-`FieldCrypto`. Session-Krypto (Fernet-Ablösung) und keyring folgen mit der
//! Dashboard-Phase.

pub mod aad;
pub mod field;
pub mod token;

pub use field::{FieldCipher, KEY_SIZE, KID, NONCE_SIZE, VERSION};
pub use token::{constant_time_eq, random_hex_token, random_urlsafe_token, token_lookup_key};
