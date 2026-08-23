//! Instagram-Reels-Uploader.
//!
//! Basis ist die **Instagram API with Instagram Login** (`graph.instagram.com`),
//! nicht der Facebook-Login-Weg ueber `graph.facebook.com`. Damit braucht der
//! Streamer keine verknuepfte Facebook-Seite, es reicht ein Instagram-Konto vom
//! Typ Business oder Creator.
//!
//! Ablauf beim Upload:
//! 1. Preflight: Token pruefen (`GET /me`) und das Tageskontingent abfragen
//!    (`GET /{ig-user-id}/content_publishing_limit`).
//! 2. Media-Container anlegen (`POST /{ig-user-id}/media`, `media_type=REELS`).
//!    Bei einer lokalen Datei mit `upload_type=resumable`, bei einer fertigen
//!    oeffentlichen Adresse stattdessen mit `video_url`.
//! 3. Nur im resumable-Fall: Datei an `rupload.facebook.com` schicken.
//! 4. Warten, bis der Container `status_code=FINISHED` meldet. Instagram
//!    transkodiert asynchron, ein sofortiges Publish schlaegt fehl.
//! 5. Veroeffentlichen (`POST /{ig-user-id}/media_publish`).

use std::time::{Duration, Instant};

use async_trait::async_trait;
use serde_json::{json, Value};

use super::{
    as_count, truncate_chars, validate_local_file, AnalyticsSnapshot, PlatformUploader, UploadError,
};
use crate::video_processor::format_hashtags;

/// Instagram nimmt Reels bis 1 GB an. Die zweite Grenze ist die Laufzeit
/// (15 Minuten); die pruefen wir hier nicht, weil `validate_video` synchron ist
/// und kein ffprobe starten soll. Der Zuschnitt der Clips liegt ohnehin weit
/// darunter.
const MAX_FILE_MB: f64 = 1024.0;
const CAPTION_MAX: usize = 2200;
/// Instagram wertet hoechstens 30 Hashtags je Beitrag aus.
const HASHTAG_MAX: usize = 30;
const DEFAULT_API_BASE: &str = "https://graph.instagram.com/v23.0";
const DEFAULT_RUPLOAD_BASE: &str = "https://rupload.facebook.com/ig-api-upload/v23.0";
/// Fallback, falls die API kein `config.quota_total` mitliefert. Aktuell sind
/// 100 API-Posts je rollierendem 24-Stunden-Fenster erlaubt.
const DEFAULT_QUOTA_TOTAL: i64 = 100;
const DEFAULT_POLL_INTERVAL: Duration = Duration::from_secs(5);
const DEFAULT_POLL_TIMEOUT: Duration = Duration::from_secs(300);
const PROVIDER: &str = "instagram_login_api_v23";

/// Media-Felder, die es auf IG-Media wirklich gibt. `video_view_count` ist ein
/// Facebook-Feld und laesst die Graph-API den ganzen Request mit HTTP 400
/// ablehnen, dann fehlen auch Likes und Kommentare.
const MEDIA_FIELDS: &str = "like_count,comments_count,media_type,permalink";
const INSIGHT_METRICS: &str =
    "views,reach,likes,comments,saved,shares,total_interactions,ig_reels_avg_watch_time,ig_reels_video_view_total_time";

/// Instagram-Reels-Uploader.
pub struct InstagramUploader {
    access_token: String,
    business_account_id: String,
    api_base: String,
    rupload_base: String,
    poll_interval: Duration,
    poll_timeout: Duration,
    http: reqwest::Client,
}

fn is_url(value: &str) -> bool {
    value.starts_with("http://") || value.starts_with("https://")
}

/// Eine Fehlerantwort der Graph-API, aufgetrennt in Statuscode und
/// `error.code`/`error.error_subcode`. Getrennt gehalten, damit der Aufrufer die
/// Codes loggen kann und nicht nur einen fertigen Fehlertext bekommt.
struct GraphFailure {
    status: u16,
    code: Option<i64>,
    subcode: Option<i64>,
    message: String,
    body: String,
}

impl GraphFailure {
    /// Bildet die bekannten Graph-Codes auf eigene Fehlerarten ab. Code 4 und
    /// Subcode 2207051 heissen "Kontingent erschoepft" (also: morgen wieder
    /// versuchen), Code 190 heisst "Token hinueber".
    fn into_error(self, ctx: &str) -> UploadError {
        if self.code == Some(4) || self.subcode == Some(2207051) || self.status == 429 {
            return UploadError::QuotaExceeded(format!("{ctx}: {} ({})", self.message, self.body));
        }
        if self.code == Some(190) {
            return UploadError::NotAuthenticated;
        }
        UploadError::Api(format!("{ctx} failed: {} {}", self.status, self.body))
    }
}

/// Erwartet eine erfolgreiche Antwort mit JSON-Body. Im Fehlerfall wird der
/// Graph-Fehler zerlegt zurueckgegeben.
async fn read_graph_json(resp: reqwest::Response) -> Result<Value, GraphFailure> {
    let status = resp.status();
    let body = resp.text().await.unwrap_or_default();
    let parsed: Value = serde_json::from_str(&body).unwrap_or_else(|_| json!({}));
    if status.is_success() {
        // Manche Endpunkte (rupload) antworten mit leerem Body. Ein leeres
        // Objekt ist dann die richtige Antwort, kein Parse-Fehler.
        return Ok(parsed);
    }
    Err(GraphFailure {
        status: status.as_u16(),
        code: parsed["error"]["code"].as_i64(),
        subcode: parsed["error"]["error_subcode"].as_i64(),
        message: parsed["error"]["message"]
            .as_str()
            .unwrap_or_default()
            .to_string(),
        body,
    })
}

/// Wie `read_graph_json`, nur direkt als `UploadError`.
async fn graph_json(resp: reqwest::Response, ctx: &str) -> Result<Value, UploadError> {
    read_graph_json(resp).await.map_err(|f| f.into_error(ctx))
}

/// Baut die Caption: Beschreibung, Leerzeile, Hashtag-Block. Der Hashtag-Block
/// wird zuerst reserviert und die Beschreibung dahinter gekuerzt, damit beim
/// Kuerzen nie ein Tag mittendrin abreisst (`#deadl`).
fn build_caption(description: &str, hashtags: &[String]) -> String {
    let capped: Vec<String> = hashtags
        .iter()
        .filter(|t| !t.is_empty())
        .take(HASHTAG_MAX)
        .cloned()
        .collect();
    let tags = format_hashtags(&capped);
    if tags.is_empty() {
        return truncate_chars(description.trim(), CAPTION_MAX);
    }
    let separator = "\n\n";
    let reserved = tags.chars().count() + separator.chars().count();
    if reserved >= CAPTION_MAX {
        // Extremfall: schon der Tag-Block sprengt die Grenze. Dann lieber nur
        // Tags als eine Beschreibung ohne jeden Tag.
        return truncate_chars(&tags, CAPTION_MAX);
    }
    let desc = truncate_chars(description.trim(), CAPTION_MAX - reserved);
    let desc = desc.trim_end();
    if desc.is_empty() {
        return tags;
    }
    format!("{desc}{separator}{tags}")
}

/// Liest eine Insights-Kennzahl. Die API liefert je nach Metrik `values[0].value`
/// oder `total_value.value`, deshalb werden beide Formen gelesen.
fn insight_value(insights: &Value, name: &str) -> Option<i64> {
    let items = insights["data"].as_array()?;
    let item = items.iter().find(|i| i["name"].as_str() == Some(name))?;
    let raw = item
        .get("values")
        .and_then(|v| v.get(0))
        .and_then(|v| v.get("value"))
        .or_else(|| item.get("total_value").and_then(|t| t.get("value")));
    Some(as_count(raw))
}

impl InstagramUploader {
    /// `business_account_id` ist die Instagram-Professional-Account-ID, wie sie
    /// `GET /me?fields=user_id` im Feld `user_id` liefert.
    pub fn new(access_token: impl Into<String>, business_account_id: impl Into<String>) -> Self {
        Self {
            access_token: access_token.into(),
            business_account_id: business_account_id.into(),
            api_base: DEFAULT_API_BASE.to_string(),
            rupload_base: DEFAULT_RUPLOAD_BASE.to_string(),
            poll_interval: DEFAULT_POLL_INTERVAL,
            poll_timeout: DEFAULT_POLL_TIMEOUT,
            http: reqwest::Client::new(),
        }
    }

    /// Überschreibt die API-Basis-URL (für Tests).
    pub fn with_api_base(mut self, base: impl Into<String>) -> Self {
        self.api_base = base.into();
        self
    }

    /// Überschreibt den Host für den resumable Datei-Upload (für Tests).
    pub fn with_rupload_base(mut self, base: impl Into<String>) -> Self {
        self.rupload_base = base.into();
        self
    }

    /// Überschreibt Abstand und Gesamtdauer der Status-Abfrage (für Tests).
    pub fn with_poll_timing(mut self, interval: Duration, timeout: Duration) -> Self {
        self.poll_interval = interval;
        self.poll_timeout = timeout;
        self
    }

    /// Prüft das Access-Token via `GET /me`. Läuft als Preflight vor dem
    /// Container-Call, damit ein abgelaufenes Token nicht erst nach dem
    /// Datei-Upload auffällt.
    pub async fn verify_token(&self) -> Result<Value, UploadError> {
        let resp = self
            .http
            .get(format!("{}/me", self.api_base))
            .query(&[
                ("access_token", self.access_token.as_str()),
                ("fields", "user_id,username"),
            ])
            .send()
            .await
            .map_err(|e| UploadError::Request(e.to_string()))?;
        graph_json(resp, "Instagram auth check").await
    }

    /// Fragt das Veröffentlichungs-Kontingent ab und bricht ab, wenn es
    /// erschöpft ist. `QuotaExceeded` heißt "auf morgen verschieben", nicht
    /// "kaputt".
    pub async fn check_publishing_limit(&self) -> Result<(), UploadError> {
        let resp = self
            .http
            .get(format!(
                "{}/{}/content_publishing_limit",
                self.api_base, self.business_account_id
            ))
            .query(&[
                ("access_token", self.access_token.as_str()),
                ("fields", "config,quota_usage"),
            ])
            .send()
            .await
            .map_err(|e| UploadError::Request(e.to_string()))?;
        let data = match graph_json(resp, "Instagram publishing limit").await {
            Ok(v) => v,
            // Kontingent- und Token-Fehler zählen, alles andere darf den Upload
            // nicht blockieren: ein kaputter Preflight ist kein Grund, einen
            // gültigen Post liegen zu lassen.
            Err(e @ (UploadError::QuotaExceeded(_) | UploadError::NotAuthenticated)) => {
                return Err(e)
            }
            Err(e) => {
                tracing::warn!(error = %e, "Instagram: Kontingent-Abfrage fehlgeschlagen, Upload läuft trotzdem weiter");
                return Ok(());
            }
        };
        let entry = &data["data"][0];
        let usage = as_count(entry.get("quota_usage"));
        let total = entry["config"]["quota_total"]
            .as_i64()
            .filter(|t| *t > 0)
            .unwrap_or(DEFAULT_QUOTA_TOTAL);
        if usage >= total {
            return Err(UploadError::QuotaExceeded(format!(
                "Instagram publishing limit reached: {usage}/{total} posts in the last 24h"
            )));
        }
        Ok(())
    }

    /// Erstellt den Reel-Container aus einer bereits öffentlichen Video-URL.
    pub async fn create_media_container(
        &self,
        video_url: &str,
        caption: &str,
        share_to_feed: bool,
    ) -> Result<String, UploadError> {
        let resp = self
            .http
            .post(format!(
                "{}/{}/media",
                self.api_base, self.business_account_id
            ))
            .query(&[
                ("access_token", self.access_token.as_str()),
                ("media_type", "REELS"),
                ("video_url", video_url),
                ("caption", caption),
                (
                    "share_to_feed",
                    if share_to_feed { "true" } else { "false" },
                ),
            ])
            .send()
            .await
            .map_err(|e| UploadError::Request(e.to_string()))?;
        let data = graph_json(resp, "Instagram create container").await?;
        data["id"]
            .as_str()
            .map(str::to_string)
            .ok_or_else(|| UploadError::Api("No container ID in response".to_string()))
    }

    /// Erstellt den Reel-Container für den resumable Datei-Upload. Liefert die
    /// Container-ID und die Adresse, an die die Datei geht.
    pub async fn create_resumable_container(
        &self,
        caption: &str,
        share_to_feed: bool,
    ) -> Result<(String, String), UploadError> {
        let resp = self
            .http
            .post(format!(
                "{}/{}/media",
                self.api_base, self.business_account_id
            ))
            .query(&[
                ("access_token", self.access_token.as_str()),
                ("media_type", "REELS"),
                ("upload_type", "resumable"),
                ("caption", caption),
                (
                    "share_to_feed",
                    if share_to_feed { "true" } else { "false" },
                ),
            ])
            .send()
            .await
            .map_err(|e| UploadError::Request(e.to_string()))?;
        let data = graph_json(resp, "Instagram create container").await?;
        let id = data["id"]
            .as_str()
            .map(str::to_string)
            .ok_or_else(|| UploadError::Api("No container ID in response".to_string()))?;
        // Die Antwort liefert die fertige Upload-Adresse gleich mit. Solange die
        // Basis nicht für Tests überschrieben ist, nehmen wir sie, damit ein
        // Hostwechsel bei Meta nicht sofort alles bricht.
        let uri = data["uri"]
            .as_str()
            .filter(|u| !u.is_empty() && self.rupload_base == DEFAULT_RUPLOAD_BASE)
            .map(str::to_string)
            .unwrap_or_else(|| format!("{}/{}", self.rupload_base.trim_end_matches('/'), id));
        Ok((id, uri))
    }

    /// Schickt die lokale Datei an den resumable-Endpunkt. Der Header heißt
    /// `Authorization: OAuth <token>`, nicht `Bearer`.
    pub async fn upload_file_resumable(
        &self,
        upload_uri: &str,
        video_path: &str,
    ) -> Result<(), UploadError> {
        let bytes = tokio::fs::read(video_path).await?;
        let file_size = bytes.len();
        let resp = self
            .http
            .post(upload_uri)
            .header(
                reqwest::header::AUTHORIZATION,
                format!("OAuth {}", self.access_token),
            )
            .header("offset", "0")
            .header("file_size", file_size.to_string())
            .header(reqwest::header::CONTENT_TYPE, "application/octet-stream")
            .body(bytes)
            .send()
            .await
            .map_err(|e| UploadError::Request(e.to_string()))?;
        graph_json(resp, "Instagram resumable upload").await?;
        Ok(())
    }

    /// Fragt den Container-Status ab (`status_code` plus Klartext in `status`).
    pub async fn container_status(&self, container_id: &str) -> Result<Value, UploadError> {
        let resp = self
            .http
            .get(format!("{}/{}", self.api_base, container_id))
            .query(&[
                ("access_token", self.access_token.as_str()),
                ("fields", "status_code,status"),
            ])
            .send()
            .await
            .map_err(|e| UploadError::Request(e.to_string()))?;
        graph_json(resp, "Instagram container status").await
    }

    /// Wartet, bis der Container fertig transkodiert ist. Nur `FINISHED` darf
    /// weiter zum Publish. `ERROR` und `EXPIRED` brechen sofort ab, weiter zu
    /// pollen bringt dort nichts mehr.
    pub async fn wait_for_finished(&self, container_id: &str) -> Result<(), UploadError> {
        let started = Instant::now();
        loop {
            let data = self.container_status(container_id).await?;
            let last = data["status_code"]
                .as_str()
                .unwrap_or("UNKNOWN")
                .to_string();
            match last.as_str() {
                "FINISHED" => return Ok(()),
                "ERROR" | "EXPIRED" => {
                    let detail = data["status"].as_str().unwrap_or("");
                    return Err(UploadError::Api(format!(
                        "Instagram container {container_id} reported {last}: {detail}"
                    )));
                }
                _ => {}
            }
            if started.elapsed() + self.poll_interval > self.poll_timeout {
                return Err(UploadError::Api(format!(
                    "Instagram container {container_id} not FINISHED within {}s (last status {last})",
                    self.poll_timeout.as_secs()
                )));
            }
            tokio::time::sleep(self.poll_interval).await;
        }
    }

    /// Veröffentlicht den Container und liefert die Media-ID.
    pub async fn publish_container(&self, container_id: &str) -> Result<String, UploadError> {
        let resp = self
            .http
            .post(format!(
                "{}/{}/media_publish",
                self.api_base, self.business_account_id
            ))
            .query(&[
                ("access_token", self.access_token.as_str()),
                ("creation_id", container_id),
            ])
            .send()
            .await
            .map_err(|e| UploadError::Request(e.to_string()))?;
        let data = graph_json(resp, "Instagram publish").await?;
        data["id"]
            .as_str()
            .map(str::to_string)
            .ok_or_else(|| UploadError::Api("No media ID in response".to_string()))
    }

    /// Reel aus einer öffentlichen Video-URL veröffentlichen (Container, warten,
    /// publish).
    pub async fn upload_reel(
        &self,
        video_url: &str,
        caption: &str,
        share_to_feed: bool,
    ) -> Result<String, UploadError> {
        let container_id = self
            .create_media_container(video_url, caption, share_to_feed)
            .await?;
        self.wait_for_finished(&container_id).await?;
        self.publish_container(&container_id).await
    }

    /// Reel aus einer lokalen Datei veröffentlichen (Container, Datei-Upload,
    /// warten, publish).
    pub async fn upload_reel_file(
        &self,
        video_path: &str,
        caption: &str,
        share_to_feed: bool,
    ) -> Result<String, UploadError> {
        let (container_id, upload_uri) = self
            .create_resumable_container(caption, share_to_feed)
            .await?;
        self.upload_file_resumable(&upload_uri, video_path).await?;
        self.wait_for_finished(&container_id).await?;
        self.publish_container(&container_id).await
    }
}

#[async_trait]
impl PlatformUploader for InstagramUploader {
    fn platform_name(&self) -> &str {
        "instagram"
    }

    fn validate_video(&self, video_path: &str) -> Result<(), UploadError> {
        // Fertige öffentliche Adresse: nichts lokal zu prüfen.
        if is_url(video_path) {
            return Ok(());
        }
        validate_local_file(video_path, MAX_FILE_MB)
    }

    /// Nimmt einen lokalen Dateipfad (Regelfall, resumable Upload) oder eine
    /// öffentliche http(s)-Adresse (Sonderfall, `video_url`).
    async fn upload_video(
        &self,
        video_path: &str,
        _title: &str,
        description: &str,
        hashtags: &[String],
    ) -> Result<String, UploadError> {
        if self.access_token.is_empty() {
            return Err(UploadError::NotAuthenticated);
        }
        if self.business_account_id.is_empty() {
            return Err(UploadError::Validation(
                "Instagram user id missing".to_string(),
            ));
        }
        self.validate_video(video_path)?;
        let caption = build_caption(description, hashtags);
        // Preflight vor dem Datei-Upload: ein totes Token oder ein volles
        // Tageskontingent soll nicht erst nach dem Hochladen auffallen.
        self.verify_token().await?;
        self.check_publishing_limit().await?;
        if is_url(video_path) {
            self.upload_reel(video_path, &caption, true).await
        } else {
            self.upload_reel_file(video_path, &caption, true).await
        }
    }

    async fn get_video_status(&self, media_id: &str) -> Value {
        self.container_status(media_id)
            .await
            .unwrap_or_else(|_| json!({}))
    }

    async fn fetch_video_analytics(
        &self,
        media_id: &str,
        bucket: &str,
    ) -> Result<AnalyticsSnapshot, UploadError> {
        let media_resp = self
            .http
            .get(format!("{}/{}", self.api_base, media_id))
            .query(&[
                ("access_token", self.access_token.as_str()),
                ("fields", MEDIA_FIELDS),
            ])
            .send()
            .await
            .map_err(|e| UploadError::Request(e.to_string()))?;
        let media = graph_json(media_resp, "Instagram analytics").await?;

        // Insights sind best-effort, aber nicht mehr still: ein Fehlschlag wird
        // mit Statuscode und Graph-Fehlercode geloggt. Ausnahme ist ein totes
        // Token, das gibt einen echten Fehler zurück, sonst füllt sich die
        // Historie mit Nullen, die man nicht von echten Nullen unterscheidet.
        let insights_resp = self
            .http
            .get(format!("{}/{}/insights", self.api_base, media_id))
            .query(&[
                ("access_token", self.access_token.as_str()),
                ("metric", INSIGHT_METRICS),
            ])
            .send()
            .await
            .map_err(|e| UploadError::Request(e.to_string()))?;
        let insights = match read_graph_json(insights_resp).await {
            Ok(v) => v,
            Err(f) if f.code == Some(190) => return Err(UploadError::NotAuthenticated),
            Err(f) => {
                tracing::warn!(
                    media_id = %media_id,
                    status = f.status,
                    graph_code = ?f.code,
                    graph_subcode = ?f.subcode,
                    message = %f.message,
                    "Instagram Insights nicht abrufbar, Snapshot ohne Insights-Werte"
                );
                json!({})
            }
        };

        let views = insight_value(&insights, "views").unwrap_or(0);
        let shares = insight_value(&insights, "shares").unwrap_or(0);
        let saved = insight_value(&insights, "saved").unwrap_or(0);
        let reach = insight_value(&insights, "reach").unwrap_or(0);
        let total_interactions = insight_value(&insights, "total_interactions").unwrap_or(0);
        // Likes und Kommentare stehen am Media selbst; die Insights dienen nur
        // als Rückfallebene, falls das Feld fehlt.
        let likes = match media.get("like_count") {
            Some(v) if !v.is_null() => as_count(Some(v)),
            _ => insight_value(&insights, "likes").unwrap_or(0),
        };
        let comments = match media.get("comments_count") {
            Some(v) if !v.is_null() => as_count(Some(v)),
            _ => insight_value(&insights, "comments").unwrap_or(0),
        };

        let mut snapshot =
            AnalyticsSnapshot::build(bucket, PROVIDER, views, likes, comments, shares);
        // `ig_reels_video_view_total_time` kommt in Millisekunden.
        if let Some(ms) = insight_value(&insights, "ig_reels_video_view_total_time") {
            snapshot.watch_time_seconds = Some(ms / 1000);
        }
        // `total_interactions` zählt auch Saves mit und ist damit näher an dem,
        // was Instagram selbst als Interaktion ausweist.
        if total_interactions > 0 && views > 0 {
            snapshot.engagement_rate = Some(total_interactions as f64 / views as f64 * 100.0);
        }
        tracing::debug!(media_id = %media_id, reach, saved, total_interactions, "Instagram Insights gelesen");
        Ok(snapshot)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{method, path, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    const BIZ: &str = "17841400000";

    fn fast(uploader: InstagramUploader) -> InstagramUploader {
        uploader.with_poll_timing(Duration::from_millis(5), Duration::from_millis(500))
    }

    fn temp_clip(name: &str) -> std::path::PathBuf {
        let p = std::env::temp_dir().join(format!("tb_ig_{name}_{}.mp4", std::process::id()));
        std::fs::write(&p, b"fake-mp4-bytes").unwrap();
        p
    }

    /// Preflight (/me + Kontingent) so mounten, dass der Upload durchlaeuft.
    async fn mount_preflight_ok(server: &MockServer) {
        Mock::given(method("GET"))
            .and(path("/me"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(json!({ "user_id": BIZ, "username": "deadlock" })),
            )
            .mount(server)
            .await;
        Mock::given(method("GET"))
            .and(path(format!("/{BIZ}/content_publishing_limit")))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "data": [{ "config": { "quota_total": 100, "quota_duration": 86400 }, "quota_usage": 3 }]
            })))
            .mount(server)
            .await;
    }

    #[tokio::test]
    async fn resumable_upload_container_datei_status_publish() {
        let server = MockServer::start().await;
        mount_preflight_ok(&server).await;

        Mock::given(method("POST"))
            .and(path(format!("/{BIZ}/media")))
            .and(query_param("upload_type", "resumable"))
            .and(query_param("media_type", "REELS"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "id": "container-1" })))
            .expect(1)
            .mount(&server)
            .await;
        // rupload-Host zeigt im Test auf denselben Server, Pfad ist /container-1.
        Mock::given(method("POST"))
            .and(path("/container-1"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "success": true })))
            .expect(1)
            .mount(&server)
            .await;
        // Erst IN_PROGRESS, dann FINISHED.
        Mock::given(method("GET"))
            .and(path("/container-1"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(json!({ "status_code": "IN_PROGRESS" })),
            )
            .up_to_n_times(1)
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/container-1"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(json!({ "status_code": "FINISHED" })),
            )
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path(format!("/{BIZ}/media_publish")))
            .and(query_param("creation_id", "container-1"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "id": "media-99" })))
            .expect(1)
            .mount(&server)
            .await;

        let clip = temp_clip("resumable");
        let uploader = fast(
            InstagramUploader::new("tok", BIZ)
                .with_api_base(server.uri())
                .with_rupload_base(server.uri()),
        );
        let id = uploader
            .upload_video(
                clip.to_str().unwrap(),
                "ignored",
                "Beschreibung",
                &["deadlock".into()],
            )
            .await
            .unwrap();
        assert_eq!(id, "media-99");
        let _ = std::fs::remove_file(&clip);
    }

    #[tokio::test]
    async fn oeffentliche_url_nutzt_video_url_pfad() {
        let server = MockServer::start().await;
        mount_preflight_ok(&server).await;
        Mock::given(method("POST"))
            .and(path(format!("/{BIZ}/media")))
            .and(query_param("video_url", "https://cdn.example/clip.mp4"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(json!({ "id": "container-url" })),
            )
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/container-url"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(json!({ "status_code": "FINISHED" })),
            )
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path(format!("/{BIZ}/media_publish")))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "id": "media-url" })))
            .expect(1)
            .mount(&server)
            .await;

        let uploader = fast(
            InstagramUploader::new("tok", BIZ)
                .with_api_base(server.uri())
                .with_rupload_base(server.uri()),
        );
        let id = uploader
            .upload_video(
                "https://cdn.example/clip.mp4",
                "ignored",
                "Beschreibung",
                &[],
            )
            .await
            .unwrap();
        assert_eq!(id, "media-url");
    }

    #[tokio::test]
    async fn status_error_bricht_ab_und_publiziert_nicht() {
        let server = MockServer::start().await;
        mount_preflight_ok(&server).await;
        Mock::given(method("POST"))
            .and(path(format!("/{BIZ}/media")))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(json!({ "id": "container-bad" })),
            )
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/container-bad"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "success": true })))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/container-bad"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "status_code": "ERROR", "status": "Error: 2207026 unsupported video format"
            })))
            .mount(&server)
            .await;
        // Publish darf gar nicht erst aufgerufen werden.
        Mock::given(method("POST"))
            .and(path(format!("/{BIZ}/media_publish")))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(json!({ "id": "darf-nicht-passieren" })),
            )
            .expect(0)
            .mount(&server)
            .await;

        let clip = temp_clip("status_error");
        let uploader = fast(
            InstagramUploader::new("tok", BIZ)
                .with_api_base(server.uri())
                .with_rupload_base(server.uri()),
        );
        let err = uploader
            .upload_video(clip.to_str().unwrap(), "t", "d", &[])
            .await
            .unwrap_err();
        match err {
            UploadError::Api(msg) => assert!(msg.contains("ERROR"), "unerwartete Meldung: {msg}"),
            other => panic!("erwartet Api, bekommen {other:?}"),
        }
        let _ = std::fs::remove_file(&clip);
    }

    #[tokio::test]
    async fn status_timeout_meldet_klaren_fehler() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/container-slow"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(json!({ "status_code": "IN_PROGRESS" })),
            )
            .mount(&server)
            .await;
        let uploader = fast(InstagramUploader::new("tok", BIZ).with_api_base(server.uri()));
        let err = uploader
            .wait_for_finished("container-slow")
            .await
            .unwrap_err();
        match err {
            UploadError::Api(msg) => {
                assert!(msg.contains("not FINISHED"), "unerwartete Meldung: {msg}")
            }
            other => panic!("erwartet Api, bekommen {other:?}"),
        }
    }

    #[tokio::test]
    async fn kontingent_erschoepft_gibt_quota_exceeded() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/me"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "user_id": BIZ })))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path(format!("/{BIZ}/content_publishing_limit")))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "data": [{ "config": { "quota_total": 100 }, "quota_usage": 100 }]
            })))
            .mount(&server)
            .await;
        // Kein Container-Call, wenn das Kontingent voll ist.
        Mock::given(method("POST"))
            .and(path(format!("/{BIZ}/media")))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(json!({ "id": "darf-nicht-passieren" })),
            )
            .expect(0)
            .mount(&server)
            .await;

        let clip = temp_clip("quota");
        let uploader = fast(
            InstagramUploader::new("tok", BIZ)
                .with_api_base(server.uri())
                .with_rupload_base(server.uri()),
        );
        let err = uploader
            .upload_video(clip.to_str().unwrap(), "t", "d", &[])
            .await
            .unwrap_err();
        assert!(
            matches!(err, UploadError::QuotaExceeded(_)),
            "erwartet QuotaExceeded, bekommen {err:?}"
        );
        let _ = std::fs::remove_file(&clip);
    }

    #[tokio::test]
    async fn graph_fehlercode_4_gibt_quota_exceeded() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/me"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "user_id": BIZ })))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path(format!("/{BIZ}/content_publishing_limit")))
            .respond_with(ResponseTemplate::new(400).set_body_json(json!({
                "error": { "message": "Application request limit reached", "code": 4, "error_subcode": 2207051 }
            })))
            .mount(&server)
            .await;

        let uploader = fast(InstagramUploader::new("tok", BIZ).with_api_base(server.uri()));
        let err = uploader.check_publishing_limit().await.unwrap_err();
        assert!(
            matches!(err, UploadError::QuotaExceeded(_)),
            "erwartet QuotaExceeded, bekommen {err:?}"
        );
    }

    #[tokio::test]
    async fn graph_fehlercode_190_gibt_not_authenticated() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/me"))
            .respond_with(ResponseTemplate::new(400).set_body_json(json!({
                "error": { "message": "Error validating access token", "type": "OAuthException", "code": 190 }
            })))
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path(format!("/{BIZ}/media")))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(json!({ "id": "darf-nicht-passieren" })),
            )
            .expect(0)
            .mount(&server)
            .await;

        let clip = temp_clip("oauth190");
        let uploader = fast(
            InstagramUploader::new("tok", BIZ)
                .with_api_base(server.uri())
                .with_rupload_base(server.uri()),
        );
        let err = uploader
            .upload_video(clip.to_str().unwrap(), "t", "d", &[])
            .await
            .unwrap_err();
        assert!(
            matches!(err, UploadError::NotAuthenticated),
            "erwartet NotAuthenticated, bekommen {err:?}"
        );
        let _ = std::fs::remove_file(&clip);
    }

    #[test]
    fn hashtags_werden_bei_30_gekappt() {
        let tags: Vec<String> = (0..35).map(|i| format!("tag{i}")).collect();
        let caption = build_caption("Beschreibung", &tags);
        assert_eq!(caption.matches('#').count(), 30);
        assert!(caption.contains("#tag29"));
        assert!(!caption.contains("#tag30"));
    }

    #[test]
    fn caption_kuerzt_beschreibung_und_laesst_tags_ganz() {
        let lang = "x".repeat(CAPTION_MAX + 500);
        let caption = build_caption(&lang, &["deadlock".into(), "haze".into()]);
        assert!(caption.chars().count() <= CAPTION_MAX);
        // Der Tag-Block bleibt vollständig am Ende stehen.
        assert!(
            caption.ends_with("#deadlock #haze"),
            "Tags abgeschnitten: {}",
            &caption[caption.len() - 40..]
        );
    }

    #[tokio::test]
    async fn analytics_liest_views_aus_insights() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/media-1"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "like_count": 40, "comments_count": 10, "media_type": "VIDEO", "permalink": "https://instagram.com/p/x"
            })))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/media-1/insights"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "data": [
                    { "name": "views", "values": [{ "value": 500 }] },
                    { "name": "reach", "values": [{ "value": 420 }] },
                    { "name": "shares", "values": [{ "value": 5 }] },
                    { "name": "saved", "values": [{ "value": 7 }] },
                    { "name": "total_interactions", "values": [{ "value": 62 }] },
                    { "name": "ig_reels_video_view_total_time", "values": [{ "value": 900_000 }] }
                ]
            })))
            .mount(&server)
            .await;

        let uploader = InstagramUploader::new("tok", BIZ).with_api_base(server.uri());
        let stats = uploader
            .fetch_video_analytics("media-1", "d1")
            .await
            .unwrap();
        assert_eq!(stats.views, 500); // aus den Insights, nicht aus video_view_count
        assert_eq!(stats.likes, 40);
        assert_eq!(stats.comments, 10);
        assert_eq!(stats.shares, 5);
        assert_eq!(stats.watch_time_seconds, Some(900)); // 900000 ms
        assert_eq!(stats.engagement_rate, Some(12.4)); // total_interactions 62 / 500
        assert_eq!(stats.provider, PROVIDER);
    }

    #[tokio::test]
    async fn analytics_insights_fehler_ist_kein_totalausfall() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/media-2"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(json!({ "like_count": 3, "comments_count": 1 })),
            )
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/media-2/insights"))
            .respond_with(ResponseTemplate::new(400).set_body_json(json!({
                "error": { "message": "unsupported metric", "code": 100 }
            })))
            .mount(&server)
            .await;

        let uploader = InstagramUploader::new("tok", BIZ).with_api_base(server.uri());
        let stats = uploader
            .fetch_video_analytics("media-2", "d1")
            .await
            .unwrap();
        assert_eq!(stats.views, 0);
        assert_eq!(stats.likes, 3);
    }

    #[tokio::test]
    async fn analytics_bei_totem_token_kein_null_datenpunkt() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/media-3"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(json!({ "like_count": 0, "comments_count": 0 })),
            )
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/media-3/insights"))
            .respond_with(ResponseTemplate::new(400).set_body_json(json!({
                "error": { "message": "Error validating access token", "type": "OAuthException", "code": 190 }
            })))
            .mount(&server)
            .await;

        let uploader = InstagramUploader::new("tok", BIZ).with_api_base(server.uri());
        let err = uploader
            .fetch_video_analytics("media-3", "d1")
            .await
            .unwrap_err();
        assert!(
            matches!(err, UploadError::NotAuthenticated),
            "erwartet NotAuthenticated, bekommen {err:?}"
        );
    }

    #[tokio::test]
    async fn fehlende_datei_wird_vor_dem_netz_abgefangen() {
        let uploader = InstagramUploader::new("tok", BIZ);
        let err = uploader
            .upload_video("/tmp/gibt-es-nicht-4711.mp4", "t", "d", &[])
            .await
            .unwrap_err();
        assert!(
            matches!(err, UploadError::Validation(_)),
            "erwartet Validation, bekommen {err:?}"
        );
    }
}
