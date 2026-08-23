//! TikTok-Uploader gegen die Content Posting API v2 (Direct Post).
//!
//! Ablauf laut Doku (developers.tiktok.com, "Direct Post" + "Media Transfer
//! Guide"):
//!
//! 1. `POST /v2/post/publish/creator_info/query/` holt die erlaubten
//!    Privacy-Level und die Interaktionsschalter des Creators. Der Call ist vor
//!    jedem Direct Post Pflicht.
//! 2. `POST /v2/post/publish/video/init/` meldet Caption, Privacy-Level und den
//!    Chunk-Plan an und liefert `publish_id` plus `upload_url`.
//! 3. Die Chunks gehen sequenziell per `PUT` an die `upload_url`.
//!
//! Einen eigenen Publish-Call gibt es nicht: TikTok startet die
//! Veroeffentlichung selbst, sobald der letzte Chunk angekommen ist. Der Status
//! laeuft danach ueber `POST /v2/post/publish/status/fetch/`.

use std::time::Duration;

use async_trait::async_trait;
use reqwest::header::CONTENT_TYPE;
use serde_json::{json, Value};
use tokio::io::AsyncReadExt;

use super::{
    expect_ok_json, tiktok_error_guard, truncate_utf16, validate_local_file, AnalyticsSnapshot,
    PlatformUploader, UploadError,
};
use crate::video_processor::format_hashtags;

/// Echte Obergrenze der Content Posting API: 4 GB je Datei.
const MAX_FILE_MB: f64 = 4096.0;
/// TikTok kennt in `post_info` nur ein Textfeld (`title`). Titel, Beschreibung
/// und Hashtags landen also zusammen in diesem einen Feld, Grenze 2200
/// UTF-16-Einheiten.
const CAPTION_MAX: usize = 2200;

/// Chunk-Grenzen aus dem Media Transfer Guide. MB wird hier binaer gerechnet
/// (MiB); das liegt bei der Untergrenze auf der sicheren Seite und bleibt bei
/// der Obergrenze innerhalb dessen, was die Doku als Beispiel zeigt.
const MIN_CHUNK_BYTES: u64 = 5 * 1024 * 1024;
const MAX_CHUNK_BYTES: u64 = 64 * 1024 * 1024;
const DEFAULT_CHUNK_BYTES: u64 = 10 * 1024 * 1024;
const MAX_CHUNKS: u64 = 1000;

const DEFAULT_API_BASE: &str = "https://open.tiktokapis.com/v2";
/// Solange die App bei TikTok nicht auditiert ist, weist TikTok jeden
/// oeffentlichen Post mit `unaudited_client_can_only_post_to_private_accounts`
/// zurueck. Default ist deshalb der private Post; oeffentlich wird erst per
/// `with_privacy_level` gesetzt, wenn das Audit durch ist.
const DEFAULT_PRIVACY: &str = "SELF_ONLY";
const JSON_CONTENT_TYPE: &str = "application/json; charset=UTF-8";

/// Versuche je HTTP-Aufruf (erster Versuch plus zwei Wiederholungen).
const MAX_ATTEMPTS: u32 = 3;
const BACKOFF_BASE_MS: u64 = 200;

/// Antwort von `creator_info/query`. Bestimmt, welches Privacy-Level erlaubt
/// ist und welche Interaktionen der Creator generell gesperrt hat.
#[derive(Debug, Clone, PartialEq)]
pub struct CreatorInfo {
    pub privacy_level_options: Vec<String>,
    pub comment_disabled: bool,
    pub duet_disabled: bool,
    pub stitch_disabled: bool,
    pub max_video_post_duration_sec: i64,
}

/// Plan fuer den Datei-Upload: wie gross ein Chunk ist und wie viele es sind.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ChunkPlan {
    chunk_size: u64,
    total_chunk_count: u64,
}

/// Chunk-Regeln der Doku als reine Funktion:
///
/// * Datei unter 5 MB geht als ein Stueck hoch, `chunk_size == video_size`.
/// * sonst 10 MiB je Chunk, `total_chunk_count = floor(video_size / chunk_size)`.
/// * jeder Chunk mindestens 5 MB und hoechstens 64 MB; der letzte Chunk nimmt
///   die Restbytes mit und darf dadurch bis 128 MB gross werden.
/// * hoechstens 1000 Chunks, notfalls wird `chunk_size` hochgezogen.
fn chunk_plan(video_size: u64) -> (u64, u64) {
    if video_size < MIN_CHUNK_BYTES {
        return (video_size, 1);
    }
    let mut chunk_size = DEFAULT_CHUNK_BYTES;
    if video_size / chunk_size > MAX_CHUNKS {
        // Bei sehr grossen Dateien den Chunk so weit hochziehen, dass die
        // 1000er-Grenze haelt.
        chunk_size = video_size.div_ceil(MAX_CHUNKS);
    }
    chunk_size = chunk_size.clamp(MIN_CHUNK_BYTES, MAX_CHUNK_BYTES);
    let total_chunk_count = video_size / chunk_size;
    if total_chunk_count <= 1 {
        // Ein einziger Chunk heisst: die ganze Datei ist der Chunk. Ein
        // `chunk_size` groesser als die Datei waere laut Doku ungueltig.
        return (video_size, 1);
    }
    (chunk_size, total_chunk_count)
}

/// Titel, Beschreibung und Hashtags in das eine TikTok-Textfeld giessen.
fn build_caption(title: &str, description: &str, hashtags: &[String]) -> String {
    let tags = format_hashtags(hashtags);
    let parts: Vec<&str> = [title.trim(), description.trim(), tags.trim()]
        .into_iter()
        .filter(|p| !p.is_empty())
        .collect();
    truncate_utf16(&parts.join("\n\n"), CAPTION_MAX)
}

/// TikTok Content-Posting-Uploader (Direct Post).
pub struct TikTokUploader {
    access_token: String,
    api_base: String,
    privacy_level: String,
    http: reqwest::Client,
}

impl TikTokUploader {
    /// Der Token kommt fertig vom credential_manager, der Uploader haelt nur
    /// ihn. Privacy-Level startet auf `SELF_ONLY`.
    pub fn new(access_token: impl Into<String>) -> Self {
        Self {
            access_token: access_token.into(),
            api_base: DEFAULT_API_BASE.to_string(),
            privacy_level: DEFAULT_PRIVACY.to_string(),
            http: reqwest::Client::new(),
        }
    }

    /// Ueberschreibt die API-Basis-URL (fuer Tests).
    pub fn with_api_base(mut self, base: impl Into<String>) -> Self {
        self.api_base = base.into();
        self
    }

    /// Setzt das gewuenschte Privacy-Level. Der Wert muss in den
    /// `privacy_level_options` des Creators stehen, sonst bricht der Upload mit
    /// einem Validation-Fehler ab.
    pub fn with_privacy_level(mut self, level: impl Into<String>) -> Self {
        self.privacy_level = level.into();
        self
    }

    /// Pflicht-Vorabfrage vor jedem Direct Post.
    pub async fn query_creator_info(&self) -> Result<CreatorInfo, UploadError> {
        let url = format!("{}/post/publish/creator_info/query/", self.api_base);
        let payload = self
            .json_with_retry("TikTok creator info", || {
                self.http
                    .post(&url)
                    .bearer_auth(&self.access_token)
                    .header(CONTENT_TYPE, JSON_CONTENT_TYPE)
            })
            .await?;
        let data = &payload["data"];
        let privacy_level_options = data["privacy_level_options"]
            .as_array()
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default();
        Ok(CreatorInfo {
            privacy_level_options,
            comment_disabled: data["comment_disabled"].as_bool().unwrap_or(false),
            duet_disabled: data["duet_disabled"].as_bool().unwrap_or(false),
            stitch_disabled: data["stitch_disabled"].as_bool().unwrap_or(false),
            max_video_post_duration_sec: data["max_video_post_duration_sec"].as_i64().unwrap_or(0),
        })
    }

    /// Meldet den Post an und bekommt `publish_id` plus `upload_url` zurueck.
    async fn init_upload(
        &self,
        caption: &str,
        info: &CreatorInfo,
        video_size: u64,
        plan: ChunkPlan,
    ) -> Result<(String, String), UploadError> {
        let url = format!("{}/post/publish/video/init/", self.api_base);
        let body = json!({
            "post_info": {
                "title": caption,
                "privacy_level": self.privacy_level,
                "disable_comment": info.comment_disabled,
                "disable_duet": info.duet_disabled,
                "disable_stitch": info.stitch_disabled,
                "video_cover_timestamp_ms": 1000,
            },
            "source_info": {
                "source": "FILE_UPLOAD",
                "video_size": video_size,
                "chunk_size": plan.chunk_size,
                "total_chunk_count": plan.total_chunk_count,
            },
        });
        let payload = self
            .json_with_retry("TikTok init upload", || {
                self.http
                    .post(&url)
                    .bearer_auth(&self.access_token)
                    .header(CONTENT_TYPE, JSON_CONTENT_TYPE)
                    .body(body.to_string())
            })
            .await?;
        let publish_id = payload["data"]["publish_id"]
            .as_str()
            .map(str::to_string)
            .ok_or_else(|| {
                UploadError::Api("TikTok init upload: kein publish_id in der Antwort".to_string())
            })?;
        let upload_url = payload["data"]["upload_url"]
            .as_str()
            .map(str::to_string)
            .ok_or_else(|| {
                UploadError::Api("TikTok init upload: keine upload_url in der Antwort".to_string())
            })?;
        Ok((publish_id, upload_url))
    }

    /// Schiebt die Datei sequenziell an die `upload_url`. Trailing Bytes gehen
    /// an den letzten Chunk, es entsteht also kein zusaetzlicher Kurz-Chunk.
    /// Nach dem letzten Chunk (HTTP 201) uebernimmt TikTok von selbst.
    async fn upload_chunks(
        &self,
        video_path: &str,
        upload_url: &str,
        video_size: u64,
        plan: ChunkPlan,
    ) -> Result<(), UploadError> {
        let mut file = tokio::fs::File::open(video_path).await?;
        let mut offset = 0u64;
        for index in 0..plan.total_chunk_count {
            let is_last = index + 1 == plan.total_chunk_count;
            let length = if is_last {
                video_size - offset
            } else {
                plan.chunk_size
            };
            let mut buffer = vec![0u8; length as usize];
            file.read_exact(&mut buffer).await?;
            let range = format!("bytes {}-{}/{}", offset, offset + length - 1, video_size);
            self.put_chunk(upload_url, buffer, &range, length).await?;
            offset += length;
        }
        Ok(())
    }

    async fn put_chunk(
        &self,
        upload_url: &str,
        body: Vec<u8>,
        range: &str,
        length: u64,
    ) -> Result<(), UploadError> {
        let mut last = UploadError::Request("kein Versuch gelaufen".to_string());
        for attempt in 1..=MAX_ATTEMPTS {
            let request = self
                .http
                .put(upload_url)
                .header(CONTENT_TYPE, "video/mp4")
                .header(reqwest::header::CONTENT_LENGTH, length.to_string())
                .header(reqwest::header::CONTENT_RANGE, range)
                .body(body.clone());
            match request.send().await {
                Ok(resp) => {
                    let status = resp.status();
                    if status.is_success() {
                        return Ok(());
                    }
                    let text = resp.text().await.unwrap_or_default();
                    if status.as_u16() == 429 {
                        return Err(UploadError::QuotaExceeded(format!(
                            "TikTok chunk upload: {text}"
                        )));
                    }
                    if !status.is_server_error() {
                        return Err(UploadError::Api(format!(
                            "TikTok chunk upload failed: {status} {text}"
                        )));
                    }
                    last = UploadError::Api(format!("TikTok chunk upload failed: {status} {text}"));
                }
                Err(e) => last = UploadError::Request(e.to_string()),
            }
            backoff(attempt).await;
        }
        Err(last)
    }

    /// Ein JSON-Aufruf mit Wiederholung bei 5xx und Netzwerkfehlern. Client-
    /// Fehler werden sofort durchgereicht, ein Retry aendert daran nichts.
    async fn json_with_retry<F>(&self, ctx: &str, build: F) -> Result<Value, UploadError>
    where
        F: Fn() -> reqwest::RequestBuilder,
    {
        let mut last = UploadError::Request("kein Versuch gelaufen".to_string());
        for attempt in 1..=MAX_ATTEMPTS {
            match build().send().await {
                Ok(resp) => {
                    if resp.status().is_server_error() {
                        let status = resp.status();
                        let text = resp.text().await.unwrap_or_default();
                        last = UploadError::Api(format!("{ctx} failed: {status} {text}"));
                    } else {
                        let payload = expect_ok_json(resp, ctx).await?;
                        // Die Content-API antwortet auch im Fehlerfall mit 200
                        // und packt den Code nach `error.code`.
                        tiktok_error_guard(&payload, ctx)?;
                        return Ok(payload);
                    }
                }
                Err(e) => last = UploadError::Request(e.to_string()),
            }
            backoff(attempt).await;
        }
        Err(last)
    }
}

/// Einfaches exponentielles Backoff, letzter Versuch wartet nicht mehr.
async fn backoff(attempt: u32) {
    if attempt >= MAX_ATTEMPTS {
        return;
    }
    tokio::time::sleep(Duration::from_millis(BACKOFF_BASE_MS << (attempt - 1))).await;
}

#[async_trait]
impl PlatformUploader for TikTokUploader {
    fn platform_name(&self) -> &str {
        "tiktok"
    }

    fn validate_video(&self, video_path: &str) -> Result<(), UploadError> {
        validate_local_file(video_path, MAX_FILE_MB)
    }

    async fn upload_video(
        &self,
        video_path: &str,
        title: &str,
        description: &str,
        hashtags: &[String],
    ) -> Result<String, UploadError> {
        if self.access_token.is_empty() {
            return Err(UploadError::NotAuthenticated);
        }
        self.validate_video(video_path)?;
        let video_size = tokio::fs::metadata(video_path).await?.len();
        if video_size == 0 {
            return Err(UploadError::Validation(format!(
                "Video file is empty: {video_path}"
            )));
        }

        let info = self.query_creator_info().await?;
        if !info
            .privacy_level_options
            .iter()
            .any(|o| o == &self.privacy_level)
        {
            return Err(UploadError::Validation(format!(
                "TikTok privacy_level {} nicht erlaubt, moeglich sind: {}",
                self.privacy_level,
                info.privacy_level_options.join(", ")
            )));
        }

        let (chunk_size, total_chunk_count) = chunk_plan(video_size);
        let plan = ChunkPlan {
            chunk_size,
            total_chunk_count,
        };
        let caption = build_caption(title, description, hashtags);
        let (publish_id, upload_url) = self.init_upload(&caption, &info, video_size, plan).await?;
        self.upload_chunks(video_path, &upload_url, video_size, plan)
            .await?;
        Ok(publish_id)
    }

    async fn get_video_status(&self, video_id: &str) -> Value {
        let url = format!("{}/post/publish/status/fetch/", self.api_base);
        let body = json!({ "publish_id": video_id });
        let result = self
            .json_with_retry("TikTok status fetch", || {
                self.http
                    .post(&url)
                    .bearer_auth(&self.access_token)
                    .header(CONTENT_TYPE, JSON_CONTENT_TYPE)
                    .body(body.to_string())
            })
            .await;
        match result {
            Ok(payload) => payload.get("data").cloned().unwrap_or_else(|| json!({})),
            Err(e) => {
                // Die Trait-Signatur kennt keinen Fehlerwert, deshalb bleibt
                // `{}` der Rueckgabewert; stumm verschluckt wird der Grund aber
                // nicht mehr.
                tracing::warn!(publish_id = %video_id, error = %e, "TikTok status fetch fehlgeschlagen");
                json!({})
            }
        }
    }

    async fn fetch_video_analytics(
        &self,
        _video_id: &str,
        _bucket: &str,
    ) -> Result<AnalyticsSnapshot, UploadError> {
        // Frueher lief hier `/v2/video/query/`. Das gehoert zur Display API,
        // braucht den Scope `video.list` (den die App nicht anfragt) und
        // erwartet eine echte Video-ID, waehrend der Uploader nur die
        // `publish_id` kennt. Ergebnis war ein leeres Resultat, aus dem
        // strukturell Nullen als "Messung" in die Datenbank liefen.
        //
        // Damit das wieder geht, braucht es drei Dinge: den Scope `video.list`
        // in der TikTok-App plus im OAuth-Flow, das Nachschlagen der echten
        // Video-ID (Status-Fetch liefert nach der Veroeffentlichung eine
        // `publicaly_available_post_id`) und erst dann die Abfrage von
        // `/v2/video/query/` mit dieser ID.
        Err(UploadError::NotImplemented(
            "TikTok Display API nicht freigeschaltet: Scope video.list fehlt".to_string(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    const MIB: u64 = 1024 * 1024;

    async fn temp_video(name: &str, bytes: usize) -> String {
        let p = std::env::temp_dir().join(name);
        tokio::fs::write(&p, vec![7u8; bytes]).await.unwrap();
        p.to_string_lossy().into_owned()
    }

    fn creator_info_body(options: Value) -> Value {
        json!({
            "data": {
                "creator_username": "ddc",
                "privacy_level_options": options,
                "comment_disabled": true,
                "duet_disabled": false,
                "stitch_disabled": true,
                "max_video_post_duration_sec": 300,
            },
            "error": { "code": "ok", "message": "", "log_id": "l1" }
        })
    }

    async fn mount_creator_info(server: &MockServer, options: Value) {
        Mock::given(method("POST"))
            .and(path("/post/publish/creator_info/query/"))
            .respond_with(ResponseTemplate::new(200).set_body_json(creator_info_body(options)))
            .mount(server)
            .await;
    }

    // --- Chunk-Plan ---------------------------------------------------------

    #[test]
    fn chunk_plan_kleine_datei_geht_am_stueck() {
        assert_eq!(chunk_plan(3 * MIB), (3 * MIB, 1));
        // 5 MB liegt genau auf der Untergrenze: ein Chunk, ganze Datei.
        assert_eq!(chunk_plan(5 * MIB), (5 * MIB, 1));
        assert_eq!(chunk_plan(10 * MIB), (10 * MIB, 1));
    }

    #[test]
    fn chunk_plan_mittlere_dateien() {
        // floor(64/10) = 6 Chunks, der letzte nimmt die restlichen 14 MiB mit.
        assert_eq!(chunk_plan(64 * MIB), (10 * MIB, 6));
        assert_eq!(chunk_plan(70 * MIB), (10 * MIB, 7));
        assert_eq!(chunk_plan(300 * MIB), (10 * MIB, 30));
        assert_eq!(chunk_plan(3 * 1024 * MIB), (10 * MIB, 307));
    }

    #[test]
    fn chunk_plan_haelt_alle_grenzen_ein() {
        for size in [
            3 * MIB,
            5 * MIB,
            10 * MIB,
            64 * MIB,
            70 * MIB,
            300 * MIB,
            3 * 1024 * MIB,
            60 * 1024 * MIB,
        ] {
            let (chunk_size, total) = chunk_plan(size);
            assert!(
                (1..=MAX_CHUNKS).contains(&total),
                "size {size}: {total} Chunks"
            );
            assert!(
                chunk_size * total <= size,
                "size {size}: Plan deckt die Datei nicht"
            );
            if total > 1 {
                assert!(chunk_size >= MIN_CHUNK_BYTES, "size {size}: Chunk zu klein");
                assert!(chunk_size <= MAX_CHUNK_BYTES, "size {size}: Chunk zu gross");
                assert_eq!(
                    total,
                    size / chunk_size,
                    "size {size}: total != floor(size/chunk)"
                );
                let last = size - (total - 1) * chunk_size;
                assert!(
                    last <= 128 * MIB,
                    "size {size}: letzter Chunk {last} ueber 128 MB"
                );
            } else {
                assert_eq!(
                    chunk_size, size,
                    "size {size}: Ein-Chunk-Plan muss die ganze Datei umfassen"
                );
            }
        }
    }

    #[test]
    fn caption_baut_ein_feld_und_kuerzt_utf16() {
        let c = build_caption("Titel", "Text", &["deadlock".into(), "haze".into()]);
        assert_eq!(c, "Titel\n\nText\n\n#deadlock #haze");
        assert_eq!(build_caption("", "Nur Text", &[]), "Nur Text");
        let lang = build_caption(&"a".repeat(3000), "Text", &[]);
        assert_eq!(lang.chars().count(), CAPTION_MAX);
    }

    // --- Voller Ablauf ------------------------------------------------------

    #[tokio::test]
    async fn upload_video_creator_info_init_und_put() {
        let server = MockServer::start().await;
        mount_creator_info(&server, json!(["PUBLIC_TO_EVERYONE", "SELF_ONLY"])).await;
        Mock::given(method("POST"))
            .and(path("/post/publish/video/init/"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "data": { "publish_id": "pub-42", "upload_url": format!("{}/upload/", server.uri()) },
                "error": { "code": "ok", "message": "", "log_id": "l2" }
            })))
            .mount(&server)
            .await;
        Mock::given(method("PUT"))
            .and(path("/upload/"))
            .respond_with(ResponseTemplate::new(201))
            .mount(&server)
            .await;

        let video = temp_video("tb_tiktok_flow.mp4", 16).await;
        let uploader = TikTokUploader::new("tok").with_api_base(server.uri());
        let id = uploader
            .upload_video(
                &video,
                "Mein Titel",
                "Beschreibung",
                &["deadlock".into(), "haze".into()],
            )
            .await
            .unwrap();
        assert_eq!(id, "pub-42");

        let requests = server.received_requests().await.unwrap();
        let init = requests
            .iter()
            .find(|r| r.url.path() == "/post/publish/video/init/")
            .expect("init call fehlt");
        let body: Value = serde_json::from_slice(&init.body).unwrap();
        assert_eq!(
            body["post_info"]["title"],
            json!("Mein Titel\n\nBeschreibung\n\n#deadlock #haze")
        );
        assert_eq!(body["post_info"]["privacy_level"], json!("SELF_ONLY"));
        // Interaktionsschalter kommen aus creator_info, nicht aus Konstanten.
        assert_eq!(body["post_info"]["disable_comment"], json!(true));
        assert_eq!(body["post_info"]["disable_duet"], json!(false));
        assert_eq!(body["post_info"]["disable_stitch"], json!(true));
        assert_eq!(body["post_info"]["video_cover_timestamp_ms"], json!(1000));
        assert_eq!(body["source_info"]["source"], json!("FILE_UPLOAD"));
        assert_eq!(body["source_info"]["video_size"], json!(16));
        assert_eq!(body["source_info"]["chunk_size"], json!(16));
        assert_eq!(body["source_info"]["total_chunk_count"], json!(1));
        assert!(body["source_info"].get("upload_id").is_none());

        let put = requests
            .iter()
            .find(|r| r.url.path() == "/upload/")
            .expect("PUT auf upload_url fehlt");
        assert_eq!(put.headers.get("content-range").unwrap(), "bytes 0-15/16");
        assert_eq!(put.headers.get("content-type").unwrap(), "video/mp4");
        assert_eq!(put.body.len(), 16);
    }

    #[tokio::test]
    async fn privacy_level_nicht_in_optionen_ist_validation() {
        let server = MockServer::start().await;
        // Nicht auditierte App: TikTok bietet nur den privaten Post an.
        mount_creator_info(&server, json!(["SELF_ONLY"])).await;

        let video = temp_video("tb_tiktok_privacy.mp4", 16).await;
        let uploader = TikTokUploader::new("tok")
            .with_api_base(server.uri())
            .with_privacy_level("PUBLIC_TO_EVERYONE");
        let err = uploader
            .upload_video(&video, "t", "d", &[])
            .await
            .unwrap_err();
        assert!(
            matches!(err, UploadError::Validation(_)),
            "erwartet Validation, war {err:?}"
        );
        // Kein init-Call: der Upload bricht vor dem Anmelden ab.
        let requests = server.received_requests().await.unwrap();
        assert!(requests
            .iter()
            .all(|r| r.url.path() != "/post/publish/video/init/"));
    }

    #[tokio::test]
    async fn error_code_bei_200_ist_kein_erfolg() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/post/publish/creator_info/query/"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "data": {},
                "error": { "code": "spam_risk_too_many_posts", "message": "zu viele Posts", "log_id": "l3" }
            })))
            .mount(&server)
            .await;

        let video = temp_video("tb_tiktok_errorcode.mp4", 16).await;
        let uploader = TikTokUploader::new("tok").with_api_base(server.uri());
        let err = uploader
            .upload_video(&video, "t", "d", &[])
            .await
            .unwrap_err();
        assert!(
            matches!(err, UploadError::QuotaExceeded(_)),
            "erwartet QuotaExceeded, war {err:?}"
        );
    }

    #[tokio::test]
    async fn init_fehlercode_bricht_vor_dem_put_ab() {
        let server = MockServer::start().await;
        mount_creator_info(&server, json!(["SELF_ONLY"])).await;
        Mock::given(method("POST"))
            .and(path("/post/publish/video/init/"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "data": {},
                "error": { "code": "invalid_param", "message": "kaputt", "log_id": "l4" }
            })))
            .mount(&server)
            .await;

        let video = temp_video("tb_tiktok_initfehler.mp4", 16).await;
        let uploader = TikTokUploader::new("tok").with_api_base(server.uri());
        let err = uploader
            .upload_video(&video, "t", "d", &[])
            .await
            .unwrap_err();
        assert!(
            matches!(err, UploadError::Api(_)),
            "erwartet Api, war {err:?}"
        );
        let requests = server.received_requests().await.unwrap();
        assert!(requests.iter().all(|r| r.url.path() != "/upload/"));
    }

    #[tokio::test]
    async fn upload_ohne_token_und_fehlerpfade() {
        let video = temp_video("tb_tiktok_fehler.mp4", 16).await;

        // Kein Token, kein HTTP.
        let no_tok = TikTokUploader::new("");
        assert!(matches!(
            no_tok.upload_video(&video, "t", "d", &[]).await,
            Err(UploadError::NotAuthenticated)
        ));

        // creator_info liefert 403 (kein 5xx, also kein Retry) -> Api-Fehler.
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/post/publish/creator_info/query/"))
            .respond_with(ResponseTemplate::new(403).set_body_string("forbidden"))
            .expect(1)
            .mount(&server)
            .await;
        let uploader = TikTokUploader::new("tok").with_api_base(server.uri());
        assert!(matches!(
            uploader.upload_video(&video, "t", "d", &[]).await,
            Err(UploadError::Api(_))
        ));

        // Fehlende Datei -> Validation.
        assert!(matches!(
            uploader
                .upload_video("/nope/missing.mp4", "t", "d", &[])
                .await,
            Err(UploadError::Validation(_))
        ));
    }

    #[tokio::test]
    async fn fuenf_xx_wird_dreimal_versucht() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/post/publish/creator_info/query/"))
            .respond_with(ResponseTemplate::new(503).set_body_string("busy"))
            .expect(3)
            .mount(&server)
            .await;

        let uploader = TikTokUploader::new("tok").with_api_base(server.uri());
        let err = uploader.query_creator_info().await.unwrap_err();
        assert!(
            matches!(err, UploadError::Api(_)),
            "erwartet Api, war {err:?}"
        );
        // `expect(3)` wird beim Drop des Servers geprueft.
    }

    // --- Status und Analytics ----------------------------------------------

    #[tokio::test]
    async fn status_laeuft_per_post_mit_publish_id() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/post/publish/status/fetch/"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "data": { "status": "PROCESSING_UPLOAD" },
                "error": { "code": "ok", "message": "", "log_id": "l5" }
            })))
            .mount(&server)
            .await;

        let uploader = TikTokUploader::new("tok").with_api_base(server.uri());
        let status = uploader.get_video_status("pub-42").await;
        assert_eq!(status["status"], "PROCESSING_UPLOAD");

        let requests = server.received_requests().await.unwrap();
        let req = requests
            .iter()
            .find(|r| r.url.path() == "/post/publish/status/fetch/")
            .unwrap();
        assert_eq!(req.method.as_str(), "POST");
        assert!(
            req.url.query().is_none(),
            "publish_id gehoert in den Body, nicht in die Query"
        );
        let body: Value = serde_json::from_slice(&req.body).unwrap();
        assert_eq!(body["publish_id"], json!("pub-42"));
    }

    #[tokio::test]
    async fn status_fehler_bleibt_leeres_objekt() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/post/publish/status/fetch/"))
            .respond_with(ResponseTemplate::new(400).set_body_string("nope"))
            .mount(&server)
            .await;
        let uploader = TikTokUploader::new("tok").with_api_base(server.uri());
        assert_eq!(uploader.get_video_status("pub-42").await, json!({}));
    }

    #[tokio::test]
    async fn analytics_meldet_fehlenden_scope_statt_nullen() {
        let uploader = TikTokUploader::new("tok");
        let err = uploader
            .fetch_video_analytics("pub-42", "d1")
            .await
            .unwrap_err();
        match err {
            UploadError::NotImplemented(msg) => assert!(msg.contains("video.list")),
            other => panic!("erwartet NotImplemented, war {other:?}"),
        }
    }
}
