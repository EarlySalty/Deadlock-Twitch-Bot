//! YouTube-Uploader (Port von `uploaders/youtube.py`).
//!
//! Das Python-Original nutzt die `google-api-python-client`-Bibliothek; hier
//! ist der **resumable Upload** der YouTube Data API v3 als Roh-HTTP umgesetzt:
//! 1. POST `…/videos?uploadType=resumable&part=snippet,status` (JSON-Body) →
//!    Upload-Session-URL im `Location`-Header.
//! 2. PUT der Video-Bytes in Stuecken an diese Session-URL, jedes mit
//!    `Content-Range`. Das letzte Stueck bringt die Video-ID zurueck.
//!
//! Shorts und lange VODs laufen seit dieser Fassung denselben Weg: stueckweise,
//! wiederaufnehmbar, mit `Authorization` an jedem Aufruf. Ein abgerissener
//! Upload beginnt damit nicht wieder bei null, und ein Stundentoken, das mitten
//! im Byte-Transfer ablaeuft, heilt sich inline.
//!
//! Token-Refresh laeuft primaer proaktiv im `refresh_worker`. Zusaetzlich heilt
//! sich der Uploader bei einem 401 inline: sind Refresh-Credentials gesetzt,
//! wird das Access-Token genau einmal ueber die Google-Token-URL erneuert und
//! der Call wiederholt (Paritaet zum google-api-python-client, uploaders-1).
//! Damit das frische Token nicht nur im Speicher landet, kann der Aufrufer
//! ueber [`YouTubeUploader::with_token_sink`] eine Senke haengen, die es
//! dauerhaft wegschreibt.
//!
//! Alle HTTP-Aufrufe laufen ueber [`YouTubeUploader::call`]: exponentielles
//! Backoff mit Jitter bei 5xx und Transportfehlern, `Retry-After` respektiert,
//! niemals eine Wiederholung bei 4xx (das waere nur schneller derselbe Fehler).

use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use async_trait::async_trait;
use reqwest::header::{CONTENT_TYPE, LOCATION, RETRY_AFTER};
use serde_json::{json, Map, Value};
use tokio::io::{AsyncReadExt, AsyncSeekExt};
use tokio::sync::Mutex;

use super::{
    as_count, expect_ok_json, truncate_chars, validate_local_file, AnalyticsSnapshot,
    PlatformUploader, UploadError,
};
use crate::video_processor::format_hashtags;

/// Beschreibungsgrenze. YouTube zaehlt hier Bytes, nicht Zeichen; ein Text aus
/// lauter Umlauten und Emoji ist frueher am Limit, als die Zeichenzahl vermuten
/// laesst.
const DESCRIPTION_MAX_BYTES: usize = 5000;
const TITLE_MAX: usize = 100;

/// Gesamtbudget fuer `snippet.tags[]`, in Zeichen. Das ist bewusst **kein**
/// Zaehllimit: die Doku begrenzt die Gesamtlaenge des Wertes auf 500 Zeichen,
/// inklusive der Trennkommas und inklusive der impliziten Anfuehrungszeichen um
/// jeden Tag, der ein Leerzeichen enthaelt. Wer stattdessen die ersten 500
/// Eintraege nimmt, faehrt ab rund 40 Hashtags in einen `invalidTags`-400 und
/// verliert den ganzen Upload.
const TAGS_MAX_CHARS: usize = 500;

/// Der Anhang, an dem YouTube den Clip als Short erkennt. Er muss die Kuerzung
/// ueberleben, deshalb wird der Rumpf davor gekuerzt und der Anhang danach
/// angehaengt.
const SHORTS_SUFFIX: &str = "\n\n#Shorts";

const CATEGORY_GAMING: &str = "20";
const DEFAULT_PRIVACY: &str = "public";
const DEFAULT_UPLOAD_BASE: &str = "https://www.googleapis.com/upload/youtube/v3";
const DEFAULT_API_BASE: &str = "https://www.googleapis.com/youtube/v3";

/// Shorts sind auf drei Minuten begrenzt; laengere Videos nimmt YouTube zwar an,
/// veroeffentlicht sie aber als normales Video im Querformat-Player.
const SHORTS_MAX_DAUER_SEKUNDEN: f64 = 180.0;
/// Zielverhaeltnis 9:16. Abweichungen darueber hinaus sind kein harter Fehler,
/// solange das Video hochkant ist; Querformat dagegen ist nie ein Short.
const SHORTS_ZIEL_VERHAELTNIS: f64 = 9.0 / 16.0;

/// Anzahl der Versuche je HTTP-Aufruf (erster Versuch eingerechnet).
const DEFAULT_VERSUCHE: u32 = 4;
/// Wartezeit vor dem zweiten Versuch; danach verdoppelt sie sich je Runde.
const DEFAULT_BACKOFF_BASIS: Duration = Duration::from_secs(1);
/// Obergrenze fuer ein von der Gegenseite gefordertes `Retry-After`.
const MAX_WARTEZEIT: Duration = Duration::from_secs(300);

/// Fehlergruende, die "komm spaeter wieder" heissen und nicht "der Code ist
/// kaputt". Der reine Substring "quota" reicht dafuer nicht: `uploadLimitExceeded`
/// und `rateLimitExceeded` enthalten ihn nicht.
const QUOTA_REASONS: [&str; 4] = [
    "quotaExceeded",
    "uploadLimitExceeded",
    "rateLimitExceeded",
    "userRateLimitExceeded",
];

/// Google-OAuth-Token-Endpoint für den inline-Refresh.
pub const GOOGLE_TOKEN_URL: &str = "https://oauth2.googleapis.com/token";

/// Groesse eines Upload-Stuecks beim stueckweisen resumable Upload. Google
/// verlangt, dass jedes Stueck ausser dem letzten ein Vielfaches von 256 KB ist.
pub const RESUMABLE_CHUNK_BYTES: u64 = 32 * 1024 * 1024;

/// Ausgang eines einzelnen Upload-Stuecks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChunkOutcome {
    /// YouTube hat die Datei vollstaendig angenommen; das ist die Video-ID.
    Fertig(String),
    /// Stueck angenommen, ab dieser Byte-Position geht es weiter. Der Wert
    /// gehoert in die Datenbank, damit ein Abbruch nicht bei null beginnt.
    Weiter(u64),
}

/// Was YouTube ueber eine bestehende Upload-Sitzung sagt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResumeStand {
    /// Der Upload ist bereits durch. YouTube antwortet in diesem Fall mit
    /// derselben Video-Ressource wie beim regulaeren Abschluss, also samt `id`.
    /// Die wegzuwerfen hiesse, ein fertiges Video als Fehlschlag zu zaehlen und
    /// beim naechsten Lauf erneut hochzuladen.
    Fertig(String),
    /// So viele Bytes liegen drueben, hier geht es weiter.
    Offset(u64),
    /// Die Sitzung gibt es nicht mehr (404/410), es muss eine neue begonnen
    /// werden. Nur diese beiden Status zaehlen als verfallen; ein 500 oder 503
    /// ist voruebergehend und darf einen mehrstuendigen Upload nicht auf null
    /// zuruecksetzen.
    Verfallen,
}

/// Ergebnis eines vollstaendigen Shorts-Uploads.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShortsUpload {
    /// Die Video-ID bei YouTube.
    pub video_id: String,
    /// Die tatsaechlich benutzte Sitzung. Wurde eine uebergebene Sitzung als
    /// verfallen erkannt, steht hier die neue.
    pub session_uri: String,
}

/// Zwischenstand waehrend des Uploads. Geht an die Fortschrittssenke, damit der
/// Aufrufer Sitzung und Byte-Position dauerhaft festhalten kann; ohne das
/// beginnt jeder Abbruch wieder bei null.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UploadFortschritt {
    pub session_uri: String,
    pub offset: u64,
}

/// Senke fuer den Upload-Fortschritt (Persistenz liegt beim Aufrufer).
pub type FortschrittSink = Arc<dyn Fn(UploadFortschritt) + Send + Sync>;

/// Was ein erfolgreicher inline-Refresh geliefert hat.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RefreshedToken {
    pub access_token: String,
    /// Restlaufzeit in Sekunden, wie von Google gemeldet.
    pub expires_in: Option<i64>,
    /// Google rotiert das Refresh-Token nur selten, aber wenn, dann hier.
    pub refresh_token: Option<String>,
    pub scope: Option<String>,
}

/// Senke fuer ein frisch geholtes Access-Token. Ohne sie bleibt der Refresh im
/// Speicher der Instanz stehen, und der naechste Lauf holt wieder das
/// abgelaufene Token aus der Datenbank.
pub type TokenSink = Arc<dyn Fn(RefreshedToken) + Send + Sync>;

/// Refresh-Credentials für den inline 401-Selbstheilungs-Pfad. Spiegelt das, was
/// der google-api-python-client aus `Credentials(...)` hält.
#[derive(Clone)]
pub struct YouTubeRefreshCreds {
    pub refresh_token: String,
    pub client_id: String,
    pub client_secret: String,
    pub token_url: String,
}

/// YouTube-Uploader (Shorts und lange VODs).
pub struct YouTubeUploader {
    /// Mutable, da der inline-Refresh es bei 401 ersetzt.
    access_token: Mutex<String>,
    refresh: Option<Arc<YouTubeRefreshCreds>>,
    token_sink: Option<TokenSink>,
    fortschritt: Option<FortschrittSink>,
    upload_base: String,
    api_base: String,
    ffprobe: String,
    versuche: u32,
    backoff_basis: Duration,
    http: reqwest::Client,
}

impl YouTubeUploader {
    pub fn new(access_token: impl Into<String>) -> Self {
        Self {
            access_token: Mutex::new(access_token.into()),
            refresh: None,
            token_sink: None,
            fortschritt: None,
            upload_base: DEFAULT_UPLOAD_BASE.to_string(),
            api_base: DEFAULT_API_BASE.to_string(),
            ffprobe: "ffprobe".to_string(),
            versuche: DEFAULT_VERSUCHE,
            backoff_basis: DEFAULT_BACKOFF_BASIS,
            http: reqwest::Client::new(),
        }
    }

    /// Setzt die Refresh-Credentials für die inline 401-Selbstheilung.
    pub fn with_refresh(mut self, refresh: YouTubeRefreshCreds) -> Self {
        self.refresh = Some(Arc::new(refresh));
        self
    }

    /// Haengt eine Senke an, die jedes inline erneuerte Access-Token bekommt.
    /// Der Aufrufer schreibt es damit in die Datenbank, statt beim naechsten
    /// Lauf erneut das abgelaufene Token zu ziehen.
    pub fn with_token_sink(mut self, sink: TokenSink) -> Self {
        self.token_sink = Some(sink);
        self
    }

    /// Haengt eine Senke an, die Sitzung und Byte-Position nach jedem Stueck
    /// bekommt (fuer die Wiederaufnahme nach einem Abbruch).
    pub fn with_progress_sink(mut self, sink: FortschrittSink) -> Self {
        self.fortschritt = Some(sink);
        self
    }

    /// Überschreibt Upload- und API-Basis-URL (für Tests).
    pub fn with_bases(
        mut self,
        upload_base: impl Into<String>,
        api_base: impl Into<String>,
    ) -> Self {
        self.upload_base = upload_base.into();
        self.api_base = api_base.into();
        self
    }

    /// Setzt Versuchszahl und Backoff-Basis (Tests laufen mit Basis null).
    pub fn with_retry(mut self, versuche: u32, backoff_basis: Duration) -> Self {
        self.versuche = versuche.max(1);
        self.backoff_basis = backoff_basis;
        self
    }

    /// Pfad zum ffprobe-Binary (Tests und abweichende Installationen).
    pub fn with_ffprobe(mut self, ffprobe: impl Into<String>) -> Self {
        self.ffprobe = ffprobe.into();
        self
    }

    async fn token(&self) -> String {
        self.access_token.lock().await.clone()
    }

    fn melde_fortschritt(&self, session_uri: &str, offset: u64) {
        if let Some(sink) = &self.fortschritt {
            sink(UploadFortschritt {
                session_uri: session_uri.to_string(),
                offset,
            });
        }
    }

    /// Tauscht das Refresh-Token gegen ein frisches Access-Token (Google-OAuth)
    /// und legt es ab. Liefert das neue Token oder einen Fehler, wenn kein
    /// Refresh konfiguriert ist / der Tausch scheitert.
    async fn refresh_access_token(&self) -> Result<String, UploadError> {
        let creds = self.refresh.as_ref().ok_or(UploadError::NotAuthenticated)?;
        let resp = self
            .http
            .post(&creds.token_url)
            .form(&[
                ("grant_type", "refresh_token"),
                ("refresh_token", creds.refresh_token.as_str()),
                ("client_id", creds.client_id.as_str()),
                ("client_secret", creds.client_secret.as_str()),
            ])
            .send()
            .await
            .map_err(|e| UploadError::Request(e.to_string()))?;
        if resp.status().as_u16() != 200 {
            let body = resp.text().await.unwrap_or_default();
            return Err(UploadError::Api(format!(
                "YouTube token refresh failed: {body}"
            )));
        }
        let data: Value = resp
            .json()
            .await
            .map_err(|e| UploadError::Request(e.to_string()))?;
        let new_token = data
            .get("access_token")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .ok_or_else(|| {
                UploadError::Api("YouTube token refresh: no access_token in response".to_string())
            })?
            .to_string();
        *self.access_token.lock().await = new_token.clone();
        if let Some(sink) = &self.token_sink {
            sink(RefreshedToken {
                access_token: new_token.clone(),
                expires_in: data.get("expires_in").and_then(Value::as_i64),
                refresh_token: data
                    .get("refresh_token")
                    .and_then(Value::as_str)
                    .filter(|s| !s.is_empty())
                    .map(str::to_string),
                scope: data
                    .get("scope")
                    .and_then(Value::as_str)
                    .map(str::to_string),
            });
        }
        Ok(new_token)
    }

    /// Der eine Weg nach draussen. Baut die Anfrage je Versuch neu (das Token
    /// kann sich zwischendurch geaendert haben) und kuemmert sich um:
    /// * 401 → genau ein inline-Refresh, dann Wiederholung,
    /// * 5xx und Transportfehler → exponentielles Backoff mit Jitter,
    ///   `Retry-After` hat Vorrang,
    /// * 4xx → sofort zurueck an den Aufrufer, eine Wiederholung waere nur
    ///   schneller derselbe Fehler.
    ///
    /// Statuscodes bleiben ungedeutet; 308 und 404 haben je nach Aufrufer eine
    /// andere Bedeutung.
    async fn call(
        &self,
        was: &str,
        baue: impl Fn(&str) -> reqwest::RequestBuilder,
    ) -> Result<reqwest::Response, UploadError> {
        let mut token = self.token().await;
        let mut refreshed = false;
        let mut versuch: u32 = 0;
        loop {
            match baue(&token).send().await {
                Ok(resp) => {
                    let status = resp.status().as_u16();
                    if status == 401 && !refreshed && self.refresh.is_some() {
                        token = self.refresh_access_token().await?;
                        refreshed = true;
                        continue;
                    }
                    if (500..600).contains(&status) && versuch + 1 < self.versuche {
                        let warte = self.wartezeit(versuch, retry_after_sekunden(resp.headers()));
                        tracing::warn!(
                            was,
                            status,
                            versuch = versuch + 1,
                            wartet_ms = warte.as_millis() as u64,
                            "YouTube antwortet mit Serverfehler, neuer Versuch"
                        );
                        versuch += 1;
                        tokio::time::sleep(warte).await;
                        continue;
                    }
                    return Ok(resp);
                }
                Err(fehler) => {
                    if versuch + 1 < self.versuche {
                        let warte = self.wartezeit(versuch, None);
                        tracing::warn!(
                            was,
                            fehler = %fehler,
                            versuch = versuch + 1,
                            wartet_ms = warte.as_millis() as u64,
                            "YouTube nicht erreichbar, neuer Versuch"
                        );
                        versuch += 1;
                        tokio::time::sleep(warte).await;
                        continue;
                    }
                    return Err(UploadError::Request(fehler.to_string()));
                }
            }
        }
    }

    /// Wartezeit vor dem naechsten Versuch: verdoppelt sich je Runde, dazu bis
    /// zu einem Viertel Jitter, damit nicht alle Uploads im Gleichtakt
    /// wiederkommen. Ein `Retry-After` der Gegenseite sticht.
    fn wartezeit(&self, versuch: u32, retry_after: Option<u64>) -> Duration {
        if let Some(sekunden) = retry_after {
            return Duration::from_secs(sekunden).min(MAX_WARTEZEIT);
        }
        let basis = self.backoff_basis * (1u32 << versuch.min(6));
        (basis + jitter(basis)).min(MAX_WARTEZEIT)
    }

    /// Startet eine resumable Session und liefert die Session-URL.
    async fn sitzung_starten(
        &self,
        metadata: &Value,
        size_bytes: u64,
        content_type: &str,
    ) -> Result<String, UploadError> {
        let resp = self
            .call("YouTube resumable init", |token| {
                self.http
                    .post(format!("{}/videos", self.upload_base))
                    .query(&[("uploadType", "resumable"), ("part", "snippet,status")])
                    .bearer_auth(token)
                    .header("X-Upload-Content-Type", content_type)
                    // Ohne die angekuendigte Groesse kann YouTube den Upload
                    // weder pruefen noch sauber wiederaufnehmen.
                    .header("X-Upload-Content-Length", size_bytes.to_string())
                    .json(metadata)
            })
            .await?;
        if !resp.status().is_success() {
            return Err(fehler_aus_antwort(resp, "YouTube resumable init").await);
        }
        resp.headers()
            .get(LOCATION)
            .and_then(|v| v.to_str().ok())
            .map(str::to_string)
            .ok_or_else(|| {
                UploadError::Api("YouTube resumable init: no Location header".to_string())
            })
    }

    /// GET `…/videos?part=<part>&id=<video_id>`.
    async fn get_videos(
        &self,
        part: &str,
        video_id: &str,
    ) -> Result<reqwest::Response, UploadError> {
        self.call("YouTube videos.list", |token| {
            self.http
                .get(format!("{}/videos", self.api_base))
                .query(&[("part", part), ("id", video_id)])
                .bearer_auth(token)
        })
        .await
    }

    // ------------------------------------------------------------------
    // Stueckweiser resumable Upload
    //
    // Shorts und mehrstuendige VODs teilen sich diesen Weg. Metadaten und
    // Byte-Position kommen von aussen, damit der Aufrufer den Stand dauerhaft
    // festhalten kann.
    // ------------------------------------------------------------------

    /// Startet eine resumable Session fuer eine grosse Datei und liefert die
    /// Session-URL. Die Metadaten kommen komplett von aussen (Titel,
    /// Beschreibung, Sichtbarkeit), weil ein VOD andere Regeln hat als ein Short.
    pub async fn start_resumable_upload(
        &self,
        metadata: &Value,
        size_bytes: u64,
    ) -> Result<String, UploadError> {
        if self.token().await.is_empty() {
            return Err(UploadError::NotAuthenticated);
        }
        self.sitzung_starten(metadata, size_bytes, "video/*").await
    }

    /// Fragt bei YouTube nach, wie weit eine Sitzung gekommen ist.
    pub async fn resumable_offset(
        &self,
        session_url: &str,
        size_bytes: u64,
    ) -> Result<ResumeStand, UploadError> {
        let resp = self
            .call("YouTube resume query", |token| {
                self.http
                    .put(session_url)
                    .bearer_auth(token)
                    .header(CONTENT_TYPE, "video/*")
                    .header("Content-Range", format!("bytes */{size_bytes}"))
                    .body(Vec::new())
            })
            .await?;
        let status = resp.status().as_u16();
        if resp.status().is_success() {
            // Fertig: die Antwort ist die Video-Ressource inklusive `id`.
            let data: Value = resp
                .json()
                .await
                .map_err(|e| UploadError::Request(e.to_string()))?;
            return data["id"]
                .as_str()
                .map(|id| ResumeStand::Fertig(id.to_string()))
                .ok_or_else(|| {
                    UploadError::Api(
                        "YouTube resume query: Upload abgeschlossen, aber keine Video-ID in der Antwort"
                            .to_string(),
                    )
                });
        }
        if status == 308 {
            // Ohne Range-Header hat YouTube noch kein einziges Byte.
            return Ok(ResumeStand::Offset(
                range_end(resp.headers()).map_or(0, |end| end + 1),
            ));
        }
        if status == 404 || status == 410 {
            return Ok(ResumeStand::Verfallen);
        }
        Err(fehler_aus_antwort(resp, "YouTube resume query").await)
    }

    /// Schiebt genau ein Stueck ab `offset` hoch. Der Aufrufer schreibt den
    /// zurueckgegebenen Stand weg und ruft erneut auf, bis [`ChunkOutcome::Fertig`].
    pub async fn upload_chunk(
        &self,
        session_url: &str,
        video_path: &Path,
        offset: u64,
    ) -> Result<ChunkOutcome, UploadError> {
        let size = tokio::fs::metadata(video_path).await?.len();
        if offset >= size {
            return Err(UploadError::Validation(format!(
                "Byte-Position {offset} liegt hinter dem Dateiende {size}"
            )));
        }
        let laenge = RESUMABLE_CHUNK_BYTES.min(size - offset) as usize;
        let mut datei = tokio::fs::File::open(video_path).await?;
        datei.seek(std::io::SeekFrom::Start(offset)).await?;
        let mut puffer = vec![0u8; laenge];
        // `read` darf kurz liefern; Google erwartet aber exakt die angekuendigte
        // Stueckgroesse, deshalb bis zum Ende fuellen.
        datei.read_exact(&mut puffer).await?;
        let ende = offset + laenge as u64 - 1;

        let resp = self
            .call("YouTube upload", |token| {
                self.http
                    .put(session_url)
                    .bearer_auth(token)
                    .header(CONTENT_TYPE, "video/*")
                    .header("Content-Range", format!("bytes {offset}-{ende}/{size}"))
                    .body(puffer.clone())
            })
            .await?;
        if resp.status().as_u16() == 308 {
            // YouTube bestaetigt den Stand; er kann hinter unserem Stueck
            // zurueckliegen, wenn ein Teil verloren ging.
            let weiter = range_end(resp.headers()).map_or(ende + 1, |end| end + 1);
            return Ok(ChunkOutcome::Weiter(weiter));
        }
        if resp.status().is_success() {
            let data: Value = resp
                .json()
                .await
                .map_err(|e| UploadError::Request(e.to_string()))?;
            return data["id"]
                .as_str()
                .map(|id| ChunkOutcome::Fertig(id.to_string()))
                .ok_or_else(|| {
                    UploadError::Api("YouTube upload: no video id in response".to_string())
                });
        }
        Err(fehler_aus_antwort(resp, "YouTube upload").await)
    }

    /// Laedt einen Short stueckweise hoch. `session` ist eine zuvor
    /// weggeschriebene Sitzung; ist sie gesetzt, wird der Upload dort
    /// fortgesetzt, statt von vorn zu beginnen. Der Fortschritt geht laufend an
    /// die Fortschrittssenke (siehe [`YouTubeUploader::with_progress_sink`]),
    /// die benutzte Sitzung steht am Ende auch im Ergebnis.
    pub async fn upload_short_resumable(
        &self,
        video_path: &str,
        title: &str,
        description: &str,
        hashtags: &[String],
        session: Option<&str>,
    ) -> Result<ShortsUpload, UploadError> {
        if self.token().await.is_empty() {
            return Err(UploadError::NotAuthenticated);
        }
        self.validate_video(video_path)?;
        self.pruefe_shorts_eignung(video_path).await?;
        let groesse = tokio::fs::metadata(video_path).await?.len();
        if groesse == 0 {
            return Err(UploadError::Validation(format!(
                "Videodatei ist leer: {video_path}"
            )));
        }
        let body = baue_snippet(title, description, hashtags)?;

        let (sitzung, mut offset) = match session.filter(|s| !s.is_empty()) {
            Some(uri) => match self.resumable_offset(uri, groesse).await? {
                ResumeStand::Fertig(video_id) => {
                    return Ok(ShortsUpload {
                        video_id,
                        session_uri: uri.to_string(),
                    })
                }
                ResumeStand::Offset(stand) => (uri.to_string(), stand),
                ResumeStand::Verfallen => {
                    tracing::info!(video_path, "Upload-Sitzung verfallen, beginne neu");
                    (self.sitzung_starten(&body, groesse, "video/mp4").await?, 0)
                }
            },
            None => (self.sitzung_starten(&body, groesse, "video/mp4").await?, 0),
        };
        self.melde_fortschritt(&sitzung, offset);

        loop {
            if offset >= groesse {
                // YouTube hat alles, meldet den Abschluss aber nicht. Ohne diese
                // Bremse liefe die Schleife leer.
                return Err(UploadError::Api(
                    "YouTube upload: vollstaendig uebertragen, aber ohne Video-ID".to_string(),
                ));
            }
            match self
                .upload_chunk(&sitzung, Path::new(video_path), offset)
                .await?
            {
                ChunkOutcome::Fertig(video_id) => {
                    return Ok(ShortsUpload {
                        video_id,
                        session_uri: sitzung.clone(),
                    })
                }
                ChunkOutcome::Weiter(stand) => {
                    if stand <= offset {
                        return Err(UploadError::Api(format!(
                            "YouTube upload: kein Fortschritt bei Byte-Position {offset}"
                        )));
                    }
                    offset = stand;
                    self.melde_fortschritt(&sitzung, offset);
                }
            }
        }
    }

    /// Prueft per ffprobe, ob die Datei ueberhaupt als Short taugt: hoechstens
    /// drei Minuten und hochkant. Laesst sich das nicht feststellen (ffprobe
    /// fehlt, Datei unlesbar), bleibt es bei einer Warnung; ein fehlendes
    /// Werkzeug darf keinen Upload verhindern.
    async fn pruefe_shorts_eignung(&self, video_path: &str) -> Result<(), UploadError> {
        let ausgabe = tokio::process::Command::new(&self.ffprobe)
            .args([
                "-v",
                "error",
                "-select_streams",
                "v:0",
                "-show_entries",
                "stream=width,height,duration:format=duration",
                "-of",
                "json",
                video_path,
            ])
            .output()
            .await;
        let daten = match ausgabe {
            Ok(out) if out.status.success() => match serde_json::from_slice::<Value>(&out.stdout) {
                Ok(v) => v,
                Err(fehler) => {
                    tracing::warn!(video_path, %fehler, "ffprobe-Ausgabe unlesbar, Shorts-Pruefung uebersprungen");
                    return Ok(());
                }
            },
            Ok(out) => {
                tracing::warn!(
                    video_path,
                    meldung = %String::from_utf8_lossy(&out.stderr).trim(),
                    "ffprobe meldet einen Fehler, Shorts-Pruefung uebersprungen"
                );
                return Ok(());
            }
            Err(fehler) => {
                tracing::warn!(video_path, %fehler, "ffprobe nicht ausfuehrbar, Shorts-Pruefung uebersprungen");
                return Ok(());
            }
        };

        let stream = daten
            .pointer("/streams/0")
            .cloned()
            .unwrap_or_else(|| json!({}));
        let dauer = zahl(stream.get("duration"))
            .or_else(|| zahl(daten.pointer("/format/duration")))
            .unwrap_or(0.0);
        if dauer > SHORTS_MAX_DAUER_SEKUNDEN {
            return Err(UploadError::Validation(format!(
                "Video ist {dauer:.0} s lang, Shorts duerfen hoechstens {SHORTS_MAX_DAUER_SEKUNDEN:.0} s haben"
            )));
        }
        let breite = zahl(stream.get("width")).unwrap_or(0.0);
        let hoehe = zahl(stream.get("height")).unwrap_or(0.0);
        if breite > 0.0 && hoehe > 0.0 {
            let verhaeltnis = breite / hoehe;
            if verhaeltnis > 1.0 {
                return Err(UploadError::Validation(format!(
                    "Video ist im Querformat ({breite:.0}x{hoehe:.0}), Shorts brauchen Hochformat (9:16)"
                )));
            }
            if (verhaeltnis - SHORTS_ZIEL_VERHAELTNIS).abs() > 0.05 {
                tracing::warn!(
                    video_path,
                    breite,
                    hoehe,
                    "Seitenverhaeltnis weicht von 9:16 ab, YouTube fuellt die Raender auf"
                );
            }
        }
        Ok(())
    }
}

/// Baut den `snippet`/`status`-Teil des Upload-Bodys und lehnt ab, was YouTube
/// ohnehin mit einem 400 zurueckweisen wuerde.
fn baue_snippet(title: &str, description: &str, hashtags: &[String]) -> Result<Value, UploadError> {
    let mut snippet = Map::new();
    snippet.insert("title".to_string(), json!(bereinige_titel(title)?));
    snippet.insert(
        "description".to_string(),
        json!(baue_beschreibung(description, hashtags)),
    );
    let tags = tags_im_zeichenbudget(hashtags, TAGS_MAX_CHARS);
    if !tags.is_empty() {
        // Ein leeres Tag-Feld quittiert die API mit 400, deshalb nur setzen,
        // wenn wirklich etwas drinsteht.
        snippet.insert("tags".to_string(), json!(tags));
    }
    snippet.insert("categoryId".to_string(), json!(CATEGORY_GAMING));
    Ok(json!({
        "snippet": Value::Object(snippet),
        "status": { "privacyStatus": DEFAULT_PRIVACY, "selfDeclaredMadeForKids": false },
    }))
}

/// Waehlt so viele Tags, wie ins Zeichenbudget passen. Gezaehlt wird wie bei
/// YouTube: die Zeichen des Tags, plus zwei fuer die impliziten
/// Anfuehrungszeichen, wenn er ein Leerzeichen enthaelt, plus eins fuer das
/// Trennkomma vor jedem weiteren Tag.
pub(crate) fn tags_im_zeichenbudget(hashtags: &[String], budget: usize) -> Vec<String> {
    let mut gewaehlt: Vec<String> = Vec::new();
    let mut belegt = 0usize;
    for tag in hashtags {
        let tag = tag.trim();
        if tag.is_empty() {
            continue;
        }
        let mut kosten = tag.chars().count();
        if tag.chars().any(char::is_whitespace) {
            kosten += 2;
        }
        if !gewaehlt.is_empty() {
            kosten += 1;
        }
        if belegt + kosten > budget {
            break;
        }
        belegt += kosten;
        gewaehlt.push(tag.to_string());
    }
    gewaehlt
}

/// Baut die Beschreibung so, dass `#Shorts` die Kuerzung ueberlebt: erst den
/// Rumpf auf das verbleibende Budget kuerzen, dann den Anhang dransetzen.
fn baue_beschreibung(description: &str, hashtags: &[String]) -> String {
    let rumpf = bereinige_text(&format!("{description}\n\n{}", format_hashtags(hashtags)));
    let platz = DESCRIPTION_MAX_BYTES.saturating_sub(SHORTS_SUFFIX.len());
    format!(
        "{}{SHORTS_SUFFIX}",
        truncate_bytes(&rumpf, platz).trim_end()
    )
}

/// Entfernt die spitzen Klammern. YouTube lehnt Titel und Beschreibungen mit
/// `<` oder `>` als `invalidTitle`/`invalidDescription` ab, und beides kommt
/// hier aus Twitch-Clip-Titeln und KI-Ausgaben, wo ein `<3` normal ist.
fn bereinige_text(text: &str) -> String {
    text.chars().filter(|c| *c != '<' && *c != '>').collect()
}

/// Titel bereinigen und ablehnen, wenn nichts uebrig bleibt. Ein leerer Titel
/// ist ein `invalidTitle`-400, den man vor dem Upload abfangen kann.
fn bereinige_titel(title: &str) -> Result<String, UploadError> {
    let sauber = truncate_chars(bereinige_text(title).trim(), TITLE_MAX)
        .trim_end()
        .to_string();
    if sauber.is_empty() {
        return Err(UploadError::Validation(
            "YouTube-Titel ist leer (nach dem Entfernen der spitzen Klammern)".to_string(),
        ));
    }
    Ok(sauber)
}

/// Kuerzt auf `max` Bytes, ohne mitten in ein Zeichen zu schneiden.
fn truncate_bytes(s: &str, max: usize) -> &str {
    if s.len() <= max {
        return s;
    }
    let mut ende = max;
    while ende > 0 && !s.is_char_boundary(ende) {
        ende -= 1;
    }
    &s[..ende]
}

/// Liest eine Zahl, die ffprobe je nach Feld als Zahl oder als String liefert.
fn zahl(value: Option<&Value>) -> Option<f64> {
    value.and_then(|v| {
        v.as_f64()
            .or_else(|| v.as_str().and_then(|s| s.trim().parse().ok()))
    })
}

/// Bis zu einem Viertel der Wartezeit als Zufallsanteil, gespeist aus der
/// Systemuhr. Reicht, um gleichzeitig gestartete Uploads auseinanderzuziehen.
fn jitter(basis: Duration) -> Duration {
    let spanne = basis.as_millis() as u64 / 4;
    if spanne == 0 {
        return Duration::ZERO;
    }
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.subsec_nanos() as u64)
        .unwrap_or(0);
    Duration::from_millis(nanos % (spanne + 1))
}

/// Liest den Fehler aus einer nicht erfolgreichen Antwort.
async fn fehler_aus_antwort(resp: reqwest::Response, kontext: &str) -> UploadError {
    let status = resp.status().as_u16();
    let body = resp.text().await.unwrap_or_default();
    quota_or_api(status, body, kontext)
}

/// Ein 403/429 wegen erschoepftem Kontingent ist kein Defekt, sondern der
/// Hinweis, den Rest zu verschieben. Entschieden wird an `error.errors[].reason`,
/// nicht am Substring "quota": `uploadLimitExceeded` und `rateLimitExceeded`
/// enthalten den gar nicht.
fn quota_or_api(status: u16, body: String, kontext: &str) -> UploadError {
    let kurz: String = body.chars().take(400).collect();
    if (status == 403 || status == 429) && ist_kontingent(&body) {
        return UploadError::QuotaExceeded(kurz);
    }
    UploadError::Api(format!("{kontext} failed ({status}): {kurz}"))
}

/// Prueft die Fehlergruende im Body gegen die Kontingent-Liste. Der
/// Textvergleich bleibt als Rueckfall, falls die Antwort kein JSON ist.
fn ist_kontingent(body: &str) -> bool {
    if let Ok(daten) = serde_json::from_str::<Value>(body) {
        let treffer = daten
            .pointer("/error/errors")
            .and_then(Value::as_array)
            .map(|errors| {
                errors.iter().any(|e| {
                    e.get("reason")
                        .and_then(Value::as_str)
                        .is_some_and(|r| QUOTA_REASONS.contains(&r))
                })
            })
            .unwrap_or(false);
        if treffer {
            return true;
        }
        if daten.pointer("/error/status").and_then(Value::as_str) == Some("RESOURCE_EXHAUSTED") {
            return true;
        }
    }
    let klein = body.to_lowercase();
    klein.contains("quota")
        || klein.contains("ratelimitexceeded")
        || klein.contains("uploadlimitexceeded")
}

/// Letzte bestaetigte Byte-Position aus dem `Range`-Header (`bytes=0-12345`).
fn range_end(headers: &reqwest::header::HeaderMap) -> Option<u64> {
    headers
        .get("range")?
        .to_str()
        .ok()?
        .rsplit('-')
        .next()?
        .trim()
        .parse()
        .ok()
}

/// `Retry-After` in Sekunden. Die Datumsform ignorieren wir bewusst, dann
/// greift das normale Backoff.
fn retry_after_sekunden(headers: &reqwest::header::HeaderMap) -> Option<u64> {
    headers.get(RETRY_AFTER)?.to_str().ok()?.trim().parse().ok()
}

#[async_trait]
impl PlatformUploader for YouTubeUploader {
    fn platform_name(&self) -> &str {
        "youtube"
    }

    fn validate_video(&self, video_path: &str) -> Result<(), UploadError> {
        // Synchron nur die Existenz (256 GB Dateigrenze erreicht hier niemand).
        // Dauer und Seitenverhaeltnis prueft `pruefe_shorts_eignung` per
        // ffprobe; das braucht await und passt deshalb nicht in diese Signatur.
        validate_local_file(video_path, f64::INFINITY)
    }

    async fn upload_video(
        &self,
        video_path: &str,
        title: &str,
        description: &str,
        hashtags: &[String],
    ) -> Result<String, UploadError> {
        self.upload_short_resumable(video_path, title, description, hashtags, None)
            .await
            .map(|fertig| fertig.video_id)
    }

    async fn get_video_status(&self, video_id: &str) -> Value {
        let result = async {
            let resp = self
                .get_videos("status,processingDetails", video_id)
                .await?;
            let data = expect_ok_json(resp, "YouTube status fetch").await?;
            let item = data["items"].get(0).cloned().unwrap_or_else(|| json!({}));
            if item.is_null() || item.as_object().map(|o| o.is_empty()).unwrap_or(true) {
                return Ok::<Value, UploadError>(json!({}));
            }
            Ok(json!({
                "status": item["status"]["uploadStatus"],
                "processing_status": item["processingDetails"]["processingStatus"],
            }))
        }
        .await;
        match result {
            Ok(status) => status,
            Err(fehler) => {
                // Best-effort, aber nicht stumm: sonst sieht niemand, dass der
                // Status seit Tagen nicht mehr abgeholt werden kann.
                tracing::warn!(video_id, %fehler, "YouTube-Status nicht abrufbar");
                json!({})
            }
        }
    }

    async fn fetch_video_analytics(
        &self,
        video_id: &str,
        bucket: &str,
    ) -> Result<AnalyticsSnapshot, UploadError> {
        if self.token().await.is_empty() {
            return Err(UploadError::NotAuthenticated);
        }
        let resp = self.get_videos("statistics", video_id).await?;
        let data = expect_ok_json(resp, "YouTube analytics").await?;
        // Geloeschtes, privates oder falsch adressiertes Video: 200 mit leerer
        // Liste. Daraus einen Snapshot mit lauter Nullen zu machen, faelscht die
        // Zeitreihe, deshalb ein Fehler.
        let stats = data["items"]
            .get(0)
            .map(|i| i["statistics"].clone())
            .ok_or_else(|| {
                UploadError::Api(format!(
                    "YouTube analytics: video not found or not visible ({video_id})"
                ))
            })?;
        Ok(AnalyticsSnapshot::build(
            bucket,
            "youtube_data_api_v3",
            as_count(stats.get("viewCount")),
            as_count(stats.get("likeCount")),
            as_count(stats.get("commentCount")),
            // `statistics` kennt nur viewCount, likeCount, dislikeCount,
            // favoriteCount und commentCount. Ein `shareCount` gibt es in der
            // Data API nicht, YouTube liefert Shares nur in YouTube Analytics.
            // Deshalb hier ehrlich 0 statt einer erfundenen Zahl.
            0,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    /// Uploader ohne echte Wartezeiten (Backoff-Basis null).
    fn uploader(server: &MockServer) -> YouTubeUploader {
        YouTubeUploader::new("tok")
            .with_bases(server.uri(), server.uri())
            .with_retry(3, Duration::ZERO)
    }

    async fn temp_video() -> String {
        let p = std::env::temp_dir().join("tb_youtube_test_clip.mp4");
        tokio::fs::write(&p, b"fake-video-bytes").await.unwrap();
        p.to_string_lossy().into_owned()
    }

    #[tokio::test]
    async fn resumable_upload_zwei_schritte() {
        let server = MockServer::start().await;
        let session_url = format!("{}/session/abc", server.uri());
        // Schritt 1: init liefert Location-Header.
        Mock::given(method("POST"))
            .and(path("/videos"))
            .respond_with(
                ResponseTemplate::new(200).insert_header("Location", session_url.as_str()),
            )
            .mount(&server)
            .await;
        // Schritt 2: PUT der Bytes → Video-ID.
        Mock::given(method("PUT"))
            .and(path("/session/abc"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "id": "yt-123" })))
            .mount(&server)
            .await;

        let up = uploader(&server);
        let video = temp_video().await;
        let id = up
            .upload_video(&video, "Titel", "Desc", &["deadlock".into()])
            .await
            .unwrap();
        assert_eq!(id, "yt-123");
    }

    // Der Shorts-Pfad meldet die benutzte Sitzung nach aussen und schreibt den
    // Fortschritt in die Senke; beides braucht der upload_worker fuer die
    // Wiederaufnahme.
    #[tokio::test]
    async fn shorts_upload_meldet_sitzung_und_fortschritt() {
        let server = MockServer::start().await;
        let session_url = format!("{}/session/abc", server.uri());
        Mock::given(method("POST"))
            .and(path("/videos"))
            .respond_with(
                ResponseTemplate::new(200).insert_header("Location", session_url.as_str()),
            )
            .mount(&server)
            .await;
        Mock::given(method("PUT"))
            .and(path("/session/abc"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "id": "yt-123" })))
            .mount(&server)
            .await;

        let gesehen = Arc::new(std::sync::Mutex::new(Vec::new()));
        let senke = gesehen.clone();
        let up = uploader(&server).with_progress_sink(Arc::new(move |f: UploadFortschritt| {
            senke.lock().unwrap().push(f);
        }));
        let video = temp_video().await;
        let fertig = up
            .upload_short_resumable(&video, "Titel", "Desc", &[], None)
            .await
            .unwrap();
        assert_eq!(fertig.video_id, "yt-123");
        assert_eq!(fertig.session_uri, session_url);
        let stand = gesehen.lock().unwrap().clone();
        assert_eq!(
            stand,
            vec![UploadFortschritt {
                session_uri: session_url,
                offset: 0
            }]
        );
    }

    // Eine mitgegebene Sitzung wird fortgesetzt, statt neu zu beginnen: der
    // erste PUT ist die Standabfrage, kein zweiter init.
    #[tokio::test]
    async fn shorts_upload_setzt_gespeicherte_sitzung_fort() {
        let server = MockServer::start().await;
        let session_url = format!("{}/session/weiter", server.uri());
        Mock::given(method("PUT"))
            .and(path("/session/weiter"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "id": "yt-schon-da" })))
            .mount(&server)
            .await;
        let up = uploader(&server);
        let video = temp_video().await;
        let fertig = up
            .upload_short_resumable(&video, "Titel", "Desc", &[], Some(&session_url))
            .await
            .unwrap();
        assert_eq!(fertig.video_id, "yt-schon-da");
    }

    #[tokio::test]
    async fn fehlerpfade() {
        let video = temp_video().await;
        // Kein Token.
        assert!(matches!(
            YouTubeUploader::new("")
                .upload_video(&video, "t", "d", &[])
                .await,
            Err(UploadError::NotAuthenticated)
        ));
        // Fehlende Datei.
        assert!(matches!(
            YouTubeUploader::new("tok")
                .upload_video("/nope.mp4", "t", "d", &[])
                .await,
            Err(UploadError::Validation(_))
        ));
        // init non-200 OHNE Refresh-Creds → kein Retry, harter Api-Fehler (1:1).
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/videos"))
            .respond_with(ResponseTemplate::new(401).set_body_string("unauth"))
            .mount(&server)
            .await;
        let up = uploader(&server);
        assert!(matches!(
            up.upload_video(&video, "t", "d", &[]).await,
            Err(UploadError::Api(_))
        ));
    }

    fn refresh_creds(token_url: String) -> YouTubeRefreshCreds {
        YouTubeRefreshCreds {
            refresh_token: "rt".into(),
            client_id: "cid".into(),
            client_secret: "secret".into(),
            token_url,
        }
    }

    // uploaders-1: läuft das Access-Token mitten im Upload ab (401), erneuert der
    // Uploader es inline genau einmal und wiederholt den Call (Parität zum
    // google-api-python-client). init 401 → Token-Refresh → init 200 (Location)
    // → PUT 200.
    #[tokio::test]
    async fn init_401_loest_inline_refresh_und_retry_aus() {
        let server = MockServer::start().await;
        let session_url = format!("{}/session/abc", server.uri());

        // Erster init-Aufruf (altes Token) → 401, nur einmal.
        Mock::given(method("POST"))
            .and(path("/videos"))
            .respond_with(ResponseTemplate::new(401).set_body_string("expired"))
            .up_to_n_times(1)
            .with_priority(1)
            .mount(&server)
            .await;
        // Token-Refresh-Endpoint liefert frisches Access-Token.
        Mock::given(method("POST"))
            .and(path("/token"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(json!({ "access_token": "fresh", "expires_in": 3600 })),
            )
            .mount(&server)
            .await;
        // Zweiter init-Aufruf (frisches Token) → 200 + Location.
        Mock::given(method("POST"))
            .and(path("/videos"))
            .respond_with(
                ResponseTemplate::new(200).insert_header("Location", session_url.as_str()),
            )
            .with_priority(2)
            .mount(&server)
            .await;
        // PUT der Bytes → Video-ID.
        Mock::given(method("PUT"))
            .and(path("/session/abc"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "id": "yt-refreshed" })))
            .mount(&server)
            .await;

        let gesehen = Arc::new(std::sync::Mutex::new(Vec::new()));
        let senke = gesehen.clone();
        let uploader = uploader(&server)
            .with_refresh(refresh_creds(format!("{}/token", server.uri())))
            .with_token_sink(Arc::new(move |t: RefreshedToken| {
                senke.lock().unwrap().push(t)
            }));
        let video = temp_video().await;
        let id = uploader
            .upload_video(&video, "Titel", "Desc", &["deadlock".into()])
            .await
            .unwrap();
        assert_eq!(id, "yt-refreshed");
        // Token wurde inline ersetzt.
        assert_eq!(uploader.token().await, "fresh");
        // ... und die Senke hat es bekommen, damit es in die Datenbank kann.
        let tokens = gesehen.lock().unwrap().clone();
        assert_eq!(tokens.len(), 1);
        assert_eq!(tokens[0].access_token, "fresh");
        assert_eq!(tokens[0].expires_in, Some(3600));
    }

    // Scheitert der Refresh selbst (z.B. ungültiges Refresh-Token), bricht der
    // Upload kontrolliert mit Api-Fehler ab (kein Endlos-Retry).
    #[tokio::test]
    async fn init_401_mit_scheiterndem_refresh_endet_im_fehler() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/videos"))
            .respond_with(ResponseTemplate::new(401).set_body_string("expired"))
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/token"))
            .respond_with(ResponseTemplate::new(400).set_body_string("invalid_grant"))
            .mount(&server)
            .await;
        let uploader =
            uploader(&server).with_refresh(refresh_creds(format!("{}/token", server.uri())));
        let video = temp_video().await;
        assert!(matches!(
            uploader.upload_video(&video, "t", "d", &[]).await,
            Err(UploadError::Api(_))
        ));
    }

    // 5xx ist voruebergehend: erst 503, dann 200, und der Upload laeuft durch.
    #[tokio::test]
    async fn backoff_wiederholt_bei_serverfehler() {
        let server = MockServer::start().await;
        let session_url = format!("{}/session/abc", server.uri());
        Mock::given(method("POST"))
            .and(path("/videos"))
            .respond_with(ResponseTemplate::new(503).set_body_string("backend error"))
            .up_to_n_times(2)
            .with_priority(1)
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/videos"))
            .respond_with(
                ResponseTemplate::new(200).insert_header("Location", session_url.as_str()),
            )
            .with_priority(2)
            .mount(&server)
            .await;
        Mock::given(method("PUT"))
            .and(path("/session/abc"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "id": "yt-nach-503" })))
            .mount(&server)
            .await;
        let up = uploader(&server);
        let video = temp_video().await;
        assert_eq!(
            up.upload_video(&video, "Titel", "Desc", &[]).await.unwrap(),
            "yt-nach-503"
        );
    }

    // Ein 400 wird nie wiederholt: die Antwort waere beim zweiten Mal dieselbe.
    #[tokio::test]
    async fn kein_backoff_bei_400() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/videos"))
            .respond_with(
                ResponseTemplate::new(400)
                    .set_body_string("{\"error\":{\"errors\":[{\"reason\":\"invalidTags\"}]}}"),
            )
            .expect(1)
            .mount(&server)
            .await;
        let up = uploader(&server);
        let video = temp_video().await;
        assert!(matches!(
            up.upload_video(&video, "t", "d", &[]).await,
            Err(UploadError::Api(_))
        ));
        // `expect(1)` prueft beim Drop des Servers, dass es genau ein Aufruf war.
        drop(up);
    }

    // Erschoepftes Tageskontingent ist eine eigene Variante, kein Api-Fehler.
    #[tokio::test]
    async fn kontingent_wird_eigenstaendig_gemeldet() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/videos"))
            .respond_with(
                ResponseTemplate::new(403)
                    .set_body_string("{\"error\":{\"errors\":[{\"reason\":\"quotaExceeded\"}]}}"),
            )
            .mount(&server)
            .await;
        let up = uploader(&server);
        let fehler = up
            .start_resumable_upload(&json!({"snippet": {}}), 4096)
            .await
            .unwrap_err();
        assert!(matches!(fehler, UploadError::QuotaExceeded(_)));
    }

    // Auch die Gruende ohne das Wort "quota" zaehlen als Kontingent, und der
    // Shorts-Pfad erkennt sie ebenfalls (frueher wurde daraus ein Api-Fehler).
    #[tokio::test]
    async fn upload_limit_gilt_auch_im_shorts_pfad() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/videos"))
            .respond_with(
                ResponseTemplate::new(403).set_body_string(
                    "{\"error\":{\"errors\":[{\"reason\":\"uploadLimitExceeded\"}]}}",
                ),
            )
            .mount(&server)
            .await;
        let up = uploader(&server);
        let video = temp_video().await;
        assert!(matches!(
            up.upload_video(&video, "t", "d", &[]).await,
            Err(UploadError::QuotaExceeded(_))
        ));
    }

    #[test]
    fn quota_erkennt_alle_gruende() {
        for grund in QUOTA_REASONS {
            let body = format!("{{\"error\":{{\"errors\":[{{\"reason\":\"{grund}\"}}]}}}}");
            assert!(
                matches!(quota_or_api(403, body, "x"), UploadError::QuotaExceeded(_)),
                "{grund} nicht erkannt"
            );
        }
        // Ein anderer 403 bleibt ein Api-Fehler.
        let anderer = "{\"error\":{\"errors\":[{\"reason\":\"forbidden\"}]}}".to_string();
        assert!(matches!(
            quota_or_api(403, anderer, "x"),
            UploadError::Api(_)
        ));
        // Und ein 400 ist nie Kontingent, egal was im Text steht.
        assert!(matches!(
            quota_or_api(400, "quota".into(), "x"),
            UploadError::Api(_)
        ));
    }

    #[tokio::test]
    async fn analytics_aus_statistics() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/videos"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "items": [{ "statistics": { "viewCount": "500", "likeCount": "40", "commentCount": "10" } }]
            })))
            .mount(&server)
            .await;
        let up = uploader(&server);
        let stats = up.fetch_video_analytics("yt-1", "d1").await.unwrap();
        assert_eq!(stats.views, 500);
        assert_eq!(stats.likes, 40);
        // Die Data API kennt kein shareCount; Shares sind fuer YouTube nicht
        // verfuegbar und bleiben deshalb bewusst 0.
        assert_eq!(stats.shares, 0);
        assert_eq!(stats.provider, "youtube_data_api_v3");
        // engagement = (40+10)/500*100 = 10, ohne Shares.
        assert_eq!(stats.engagement_rate, Some(10.0));
    }

    // Auch wenn YouTube ein shareCount mitschickte: es fliesst nicht ein.
    #[tokio::test]
    async fn analytics_ignoriert_erfundenes_sharecount() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/videos"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "items": [{ "statistics": { "viewCount": "100", "likeCount": "0", "commentCount": "0", "shareCount": "99" } }]
            })))
            .mount(&server)
            .await;
        let stats = uploader(&server)
            .fetch_video_analytics("yt-1", "d1")
            .await
            .unwrap();
        assert_eq!(stats.shares, 0);
    }

    // Geloescht, privat oder falsche ID: 200 mit leerer Liste. Das darf kein
    // Snapshot mit lauter Nullen werden.
    #[tokio::test]
    async fn analytics_leere_items_sind_ein_fehler() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/videos"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "items": [] })))
            .mount(&server)
            .await;
        let fehler = uploader(&server)
            .fetch_video_analytics("yt-weg", "d1")
            .await
            .unwrap_err();
        assert!(matches!(fehler, UploadError::Api(_)));
    }

    /// Legt eine Testdatei mit `groesse` Bytes an.
    async fn temp_datei(name: &str, groesse: usize) -> std::path::PathBuf {
        let p = std::env::temp_dir().join(name);
        tokio::fs::write(&p, vec![7u8; groesse]).await.unwrap();
        p
    }

    // Stueckweiser Upload: das letzte Stueck kommt mit 200 zurueck und traegt
    // die Video-ID.
    #[tokio::test]
    async fn chunk_upload_liefert_video_id() {
        let server = MockServer::start().await;
        Mock::given(method("PUT"))
            .and(path("/session/vod"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "id": "yt-vod" })))
            .mount(&server)
            .await;
        let up = uploader(&server);
        let datei = temp_datei("tb_vod_chunk_fertig.mp4", 1024).await;
        let outcome = up
            .upload_chunk(&format!("{}/session/vod", server.uri()), &datei, 0)
            .await
            .unwrap();
        assert_eq!(outcome, ChunkOutcome::Fertig("yt-vod".into()));
    }

    // 308 heisst "angenommen, mach weiter"; der neue Stand kommt aus dem
    // Range-Header und ist das, was der Aufrufer wegschreibt.
    #[tokio::test]
    async fn chunk_upload_meldet_naechste_position() {
        let server = MockServer::start().await;
        Mock::given(method("PUT"))
            .and(path("/session/vod"))
            .respond_with(ResponseTemplate::new(308).insert_header("Range", "bytes=0-511"))
            .mount(&server)
            .await;
        let up = uploader(&server);
        let datei = temp_datei("tb_vod_chunk_weiter.mp4", 1024).await;
        let outcome = up
            .upload_chunk(&format!("{}/session/vod", server.uri()), &datei, 0)
            .await
            .unwrap();
        assert_eq!(outcome, ChunkOutcome::Weiter(512));
    }

    // Wiederaufnahme: 2xx heisst fertig samt Video-ID, 308 ohne Range heisst
    // null Bytes, mit Range heisst Fortsetzen, 404/410 heisst verfallen, und
    // ein 5xx ist ein Fehler und keine verfallene Sitzung.
    #[tokio::test]
    async fn resume_offset_faelle() {
        let server = MockServer::start().await;
        Mock::given(method("PUT"))
            .and(path("/fertig"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "id": "yt-fertig" })))
            .mount(&server)
            .await;
        Mock::given(method("PUT"))
            .and(path("/leer"))
            .respond_with(ResponseTemplate::new(308))
            .mount(&server)
            .await;
        Mock::given(method("PUT"))
            .and(path("/teilweise"))
            .respond_with(ResponseTemplate::new(308).insert_header("Range", "bytes=0-99"))
            .mount(&server)
            .await;
        Mock::given(method("PUT"))
            .and(path("/verfallen"))
            .respond_with(ResponseTemplate::new(404))
            .mount(&server)
            .await;
        Mock::given(method("PUT"))
            .and(path("/weg"))
            .respond_with(ResponseTemplate::new(410))
            .mount(&server)
            .await;
        Mock::given(method("PUT"))
            .and(path("/kaputt"))
            .respond_with(ResponseTemplate::new(500).set_body_string("backend error"))
            .mount(&server)
            .await;
        let up = uploader(&server);
        let uri = |p: &str| format!("{}{p}", server.uri());
        assert_eq!(
            up.resumable_offset(&uri("/fertig"), 500).await.unwrap(),
            ResumeStand::Fertig("yt-fertig".into())
        );
        assert_eq!(
            up.resumable_offset(&uri("/leer"), 500).await.unwrap(),
            ResumeStand::Offset(0)
        );
        assert_eq!(
            up.resumable_offset(&uri("/teilweise"), 500).await.unwrap(),
            ResumeStand::Offset(100)
        );
        assert_eq!(
            up.resumable_offset(&uri("/verfallen"), 500).await.unwrap(),
            ResumeStand::Verfallen
        );
        assert_eq!(
            up.resumable_offset(&uri("/weg"), 500).await.unwrap(),
            ResumeStand::Verfallen
        );
        // Kein Verfallen: ein Serverfehler darf keinen Upload auf null werfen.
        assert!(matches!(
            up.resumable_offset(&uri("/kaputt"), 500).await,
            Err(UploadError::Api(_))
        ));
    }

    #[tokio::test]
    async fn status_mapping() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/videos"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "items": [{ "status": { "uploadStatus": "uploaded" }, "processingDetails": { "processingStatus": "processing" } }]
            })))
            .mount(&server)
            .await;
        let up = uploader(&server);
        let s = up.get_video_status("yt-1").await;
        assert_eq!(s["status"], "uploaded");
        assert_eq!(s["processing_status"], "processing");
    }

    // ------------------------------------------------------------------
    // Reine Funktionen: Tags, Beschreibung, Titel
    // ------------------------------------------------------------------

    #[test]
    fn tag_budget_zaehlt_zeichen_statt_eintraege() {
        // 40 Tags a 20 Zeichen sind 800 Zeichen plus Kommas, also weit ueber
        // dem Budget. Frueher gingen alle 40 raus und der Upload kippte.
        let viele: Vec<String> = (0..40).map(|i| format!("deadlockclip{i:08}")).collect();
        let gewaehlt = tags_im_zeichenbudget(&viele, TAGS_MAX_CHARS);
        assert!(gewaehlt.len() < viele.len());
        assert!(zeichenlast(&gewaehlt) <= TAGS_MAX_CHARS);
        // Ein Tag mehr wuerde das Budget reissen.
        let naechster = &viele[gewaehlt.len()];
        let mut zu_viel = gewaehlt.clone();
        zu_viel.push(naechster.clone());
        assert!(zeichenlast(&zu_viel) > TAGS_MAX_CHARS);
    }

    #[test]
    fn tag_budget_rechnet_kommas_und_anfuehrungszeichen_mit() {
        // "ab" (2) + Komma (1) + "c d" (3 + 2 Anfuehrungszeichen) = 8.
        let tags = vec!["ab".to_string(), "c d".to_string()];
        assert_eq!(tags_im_zeichenbudget(&tags, 8), tags);
        assert_eq!(tags_im_zeichenbudget(&tags, 7), vec!["ab".to_string()]);
        // Leere Eintraege fallen raus, sie wuerden nur ein Komma kosten.
        let mit_leer = vec!["ab".to_string(), "  ".to_string(), "cd".to_string()];
        assert_eq!(
            tags_im_zeichenbudget(&mit_leer, 500),
            vec!["ab".to_string(), "cd".to_string()]
        );
        // Passt nicht mal der erste Tag, bleibt die Liste leer.
        assert!(tags_im_zeichenbudget(&tags, 1).is_empty());
    }

    /// Zeichenlast wie YouTube sie zaehlt (Kommas und implizite
    /// Anfuehrungszeichen inklusive).
    fn zeichenlast(tags: &[String]) -> usize {
        tags.iter()
            .map(|t| t.chars().count() + if t.contains(' ') { 2 } else { 0 })
            .sum::<usize>()
            + tags.len().saturating_sub(1)
    }

    #[test]
    fn beschreibung_behaelt_shorts_anhang() {
        let lang = "ä".repeat(6000); // 12000 Bytes, doppelt so viel wie erlaubt
        let text = baue_beschreibung(&lang, &["deadlock".into()]);
        assert!(
            text.ends_with(SHORTS_SUFFIX),
            "der Shorts-Anhang muss ueberleben"
        );
        assert!(
            text.len() <= DESCRIPTION_MAX_BYTES,
            "Bytes, nicht Zeichen: {}",
            text.len()
        );
        // Nicht mitten in ein Zeichen geschnitten.
        assert!(std::str::from_utf8(text.as_bytes()).is_ok());
    }

    #[test]
    fn beschreibung_ohne_spitze_klammern() {
        let text = baue_beschreibung("Gruss <3 an <alle>", &[]);
        assert!(!text.contains('<') && !text.contains('>'));
        assert!(text.contains("Gruss 3 an alle"));
    }

    #[test]
    fn titel_wird_bereinigt_und_leer_abgelehnt() {
        assert_eq!(
            bereinige_titel("Clip <3 des Tages").unwrap(),
            "Clip 3 des Tages"
        );
        assert_eq!(
            bereinige_titel(&"a".repeat(200)).unwrap().chars().count(),
            TITLE_MAX
        );
        assert!(matches!(
            bereinige_titel("   ").unwrap_err(),
            UploadError::Validation(_)
        ));
        assert!(matches!(
            bereinige_titel("<>").unwrap_err(),
            UploadError::Validation(_)
        ));
    }

    #[tokio::test]
    async fn leerer_titel_erreicht_die_api_nicht() {
        // Kein Mock noetig: der Fehler faellt vor dem ersten HTTP-Aufruf.
        let video = temp_video().await;
        let up = YouTubeUploader::new("tok").with_bases("http://127.0.0.1:1", "http://127.0.0.1:1");
        assert!(matches!(
            up.upload_video(&video, "<>", "d", &[]).await,
            Err(UploadError::Validation(_))
        ));
    }

    #[test]
    fn snippet_laesst_leere_tags_weg() {
        let ohne = baue_snippet("Titel", "Desc", &[]).unwrap();
        assert!(ohne["snippet"].get("tags").is_none());
        let mit = baue_snippet("Titel", "Desc", &["deadlock".into()]).unwrap();
        assert_eq!(mit["snippet"]["tags"], json!(["deadlock"]));
    }

    #[test]
    fn byteweise_kuerzung_schneidet_an_zeichengrenze() {
        assert_eq!(truncate_bytes("äbc", 1), "");
        assert_eq!(truncate_bytes("äbc", 2), "ä");
        assert_eq!(truncate_bytes("äbc", 3), "äb");
        assert_eq!(truncate_bytes("abc", 99), "abc");
    }
}
