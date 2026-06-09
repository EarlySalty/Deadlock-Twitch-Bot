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
