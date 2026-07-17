//! Demo-Downloader: lädt die bz2-komprimierte Source-2-`.dem` eines Matches
//! über die Salts-URL der deadlock-api.com, entpackt sie und cached sie lokal.
//!
//! Port von `bot/highlight_clipper/demo_downloader.py`. Fehler degradieren auf
//! `None` (Python: `return None` + geloggte Exception). Cache-Verzeichnis und
//! Basis-URL sind injizierbar (Tests); produktiv [`DEMO_CACHE_DIR`] +
//! [`crate::deadlock_client::DEADLOCK_API_BASE`]. Entpackt wird mit `bzip2-rs`
//! (pure Rust, decompress-only) — kein System-libbz2 nötig.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// Lokales Cache-Verzeichnis für entpackte Demos (Python: `data/highlight_clipper/demos`).
pub const DEMO_CACHE_DIR: &str = "data/highlight_clipper/demos";

const SALTS_TIMEOUT: Duration = Duration::from_secs(10);
const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(120);

/// Gibt den Pfad zur entpackten `.dem`-Datei zurück, lädt + entpackt sie ggf.
///
/// Cache-Hit (Datei existiert) → sofort zurück, kein Netzwerk. Sonst: Salts-URL
/// holen → bz2 herunterladen → entpacken → schreiben. Jeder Fehler → `None`.
pub async fn get_demo_path(base_url: &str, cache_dir: &Path, match_id: i64) -> Option<PathBuf> {
    if let Err(e) = std::fs::create_dir_all(cache_dir) {
        tracing::error!(error = %e, "HighlightClipper: Cache-Verzeichnis nicht anlegbar");
        return None;
    }
    let dem_path = cache_dir.join(format!("{match_id}.dem"));
    if dem_path.exists() {
        return Some(dem_path);
    }

    let demo_url = get_demo_url(base_url, match_id).await?;
    tracing::info!(match_id, "HighlightClipper: Demo-Download");
    let bz2_data = download_bytes(&demo_url).await?;

    let raw = match decompress_bz2(&bz2_data) {
        Some(raw) => raw,
        None => {
            tracing::error!(
                match_id,
                "HighlightClipper: Demo-Dekomprimierung fehlgeschlagen"
            );
            return None;
        }
    };

    if let Err(e) = std::fs::write(&dem_path, &raw) {
        tracing::error!(error = %e, match_id, "HighlightClipper: Demo-Schreiben fehlgeschlagen");
        return None;
    }
    tracing::info!(
        file = %dem_path.display(),
        mb = raw.len() as f64 / 1_048_576.0,
        "HighlightClipper: Demo entpackt"
    );
    Some(dem_path)
}

/// Löscht die gecachte `.dem`-Datei eines Matches (idempotent, Python `unlink(missing_ok=True)`).
pub fn cleanup_demo(cache_dir: &Path, match_id: i64) {
    let path = cache_dir.join(format!("{match_id}.dem"));
    match std::fs::remove_file(&path) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => tracing::warn!(
            %error,
            match_id,
            path = %path.display(),
            "HighlightClipper: Demo-Cache konnte nicht geloescht werden"
        ),
    }
}

/// Holt die `demo_url` aus der Salts-Antwort (10s Timeout). Nicht-200, fehlendes
/// oder leeres Feld → `None` (Python `if not demo_url`).
async fn get_demo_url(base_url: &str, match_id: i64) -> Option<String> {
    let url = format!("{base_url}/matches/{match_id}/salts");
    let client = match reqwest::Client::builder().timeout(SALTS_TIMEOUT).build() {
        Ok(client) => client,
        Err(error) => {
            tracing::warn!(%error, match_id, "HighlightClipper: Salts-Client konnte nicht gebaut werden");
            return None;
        }
    };
    let resp = match client.get(&url).send().await {
        Ok(resp) => resp,
        Err(error) => {
            tracing::warn!(%error, match_id, "HighlightClipper: Salts-Request fehlgeschlagen");
            return None;
        }
    };
    if !resp.status().is_success() {
        tracing::warn!(
            status = resp.status().as_u16(),
            match_id,
            "HighlightClipper: Salts-Request non-2xx"
        );
        return None;
    }
    let data = match resp.json::<serde_json::Value>().await {
        Ok(data) => data,
        Err(error) => {
            tracing::warn!(%error, match_id, "HighlightClipper: Salts-JSON nicht lesbar");
            return None;
        }
    };
    data.get("demo_url")?
        .as_str()
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
}

/// Lädt die Bytes der Demo-URL (120s Timeout). HTTP ≠ 2xx oder Fehler → `None`.
async fn download_bytes(url: &str) -> Option<Vec<u8>> {
    let client = match reqwest::Client::builder().timeout(DOWNLOAD_TIMEOUT).build() {
        Ok(client) => client,
        Err(error) => {
            tracing::warn!(%error, "HighlightClipper: Demo-Download-Client konnte nicht gebaut werden");
            return None;
        }
    };
    let resp = match client.get(url).send().await {
        Ok(resp) => resp,
        Err(error) => {
            tracing::warn!(%error, "HighlightClipper: Demo-Download fehlgeschlagen");
            return None;
        }
    };
    if !resp.status().is_success() {
        tracing::warn!(status = %resp.status(), "HighlightClipper: Demo-Download HTTP-Fehler");
        return None;
    }
    match resp.bytes().await {
        Ok(bytes) => Some(bytes.to_vec()),
        Err(error) => {
            tracing::warn!(%error, "HighlightClipper: Demo-Download-Body nicht lesbar");
            None
        }
    }
}

/// Entpackt einen kompletten bz2-Puffer (Python `bz2.decompress`). Fehler → `None`.
fn decompress_bz2(data: &[u8]) -> Option<Vec<u8>> {
    let mut decoder = bzip2_rs::DecoderReader::new(data);
    let mut out = Vec::new();
    if let Err(error) = decoder.read_to_end(&mut out) {
        tracing::warn!(%error, "HighlightClipper: Demo-BZ2 konnte nicht entpackt werden");
        return None;
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    // `printf 'deadlock-demo-test-payload-0123456789' | bzip2 -c`
    const BZ2_BLOB: &[u8] = &[
        66, 90, 104, 57, 49, 65, 89, 38, 83, 89, 241, 205, 100, 41, 0, 0, 9, 153, 128, 0, 2, 127,
        224, 46, 14, 204, 32, 32, 0, 34, 140, 131, 33, 161, 161, 160, 160, 3, 17, 166, 154, 52, 76,
        183, 35, 41, 208, 142, 182, 186, 85, 122, 10, 184, 5, 118, 183, 225, 54, 103, 16, 248, 15,
        226, 238, 72, 167, 10, 18, 30, 57, 172, 133, 32,
    ];
    const BLOB_PLAIN: &str = "deadlock-demo-test-payload-0123456789";

    fn fresh_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(name);
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn decompress_bz2_roundtrip() {
        let out = decompress_bz2(BZ2_BLOB).expect("decompress");
        assert_eq!(String::from_utf8(out).unwrap(), BLOB_PLAIN);
    }

    #[test]
    fn decompress_bz2_muell_gibt_none() {
        assert!(decompress_bz2(b"not-bzip2-data").is_none());
    }

    #[tokio::test]
    async fn cache_hit_ohne_netzwerk() {
        let dir = fresh_dir("tb_highlight_cachehit");
        std::fs::create_dir_all(&dir).unwrap();
        let existing = dir.join("4242.dem");
        std::fs::write(&existing, b"cached").unwrap();
        // Bogus base_url: darf nicht erreicht werden, da Cache-Hit short-circuited.
        let out = get_demo_path("http://127.0.0.1:1", &dir, 4242).await;
        assert_eq!(out, Some(existing));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn cleanup_entfernt_datei() {
        let dir = fresh_dir("tb_highlight_cleanup");
        std::fs::create_dir_all(&dir).unwrap();
        let f = dir.join("99.dem");
        std::fs::write(&f, b"x").unwrap();
        assert!(f.exists());
        cleanup_demo(&dir, 99);
        assert!(!f.exists());
        cleanup_demo(&dir, 99); // idempotent
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn salts_ohne_demo_url_gibt_none() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/matches/7/salts"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({})))
            .mount(&server)
            .await;
        assert!(get_demo_url(&server.uri(), 7).await.is_none());
    }

    #[tokio::test]
    async fn voller_download_entpackt_und_schreibt() {
        let server = MockServer::start().await;
        let demo_url = format!("{}/cdn/demo.bz2", server.uri());
        Mock::given(method("GET"))
            .and(path("/matches/100/salts"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(serde_json::json!({ "demo_url": demo_url })),
            )
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/cdn/demo.bz2"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(BZ2_BLOB))
            .mount(&server)
            .await;

        let dir = fresh_dir("tb_highlight_fulldl");
        let out = get_demo_path(&server.uri(), &dir, 100).await.expect("path");
        assert_eq!(out, dir.join("100.dem"));
        let content = std::fs::read_to_string(&out).unwrap();
        assert_eq!(content, BLOB_PLAIN);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
