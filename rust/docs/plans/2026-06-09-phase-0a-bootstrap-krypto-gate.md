# Phase 0a — Bootstrap + Krypto-Gate — Implementierungsplan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (empfohlen) oder superpowers:executing-plans, um diesen Plan Task für Task umzusetzen. Schritte nutzen Checkbox-Syntax (`- [ ]`) zum Tracking.

**Goal:** Ein kompilierender Cargo-Workspace unter `rust/` mit `tb-error` und `tb-crypto`, dessen AES-256-GCM-Feldverschlüsselung **byte-identisch** zum Python-`FieldCrypto` ist — bewiesen durch einen Cross-Language-Interop-Test (der harte Gate aus ADR 0003).

**Architecture:** Drei-Schichten-Cargo-Workspace (siehe `rust/docs/01-architecture.md`). 0a baut nur die unterste Schicht: `tb-error` (zentrale Fehlertypen) und `tb-crypto` (Feldverschlüsselung). Komposition über Structs/Traits, keine globalen Mutable-States. Der Interop-Test ruft den realen Python-Krypto-Pfad als Oracle auf und entscheidet, ob bestehende verschlüsselte Tokens in Phase 6/8 weiterleben oder ein Re-Auth nötig wird.

**Tech Stack:** Rust (stable), `aes-gcm` 0.10 (RustCrypto `Aes256Gcm`), `hex`, `rand`, `zeroize`, `thiserror`; Python 3 + `cryptography` (nur im Test als Oracle).

---

## Scope & Dekomposition

Phase 0 (Foundation) ist zu groß für *einen* bite-sized Plan und wird in drei unabhängig lauffähige Teilpläne zerlegt:

- **0a (dieser Plan)** — Toolchain-Bootstrap, Workspace-Skelett, `tb-error`, `tb-crypto` **+ Krypto-Interop-Gate**. Liefert das ADR-0003-Urteil (höchstes Risiko zuerst).
- **0b** — `tb-domain`, `tb-config`, `tb-observability`, `tb-db` (sqlx-Pool, refinery-Baseline, read-only Vertrags-Tests gegen Prod-Schema).
- **0c** — `tb-transport-twitch`, `tb-transport-discord` (BrokerRelay), `tb-http-core`. `tb-eventsub`/`tb-llm` bewusst auf Phase 4/5 verschoben (YAGNI).

Jeder Teilplan endet mit grünen Tests und einem Commit. 0a berührt **keinen** Live-Pfad (Schritt 0 im Cutover-Plan: „kein Live-Cutover").

## Verifizierter Krypto-Vertrag (Grundlage für tb-crypto)

Aus dem Python-Quellcode extrahiert und durch zwei unabhängige Reviewer byte-genau bestätigt (`bot/compat/field_crypto.py`):

| Eigenschaft | Wert |
|---|---|
| Algorithmus | AES-256-GCM (`cryptography` hazmat `AESGCM`) |
| Key-Quelle | Env `DB_MASTER_KEY_V1`, **Hex** (`bytes.fromhex`), **exakt 32 Byte**, **kein KDF, kein base64** |
| Nonce | 12 Byte, CSPRNG (`secrets.token_bytes`), frisch pro Verschlüsselung |
| Tag | 16 Byte, von AESGCM automatisch an Ciphertext angehängt |
| Blob-Layout (BYTEA) | `version[1]=0x01` `‖` `kid_len[1]` `‖` `kid_bytes` (`v1`) `‖` `nonce[12]` `‖` `ciphertext‖tag[16]` |
| Output-Encoding | rohe Bytes in PostgreSQL `BYTEA` (kein base64/hex/JSON/Präfix) |
| AAD raid | `twitch_raid_auth\|<column>\|<twitch_user_id>\|<enc_version>` |
| AAD social | `social_media_platform_auth\|<column>\|<platform>\|<streamer_login\|global>\|<enc_version>` |
| AAD-Hinweis | Beim **Schreiben** ist `enc_version` Literal `1`; beim **Lesen** = `row["enc_version"]` (raid mit `or 1`-Default, social ohne). Phase 0a fixiert `enc_version=1`. |
| Single Source | `bot/compat/field_crypto.py` (`service.field_crypto` existiert nicht → Fallback immer aktiv) |

> **Fernet (`dashboard_sessions`)** ist **nicht** Teil von 0a. Befund: Auf Linux ist `keyring` per Default aus (`DEADLOCK_ENABLE_KEYRING` nicht gesetzt) → der Fernet-Key wird pro Prozess neu erzeugt → Sessions überleben heute schon **keinen** Restart. Damit ist der ADR-0003-Default „Sessions invalidieren / Re-Login" praktisch kostenlos; Fernet wird in der Dashboard-Phase durch AES-256-GCM ersetzt, nicht nachgebaut.

## Dateistruktur nach 0a

```
rust/
  Cargo.toml                       # [workspace] members + [workspace.dependencies]
  rust-toolchain.toml              # channel = stable + clippy/rustfmt
  .gitignore                       # target/
  crates/
    tb-error/
      Cargo.toml
      src/lib.rs                   # CryptoError (thiserror)
    tb-crypto/
      Cargo.toml
      src/lib.rs                   # pub mod aad; pub mod field; re-exports
      src/field.rs                 # FieldCipher: from_env/from_hex_key/encrypt/decrypt
      src/aad.rs                   # raid_auth(), social_media() AAD-Builder
      tests/py_oracle.py           # hermetisches Python-Oracle (cryptography)
      tests/interop.rs             # Cross-Language-Gate (Python ↔ Rust)
  docs/                            # bestehende Design-Doku + plans/
```

---

## Task 0: Toolchain-Bootstrap & Oracle-Voraussetzungen

**Files:** keine (nur Umgebung). Begründung: `cargo`/`rustc`/`rustup` sind aktuell **nicht** installiert (geprüft), und der Gate-Test braucht ein `python3` mit `cryptography`.

- [ ] **Step 1: rustup + stable-Toolchain installieren (user-level, kein sudo)**

Run:
```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --default-toolchain stable --profile minimal
source "$HOME/.cargo/env"
rustup component add clippy rustfmt
```
Expected: `rustup`/`cargo`/`rustc` unter `~/.cargo/bin`; `clippy` + `rustfmt` hinzugefügt.

- [ ] **Step 2: Toolchain verifizieren**

Run:
```bash
source "$HOME/.cargo/env"; cargo --version && rustc --version && cargo clippy --version
```
Expected: drei Versionszeilen, kein „command not found".

- [ ] **Step 3: Python-Oracle-Interpreter feststellen**

Run:
```bash
python3 -c "import cryptography; print('cryptography', cryptography.__version__)" \
  || echo "FEHLT: cryptography in python3"
```
Expected: `cryptography <version>`.
Falls es fehlt, einen Interpreter mit `cryptography` wählen (z. B. das venv des Bots) und dessen Pfad später als `TB_PY_ORACLE` setzen:
```bash
for p in /home/naniadm/Documents/Deadlock-Twitch-Bot/.venv/bin/python3 \
         /home/naniadm/Documents/Deadlock-Twitch-Bot/venv/bin/python3; do
  [ -x "$p" ] && "$p" -c "import cryptography" 2>/dev/null && echo "ORACLE=$p" && break
done
```
Expected: eine `ORACLE=...`-Zeile **oder** `python3` aus Step 3 reicht. Den funktionierenden Interpreter notieren — `tests/interop.rs` nutzt standardmäßig `python3`, override via `TB_PY_ORACLE`.

- [ ] **Step 4: Kein Commit** (reine Umgebungsänderung).

---

## Task 1: Workspace-Skelett

**Files:**
- Create: `rust/Cargo.toml`
- Create: `rust/rust-toolchain.toml`
- Create: `rust/.gitignore`

- [ ] **Step 1: Workspace-Manifest anlegen**

Create `rust/Cargo.toml`:
```toml
[workspace]
resolver = "2"
members = [
    "crates/tb-error",
    "crates/tb-crypto",
]

[workspace.package]
edition = "2021"

# Zentrale Versionspflege — jede Crate referenziert via `{ workspace = true }`.
[workspace.dependencies]
thiserror = "1"
aes-gcm = "0.10"
hex = "0.4"
rand = "0.8"
zeroize = "1"
serde_json = "1"
tb-error = { path = "crates/tb-error" }
tb-crypto = { path = "crates/tb-crypto" }
```

- [ ] **Step 2: Toolchain pinnen**

Create `rust/rust-toolchain.toml`:
```toml
[toolchain]
channel = "stable"
components = ["clippy", "rustfmt"]
```

- [ ] **Step 3: Build-Artefakte ignorieren**

Create `rust/.gitignore`:
```gitignore
/target
```

- [ ] **Step 4: Workspace ist (noch) leer — Mitglieder folgen in Task 2/3.** Kein Build hier (Members existieren noch nicht). Kein Commit (zusammen mit Task 2 committen, damit der erste Commit baut).

---

## Task 2: `tb-error` — zentrale Fehlertypen

**Files:**
- Create: `rust/crates/tb-error/Cargo.toml`
- Create: `rust/crates/tb-error/src/lib.rs`

- [ ] **Step 1: Crate-Manifest**

Create `rust/crates/tb-error/Cargo.toml`:
```toml
[package]
name = "tb-error"
version = "0.1.0"
edition.workspace = true

[dependencies]
thiserror.workspace = true
```

- [ ] **Step 2: CryptoError definieren**

Create `rust/crates/tb-error/src/lib.rs`:
```rust
//! Zentrale Fehlertypen des Rust-Twitch-Bots.
//!
//! Phase 0a: nur [`CryptoError`] (Feldverschlüsselung). Weitere Domänen-Fehler
//! (DB, HTTP, Transport) folgen in späteren Phasen jeweils in eigenen `thiserror`-Enums.

use thiserror::Error;

/// Fehler der Feldverschlüsselung (`tb-crypto`).
///
/// Spiegelt die Python-Fehlerklassen `KeyMissing` / `InvalidPayload` /
/// `DecryptFailed` / `CryptoError` wider. Die Varianten tragen **keine**
/// Klartext- oder Key-Werte, damit nichts Geheimes in Logs landet.
#[derive(Debug, Error)]
pub enum CryptoError {
    /// Master-Key fehlt, ist kein gültiges Hex oder hat nicht exakt 32 Byte.
    #[error("encryption key missing or invalid")]
    KeyMissing,

    /// Der verschlüsselte Blob ist strukturell ungültig (zu kurz, falsche Version, …).
    #[error("invalid encrypted payload: {0}")]
    InvalidPayload(String),

    /// Entschlüsselung schlug fehl (AAD-/Tag-Mismatch oder defekter Ciphertext).
    #[error("decryption failed")]
    DecryptFailed,

    /// Verschlüsselung schlug fehl.
    #[error("encryption failed")]
    EncryptFailed,
}
```

- [ ] **Step 3: Crate baut**

Run:
```bash
cd /home/naniadm/Documents/Deadlock-Twitch-Bot/rust && source "$HOME/.cargo/env"
cargo build -p tb-error
```
Expected: `Compiling tb-error v0.1.0` … `Finished`. (Hinweis: Der Workspace listet auch `tb-crypto` — der existiert noch nicht; `cargo build -p tb-error` baut gezielt nur diese Crate. Falls Cargo über das fehlende Member meckert, ist das in Task 3 behoben; alternativ `tb-crypto` temporär aus `members` auskommentieren und nach Task 3 wieder rein.)

- [ ] **Step 4: Commit (erster bauender Stand)**

```bash
cd /home/naniadm/Documents/Deadlock-Twitch-Bot
git add rust/Cargo.toml rust/rust-toolchain.toml rust/.gitignore rust/crates/tb-error
git commit -m "$(printf 'feat(rust): Workspace-Skelett + tb-error (CryptoError)\n\nCo-authored-by: Claude Code (Claude Opus 4.8) <claude-code@local>')"
```

> Anmerkung zum Workspace-Member-Konflikt: Damit der Commit aus Step 4 garantiert baut, kann `tb-crypto` in `rust/Cargo.toml` `members` bis Task 3 auskommentiert bleiben. Dann ist Step 3 ein voller `cargo build` ohne Sonderfall. Diese Variante ist vorzuziehen.

---

## Task 3: `tb-crypto` — `FieldCipher` (Rust-interne Korrektheit)

**Files:**
- Create: `rust/crates/tb-crypto/Cargo.toml`
- Create: `rust/crates/tb-crypto/src/lib.rs`
- Create: `rust/crates/tb-crypto/src/field.rs`
- Create: `rust/crates/tb-crypto/src/aad.rs`
- Modify: `rust/Cargo.toml` (`tb-crypto` in `members` aktivieren, falls in Task 2 auskommentiert)

- [ ] **Step 1: Crate-Manifest**

Create `rust/crates/tb-crypto/Cargo.toml`:
```toml
[package]
name = "tb-crypto"
version = "0.1.0"
edition.workspace = true

[dependencies]
tb-error.workspace = true
aes-gcm.workspace = true
hex.workspace = true
rand.workspace = true
zeroize.workspace = true

[dev-dependencies]
serde_json.workspace = true
```

- [ ] **Step 2: AAD-Builder schreiben**

Create `rust/crates/tb-crypto/src/aad.rs`:
```rust
//! AAD-Builder — byte-identisch zu den Python-AAD-Strings.
//!
//! Das AAD (Additional Authenticated Data) ist **nicht** Teil des gespeicherten Blobs.
//! Es wird beim Ver- und Entschlüsseln aus den Spaltenwerten rekonstruiert; weicht es
//! auch nur um ein Byte ab, schlägt die GCM-Tag-Prüfung fehl (`DecryptFailed`).
//!
//! `affiliate_pii` und `engagement_sender_auth` nutzen abweichende AAD-Formate und
//! werden mit ihren jeweiligen Feature-Crates ergänzt (nicht Teil von Phase 0a).

/// `twitch_raid_auth|<column>|<twitch_user_id>|<enc_version>`
pub fn raid_auth(column: &str, twitch_user_id: &str, enc_version: i64) -> String {
    format!("twitch_raid_auth|{column}|{twitch_user_id}|{enc_version}")
}

/// `social_media_platform_auth|<column>|<platform>|<streamer_login|global>|<enc_version>`
///
/// `streamer_login = None` ⇒ Literal `global` (entspricht `streamer_login or 'global'`).
pub fn social_media(
    column: &str,
    platform: &str,
    streamer_login: Option<&str>,
    enc_version: i64,
) -> String {
    let row = streamer_login.unwrap_or("global");
    format!("social_media_platform_auth|{column}|{platform}|{row}|{enc_version}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn raid_aad_matches_python_format() {
        assert_eq!(
            raid_auth("access_token", "123456", 1),
            "twitch_raid_auth|access_token|123456|1"
        );
    }

    #[test]
    fn social_aad_uses_global_when_no_streamer() {
        assert_eq!(
            social_media("refresh_token", "tiktok", None, 1),
            "social_media_platform_auth|refresh_token|tiktok|global|1"
        );
        assert_eq!(
            social_media("client_secret", "youtube", Some("dragskope"), 1),
            "social_media_platform_auth|client_secret|youtube|dragskope|1"
        );
    }
}
```

- [ ] **Step 3: Failing Test für `FieldCipher` schreiben**

Create `rust/crates/tb-crypto/src/field.rs` zunächst NUR mit dem Testmodul (die Implementierung kommt in Step 5, damit der Test zuerst rot ist):
```rust
//! AES-256-GCM-Feldverschlüsselung — byte-identisch zu `bot/compat/field_crypto.py`.

#[cfg(test)]
mod tests {
    use super::*;

    // Fester 32-Byte-Testschlüssel (Hex). NUR für Tests — niemals ein Prod-Key.
    const TEST_KEY_HEX: &str =
        "000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f";

    #[test]
    fn round_trip_recovers_plaintext() {
        let c = FieldCipher::from_hex_key(TEST_KEY_HEX, KID).unwrap();
        let aad = crate::aad::raid_auth("access_token", "555", 1);
        let blob = c.encrypt_field("twitch-token-xyz", &aad).unwrap();
        let back = c.decrypt_field(&blob, &aad).unwrap();
        assert_eq!(back, "twitch-token-xyz");
    }

    #[test]
    fn blob_has_expected_framing() {
        let c = FieldCipher::from_hex_key(TEST_KEY_HEX, KID).unwrap();
        let aad = crate::aad::raid_auth("access_token", "555", 1);
        let blob = c.encrypt_field("x", &aad).unwrap();
        assert_eq!(blob[0], VERSION); // 0x01
        assert_eq!(blob[1] as usize, KID.len()); // kid_len = 2
        assert_eq!(&blob[2..2 + KID.len()], KID.as_bytes()); // "v1"
        // Header(4) + nonce(12) + 1 Byte ct + 16 Byte tag = 33
        assert_eq!(blob.len(), 2 + KID.len() + NONCE_SIZE + 1 + 16);
    }

    #[test]
    fn wrong_aad_fails_decrypt() {
        let c = FieldCipher::from_hex_key(TEST_KEY_HEX, KID).unwrap();
        let blob = c
            .encrypt_field("secret", &crate::aad::raid_auth("access_token", "1", 1))
            .unwrap();
        let err = c.decrypt_field(&blob, &crate::aad::raid_auth("access_token", "2", 1));
        assert!(err.is_err(), "AAD-Mismatch muss fehlschlagen");
    }

    #[test]
    fn tampered_ciphertext_fails() {
        let c = FieldCipher::from_hex_key(TEST_KEY_HEX, KID).unwrap();
        let aad = crate::aad::raid_auth("access_token", "1", 1);
        let mut blob = c.encrypt_field("secret", &aad).unwrap();
        *blob.last_mut().unwrap() ^= 0xff; // Tag kippen
        assert!(c.decrypt_field(&blob, &aad).is_err());
    }

    #[test]
    fn rejects_non_32_byte_key() {
        assert!(FieldCipher::from_hex_key("00112233", KID).is_err());
    }
}
```

- [ ] **Step 4: Test ausführen — muss fehlschlagen (nichts implementiert)**

Run:
```bash
cd /home/naniadm/Documents/Deadlock-Twitch-Bot/rust && source "$HOME/.cargo/env"
cargo test -p tb-crypto --lib 2>&1 | head -30
```
Expected: Kompilier-Fehler `cannot find ... FieldCipher / VERSION / KID / NONCE_SIZE` — Test ist rot.

- [ ] **Step 5: `FieldCipher` implementieren**

Setze den Implementierungsblock OBEN in `rust/crates/tb-crypto/src/field.rs` ein (vor `#[cfg(test)] mod tests`):
```rust
use aes_gcm::aead::{Aead, KeyInit, Payload};
use aes_gcm::{Aes256Gcm, Key, Nonce};
use rand::RngCore;
use tb_error::CryptoError;
use zeroize::Zeroizing;

/// Format-Version (erstes Blob-Byte). Entspricht `FieldCrypto.VERSION = 1`.
pub const VERSION: u8 = 1;
/// GCM-Nonce-Länge in Byte. Entspricht `NONCE_SIZE = 12`.
pub const NONCE_SIZE: usize = 12;
/// AES-256-Schlüssellänge in Byte. Entspricht `KEY_SIZE = 32`.
pub const KEY_SIZE: usize = 32;
/// Einziger Key-Slot. Entspricht `self._keys['v1']`.
pub const KID: &str = "v1";

/// AES-256-GCM-Feldchiffre mit dem byte-genauen Blob-Format des Python-`FieldCrypto`.
pub struct FieldCipher {
    cipher: Aes256Gcm,
    kid: String,
}

impl FieldCipher {
    /// Lädt den Master-Key aus `DB_MASTER_KEY_V1` (Hex, exakt 32 Byte).
    /// Kein KDF, kein base64 — byte-identisch zu `FieldCrypto._load_keys`.
    pub fn from_env() -> Result<Self, CryptoError> {
        let raw = std::env::var("DB_MASTER_KEY_V1").map_err(|_| CryptoError::KeyMissing)?;
        Self::from_hex_key(raw.trim(), KID)
    }

    /// Baut die Chiffre aus einem Hex-Schlüssel. Die dekodierten Key-Bytes werden
    /// nach der Übergabe an die Chiffre wieder genullt (`Zeroizing`).
    pub fn from_hex_key(hex_key: &str, kid: &str) -> Result<Self, CryptoError> {
        let bytes = Zeroizing::new(hex::decode(hex_key).map_err(|_| CryptoError::KeyMissing)?);
        if bytes.len() != KEY_SIZE {
            return Err(CryptoError::KeyMissing);
        }
        let key = Key::<Aes256Gcm>::from_slice(&bytes);
        Ok(Self {
            cipher: Aes256Gcm::new(key),
            kid: kid.to_string(),
        })
    }

    /// Verschlüsselt `plaintext` unter `aad` und baut den BYTEA-Blob:
    /// `version[1] ‖ kid_len[1] ‖ kid ‖ nonce[12] ‖ ciphertext‖tag[16]`.
    pub fn encrypt_field(&self, plaintext: &str, aad: &str) -> Result<Vec<u8>, CryptoError> {
        let mut nonce_bytes = [0u8; NONCE_SIZE];
        rand::rngs::OsRng.fill_bytes(&mut nonce_bytes);
        self.encrypt_field_with_nonce(plaintext, aad, &nonce_bytes)
    }

    /// Wie [`encrypt_field`], aber mit fester Nonce — für deterministische Testvektoren.
    pub fn encrypt_field_with_nonce(
        &self,
        plaintext: &str,
        aad: &str,
        nonce_bytes: &[u8; NONCE_SIZE],
    ) -> Result<Vec<u8>, CryptoError> {
        let nonce = Nonce::from_slice(nonce_bytes);
        let ct = self
            .cipher
            .encrypt(
                nonce,
                Payload {
                    msg: plaintext.as_bytes(),
                    aad: aad.as_bytes(),
                },
            )
            .map_err(|_| CryptoError::EncryptFailed)?;
        let kid = self.kid.as_bytes();
        let mut out = Vec::with_capacity(2 + kid.len() + NONCE_SIZE + ct.len());
        out.push(VERSION);
        out.push(kid.len() as u8);
        out.extend_from_slice(kid);
        out.extend_from_slice(nonce_bytes);
        out.extend_from_slice(&ct);
        Ok(out)
    }

    /// Entschlüsselt einen Python-/Rust-erzeugten Blob unter demselben `aad`.
    pub fn decrypt_field(&self, blob: &[u8], aad: &str) -> Result<String, CryptoError> {
        if blob.len() < 15 {
            return Err(CryptoError::InvalidPayload("blob too short".into()));
        }
        let version = blob[0];
        let kid_len = blob[1] as usize;
        if version != VERSION {
            return Err(CryptoError::InvalidPayload(format!("unknown version: {version}")));
        }
        let kid_end = 2 + kid_len;
        if blob.len() < kid_end + NONCE_SIZE {
            return Err(CryptoError::InvalidPayload("blob truncated (missing nonce)".into()));
        }
        let kid = std::str::from_utf8(&blob[2..kid_end])
            .map_err(|_| CryptoError::InvalidPayload("invalid key id encoding".into()))?;
        if kid != self.kid {
            return Err(CryptoError::KeyMissing);
        }
        let nonce_end = kid_end + NONCE_SIZE;
        let nonce = Nonce::from_slice(&blob[kid_end..nonce_end]);
        let ct = &blob[nonce_end..];
        if ct.is_empty() {
            return Err(CryptoError::InvalidPayload("blob truncated (missing ciphertext)".into()));
        }
        let pt = self
            .cipher
            .decrypt(nonce, Payload { msg: ct, aad: aad.as_bytes() })
            .map_err(|_| CryptoError::DecryptFailed)?;
        String::from_utf8(pt).map_err(|_| CryptoError::DecryptFailed)
    }

    /// Der Key-Slot dieser Chiffre (`"v1"`).
    pub fn kid(&self) -> &str {
        &self.kid
    }
}
```

- [ ] **Step 6: `lib.rs` mit Modulen + Re-Exports**

Create `rust/crates/tb-crypto/src/lib.rs`:
```rust
//! Feldverschlüsselung des Rust-Twitch-Bots.
//!
//! Phase 0a: AES-256-GCM-Feldchiffre (`raid_auth`, `social_media`) byte-identisch zum
//! Python-`FieldCrypto`. Session-Krypto (Fernet-Ablösung) und keyring folgen mit der
//! Dashboard-Phase.

pub mod aad;
pub mod field;

pub use field::{FieldCipher, KEY_SIZE, KID, NONCE_SIZE, VERSION};
```

- [ ] **Step 7: `tb-crypto` als Workspace-Member aktivieren**

Falls in Task 2 auskommentiert: in `rust/Cargo.toml` die Zeile `"crates/tb-crypto",` in `members` einkommentieren.

- [ ] **Step 8: Tests laufen — müssen grün sein**

Run:
```bash
cd /home/naniadm/Documents/Deadlock-Twitch-Bot/rust && source "$HOME/.cargo/env"
cargo test -p tb-crypto --lib
```
Expected: `test result: ok.` mit allen 7 Tests (`aad::tests` 2 + `field::tests` 5) bestanden.

- [ ] **Step 9: Commit**

```bash
cd /home/naniadm/Documents/Deadlock-Twitch-Bot
git add rust/Cargo.toml rust/crates/tb-crypto
git commit -m "$(printf 'feat(rust): tb-crypto FieldCipher (AES-256-GCM, Python-Blob-Format)\n\nCo-authored-by: Claude Code (Claude Opus 4.8) <claude-code@local>')"
```

---

## Task 4: Krypto-Interop-Gate (Python ↔ Rust) — **der harte Gate**

**Files:**
- Create: `rust/crates/tb-crypto/tests/py_oracle.py`
- Create: `rust/crates/tb-crypto/tests/interop.rs`

**Warum so:** Der Gate beweist *Format*-Kompatibilität, die **key-unabhängig** ist (Algorithmus/Framing/AAD). Daher arbeitet der automatisierte Test mit einem **synthetischen** Testschlüssel — der Prod-Key (`DB_MASTER_KEY_V1`) wird nie geladen oder ausgegeben. Da das Framing key-unabhängig ist und Rust denselben 32-Byte-Key identisch lädt, folgt aus „Format passt" automatisch „Rust kann Prod-Blobs lesen". Ein optionaler, manueller Prod-Smoke-Test (mit echtem Key, ohne Klartext-Ausgabe) steht am Ende — **nicht** Teil der CI.

- [ ] **Step 1: Hermetisches Python-Oracle schreiben**

Create `rust/crates/tb-crypto/tests/py_oracle.py`:
```python
#!/usr/bin/env python3
"""Hermetisches Oracle: repliziert das Byte-Format von bot/compat/field_crypto.py.

Verifiziert byte-genau gegen den Produktionscode (zwei unabhängige Reviewer, 0 Abweichungen).
Liest eine JSON-Anfrage von stdin, schreibt eine JSON-Antwort nach stdout.
Der Schlüssel kommt aus DB_MASTER_KEY_V1 (Hex) — im Test ein Wegwerf-Key, nie der Prod-Key.

  encrypt: {"plaintext": str, "aad": str, "nonce_hex": optional str} -> {"blob_hex": str}
  decrypt: {"blob_hex": str, "aad": str}                            -> {"plaintext": str}
"""
import json
import os
import struct
import sys

from cryptography.hazmat.primitives.ciphers.aead import AESGCM

VERSION = 1
NONCE_SIZE = 12
KID = "v1"


def load_key() -> bytes:
    key = bytes.fromhex(os.environ["DB_MASTER_KEY_V1"].strip())
    if len(key) != 32:
        raise SystemExit("DB_MASTER_KEY_V1 must be 32 bytes (64 hex chars)")
    return key


def encrypt(key: bytes, plaintext: str, aad: str, nonce: bytes) -> bytes:
    ct = AESGCM(key).encrypt(nonce, plaintext.encode("utf-8"), aad.encode("utf-8"))
    kid_b = KID.encode("ascii")
    return struct.pack("BB", VERSION, len(kid_b)) + kid_b + nonce + ct


def decrypt(key: bytes, blob: bytes, aad: str) -> str:
    version, kid_len = struct.unpack("BB", blob[:2])
    if version != VERSION:
        raise SystemExit(f"unknown version {version}")
    kid_end = 2 + kid_len
    nonce = blob[kid_end:kid_end + NONCE_SIZE]
    ct = blob[kid_end + NONCE_SIZE:]
    return AESGCM(key).decrypt(nonce, ct, aad.encode("utf-8")).decode("utf-8")


def main() -> None:
    cmd = sys.argv[1]
    key = load_key()
    req = json.load(sys.stdin)
    if cmd == "encrypt":
        nonce = bytes.fromhex(req["nonce_hex"]) if req.get("nonce_hex") else os.urandom(NONCE_SIZE)
        json.dump({"blob_hex": encrypt(key, req["plaintext"], req["aad"], nonce).hex()}, sys.stdout)
    elif cmd == "decrypt":
        pt = decrypt(key, bytes.fromhex(req["blob_hex"]), req["aad"])
        json.dump({"plaintext": pt}, sys.stdout)
    else:
        raise SystemExit(f"unknown command {cmd}")


if __name__ == "__main__":
    main()
```

- [ ] **Step 2: Failing Cross-Language-Test schreiben**

Create `rust/crates/tb-crypto/tests/interop.rs`:
```rust
//! Cross-Language-Krypto-Gate (ADR 0003): beweist, dass Rust `tb-crypto` und der
//! Python-Krypto-Pfad dasselbe Blob-Format sprechen. Nutzt einen synthetischen
//! Wegwerf-Schlüssel — niemals den Prod-Key.

use std::io::Write;
use std::process::{Command, Stdio};

use serde_json::{json, Value};
use tb_crypto::{aad, FieldCipher, NONCE_SIZE};

// Synthetischer 32-Byte-Testschlüssel (Hex). NUR Test.
const TEST_KEY_HEX: &str = "0f0e0d0c0b0a09080706050403020100ffeeddccbbaa99887766554433221100";

/// Wählt den Python-Interpreter: `TB_PY_ORACLE` oder Default `python3`.
fn py_bin() -> String {
    std::env::var("TB_PY_ORACLE").unwrap_or_else(|_| "python3".to_string())
}

fn oracle_path() -> String {
    format!("{}/tests/py_oracle.py", env!("CARGO_MANIFEST_DIR"))
}

/// Stellt sicher, dass das Oracle lauffähig ist; sonst panic mit Anleitung
/// (der Gate darf nicht stillschweigend übersprungen werden).
fn ensure_oracle() {
    let out = Command::new(py_bin())
        .arg("-c")
        .arg("import cryptography")
        .output()
        .unwrap_or_else(|e| panic!("Python-Interpreter '{}' nicht ausführbar: {e}", py_bin()));
    assert!(
        out.status.success(),
        "Python-Oracle braucht das Modul 'cryptography'. Setze TB_PY_ORACLE auf einen \
         Interpreter mit cryptography (z. B. das venv des Bots)."
    );
}

fn run_oracle(cmd: &str, req: Value) -> Value {
    let mut child = Command::new(py_bin())
        .arg(oracle_path())
        .arg(cmd)
        .env("DB_MASTER_KEY_V1", TEST_KEY_HEX)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("Oracle-Start fehlgeschlagen");
    child
        .stdin
        .take()
        .unwrap()
        .write_all(req.to_string().as_bytes())
        .unwrap();
    let out = child.wait_with_output().unwrap();
    assert!(
        out.status.success(),
        "Oracle '{cmd}' fehlgeschlagen: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    serde_json::from_slice(&out.stdout).expect("Oracle lieferte kein gültiges JSON")
}

#[test]
fn python_encrypts_rust_decrypts() {
    ensure_oracle();
    let aad = aad::social_media("access_token", "tiktok", Some("dragskope"), 1);
    let resp = run_oracle("encrypt", json!({"plaintext": "py->rust-secret", "aad": aad}));
    let blob = hex::decode(resp["blob_hex"].as_str().unwrap()).unwrap();

    let cipher = FieldCipher::from_hex_key(TEST_KEY_HEX, "v1").unwrap();
    let pt = cipher.decrypt_field(&blob, &aad).unwrap();
    assert_eq!(pt, "py->rust-secret");
}

#[test]
fn rust_encrypts_python_decrypts() {
    ensure_oracle();
    let aad = aad::raid_auth("refresh_token", "987654", 1);
    let cipher = FieldCipher::from_hex_key(TEST_KEY_HEX, "v1").unwrap();
    let blob = cipher.encrypt_field("rust->py-secret", &aad).unwrap();

    let resp = run_oracle(
        "decrypt",
        json!({"blob_hex": hex::encode(&blob), "aad": aad}),
    );
    assert_eq!(resp["plaintext"].as_str().unwrap(), "rust->py-secret");
}

#[test]
fn identical_nonce_yields_identical_blob() {
    ensure_oracle();
    // Fixe Nonce ⇒ deterministischer Blob. Pinnt das Framing exakt zwischen beiden Sprachen.
    let nonce = [7u8; NONCE_SIZE];
    let aad = aad::raid_auth("access_token", "42", 1);

    let cipher = FieldCipher::from_hex_key(TEST_KEY_HEX, "v1").unwrap();
    let rust_blob = cipher
        .encrypt_field_with_nonce("pin-me", &aad, &nonce)
        .unwrap();

    let resp = run_oracle(
        "encrypt",
        json!({"plaintext": "pin-me", "aad": aad, "nonce_hex": hex::encode(nonce)}),
    );
    let py_blob = hex::decode(resp["blob_hex"].as_str().unwrap()).unwrap();

    assert_eq!(
        hex::encode(&rust_blob),
        hex::encode(&py_blob),
        "Rust- und Python-Blob müssen bei gleicher Nonce byte-identisch sein"
    );
}
```

- [ ] **Step 3: `hex` als dev-dependency ergänzen**

Der Test nutzt `hex` direkt. In `rust/crates/tb-crypto/Cargo.toml` unter `[dev-dependencies]` ergänzen:
```toml
[dev-dependencies]
serde_json.workspace = true
hex.workspace = true
```

- [ ] **Step 4: Gate ausführen**

Run:
```bash
cd /home/naniadm/Documents/Deadlock-Twitch-Bot/rust && source "$HOME/.cargo/env"
cargo test -p tb-crypto --test interop
```
Expected: `test result: ok. 3 passed` — `python_encrypts_rust_decrypts`, `rust_encrypts_python_decrypts`, `identical_nonce_yields_identical_blob`.

**Gate-Entscheid (ADR 0003):**
- **3 grün** ⇒ AES-256-GCM-Interop bewiesen. ADR 0003 Punkt 1 auf „akzeptiert: Tokens überleben" setzen; Phase 6/8 brauchen **kein** Re-Auth.
- **`identical_nonce_…` rot, andere grün** ⇒ Framing weicht ab (Header/Reihenfolge). Vor Weiterarbeit fixen — sonst sind Prod-Blobs unlesbar.
- **Alle rot / Oracle-Fehler** ⇒ erst `TB_PY_ORACLE`/`cryptography` prüfen (Task 0 Step 3). Bleibt es rot, ist das Format inkompatibel ⇒ Re-Auth-Pfad in ADR 0003 ziehen.

- [ ] **Step 5: Commit**

```bash
cd /home/naniadm/Documents/Deadlock-Twitch-Bot
git add rust/crates/tb-crypto
git commit -m "$(printf 'test(rust): Krypto-Interop-Gate Python<->Rust (ADR 0003)\n\nCo-authored-by: Claude Code (Claude Opus 4.8) <claude-code@local>')"
```

---

## Task 5: Qualitätssicherung + Doku-Sync + Push

**Files:**
- Modify: `rust/docs/adr/0003-crypto-interop-or-reauth.md` (Gate-Ergebnis eintragen)
- Modify: `rust/docs/README.md` (Plan-Verweis), `rust/docs/04-cutover-plan.md` (Schritt-0-Status, falls grün)

- [ ] **Step 1: Format + Lint (Warnungen = Fehler)**

Run:
```bash
cd /home/naniadm/Documents/Deadlock-Twitch-Bot/rust && source "$HOME/.cargo/env"
cargo fmt --all -- --check && cargo clippy --workspace --all-targets -- -D warnings
```
Expected: keine Diff-Ausgabe von `fmt`, `clippy` endet mit `Finished` ohne Warnungen. Bei `fmt`-Diff: `cargo fmt --all` ausführen und committen.

- [ ] **Step 2: Voller Workspace-Test**

Run:
```bash
cd /home/naniadm/Documents/Deadlock-Twitch-Bot/rust && source "$HOME/.cargo/env"
cargo test --workspace
```
Expected: alle Tests grün (lib + interop).

- [ ] **Step 3: ADR 0003 mit Gate-Ergebnis aktualisieren**

In `rust/docs/adr/0003-crypto-interop-or-reauth.md` den Status von „vorgeschlagen" auf das tatsächliche Ergebnis setzen (z. B. „akzeptiert: AES-256-GCM-Interop grün, Tokens überleben") und unter „Offen" den erledigten Punkt streichen. Den Fernet-Linux-Befund (Sessions überleben heute schon keinen Restart) als Begründung für den Re-Login-Default ergänzen.

- [ ] **Step 4: Push (abgeschlossener, verifizierter Teil)**

```bash
cd /home/naniadm/Documents/Deadlock-Twitch-Bot
git add rust/docs
git commit -m "$(printf 'docs(rust): ADR 0003 Gate-Ergebnis + Phase-0a-Status\n\nCo-authored-by: Claude Code (Claude Opus 4.8) <claude-code@local>')"
git push origin main
```
> Interne Doku/Foundation-Code ⇒ **kein** CHANGELOG-Eintrag, **keine** Discord-/In-App-Spiegelung (Regel: Changelog nur bei user-sichtbaren Features).

- [ ] **Step 5 (optional, manuell, secret-sicher): Prod-Blob-Smoke-Test**

Nur falls über den Format-Beweis hinaus ein echter Prod-Blob geprüft werden soll. **Niemals** Klartext-Token ausgeben — nur ein Boolean. Ablauf: `DB_MASTER_KEY_V1` über den Infisical-Wrapper in die Shell laden, einen `access_token_enc`-BYTEA aus der DB ziehen, mit `FieldCipher::from_env()` + dem korrekten AAD entschlüsseln und nur `ok=true/false` ausgeben. Diese Prüfung ist **nicht** Teil der CI und wird nach Gebrauch mit `unset DB_MASTER_KEY_V1` entfernt.

---

## Self-Review (vom Plan-Autor durchgeführt)

**1. Spec-Abdeckung (gegen `04-cutover-plan.md` Schritt 0 + `01-architecture.md`):**
- „`tb-error/.../crypto` gebaut + getestet" ✓ (Task 2–4). `tb-domain/config/db/transport-*` bewusst nach 0b/0c verschoben — als Scope-Schnitt dokumentiert.
- „Crypto-Interop-Test gegen bestehende Blobs grün **oder** Re-Auth-Entscheid" ✓ (Task 4, beide Ausgänge behandelt).
- „kein Live-Cutover / Rollback = Crate verwerfen" ✓ (0a berührt keinen Live-Pfad).

**2. Platzhalter-Scan:** keine TBD/TODO; jeder Code-Schritt enthält vollständigen Code, jeder Run-Schritt erwartete Ausgabe.

**3. Typ-/Namens-Konsistenz:** `FieldCipher`, `encrypt_field`, `encrypt_field_with_nonce`, `decrypt_field`, `from_hex_key`, `from_env`, Konstanten `VERSION/NONCE_SIZE/KEY_SIZE/KID`, `aad::raid_auth`/`aad::social_media`, `CryptoError`-Varianten — durchgängig identisch in Implementierung und Tests (lib + interop).

**4. Mehrdeutigkeit:** `enc_version` in 0a fest `1` (Schreibpfad-Konvention); dynamisches Lesen erst mit dem DB-Lesepfad in der Feature-Phase relevant — explizit notiert.

**Bekannte, bewusste Grenzen:** keyring-Fallback (Windows-only) nicht portiert (Linux-Deployment nutzt Env). `affiliate_pii`/`engagement_sender_auth`-AAD-Builder folgen mit ihren Feature-Crates. Fernet/Sessions → Dashboard-Phase.
