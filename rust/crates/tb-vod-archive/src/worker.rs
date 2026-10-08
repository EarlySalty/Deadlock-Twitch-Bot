//! Der Worker: zweimal taeglich VODs entdecken, laden und hochladen.
//!
//! Reihenfolge ist Absicht. Zuerst wird geladen, dann hochgeladen, und der
//! Download laeuft auch ohne YouTube-Verbindung. Das lokale Archiv ist der
//! eigentliche Verlustschutz; ein fehlender Login verschiebt nur den Upload,
//! er darf nie den Download verhindern.
//!
//! Der Worker kennt keinen festen Kanal, sondern arbeitet alle Streamer ab,
//! die das Archiv im Dashboard eingeschaltet haben.
//!
//! Grenzen je Lauf gelten ueber alle Streamer zusammen, nicht je Streamer
//! erneut: Uploads sind knapp, weil ein einzelner 1600 der 10000 Einheiten
//! Tageskontingent kostet, und dieses Kontingent haengt am Google-Projekt, das
//! sich alle teilen. Downloads kosten kein Kontingent, aber Zeit und Platte,
//! also ebenfalls eine gemeinsame Ressource. Damit trotzdem kein Kanal das
//! ganze Kontingent frisst, werden die Warteschlangen reihum verschraenkt und
//! der Startplatz wandert von Lauf zu Lauf weiter.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use async_trait::async_trait;
use serde_json::Value;
use sqlx::PgPool;
use tb_social_media::credentials::CredentialManager;
use tb_social_media::upload_worker::youtube_uploader;
use tb_social_media::uploaders::youtube::{
    ChunkOutcome, ResumeStand, VideoZustand, YouTubeUploader,
};
use tb_social_media::uploaders::UploadError;
use tb_social_media::vod_archive::{aktive_vod_archive_streamer, VodArchiveSettings};

use crate::config::{wurzel_oder_elternteil, VodArchiveConfig};
use crate::error::VodArchiveError;
use crate::metadata::baue_metadaten;
use crate::store;
use crate::twitch::{self, CommandRunner};

/// Erster Lauf erst nach dieser Frist, damit der Bot-Start nicht sofort einen
/// mehrstuendigen Download anwirft.
const INITIAL_DELAY_SECS: u64 = 300;

/// Auszeit fuer ein von YouTube verworfenes Teil, bevor es erneut
/// hochgeladen wird. Sonst zieht jeder Lauf dieselbe Ablehnung mit einem
/// Volllast-Upload neu durch.
const ABGELEHNT_PAUSE_STUNDEN: i64 = 12;

/// Der Upload-Weg eines einzelnen Teils. Als Trait wie [`CommandRunner`],
/// damit der Ablauf des Worker samt seiner Deckel ohne echtes YouTube
/// pruefbar bleibt. Im Betrieb steckt dahinter der [`YouTubeUploader`] des
/// jeweiligen Streamers.
#[async_trait]
pub trait TeilHochlader: Send + Sync {
    async fn resumable_offset(
        &self,
        sitzung: &str,
        groesse: u64,
    ) -> Result<ResumeStand, UploadError>;
    async fn start_resumable_upload(
        &self,
        metadaten: &Value,
        groesse: u64,
    ) -> Result<String, UploadError>;
    async fn upload_chunk(
        &self,
        sitzung: &str,
        pfad: &Path,
        offset: u64,
    ) -> Result<ChunkOutcome, UploadError>;

    /// Verarbeitungsstand eines bereits hochgeladenen Videos. `None`, wenn
    /// YouTube die Video-ID nicht mehr kennt.
    async fn video_status(&self, video_id: &str) -> Result<Option<VideoZustand>, UploadError>;
    async fn add_to_playlist(
        &self,
        _playlist_id: &str,
        _video_id: &str,
    ) -> Result<(), UploadError> {
        Ok(())
    }
}

#[async_trait]
impl TeilHochlader for YouTubeUploader {
    async fn resumable_offset(
        &self,
        sitzung: &str,
        groesse: u64,
    ) -> Result<ResumeStand, UploadError> {
        YouTubeUploader::resumable_offset(self, sitzung, groesse).await
    }

    async fn start_resumable_upload(
        &self,
        metadaten: &Value,
        groesse: u64,
    ) -> Result<String, UploadError> {
        YouTubeUploader::start_resumable_upload(self, metadaten, groesse).await
    }

    async fn upload_chunk(
        &self,
        sitzung: &str,
        pfad: &Path,
        offset: u64,
    ) -> Result<ChunkOutcome, UploadError> {
        YouTubeUploader::upload_chunk(self, sitzung, pfad, offset).await
    }

    async fn video_status(&self, video_id: &str) -> Result<Option<VideoZustand>, UploadError> {
        YouTubeUploader::video_status(self, video_id).await
    }

    async fn add_to_playlist(&self, playlist_id: &str, video_id: &str) -> Result<(), UploadError> {
        YouTubeUploader::add_to_playlist(self, playlist_id, video_id).await
    }
}

/// Woher der YouTube-Zugang eines Kanals kommt. `None` heisst: keine
/// Verbindung, es wird nur lokal archiviert.
#[async_trait]
pub trait HochladerQuelle: Send + Sync {
    async fn fuer(&self, twitch_user_id: &str) -> Option<Arc<dyn TeilHochlader>>;
}

/// Betriebsfassung: der Zugang kommt aus den Credentials **dieses** Streamers.
///
/// Der globale Rueckfall von [`CredentialManager::get_credentials`] wird
/// bewusst verworfen: er wuerde das VOD eines Partners auf den YouTube-Kanal
/// des Betreibers schieben.
pub struct StreamerZugang {
    credentials: CredentialManager,
}

#[async_trait]
impl HochladerQuelle for StreamerZugang {
    async fn fuer(&self, twitch_user_id: &str) -> Option<Arc<dyn TeilHochlader>> {
        let status = self
            .credentials
            .get_all_platforms_status(Some(twitch_user_id))
            .await
            .ok()?;
        if !status.iter().any(|status| {
            status.platform == "youtube"
                && status.connected
                && !status.expired
                && !status.needs_reauth
                && !status.uses_global_fallback
        }) {
            return None;
        }
        let creds = self
            .credentials
            .get_channel_credentials_for_id("youtube", twitch_user_id)
            .await?;
        if !creds.scopes.as_deref().is_some_and(|scopes| {
            scopes.split_whitespace().any(|scope| {
                scope == "https://www.googleapis.com/auth/youtube.upload"
                    || scope == "https://www.googleapis.com/auth/youtube"
                    || scope == "https://www.googleapis.com/auth/youtube.force-ssl"
            })
        }) {
            return None;
        }
        Some(Arc::new(youtube_uploader(&creds)))
    }
}

pub struct VodArchiveWorker {
    session_cipher: Arc<tb_crypto::FieldCipher>,
    pool: PgPool,
    config: VodArchiveConfig,
    zugang: Arc<dyn HochladerQuelle>,
    check_credentials: Option<CredentialManager>,
    runner: Arc<dyn CommandRunner>,
    twitch_client: Option<tb_transport_twitch::HelixClient>,
    /// Zaehlt die Laeufe, damit der Startplatz der Warteschlange wandert.
    laeufe: AtomicUsize,
}

/// Was ein Lauf bewegt hat. Nur fuer Log und Tests.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct LaufBilanz {
    /// Geladene VODs.
    pub geladen: usize,
    /// Hochgeladene **Teile**, nicht VODs. Das Kontingent kostet je Teil: ein
    /// geschnittenes VOD kostet so viel wie es Teile hat.
    pub hochgeladen: usize,
    pub uebersprungen: usize,
}

/// Was mit dem naechsten VOD geschieht. Ausgelagert, damit die beiden Deckel
/// ohne Download und ohne YouTube pruefbar sind.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Aktion {
    Bearbeite,
    /// Passt nicht mehr in diesen Lauf, das naechste VOD vielleicht schon.
    Ueberspringe,
    /// Beide Deckel sind voll, weitersuchen bringt nichts mehr.
    Beende,
}

/// Entscheidet anhand beider Deckel, was mit dem naechsten VOD geschieht.
///
/// Ein VOD, das noch geladen werden muss, darf auch dann laufen, wenn das
/// Upload-Budget schon leer ist: das lokale Archiv ist der Verlustschutz, und
/// hochgeladen wird dann eben beim naechsten Lauf. Der Upload-Deckel greift in
/// diesem Fall weiter unten, je Teil.
pub fn naechste_aktion(
    config: &VodArchiveConfig,
    bilanz: &LaufBilanz,
    braucht_download: bool,
) -> Aktion {
    let downloads_frei = bilanz.geladen < config.max_downloads_per_run;
    let uploads_frei = bilanz.hochgeladen < config.max_uploads_per_run;
    if !downloads_frei && !uploads_frei {
        return Aktion::Beende;
    }
    if braucht_download {
        // Ohne Download-Budget ist an diesem VOD nichts zu holen: hochladen
        // laesst sich nur, was lokal liegt.
        if downloads_frei {
            Aktion::Bearbeite
        } else {
            Aktion::Ueberspringe
        }
    } else if uploads_frei {
        Aktion::Bearbeite
    } else {
        Aktion::Ueberspringe
    }
}

impl VodArchiveWorker {
    pub fn new(
        pool: PgPool,
        config: VodArchiveConfig,
        credentials: CredentialManager,
        session_cipher: Arc<tb_crypto::FieldCipher>,
    ) -> Self {
        let checks = CredentialManager::new(pool.clone(), session_cipher.clone());
        let mut worker = Self::mit_zugang(
            pool,
            config,
            Arc::new(StreamerZugang { credentials }),
            session_cipher,
        );
        worker.check_credentials = Some(checks);
        worker
    }

    /// Wie [`Self::new`], aber mit fertiger Upload-Quelle statt der
    /// Credential-Tabelle (Tests).
    pub fn mit_zugang(
        pool: PgPool,
        config: VodArchiveConfig,
        zugang: Arc<dyn HochladerQuelle>,
        session_cipher: Arc<tb_crypto::FieldCipher>,
    ) -> Self {
        Self {
            session_cipher,
            pool,
            config,
            zugang,
            check_credentials: None,
            runner: Arc::new(twitch::TokioCommandRunner),
            twitch_client: None,
            laeufe: AtomicUsize::new(0),
        }
    }

    /// Tauscht den Prozess-Starter aus (Tests).
    pub fn with_runner(mut self, runner: Arc<dyn CommandRunner>) -> Self {
        self.runner = runner;
        self
    }

    pub fn with_twitch_client(mut self, client: Option<tb_transport_twitch::HelixClient>) -> Self {
        self.twitch_client = client;
        self
    }

    async fn keine_vollstaendigen_teile(&self, vod_id: i64) -> Result<bool, VodArchiveError> {
        let teile = store::teile(&self.pool, vod_id, &self.session_cipher).await?;
        for teil in teile {
            match std::fs::metadata(&teil.file_path) {
                Ok(meta) if !meta.is_file() || meta.len() > 0 => return Ok(false),
                Ok(_) => {}
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => return Err(e.into()),
            }
        }
        Ok(true)
    }

    pub async fn run(&self) {
        let checking = async {
            loop {
                if let Some(credentials) = &self.check_credentials {
                    if let Err(error) = crate::youtube_check::run_once(
                        &self.pool,
                        credentials,
                        &self.config.youtube,
                    )
                    .await
                    {
                        tracing::error!(%error, "YouTube-Abgleich fehlgeschlagen");
                    }
                }
                tokio::time::sleep(std::time::Duration::from_secs(
                    self.config.youtube.youtube_poll_seconds,
                ))
                .await;
            }
        };
        let archiving = async {
            tokio::time::sleep(std::time::Duration::from_secs(INITIAL_DELAY_SECS)).await;
            loop {
                self.run_once().await;
                tokio::time::sleep(self.config.interval).await;
            }
        };
        tokio::join!(checking, archiving);
    }

    /// Ein vollstaendiger Lauf. Faengt alle Fehler ab, weil der Worker sonst
    /// nach dem ersten kaputten VOD fuer immer schweigt.
    pub async fn run_once(&self) {
        if let Err(error) = self.raeume_auf().await {
            tracing::error!(%error, "Temporäre VOD-Dateien konnten nicht entfernt werden");
        }
        let streamer = match aktive_vod_archive_streamer(&self.pool).await {
            Ok(liste) => liste,
            Err(fehler) => {
                tracing::error!(%fehler, "VOD-Archiv: Einstellungen nicht lesbar");
                return;
            }
        };
        if streamer.is_empty() {
            tracing::debug!("VOD-Archiv: kein Kanal eingeschaltet");
            return;
        }
        match self.lauf(&streamer).await {
            Ok(bilanz) => tracing::info!(
                kanaele = streamer.len(),
                geladen = bilanz.geladen,
                hochgeladen = bilanz.hochgeladen,
                uebersprungen = bilanz.uebersprungen,
                "VOD-Archiv-Lauf beendet"
            ),
            Err(fehler) => tracing::error!(%fehler, "VOD-Archiv-Lauf abgebrochen"),
        }
    }

    async fn lauf(&self, streamer: &[VodArchiveSettings]) -> Result<LaufBilanz, VodArchiveError> {
        let mut bilanz = LaufBilanz::default();

        if !self.platz_reicht() {
            tracing::warn!(
                mindestens_gb = self.config.min_free_gb,
                "Zu wenig Plattenplatz, VOD-Archiv setzt aus"
            );
            return Ok(bilanz);
        }

        // Ein Kanal, dessen Liste gerade nicht abrufbar ist, darf die anderen
        // nicht mitreissen.
        for einstellung in streamer {
            if let Err(fehler) = self.entdecke(einstellung).await {
                tracing::error!(
                    kanal = %einstellung.streamer_login,
                    %fehler,
                    "VODs nicht abrufbar, Kanal wird uebersprungen"
                );
            }
        }

        // Ohne YouTube-Zugang wird nur geladen. Das ist der wichtigere Teil.
        let mut uploader: HashMap<String, Option<Arc<dyn TeilHochlader>>> = HashMap::new();
        for einstellung in streamer {
            let Some(twitch_user_id) = einstellung.twitch_user_id.as_deref() else {
                continue;
            };
            let zugang = self.zugang.fuer(twitch_user_id).await;
            if zugang.is_none() {
                tracing::info!(
                    kanal = %einstellung.streamer_login,
                    "Kein eigener YouTube-Zugang hinterlegt, es wird nur lokal archiviert. \
                     Der Upload startet, sobald die Verbindung im Dashboard steht."
                );
            }
            uploader.insert(twitch_user_id.to_owned(), zugang);
        }

        // Reserve auf beide Grenzen, damit nach einem Download im selben Lauf
        // noch Uploads gefunden werden.
        let reserve =
            (self.config.max_downloads_per_run + self.config.max_uploads_per_run) as i64 + 10;
        let mut warteschlangen = Vec::with_capacity(streamer.len());
        for einstellung in streamer {
            let Some(twitch_user_id) = einstellung.twitch_user_id.as_deref() else {
                continue;
            };
            let offen = store::offene_vods(&self.pool, twitch_user_id, reserve).await?;
            if !offen.is_empty() {
                warteschlangen.push((einstellung, offen));
            }
        }

        let versatz = self.laeufe.fetch_add(1, Ordering::Relaxed);
        for (einstellung, mut vod) in verschraenke(warteschlangen, versatz) {
            let mut guard = self.pool.begin().await?;
            let Some(lock_id) = i32::try_from(vod.id).ok() else {
                continue;
            };
            let locked: bool =
                sqlx::query_scalar("SELECT pg_try_advisory_xact_lock(186976768, $1)")
                    .bind(lock_id)
                    .fetch_one(&mut *guard)
                    .await?;
            if !locked {
                continue;
            }
            let state: (String, Option<String>, bool) = sqlx::query_as("SELECT status, local_path, drive_requested FROM twitch_vod_archive_vods WHERE id=$1")
                .bind(vod.id).fetch_one(&self.pool).await?;
            vod.status = state.0;
            vod.local_path = state.1;
            if matches!(
                vod.status.as_str(),
                "uploaded" | "archived" | "unavailable" | "drive_uploaded"
            ) {
                continue;
            }
            let braucht_download = vod.braucht_download();
            match naechste_aktion(&self.config, &bilanz, braucht_download) {
                Aktion::Beende => {
                    tracing::info!("Grenzen erreicht, der Rest folgt beim naechsten Lauf");
                    break;
                }
                Aktion::Ueberspringe => {
                    bilanz.uebersprungen += 1;
                    continue;
                }
                Aktion::Bearbeite => {}
            }
            if !self.platz_reicht() {
                tracing::warn!("Plattenplatz aufgebraucht, der Rest folgt spaeter");
                break;
            }

            let Some(twitch_user_id) = einstellung.twitch_user_id.as_deref() else {
                continue;
            };
            let zugang = uploader.get(twitch_user_id).and_then(|u| u.clone());
            // Die Bilanz geht mit hinein, damit jeder einzelne Teil sofort
            // gegen den Deckel zaehlt. Bricht das VOD auf halbem Weg ab,
            // bleiben die bis dahin verbrauchten Einheiten trotzdem gezaehlt.
            match self
                .bearbeite(&vod, zugang.as_deref(), einstellung, &mut bilanz)
                .await
            {
                Ok(()) => {}
                Err(fehler) if fehler.ist_kontingent() => {
                    // Das Tageskontingent haengt am Google-Projekt, nicht am
                    // Nutzertoken: ist es leer, ist es fuer alle Kanaele leer.
                    tracing::warn!(
                        kanal = %einstellung.streamer_login,
                        %fehler,
                        "YouTube-Tageskontingent erschoepft, Abbruch bis morgen"
                    );
                    store::setze_fehler(
                        &self.pool,
                        vod.id,
                        store::STATUS_UPLOAD_FEHLER,
                        "quotaExceeded",
                    )
                    .await?;
                    break;
                }
                Err(fehler) => {
                    if let VodArchiveError::VideoNichtGefunden { twitch_id } = &fehler {
                        if braucht_download
                            && twitch_id == &vod.twitch_id
                            && vod.local_path.is_none()
                        {
                            let verzeichnis =
                                self.config.verzeichnis_fuer(&einstellung.streamer_login);
                            if let (Some(pruefer), Ok((false, restdaten))) = (
                                self.twitch_client.as_ref(),
                                lokale_vod_daten(&verzeichnis, &vod.twitch_id),
                            ) {
                                if matches!(self.keine_vollstaendigen_teile(vod.id).await, Ok(true))
                                    && matches!(
                                        pruefer.video_verfuegbar(&vod.twitch_id).await,
                                        Ok(false)
                                    )
                                {
                                    if store::setze_nicht_verfuegbar(&self.pool, vod.id, restdaten)
                                        .await?
                                    {
                                        tracing::warn!(vod = %vod.twitch_id, "VOD ist bei Twitch nicht mehr verfügbar und nicht vollständig gesichert");
                                    }
                                    continue;
                                }
                            }
                        }
                    }
                    let status = if braucht_download {
                        store::STATUS_DOWNLOAD_FEHLER
                    } else {
                        store::STATUS_UPLOAD_FEHLER
                    };
                    tracing::error!(
                        kanal = %einstellung.streamer_login,
                        vod = %vod.twitch_id,
                        %fehler,
                        "VOD fehlgeschlagen"
                    );
                    store::setze_bearbeitungsfehler(
                        &self.pool,
                        vod.id,
                        status,
                        &fehler.to_string(),
                    )
                    .await?;
                }
            }
        }

        self.raeume_auf().await?;
        Ok(bilanz)
    }

    /// Traegt neue VODs eines Kanals ein. Ein laufender Stream wird
    /// ausgelassen, sein VOD waere sonst nur zur Haelfte im Archiv.
    async fn entdecke(&self, einstellung: &VodArchiveSettings) -> Result<(), VodArchiveError> {
        let kanal = einstellung.streamer_login.as_str();
        let twitch_user_id = einstellung
            .twitch_user_id
            .as_deref()
            .filter(|id| !id.is_empty() && id.bytes().all(|byte| byte.is_ascii_digit()))
            .ok_or_else(|| sqlx::Error::Protocol("twitch_user_id fehlt".into()))?;
        let client =
            self.twitch_client
                .as_ref()
                .ok_or(tb_transport_twitch::HelixError::InvalidResponse(
                    "Twitch-Client fehlt",
                ))?;
        let mut vods = crate::discovery::liste_vods(client, twitch_user_id).await?;
        if vods.is_empty() {
            tracing::info!(kanal = %kanal, "Keine VODs gefunden");
            return Ok(());
        }
        let streams = client
            .get_streams_by_user_ids(&[twitch_user_id.to_string()], None)
            .await?;
        if streams
            .iter()
            .any(|stream| stream.user_id != twitch_user_id)
        {
            return Err(tb_transport_twitch::HelixError::InvalidResponse(
                "Live-Antwort enthält eine fremde Twitch-ID",
            )
            .into());
        }
        if !streams.is_empty() {
            tracing::info!(kanal = %kanal, "Kanal ist live, das neueste VOD wartet auf das Streamende");
            vods.remove(0);
        }
        for vod in &vods {
            if store::merke_vod(
                &self.pool,
                &vod.twitch_id,
                kanal,
                twitch_user_id,
                &vod.title,
                vod.duration_sec,
            )
            .await?
            {
                tracing::info!(kanal = %kanal, vod = %vod.twitch_id, titel = %vod.title, "Neues VOD entdeckt");
            }
        }
        Ok(())
    }

    /// Laedt das VOD (falls noetig) und schiebt seine Teile hoch. Alles, was
    /// wirklich passiert ist, landet sofort in `bilanz`: der Upload-Deckel
    /// zaehlt Teile, und ein Abbruch mittendrin darf die bereits verbrauchten
    /// Kontingent-Einheiten nicht vergessen.
    async fn bearbeite(
        &self,
        vod: &store::Vod,
        uploader: Option<&dyn TeilHochlader>,
        einstellung: &VodArchiveSettings,
        bilanz: &mut LaufBilanz,
    ) -> Result<(), VodArchiveError> {
        let kanal = einstellung.streamer_login.as_str();
        if crate::youtube_check::complete_recovery(
            &self.pool,
            einstellung.twitch_user_id.as_deref().unwrap_or_default(),
            vod.id,
            self.config.youtube.youtube_check_hours,
        )
        .await?
        {
            tracing::info!(vod = %vod.twitch_id, "Vollständig bestätigte Wiederherstellung ist abgeschlossen");
            return Ok(());
        }
        let mut aufgenommen_am = vod.recorded_at;

        if vod.braucht_download() {
            tracing::info!(kanal = %kanal, vod = %vod.twitch_id, titel = %vod.title, "Lade VOD");
            store::setze_status(&self.pool, vod.id, store::STATUS_LAEDT).await?;
            let verzeichnis = self.config.verzeichnis_fuer(kanal);
            // local_path wird erst nach erfolgreichem Download geschrieben.
            // Ein späterer Uploadfehler darf diese vollständige Kopie erneut nutzen.
            let vorhanden = match vod.local_path.as_deref() {
                Some(pfad) => match std::fs::metadata(pfad) {
                    Ok(meta) if meta.is_file() && meta.len() > 0 => Some(twitch::Download {
                        pfad: pfad.into(),
                        aufgenommen_am: vod.recorded_at,
                    }),
                    Ok(_) => None,
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
                    Err(e) => return Err(e.into()),
                },
                None => None,
            };
            let download = if let Some(download) = vorhanden {
                download
            } else {
                twitch::lade_vod(
                    self.runner.as_ref(),
                    &self.config,
                    &verzeichnis,
                    &vod.twitch_id,
                )
                .await?
            };
            let laenge =
                twitch::miss_laenge(self.runner.as_ref(), &self.config, &download.pfad).await;
            store::setze_geladen(
                &self.pool,
                vod.id,
                &download.pfad.display().to_string(),
                download.aufgenommen_am,
                laenge,
            )
            .await?;
            aufgenommen_am = download.aufgenommen_am.or(aufgenommen_am);

            let teile = twitch::schneide_bei_bedarf(
                self.runner.as_ref(),
                &self.config,
                &download.pfad,
                laenge,
            )
            .await?;
            let dateien: Vec<String> = teile.iter().map(|p| p.display().to_string()).collect();
            store::setze_teile(&self.pool, vod.id, kanal, &dateien).await?;
            bilanz.geladen += 1;
            tracing::info!(kanal = %kanal, vod = %vod.twitch_id, teile = dateien.len(), "VOD liegt lokal");
        }

        let drive: bool =
            sqlx::query_scalar("SELECT drive_requested FROM twitch_vod_archive_vods WHERE id=$1")
                .bind(vod.id)
                .fetch_one(&self.pool)
                .await?;
        if drive {
            self.lade_drive_hoch(vod, einstellung, aufgenommen_am)
                .await?;
            return Ok(());
        }

        let Some(uploader) = uploader else {
            return Ok(());
        };

        let teile = store::teile(&self.pool, vod.id, &self.session_cipher).await?;
        if teile.is_empty() {
            // Ein frueherer Lauf ist zwischen `setze_geladen` und `setze_teile`
            // gestorben. Hier "hochgeladen" einzutragen waere eine Luege: bei
            // YouTube liegt nichts, und das Aufraeumen wuerde danach die
            // einzige lokale Kopie loeschen, waehrend das Original auf Twitch
            // laengst weg ist. Also zurueck in die Download-Warteschlange.
            tracing::warn!(
                kanal = %kanal,
                vod = %vod.twitch_id,
                "Keine Teile eingetragen, das VOD wird erneut geladen"
            );
            store::setze_fehler(
                &self.pool,
                vod.id,
                store::STATUS_DOWNLOAD_FEHLER,
                "keine Teile eingetragen, der Download wird wiederholt",
            )
            .await?;
            return Ok(());
        }

        let anzahl = teile.len();
        let mut offen = 0usize;
        let mut bestaetigte_altteile = false;
        for teil in &teile {
            if teil.status == store::TEIL_FERTIG {
                continue;
            }
            if crate::youtube_check::part_processed(
                &self.pool,
                einstellung.twitch_user_id.as_deref().unwrap_or_default(),
                vod.id,
                teil.id,
            )
            .await?
            {
                bestaetigte_altteile = true;
                tracing::debug!(vod = %vod.twitch_id, teil = teil.part_index, "Bestätigter YouTube-Teil bleibt unverändert");
                continue;
            }
            if teil.abgelehnt_kuehlt_noch(ABGELEHNT_PAUSE_STUNDEN) {
                // Frisch von YouTube verworfen: die Auszeit verhindert, dass
                // jeder Lauf dieselbe Ablehnung mit einem Volllast-Upload neu
                // durchzieht. Solange das Teil aussteht, bleibt das VOD offen.
                tracing::debug!(
                    vod = %vod.twitch_id,
                    teil = teil.part_index,
                    "Verworfenes Teil pausiert, der Upload folgt nach der Auszeit"
                );
                offen += 1;
                continue;
            }
            if bilanz.hochgeladen >= self.config.max_uploads_per_run {
                // Der Deckel zaehlt Uploads, nicht VODs: ein 20-Stunden-VOD
                // kostet zwei Uploads, also 3200 Einheiten.
                offen += 1;
                continue;
            }
            store::setze_status(&self.pool, vod.id, "uploading").await?;
            if let Err(error) = self
                .lade_teil_hoch(vod, teil, anzahl, aufgenommen_am, uploader, einstellung)
                .await
            {
                store::setze_teil_fehler(&self.pool, teil.id, &error.to_string()).await?;
                return Err(error);
            }
            bilanz.hochgeladen += 1;
        }

        if offen > 0 {
            sqlx::query("UPDATE twitch_vod_archive_vods SET status=CASE WHEN EXISTS (SELECT 1 FROM twitch_vod_archive_parts WHERE vod_id=$1 AND status IN ('failed','rejected')) THEN 'upload_failed' ELSE 'downloaded' END, updated_at=NOW() WHERE id=$1 AND status='uploading'")
                .bind(vod.id).execute(&self.pool).await?;
            tracing::info!(
                kanal = %kanal,
                vod = %vod.twitch_id,
                offen,
                "Upload-Deckel erreicht, die restlichen Teile folgen beim naechsten Lauf"
            );
            return Ok(());
        }

        if bestaetigte_altteile {
            sqlx::query("UPDATE twitch_vod_archive_vods SET status='downloaded',updated_at=NOW() WHERE id=$1 AND status='uploading'")
                .bind(vod.id).execute(&self.pool).await?;
            tracing::info!(vod = %vod.twitch_id, "Bestätigte frühere Teile bleiben erhalten; der YouTube-Abgleich prüft den Gesamtstand");
            return Ok(());
        }

        if let Some(playlist) = &self.config.playlist_id {
            for part in store::teile(&self.pool, vod.id, &self.session_cipher).await? {
                if let Some(video) = part.youtube_video_id {
                    uploader.add_to_playlist(playlist, &video).await?;
                }
            }
        }
        store::setze_hochgeladen(&self.pool, vod.id).await?;
        self.raeume_auf().await?;
        Ok(())
    }

    async fn lade_teil_hoch(
        &self,
        vod: &store::Vod,
        teil: &store::Teil,
        anzahl: usize,
        aufgenommen_am: Option<chrono::NaiveDate>,
        uploader: &dyn TeilHochlader,
        einstellung: &VodArchiveSettings,
    ) -> Result<(), VodArchiveError> {
        let pfad = PathBuf::from(&teil.file_path);
        if !pfad.is_file() {
            return Err(VodArchiveError::DateiFehlt(teil.file_path.clone()));
        }
        let groesse = tokio::fs::metadata(&pfad).await?.len();

        // Wiederaufnahme: gibt es eine Sitzung, entscheidet allein YouTube, wie
        // weit sie gekommen ist. Der eigene Stand aus der Datenbank ist nur
        // Anzeige; scheitert die Nachfrage, bricht der Teil ab und der
        // naechste Lauf fragt erneut. Blind auf dem gespeicherten Stand
        // weiterzuschreiben waere geraten, und bei abweichendem Stand liegt
        // drueben eine kaputte Datei.
        let (sitzung, mut offset) = match teil.upload_session_uri.as_deref() {
            Some(uri) => match uploader.resumable_offset(uri, groesse).await? {
                ResumeStand::Fertig(video_id) => {
                    // YouTube hat den Upload schon abgeschlossen und liefert die
                    // Video-ID gleich mit. Frueher fiel sie hier weg und ein
                    // fertiges VOD galt als Fehlschlag, der beim naechsten Lauf
                    // erneut hochgeladen wurde.
                    store::setze_teil_fertig(&self.pool, teil.id, &video_id).await?;
                    tracing::info!(
                        vod = %vod.twitch_id,
                        teil = teil.part_index,
                        "Bereits vollstaendig hochgeladen: https://youtu.be/{video_id}"
                    );
                    return Ok(());
                }
                ResumeStand::Offset(stand) => {
                    tracing::info!(vod = %vod.twitch_id, teil = teil.part_index, stand, "Setze Upload fort");
                    (uri.to_string(), stand)
                }
                ResumeStand::Verfallen => {
                    tracing::info!(vod = %vod.twitch_id, teil = teil.part_index, "Sitzung verfallen, beginne neu");
                    store::loesche_teil_sitzung(&self.pool, teil.id).await?;
                    (
                        self.beginne_sitzung(
                            vod,
                            teil,
                            anzahl,
                            aufgenommen_am,
                            uploader,
                            einstellung,
                            groesse,
                        )
                        .await?,
                        0,
                    )
                }
            },
            None => (
                self.beginne_sitzung(
                    vod,
                    teil,
                    anzahl,
                    aufgenommen_am,
                    uploader,
                    einstellung,
                    groesse,
                )
                .await?,
                0,
            ),
        };

        tracing::info!(
            kanal = %einstellung.streamer_login,
            vod = %vod.twitch_id,
            teil = teil.part_index + 1,
            von = anzahl,
            gb = format!("{:.1}", groesse as f64 / 1024.0_f64.powi(3)),
            "Lade hoch"
        );

        // Stueck fuer Stueck, nach jedem Stueck den Stand festhalten.
        loop {
            if offset >= groesse {
                // YouTube hat alles, meldet den Abschluss aber erst beim
                // naechsten Stueck. Ohne diese Bremse liefe die Schleife leer.
                return Err(VodArchiveError::Werkzeug {
                    schritt: "Upload".to_string(),
                    meldung: "vollstaendig uebertragen, aber ohne Video-ID".to_string(),
                });
            }
            match uploader.upload_chunk(&sitzung, &pfad, offset).await {
                Ok(ChunkOutcome::Fertig(video_id)) => {
                    store::setze_teil_fertig(&self.pool, teil.id, &video_id).await?;
                    tracing::info!(vod = %vod.twitch_id, teil = teil.part_index, "Fertig: https://youtu.be/{video_id}");
                    return Ok(());
                }
                Ok(ChunkOutcome::Weiter(stand)) => {
                    offset = stand;
                    store::setze_teil_offset(&self.pool, teil.id, stand as i64).await?;
                    tracing::debug!(
                        vod = %vod.twitch_id,
                        prozent = format!("{:.1}", 100.0 * stand as f64 / groesse as f64),
                        "Upload-Fortschritt"
                    );
                }
                Err(fehler) => {
                    // Der Stand bleibt stehen, der naechste Lauf setzt dort an.
                    store::setze_teil_fehler(&self.pool, teil.id, &fehler.to_string()).await?;
                    return Err(fehler.into());
                }
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    async fn beginne_sitzung(
        &self,
        vod: &store::Vod,
        teil: &store::Teil,
        anzahl: usize,
        aufgenommen_am: Option<chrono::NaiveDate>,
        uploader: &dyn TeilHochlader,
        einstellung: &VodArchiveSettings,
        groesse: u64,
    ) -> Result<String, VodArchiveError> {
        let metadaten = baue_metadaten(
            &self.config,
            &einstellung.streamer_login,
            &vod.title,
            &vod.twitch_id,
            aufgenommen_am,
            teil.part_index as usize,
            anzahl,
            &einstellung.privacy,
        );
        let uri = uploader.start_resumable_upload(&metadaten, groesse).await?;
        store::setze_teil_sitzung(&self.pool, teil.id, &uri, 0, &self.session_cipher).await?;
        Ok(uri)
    }

    async fn lade_drive_hoch(
        &self,
        vod: &store::Vod,
        settings: &VodArchiveSettings,
        recorded_at: Option<chrono::NaiveDate>,
    ) -> Result<(), VodArchiveError> {
        let parts = store::teile(&self.pool, vod.id, &self.session_cipher).await?;
        if parts.is_empty() {
            return Err(VodArchiveError::DateiFehlt(vod.twitch_id.clone()));
        }
        let date = recorded_at.unwrap_or_else(|| chrono::Utc::now().date_naive());
        let remote = format!(
            "{}/{}/{}/{}",
            self.config.drive_remote_base.trim_end_matches('/'),
            date,
            settings.twitch_user_id.as_deref().unwrap_or("unknown"),
            vod.twitch_id
        );
        store::setze_status(&self.pool, vod.id, "uploading").await?;
        for part in &parts {
            let path = Path::new(&part.file_path);
            let file = path
                .file_name()
                .and_then(|name| name.to_str())
                .ok_or_else(|| VodArchiveError::DateiFehlt(part.file_path.clone()))?;
            let target = format!("{remote}/{file}");
            let result = self
                .runner
                .run(
                    &self.config.rclone,
                    &["copyto".into(), part.file_path.clone(), target.clone()],
                    self.config.download_timeout,
                )
                .await?;
            if !result.success {
                return Err(VodArchiveError::Werkzeug {
                    schritt: "Drive-Upload".into(),
                    meldung: "Die Datei konnte nicht auf Drive gespeichert werden.".into(),
                });
            }
            let check = self
                .runner
                .run(
                    &self.config.rclone,
                    &["lsjson".into(), target, "--stat".into()],
                    std::time::Duration::from_secs(60),
                )
                .await?;
            let size = tokio::fs::metadata(path).await?.len();
            if !check.success
                || serde_json::from_str::<Value>(&check.stdout)
                    .ok()
                    .and_then(|value| value["Size"].as_u64())
                    != Some(size)
            {
                return Err(VodArchiveError::Werkzeug {
                    schritt: "Drive-Prüfung".into(),
                    meldung: "Die Kopie auf Drive konnte nicht bestätigt werden.".into(),
                });
            }
        }
        let link = self
            .runner
            .run(
                &self.config.rclone,
                &["link".into(), remote],
                std::time::Duration::from_secs(60),
            )
            .await?;
        let url = link.stdout.trim();
        if !link.success
            || !url.starts_with("https://drive.google.com/")
            || url.chars().any(char::is_control)
        {
            return Err(VodArchiveError::Werkzeug {
                schritt: "Drive-Link".into(),
                meldung: "Der Link zur Drive-Kopie fehlt. Die Dateien bleiben vorerst erhalten."
                    .into(),
            });
        }
        sqlx::query("UPDATE twitch_vod_archive_vods SET status='drive_uploaded', drive_url=$2, uploaded_at=NOW(), last_error=NULL, updated_at=NOW() WHERE id=$1")
            .bind(vod.id).bind(url).execute(&self.pool).await?;
        self.raeume_auf().await?;
        Ok(())
    }

    async fn raeume_auf(&self) -> Result<(), VodArchiveError> {
        let rows: Vec<(i64, String, String, Option<String>, String)> = sqlx::query_as(
            "SELECT id, twitch_id, local_path, twitch_user_id, status FROM twitch_vod_archive_vods WHERE status IN ('uploaded','drive_uploaded') AND local_path IS NOT NULL"
        ).fetch_all(&self.pool).await?;
        for (id, twitch_id, local_path, user_id, status) in rows {
            let mut cleanup_guard = None;
            if status == "uploaded" {
                let Some(user_id) = user_id else {
                    continue;
                };
                cleanup_guard = crate::youtube_check::cleanup_proof(
                    &self.pool,
                    &user_id,
                    id,
                    self.config.youtube.youtube_check_hours,
                )
                .await?;
                if cleanup_guard.is_none() {
                    continue;
                }
            }
            let root = std::fs::canonicalize(&self.config.download_dir)?;
            let directory = Path::new(&local_path).parent().ok_or_else(|| {
                std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    "VOD-Dateipfad ohne Verzeichnis",
                )
            })?;
            let directory = std::fs::canonicalize(directory)?;
            if !directory.starts_with(root) {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    "VOD-Datei liegt außerhalb des temporären Archivverzeichnisses",
                )
                .into());
            }
            loesche_dateien(&directory, &twitch_id)?;
            let update = sqlx::query("UPDATE twitch_vod_archive_vods SET local_path=NULL, status=CASE WHEN status='uploaded' THEN 'archived' ELSE status END, updated_at=NOW() WHERE id=$1").bind(id);
            if let Some(mut guard) = cleanup_guard {
                update.execute(&mut *guard).await?;
                guard.commit().await?;
            } else {
                update.execute(&self.pool).await?;
            }
        }
        Ok(())
    }

    /// Reicht der Plattenplatz noch? Ein VOD kann zweistellige Gigabyte gross
    /// sein, eine volle Platte wuerde den ganzen Bot mitreissen. Gemessen wird
    /// die gemeinsame Wurzel: es ist fuer alle Streamer dieselbe Platte.
    fn platz_reicht(&self) -> bool {
        match freier_platz_gb(&self.config.download_dir) {
            Some(frei) => frei >= self.config.min_free_gb,
            // Laesst sich der Platz nicht messen, wird nicht blockiert: der
            // Download bricht sonst aus dem falschen Grund ab.
            None => true,
        }
    }
}

/// Vollständige Medien bleiben im bisherigen Pfad; Teilreste werden nur vermerkt.
fn lokale_vod_daten(verzeichnis: &Path, twitch_id: &str) -> std::io::Result<(bool, bool)> {
    let mut voll = false;
    let mut reste = false;
    let eintraege = match std::fs::read_dir(verzeichnis) {
        Ok(eintraege) => eintraege,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok((false, false)),
        Err(e) => return Err(e),
    };
    for eintrag in eintraege {
        let eintrag = eintrag?;
        let name = eintrag.file_name();
        let Some(name) = name.to_str() else {
            continue;
        };
        if !name.starts_with(&format!("{twitch_id}.")) {
            continue;
        }
        let meta = eintrag.metadata()?;
        if !meta.is_file() {
            continue;
        }
        let medien = name.ends_with(".mp4") || name.ends_with(".mkv") || name.ends_with(".ts");
        if medien && meta.len() > 0 {
            voll = true;
        }
        if name.ends_with(".part") || medien {
            reste = true;
        }
    }
    Ok((voll, reste))
}

/// Verschraenkt die Warteschlangen mehrerer Streamer reihum: erst das aelteste
/// VOD jedes Kanals, dann das zweitaelteste und so weiter. So verbraucht ein
/// Kanal mit dreissig offenen VODs nicht das ganze Tageskontingent, waehrend
/// ein anderer wartet. `versatz` verschiebt den Startplatz, damit ueber die
/// Laeufe hinweg nicht immer derselbe Kanal zuerst drankommt.
fn verschraenke<T>(
    mut warteschlangen: Vec<(T, Vec<store::Vod>)>,
    versatz: usize,
) -> Vec<(T, store::Vod)>
where
    T: Clone,
{
    if warteschlangen.is_empty() {
        return Vec::new();
    }
    let start = versatz % warteschlangen.len();
    warteschlangen.rotate_left(start);
    let tiefe = warteschlangen
        .iter()
        .map(|(_, vods)| vods.len())
        .max()
        .unwrap_or(0);
    let mut reihe = Vec::new();
    for index in 0..tiefe {
        for (schluessel, vods) in &warteschlangen {
            if let Some(vod) = vods.get(index) {
                reihe.push((schluessel.clone(), vod.clone()));
            }
        }
    }
    reihe
}

/// Freier Platz in Gigabyte. Nutzt `statvfs` ueber das `df`-Werkzeug, weil der
/// Workspace keine libc-Bindung mitbringt.
fn freier_platz_gb(pfad: &Path) -> Option<u64> {
    let ziel = wurzel_oder_elternteil(pfad);
    let ausgabe = std::process::Command::new("df")
        .args(["-BG", "--output=avail"])
        .arg(&ziel)
        .output()
        .ok()?;
    parse_df(&String::from_utf8_lossy(&ausgabe.stdout))
}

/// Liest die Gigabyte-Zahl aus der zweiten Zeile der df-Ausgabe.
pub fn parse_df(ausgabe: &str) -> Option<u64> {
    ausgabe
        .lines()
        .nth(1)?
        .trim()
        .trim_end_matches('G')
        .parse()
        .ok()
}

/// Entfernt alle Dateien eines VOD, also Quelle, Teile und info.json.
fn loesche_dateien(verzeichnis: &Path, twitch_id: &str) -> std::io::Result<()> {
    if twitch_id.is_empty() || !twitch_id.bytes().all(|byte| byte.is_ascii_alphanumeric()) {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "Ungültige VOD-ID",
        ));
    }
    let eintraege = match std::fs::read_dir(verzeichnis) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error),
    };
    let praefix = format!("{twitch_id}.");
    for eintrag in eintraege {
        let eintrag = eintrag?;
        if eintrag.file_name().to_string_lossy().starts_with(&praefix) {
            std::fs::remove_file(eintrag.path())?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn restdateien_bleiben_bei_der_verfuegbarkeitspruefung_erhalten() {
        let dir = temp_verzeichnis("verfuegbarkeit_reste");
        std::fs::create_dir_all(&dir).unwrap();
        let rest = dir.join("v123.mp4.part");
        std::fs::write(&rest, b"rest").unwrap();
        assert_eq!(
            super::lokale_vod_daten(&dir, "v123").unwrap(),
            (false, true)
        );
        let voll = dir.join("v123.mp4");
        std::fs::write(&voll, b"media").unwrap();
        assert_eq!(super::lokale_vod_daten(&dir, "v123").unwrap(), (true, true));
        assert!(rest.exists() && voll.exists());
        std::fs::remove_dir_all(dir).unwrap();
    }
    use super::*;
    fn test_cipher() -> tb_crypto::FieldCipher {
        tb_crypto::FieldCipher::from_hex_key(&"11".repeat(32), "v1").unwrap()
    }

    #[test]
    fn df_ausgabe_wird_gelesen() {
        assert_eq!(parse_df("Avail\n  123G\n"), Some(123));
        assert_eq!(parse_df("Avail\n0G\n"), Some(0));
        assert_eq!(parse_df("Avail\n"), None);
        assert_eq!(parse_df(""), None);
    }

    fn vod(twitch_id: &str) -> store::Vod {
        store::Vod {
            id: 0,
            twitch_id: twitch_id.to_string(),
            title: String::new(),
            duration_sec: 0,
            recorded_at: None,
            status: store::STATUS_NEU.to_string(),
            local_path: None,
        }
    }

    #[test]
    fn kein_kanal_frisst_das_ganze_kontingent() {
        let warteschlangen = vec![
            ("a", vec![vod("a1"), vod("a2"), vod("a3")]),
            ("b", vec![vod("b1")]),
            ("c", vec![vod("c1"), vod("c2")]),
        ];
        let reihe = verschraenke(warteschlangen, 0);
        let namen: Vec<&str> = reihe
            .iter()
            .map(|(k, v)| {
                assert!(v.twitch_id.starts_with(k));
                *k
            })
            .collect();
        // Runde eins nimmt von jedem Kanal eines, erst danach kommt die zweite
        // Runde. Wer zuerst leer ist, faellt einfach raus.
        assert_eq!(namen, vec!["a", "b", "c", "a", "c", "a"]);
    }

    #[test]
    fn der_startplatz_wandert_von_lauf_zu_lauf() {
        let bauen = || {
            vec![
                ("a", vec![vod("a1")]),
                ("b", vec![vod("b1")]),
                ("c", vec![vod("c1")]),
            ]
        };
        let erster: Vec<&str> = verschraenke(bauen(), 0)
            .into_iter()
            .map(|(k, _)| k)
            .collect();
        let zweiter: Vec<&str> = verschraenke(bauen(), 1)
            .into_iter()
            .map(|(k, _)| k)
            .collect();
        let vierter: Vec<&str> = verschraenke(bauen(), 3)
            .into_iter()
            .map(|(k, _)| k)
            .collect();
        assert_eq!(erster, vec!["a", "b", "c"]);
        assert_eq!(zweiter, vec!["b", "c", "a"]);
        // Nach einer vollen Runde ist wieder der erste dran.
        assert_eq!(vierter, erster);
    }

    #[test]
    fn leere_warteschlange_bleibt_leer() {
        let leer: Vec<(&str, Vec<store::Vod>)> = Vec::new();
        assert!(verschraenke(leer, 7).is_empty());
    }

    #[test]
    fn der_download_pfad_haengt_nicht_am_upload_budget() {
        let cfg = VodArchiveConfig::default();
        let voll = LaufBilanz {
            geladen: 0,
            hochgeladen: cfg.max_uploads_per_run,
            uebersprungen: 0,
        };
        // Ohne Upload-Budget wird trotzdem geladen: das lokale Archiv ist der
        // Verlustschutz. Ein reines Upload-VOD hat dagegen nichts zu tun.
        assert_eq!(naechste_aktion(&cfg, &voll, true), Aktion::Bearbeite);
        assert_eq!(naechste_aktion(&cfg, &voll, false), Aktion::Ueberspringe);

        let downloads_voll = LaufBilanz {
            geladen: cfg.max_downloads_per_run,
            hochgeladen: 0,
            uebersprungen: 0,
        };
        assert_eq!(
            naechste_aktion(&cfg, &downloads_voll, true),
            Aktion::Ueberspringe
        );
        assert_eq!(
            naechste_aktion(&cfg, &downloads_voll, false),
            Aktion::Bearbeite
        );

        let beides_voll = LaufBilanz {
            geladen: cfg.max_downloads_per_run,
            hochgeladen: cfg.max_uploads_per_run,
            uebersprungen: 0,
        };
        assert_eq!(naechste_aktion(&cfg, &beides_voll, true), Aktion::Beende);
    }

    // ----------------------------------------------------------------------
    // Ablauf-Tests gegen ein Wegwerf-Schema. Ohne TB_TEST_DATABASE_URL
    // ueberspringen sie still, wie im uebrigen Workspace.
    // ----------------------------------------------------------------------

    use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
    use std::str::FromStr;
    use std::sync::Mutex;
    use std::time::Duration;

    use crate::twitch::CommandOutput;

    async fn pool(schema: &str) -> Option<PgPool> {
        let dsn = std::env::var("TB_TEST_DATABASE_URL").ok()?;
        let admin = PgPoolOptions::new()
            .max_connections(1)
            .connect(&dsn)
            .await
            .ok()?;
        sqlx::query(sqlx::AssertSqlSafe(format!(
            "DROP SCHEMA IF EXISTS {schema} CASCADE"
        )))
        .execute(&admin)
        .await
        .unwrap();
        sqlx::query(sqlx::AssertSqlSafe(format!("CREATE SCHEMA {schema}")))
            .execute(&admin)
            .await
            .unwrap();
        admin.close().await;
        let opts = PgConnectOptions::from_str(&dsn)
            .unwrap()
            .options([("search_path", schema)]);
        let pool = PgPoolOptions::new()
            .max_connections(2)
            .connect_with(opts)
            .await
            .unwrap();
        for ddl in [
            "CREATE TABLE twitch_vod_archive_vods (id BIGSERIAL PRIMARY KEY, twitch_id TEXT NOT NULL UNIQUE, \
             streamer_login TEXT NOT NULL, twitch_user_id TEXT, title TEXT NOT NULL, duration_sec BIGINT NOT NULL DEFAULT 0, \
             recorded_at DATE, status TEXT NOT NULL DEFAULT 'new', local_path TEXT, last_error TEXT, \
             discovered_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP, downloaded_at TIMESTAMPTZ, \
             uploaded_at TIMESTAMPTZ, updated_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP, \
             hidden_at TIMESTAMPTZ, drive_requested BOOLEAN NOT NULL DEFAULT FALSE, drive_url TEXT, last_attempt_at TIMESTAMPTZ)",
            "CREATE TABLE twitch_vod_archive_parts (id BIGSERIAL PRIMARY KEY, vod_id BIGINT NOT NULL \
             REFERENCES twitch_vod_archive_vods (id) ON DELETE CASCADE, streamer_login TEXT, \
             part_index INTEGER NOT NULL, \
             file_path TEXT NOT NULL, size_bytes BIGINT NOT NULL DEFAULT 0, status TEXT NOT NULL DEFAULT 'pending', \
             upload_session_uri TEXT, upload_offset BIGINT NOT NULL DEFAULT 0, youtube_video_id TEXT, \
             last_error TEXT, updated_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP, \
             UNIQUE (vod_id, part_index))",
        ] {
            sqlx::query(sqlx::AssertSqlSafe(ddl))
                .execute(&pool)
                .await
                .unwrap();
        }
        crate::youtube_check::test_schema(&pool).await;
        static NEXT_ID: AtomicUsize = AtomicUsize::new(100_000);
        let first_id = NEXT_ID.fetch_add(1_000, Ordering::SeqCst) as i64;
        sqlx::query("SELECT setval('twitch_vod_archive_vods_id_seq', $1, false)")
            .bind(first_id)
            .execute(&pool)
            .await
            .unwrap();
        Some(pool)
    }

    #[derive(Default)]
    struct ZaehlenderHochlader {
        uploads: AtomicUsize,
    }

    #[async_trait]
    impl TeilHochlader for ZaehlenderHochlader {
        async fn resumable_offset(
            &self,
            _sitzung: &str,
            _groesse: u64,
        ) -> Result<ResumeStand, UploadError> {
            Ok(ResumeStand::Offset(0))
        }

        async fn start_resumable_upload(
            &self,
            _metadaten: &Value,
            _groesse: u64,
        ) -> Result<String, UploadError> {
            Ok("https://sitzung.test/1".to_string())
        }

        async fn upload_chunk(
            &self,
            _sitzung: &str,
            _pfad: &Path,
            _offset: u64,
        ) -> Result<ChunkOutcome, UploadError> {
            let nummer = self.uploads.fetch_add(1, Ordering::SeqCst);
            Ok(ChunkOutcome::Fertig(format!("yt-{nummer}")))
        }

        async fn video_status(&self, _video_id: &str) -> Result<Option<VideoZustand>, UploadError> {
            panic!("Der Archivlauf darf keine separate Video-Prüfung ausführen")
        }
    }

    struct FesteQuelle(Arc<ZaehlenderHochlader>);

    #[async_trait]
    impl HochladerQuelle for FesteQuelle {
        async fn fuer(&self, _twitch_user_id: &str) -> Option<Arc<dyn TeilHochlader>> {
            Some(self.0.clone())
        }
    }

    /// Ersatz fuer yt-dlp und ffprobe: keine neuen VODs in der Liste, ein
    /// Download legt eine echte Datei an, die Laenge bleibt unter der
    /// Schnittgrenze.
    struct WerkzeugAttrappe {
        aufrufe: Mutex<Vec<Vec<String>>>,
    }

    impl WerkzeugAttrappe {
        fn neu() -> Self {
            Self {
                aufrufe: Mutex::new(Vec::new()),
            }
        }

        fn antwort(stdout: &str) -> CommandOutput {
            CommandOutput {
                success: true,
                stdout: stdout.to_string(),
                stderr: String::new(),
            }
        }
    }

    #[async_trait]
    impl CommandRunner for WerkzeugAttrappe {
        async fn run(
            &self,
            _program: &Path,
            args: &[String],
            _timeout: Duration,
        ) -> Result<CommandOutput, VodArchiveError> {
            self.aufrufe.lock().unwrap().push(args.to_vec());
            if args.iter().any(|a| a == "-J") {
                return Ok(Self::antwort("{}"));
            }
            if let Some(stelle) = args.iter().position(|a| a == "-o") {
                let ziel = args[stelle + 1].replace("%(ext)s", "mp4");
                if let Some(eltern) = Path::new(&ziel).parent() {
                    std::fs::create_dir_all(eltern).unwrap();
                }
                std::fs::write(&ziel, b"videodaten").unwrap();
                return Ok(Self::antwort(""));
            }
            if args.iter().any(|a| a == "format=duration") {
                return Ok(Self::antwort("60"));
            }
            Ok(Self::antwort(""))
        }
    }

    fn einstellung(login: &str) -> VodArchiveSettings {
        VodArchiveSettings {
            streamer_login: login.to_string(),
            twitch_user_id: Some("42".to_string()),
            enabled: true,
            privacy: "unlisted".to_string(),
        }
    }

    fn temp_verzeichnis(name: &str) -> PathBuf {
        let mut pfad = std::env::temp_dir();
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        pfad.push(format!("tb_vod_{name}_{nanos}"));
        std::fs::create_dir_all(&pfad).unwrap();
        pfad
    }

    fn config(verzeichnis: &Path) -> VodArchiveConfig {
        VodArchiveConfig {
            download_dir: verzeichnis.to_path_buf(),
            max_uploads_per_run: 2,
            // Der Plattenplatz ist hier nicht das Thema.
            min_free_gb: 0,
            ..VodArchiveConfig::default()
        }
    }

    fn worker(
        pool: &PgPool,
        cfg: VodArchiveConfig,
        hochlader: &Arc<ZaehlenderHochlader>,
    ) -> VodArchiveWorker {
        VodArchiveWorker::mit_zugang(
            pool.clone(),
            cfg,
            Arc::new(FesteQuelle(hochlader.clone())),
            Arc::new(test_cipher()),
        )
        .with_runner(Arc::new(WerkzeugAttrappe::neu()))
    }

    fn recovery_observations(mut observations: Value) -> Value {
        for observation in observations.as_array_mut().unwrap() {
            observation["source_twitch_id"] = serde_json::json!("v1");
            observation["source_duration_sec"] = serde_json::json!(60);
            observation["observed_at"] = serde_json::json!("2026-10-01T13:00:00Z");
        }
        observations
    }

    #[tokio::test]
    async fn processed_recovery_parts_keep_history_through_preparation_and_upload_entry() {
        for (schema, original) in [
            ("t_vod_recovery_prepared", false),
            ("t_vod_recovery_original", true),
        ] {
            let pool = pool(schema).await.expect("synthetic PostgreSQL required");
            let directory = temp_verzeichnis(schema);
            let source = directory.join("v1.mp4");
            std::fs::write(&source, b"synthetic source").unwrap();
            let mut files = Vec::new();
            for index in 0..5 {
                let file = directory.join(format!("v1.part{index}.mp4"));
                std::fs::write(&file, b"synthetic part").unwrap();
                files.push(file.display().to_string());
            }
            store::merke_vod(&pool, "v1", "earlysalty", "42", "Synthetic recovery", 60)
                .await
                .unwrap();
            let id: i64 =
                sqlx::query_scalar("SELECT id FROM twitch_vod_archive_vods WHERE twitch_id='v1'")
                    .fetch_one(&pool)
                    .await
                    .unwrap();
            store::setze_geladen(&pool, id, source.to_str().unwrap(), None, 60)
                .await
                .unwrap();
            store::setze_teile(&pool, id, "earlysalty", &files)
                .await
                .unwrap();
            sqlx::query("UPDATE twitch_vod_archive_vods SET uploaded_at=CASE WHEN $2 THEN NULL ELSE '2026-10-01T13:00:00Z'::timestamptz END WHERE id=$1")
            .bind(id).bind(original)
            .execute(&pool)
            .await
            .unwrap();
            sqlx::query("UPDATE twitch_vod_archive_parts SET status=CASE part_index WHEN 0 THEN 'rejected' WHEN 1 THEN 'failed' WHEN 2 THEN 'pending' ELSE 'failed' END,youtube_video_id=CASE WHEN part_index=0 THEN 'found0' END,last_error='historical',updated_at='2026-10-01T13:00:00Z' WHERE vod_id=$1").bind(id).execute(&pool).await.unwrap();
            sqlx::query("INSERT INTO social_media_platform_auth(platform,twitch_user_id,platform_user_id) VALUES ('youtube','42','own')").execute(&pool).await.unwrap();
            sqlx::query("INSERT INTO twitch_vod_youtube_checks(vod_id,auth_id,auth_revision,channel_id,state,observations,upload_snapshot) SELECT $1,a.id,md5(COALESCE(a.refresh_token_enc::text,'') || COALESCE(a.platform_user_id,'') || COALESCE(a.authorized_at::text,'')),'own','rejected',$2,(SELECT jsonb_agg(jsonb_build_object('id',p.id,'index',p.part_index,'status',p.status,'video_id',p.youtube_video_id,'updated_at',p.updated_at) ORDER BY p.part_index) FROM twitch_vod_archive_parts p WHERE p.vod_id=$1) FROM social_media_platform_auth a WHERE twitch_user_id='42'")
                .bind(id).bind(recovery_observations(serde_json::json!([
                    {"video_id":"found0","part_index":0,"state":"processed"},
                    {"video_id":"found1","part_index":1,"part_total":5,"state":"processed"},
                    {"video_id":"found2","part_index":2,"part_total":5,"state":"processed"}
                ]))).execute(&pool).await.unwrap();
            let protected: Value = sqlx::query_scalar("SELECT jsonb_agg(to_jsonb(p) ORDER BY part_index) FROM twitch_vod_archive_parts p WHERE vod_id=$1 AND part_index<3").bind(id).fetch_one(&pool).await.unwrap();
            let proof: Value = sqlx::query_scalar(
                "SELECT jsonb_build_array(observations,upload_snapshot,last_success_at,channel_id) FROM twitch_vod_youtube_checks c WHERE vod_id=$1",
            )
            .bind(id)
            .fetch_one(&pool)
            .await
            .unwrap();
            sqlx::query("UPDATE twitch_vod_archive_parts SET status='pending',last_error=NULL,updated_at=NOW() WHERE vod_id=$1 AND part_index>=3").bind(id).execute(&pool).await.unwrap();
            store::setze_status(&pool, id, if original { "new" } else { "downloaded" })
                .await
                .unwrap();
            crate::youtube_check::test_save_recovery(
                &pool,
                id,
                &[],
                "error",
                false,
                Some("connection"),
            )
            .await;
            crate::youtube_check::test_save_recovery(&pool, id, &[], "searching", false, None)
                .await;
            crate::youtube_check::test_save_recovery(
                &pool,
                id,
                &[],
                "error",
                false,
                Some("connection"),
            )
            .await;
            let uploader = Arc::new(ZaehlenderHochlader::default());
            let archive = worker(&pool, config(&directory), &uploader);
            let balance = archive.lauf(&[einstellung("earlysalty")]).await.unwrap();
            assert_eq!(balance.hochgeladen, 2);
            assert_eq!(balance.geladen, usize::from(original));
            assert_eq!(uploader.uploads.load(Ordering::SeqCst), 2);
            let state: (
                String,
                Option<String>,
                Option<chrono::DateTime<chrono::Utc>>,
            ) = sqlx::query_as(
                "SELECT status,last_error,uploaded_at FROM twitch_vod_archive_vods WHERE id=$1",
            )
            .bind(id)
            .fetch_one(&pool)
            .await
            .unwrap();
            assert_eq!(state.0, "downloaded");
            assert!(state.1.is_none());
            assert_eq!(
                state.2.as_ref().map(chrono::DateTime::to_rfc3339),
                (!original).then(|| "2026-10-01T13:00:00+00:00".to_string())
            );
            let preserved: Value = sqlx::query_scalar("SELECT jsonb_agg(to_jsonb(p) ORDER BY part_index) FROM twitch_vod_archive_parts p WHERE vod_id=$1 AND part_index<3").bind(id).fetch_one(&pool).await.unwrap();
            assert_eq!(protected, preserved);
            let unchanged: Value = sqlx::query_scalar(
                "SELECT jsonb_build_array(observations,upload_snapshot,last_success_at,channel_id) FROM twitch_vod_youtube_checks c WHERE vod_id=$1",
            )
            .bind(id)
            .fetch_one(&pool)
            .await
            .unwrap();
            assert_eq!(proof, unchanged);
            let balance = archive.lauf(&[einstellung("earlysalty")]).await.unwrap();
            assert_eq!(balance.hochgeladen, 0);
            assert_eq!(uploader.uploads.load(Ordering::SeqCst), 2);
            assert!(source.exists());
            assert!(crate::youtube_check::cleanup_proof(&pool, "42", id, 24)
                .await
                .unwrap()
                .is_none());
            let before_check: Value =
                sqlx::query_scalar("SELECT to_jsonb(v) FROM twitch_vod_archive_vods v WHERE id=$1")
                    .bind(id)
                    .fetch_one(&pool)
                    .await
                    .unwrap();
            sqlx::query("INSERT INTO twitch_vod_youtube_scans(twitch_user_id,auth_id,auth_revision,channel_id,playlist_id,complete,last_success_at) SELECT '42',id,md5(COALESCE(refresh_token_enc::text,'') || COALESCE(platform_user_id,'') || COALESCE(authorized_at::text,'')),'own','synthetic',TRUE,NOW() FROM social_media_platform_auth").execute(&pool).await.unwrap();
            let observations = (0..5)
                .map(|index| crate::youtube_check::Beobachtung {
                    video_id: if index < 3 {
                        format!("found{index}")
                    } else {
                        format!("yt-{}", index - 3)
                    },
                    part_index: Some(index),
                    part_total: Some(5),
                    duration_sec: Some(12),
                    state: "processed".into(),
                    privacy: Some("private".into()),
                    observed_at: chrono::Utc::now(),
                })
                .collect::<Vec<_>>();
            crate::youtube_check::test_save_recovery(
                &pool,
                id,
                &observations,
                "confirmed",
                true,
                None,
            )
            .await;
            let after_check: Value =
                sqlx::query_scalar("SELECT to_jsonb(v) FROM twitch_vod_archive_vods v WHERE id=$1")
                    .bind(id)
                    .fetch_one(&pool)
                    .await
                    .unwrap();
            assert_eq!(before_check, after_check);
            let balance = archive.lauf(&[einstellung("earlysalty")]).await.unwrap();
            assert_eq!(balance.hochgeladen, 0);
            assert!(store::offene_vods(&pool, "42", 10)
                .await
                .unwrap()
                .is_empty());
            assert!(!source.exists());
            let completed: (String, Option<chrono::DateTime<chrono::Utc>>) = sqlx::query_as(
                "SELECT status,uploaded_at FROM twitch_vod_archive_vods WHERE id=$1",
            )
            .bind(id)
            .fetch_one(&pool)
            .await
            .unwrap();
            assert_eq!(completed.0, "archived");
            assert_eq!(completed.1, state.2);
            let final_parts: Value = sqlx::query_scalar("SELECT jsonb_agg(to_jsonb(p) ORDER BY part_index) FROM twitch_vod_archive_parts p WHERE vod_id=$1 AND part_index<3").bind(id).fetch_one(&pool).await.unwrap();
            assert_eq!(protected, final_parts);
            let balance = archive.lauf(&[einstellung("earlysalty")]).await.unwrap();
            assert_eq!(balance.hochgeladen, 0);
            assert_eq!(uploader.uploads.load(Ordering::SeqCst), 2);
            std::fs::remove_dir_all(directory).unwrap();
            pool.close().await;
        }
        eprintln!("YOUTUBE_WORKER_DB_PROOF: prepared-part and original-source paths keep processed legacy rejected/failed/ID-less pending rows and original evidence unchanged; only two genuinely unconfirmed parts upload, repeated execution adds zero uploads");
    }

    #[tokio::test]
    async fn idless_mismatched_parts_reach_preparation_and_actual_upload_after_read_failure() {
        for original in [false, true] {
            for (parts, total, seconds) in [(2, 3, 60), (1, 1, 59)] {
                let pool = pool("t_vod_idless_correspondence")
                    .await
                    .expect("synthetic PostgreSQL required");
                let directory = temp_verzeichnis("idless_correspondence");
                let source = directory.join("v1.mp4");
                std::fs::write(&source, b"synthetic source").unwrap();
                let mut files = Vec::new();
                for index in 0..parts {
                    let file = directory.join(format!("v1.part{index}.mp4"));
                    std::fs::write(&file, b"synthetic part").unwrap();
                    files.push(file.display().to_string());
                }
                store::merke_vod(
                    &pool,
                    "v1",
                    "earlysalty",
                    "42",
                    "Synthetic correspondence",
                    60,
                )
                .await
                .unwrap();
                let id: i64 = sqlx::query_scalar(
                    "SELECT id FROM twitch_vod_archive_vods WHERE twitch_id='v1'",
                )
                .fetch_one(&pool)
                .await
                .unwrap();
                store::setze_geladen(&pool, id, source.to_str().unwrap(), None, 60)
                    .await
                    .unwrap();
                store::setze_teile(&pool, id, "earlysalty", &files)
                    .await
                    .unwrap();
                sqlx::query("UPDATE twitch_vod_archive_parts SET status='failed',last_error='historical',updated_at='2026-10-01T13:00:00Z' WHERE vod_id=$1")
                    .bind(id).execute(&pool).await.unwrap();
                store::setze_status(&pool, id, "archived").await.unwrap();
                sqlx::query("INSERT INTO social_media_platform_auth(platform,twitch_user_id,platform_user_id) VALUES ('youtube','42','own')")
                    .execute(&pool).await.unwrap();
                let observation = crate::youtube_check::Beobachtung {
                    video_id: "found".into(),
                    part_index: Some(0),
                    part_total: Some(total),
                    duration_sec: Some(seconds),
                    state: "processed".into(),
                    privacy: Some("private".into()),
                    observed_at: chrono::Utc::now(),
                };
                crate::youtube_check::test_save_recovery(
                    &pool,
                    id,
                    &[observation],
                    "partial",
                    false,
                    None,
                )
                .await;
                crate::youtube_check::test_save_recovery(
                    &pool,
                    id,
                    &[],
                    "error",
                    false,
                    Some("connection"),
                )
                .await;
                let part_id: i64 = sqlx::query_scalar(
                    "SELECT id FROM twitch_vod_archive_parts WHERE vod_id=$1 AND part_index=0",
                )
                .bind(id)
                .fetch_one(&pool)
                .await
                .unwrap();
                assert!(
                    !crate::youtube_check::part_processed(&pool, "42", id, part_id)
                        .await
                        .unwrap()
                );
                store::setze_status(&pool, id, if original { "new" } else { "downloaded" })
                    .await
                    .unwrap();
                if !original {
                    files[0] = source.display().to_string();
                    store::setze_teile(&pool, id, "earlysalty", &files)
                        .await
                        .unwrap();
                    let prepared: String = sqlx::query_scalar(
                        "SELECT file_path FROM twitch_vod_archive_parts WHERE id=$1",
                    )
                    .bind(part_id)
                    .fetch_one(&pool)
                    .await
                    .unwrap();
                    assert_eq!(prepared, files[0]);
                }
                let uploader = Arc::new(ZaehlenderHochlader::default());
                let archive = worker(&pool, config(&directory), &uploader);
                let balance = archive.lauf(&[einstellung("earlysalty")]).await.unwrap();
                assert_eq!(balance.hochgeladen, parts);
                assert_eq!(balance.geladen, usize::from(original));
                assert_eq!(uploader.uploads.load(Ordering::SeqCst), parts);
                assert!(sqlx::query_scalar::<_, bool>("SELECT bool_and(status='done' AND youtube_video_id IS NOT NULL) FROM twitch_vod_archive_parts WHERE vod_id=$1")
                    .bind(id).fetch_one(&pool).await.unwrap());
                assert!(source.exists());
                let balance = archive.lauf(&[einstellung("earlysalty")]).await.unwrap();
                assert_eq!(balance.hochgeladen, 0);
                assert_eq!(uploader.uploads.load(Ordering::SeqCst), parts);
                std::fs::remove_dir_all(directory).unwrap();
                pool.close().await;
            }
        }
        eprintln!("YOUTUBE_WORKER_DB_PROOF: ID-less 1/3 match for two local parts and short singleton remain unconfirmed after read failure; both preparation paths reach actual counted uploads, repeat adds zero uploads and local source remains protected");
    }

    #[tokio::test]
    async fn processed_recovery_guard_rejects_changed_targets_parts_and_mismatched_observations() {
        let pool = pool("t_vod_recovery_binding")
            .await
            .expect("synthetic PostgreSQL required");
        let directory = temp_verzeichnis("recovery_binding");
        let file = directory.join("v1.mp4");
        std::fs::write(&file, b"synthetic source").unwrap();
        store::merke_vod(&pool, "v1", "earlysalty", "42", "Synthetic binding", 60)
            .await
            .unwrap();
        let id: i64 =
            sqlx::query_scalar("SELECT id FROM twitch_vod_archive_vods WHERE twitch_id='v1'")
                .fetch_one(&pool)
                .await
                .unwrap();
        store::setze_geladen(&pool, id, file.to_str().unwrap(), None, 60)
            .await
            .unwrap();
        store::setze_teile(&pool, id, "earlysalty", &[file.display().to_string()])
            .await
            .unwrap();
        sqlx::query("INSERT INTO social_media_platform_auth(platform,twitch_user_id,platform_user_id) VALUES ('youtube','42','own')").execute(&pool).await.unwrap();
        let part_id: i64 =
            sqlx::query_scalar("SELECT id FROM twitch_vod_archive_parts WHERE vod_id=$1")
                .bind(id)
                .fetch_one(&pool)
                .await
                .unwrap();
        sqlx::query("INSERT INTO twitch_vod_youtube_checks(vod_id,auth_id,auth_revision,channel_id,state,complete,observations,upload_snapshot) SELECT $1,a.id,md5(COALESCE(a.refresh_token_enc::text,'') || COALESCE(a.platform_user_id,'') || COALESCE(a.authorized_at::text,'')),'own','confirmed',TRUE,$2,(SELECT jsonb_agg(jsonb_build_object('id',p.id,'index',p.part_index,'status',p.status,'video_id',p.youtube_video_id,'updated_at',p.updated_at) ORDER BY p.part_index) FROM twitch_vod_archive_parts p WHERE p.vod_id=$1) FROM social_media_platform_auth a WHERE twitch_user_id='42'")
            .bind(id).bind(recovery_observations(serde_json::json!([{"video_id":"found","part_index":null,"duration_sec":60,"state":"processed"}]))).execute(&pool).await.unwrap();
        assert!(
            crate::youtube_check::part_processed(&pool, "42", id, part_id)
                .await
                .unwrap()
        );
        assert!(
            !crate::youtube_check::part_processed(&pool, "99", id, part_id)
                .await
                .unwrap()
        );
        let auth: Value =
            sqlx::query_scalar("SELECT to_jsonb(a) FROM social_media_platform_auth a")
                .fetch_one(&pool)
                .await
                .unwrap();
        let vod: Value = sqlx::query_scalar("SELECT to_jsonb(v) FROM twitch_vod_archive_vods v")
            .fetch_one(&pool)
            .await
            .unwrap();
        let part: Value = sqlx::query_scalar("SELECT to_jsonb(p) FROM twitch_vod_archive_parts p")
            .fetch_one(&pool)
            .await
            .unwrap();
        let check: Value =
            sqlx::query_scalar("SELECT to_jsonb(c) FROM twitch_vod_youtube_checks c")
                .fetch_one(&pool)
                .await
                .unwrap();
        let uploader = Arc::new(ZaehlenderHochlader::default());
        let balance = worker(&pool, config(&directory), &uploader)
            .lauf(&[einstellung("earlysalty")])
            .await
            .unwrap();
        assert_eq!(balance.hochgeladen, 0);
        assert_eq!(uploader.uploads.load(Ordering::SeqCst), 0);
        let preserved: Value =
            sqlx::query_scalar("SELECT to_jsonb(p) FROM twitch_vod_archive_parts p")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(part, preserved);
        sqlx::query("UPDATE twitch_vod_youtube_checks SET last_error='connection',state='error',complete=FALSE").execute(&pool).await.unwrap();
        assert!(
            crate::youtube_check::part_processed(&pool, "42", id, part_id)
                .await
                .unwrap()
        );
        let balance = worker(&pool, config(&directory), &uploader)
            .lauf(&[einstellung("earlysalty")])
            .await
            .unwrap();
        assert_eq!(balance.hochgeladen, 0);
        assert_eq!(uploader.uploads.load(Ordering::SeqCst), 0);
        sqlx::query(
            "UPDATE twitch_vod_youtube_checks SET last_error=NULL,state='confirmed',complete=TRUE",
        )
        .execute(&pool)
        .await
        .unwrap();
        for mutation in [
            "UPDATE social_media_platform_auth SET platform_user_id='different'",
            "UPDATE social_media_platform_auth SET authorized_at='2026-10-01'",
            "UPDATE social_media_platform_auth SET enabled=0",
            "UPDATE twitch_vod_youtube_checks SET auth_id=auth_id+100",
            "UPDATE twitch_vod_youtube_checks SET auth_revision='stale'",
            "UPDATE twitch_vod_youtube_checks SET channel_id=NULL",
            "UPDATE twitch_vod_youtube_checks SET observations=jsonb_set(observations,'{0,part_index}','9'::jsonb)",
            "UPDATE twitch_vod_youtube_checks SET observations=jsonb_set(observations,'{0,part_total}','2'::jsonb)",
            "UPDATE twitch_vod_youtube_checks SET observations=jsonb_set(observations,'{0,duration_sec}','59'::jsonb)",
            "UPDATE twitch_vod_youtube_checks SET observations=jsonb_set(observations,'{0,duration_sec}','66'::jsonb)",
            "UPDATE twitch_vod_youtube_checks SET observations=jsonb_set(observations,'{0,duration_sec}','null'::jsonb)",
            "UPDATE twitch_vod_youtube_checks SET observations=jsonb_set(observations,'{0,state}','\"rejected\"'::jsonb)",
            "UPDATE twitch_vod_archive_vods SET twitch_id='different'",
            "UPDATE twitch_vod_archive_vods SET duration_sec=61",
            "UPDATE twitch_vod_archive_parts SET youtube_video_id='different'",
            "UPDATE twitch_vod_archive_parts SET status='failed'",
            "UPDATE twitch_vod_archive_parts SET updated_at='2026-10-01T00:00:00Z'",
        ] {
            let mut tx = pool.begin().await.unwrap();
            sqlx::query(mutation).execute(&mut *tx).await.unwrap();
            tx.commit().await.unwrap();
            assert!(!crate::youtube_check::part_processed(&pool, "42", id, part_id).await.unwrap(), "{mutation}");
            let uploader = Arc::new(ZaehlenderHochlader::default());
            let archive = worker(&pool, config(&directory), &uploader);
            let balance = archive.lauf(&[einstellung("earlysalty")]).await.unwrap();
            assert_eq!(balance.hochgeladen, 1, "{mutation}");
            assert_eq!(uploader.uploads.load(Ordering::SeqCst), 1, "{mutation}");
            sqlx::query("TRUNCATE twitch_vod_archive_vods,social_media_platform_auth CASCADE").execute(&pool).await.unwrap();
            for (table, saved) in [
                ("social_media_platform_auth", &auth),
                ("twitch_vod_archive_vods", &vod),
                ("twitch_vod_archive_parts", &part),
                ("twitch_vod_youtube_checks", &check),
            ] {
                sqlx::query(sqlx::AssertSqlSafe(format!("INSERT INTO {table} SELECT * FROM jsonb_populate_record(NULL::{table},$1)"))).bind(saved).execute(&pool).await.unwrap();
            }
        }
        std::fs::remove_dir_all(directory).unwrap();
        pool.close().await;
    }

    /// Fund 1: der Upload-Deckel griff nur auf dem reinen Upload-Pfad. Sechs
    /// frische VODs wurden geladen **und** hochgeladen, also sechs Uploads
    /// statt zwei, und damit zweimal taeglich 9600 statt 3200 Einheiten.
    #[tokio::test]
    async fn der_upload_deckel_gilt_auch_auf_dem_download_pfad() {
        let Some(pool) = pool("t_vod_deckel_download").await else {
            return;
        };
        for nummer in 0..6 {
            store::merke_vod(
                &pool,
                &format!("v{nummer}"),
                "earlysalty",
                "42",
                "Stream",
                60,
            )
            .await
            .unwrap();
        }
        let verzeichnis = temp_verzeichnis("deckel_download");
        let cfg = config(&verzeichnis);
        assert_eq!((cfg.max_downloads_per_run, cfg.max_uploads_per_run), (6, 2));

        let hochlader = Arc::new(ZaehlenderHochlader::default());
        let bilanz = worker(&pool, cfg, &hochlader)
            .lauf(&[einstellung("earlysalty")])
            .await
            .unwrap();

        assert_eq!(
            bilanz.geladen, 6,
            "alle sechs duerfen lokal gesichert werden"
        );
        assert_eq!(
            hochlader.uploads.load(Ordering::SeqCst),
            2,
            "der Upload-Deckel muss auch dann greifen, wenn im selben Lauf geladen wurde"
        );
        assert_eq!(bilanz.hochgeladen, 2);
        // Vier VODs liegen lokal und warten auf den naechsten Lauf.
        let offen = store::offene_vods(&pool, "42", 50).await.unwrap();
        assert_eq!(offen.len(), 4);
        assert!(offen.iter().all(|vod| !vod.braucht_download()));

        let _ = std::fs::remove_dir_all(verzeichnis);
    }

    /// Fund 2: der Deckel zaehlte ein VOD, nicht einen Upload. Ein
    /// geschnittenes VOD verbrauchte so viel Kontingent wie es Teile hat und
    /// buchte trotzdem nur eine Einheit.
    #[tokio::test]
    async fn jeder_teil_zaehlt_einzeln_gegen_den_upload_deckel() {
        let Some(pool) = pool("t_vod_deckel_teile").await else {
            return;
        };
        let verzeichnis = temp_verzeichnis("deckel_teile");
        let mut dateien = Vec::new();
        for teil in 0..3 {
            let pfad = verzeichnis.join(format!("v1.part00{teil}.mp4"));
            std::fs::write(&pfad, b"videodaten").unwrap();
            dateien.push(pfad.display().to_string());
        }

        store::merke_vod(&pool, "v1", "earlysalty", "42", "Langer Stream", 60_000)
            .await
            .unwrap();
        let vod = store::offene_vods(&pool, "42", 10).await.unwrap()[0].clone();
        store::setze_geladen(
            &pool,
            vod.id,
            &verzeichnis.join("v1.mp4").display().to_string(),
            None,
            60_000,
        )
        .await
        .unwrap();
        store::setze_teile(&pool, vod.id, "earlysalty", &dateien)
            .await
            .unwrap();

        let hochlader = Arc::new(ZaehlenderHochlader::default());
        let bilanz = worker(&pool, config(&verzeichnis), &hochlader)
            .lauf(&[einstellung("earlysalty")])
            .await
            .unwrap();

        assert_eq!(
            hochlader.uploads.load(Ordering::SeqCst),
            2,
            "drei Teile duerfen bei einem Deckel von zwei nicht alle laufen"
        );
        assert_eq!(bilanz.hochgeladen, 2);
        let teile = store::teile(&pool, vod.id, &test_cipher()).await.unwrap();
        assert_eq!(
            teile
                .iter()
                .filter(|teil| teil.status == store::TEIL_FERTIG)
                .count(),
            2
        );
        // Ein Teil fehlt noch, also ist das VOD nicht fertig und bleibt in der
        // Warteschlange.
        let offen = store::offene_vods(&pool, "42", 10).await.unwrap();
        assert_eq!(offen.len(), 1);

        let _ = std::fs::remove_dir_all(verzeichnis);
    }

    /// Fund 3: `setze_hochgeladen` lief bedingungslos. Ein VOD, dessen
    /// Teileliste nie geschrieben wurde, galt danach als hochgeladen, obwohl
    /// bei YouTube nichts ankam. Das Aufraeumen haette die einzige Kopie
    /// geloescht.
    #[tokio::test]
    async fn ein_vod_ohne_teile_gilt_nicht_als_hochgeladen() {
        let Some(pool) = pool("t_vod_ohne_teile").await else {
            return;
        };
        let verzeichnis = temp_verzeichnis("ohne_teile");
        store::merke_vod(&pool, "v1", "earlysalty", "42", "Abgebrochen", 60)
            .await
            .unwrap();
        let vod = store::offene_vods(&pool, "42", 10).await.unwrap()[0].clone();
        // Zustand nach einem Lauf, der zwischen setze_geladen und setze_teile
        // gestorben ist: Status downloaded, aber keine Teile.
        store::setze_geladen(
            &pool,
            vod.id,
            &verzeichnis.join("v1.mp4").display().to_string(),
            None,
            60,
        )
        .await
        .unwrap();
        assert!(store::teile(&pool, vod.id, &test_cipher())
            .await
            .unwrap()
            .is_empty());

        let hochlader = Arc::new(ZaehlenderHochlader::default());
        let bilanz = worker(&pool, config(&verzeichnis), &hochlader)
            .lauf(&[einstellung("earlysalty")])
            .await
            .unwrap();

        assert_eq!(hochlader.uploads.load(Ordering::SeqCst), 0);
        assert_eq!(bilanz.hochgeladen, 0);
        let offen = store::offene_vods(&pool, "42", 10).await.unwrap();
        assert_eq!(
            offen.len(),
            1,
            "ohne einen einzigen Teil darf das VOD nicht als hochgeladen gelten"
        );
        // Und es geht zurueck in die Download-Warteschlange, sonst haengt es
        // fuer immer im Upload-Pfad ohne etwas zum Hochladen.
        assert!(offen[0].braucht_download());

        let _ = std::fs::remove_dir_all(verzeichnis);
    }

    #[tokio::test]
    async fn dateien_bleiben_bis_zur_bestaetigten_verarbeitung() {
        let Some(pool) = pool("t_vod_confirmed_cleanup").await else {
            return;
        };
        let directory = temp_verzeichnis("confirmed_cleanup");
        let path = directory.join("v1.mp4");
        let other = directory.join("v10.mp4");
        std::fs::write(&path, b"media").unwrap();
        std::fs::write(&other, b"other").unwrap();
        store::merke_vod(&pool, "v1", "renamed", "42", "Stream", 60)
            .await
            .unwrap();
        let vod = store::offene_vods(&pool, "42", 1).await.unwrap().remove(0);
        store::setze_geladen(&pool, vod.id, &path.display().to_string(), None, 60)
            .await
            .unwrap();
        store::setze_teile(&pool, vod.id, "renamed", &[path.display().to_string()])
            .await
            .unwrap();
        let part = store::teile(&pool, vod.id, &test_cipher())
            .await
            .unwrap()
            .remove(0);
        store::setze_teil_fertig(&pool, part.id, "confirmed_video")
            .await
            .unwrap();
        store::setze_hochgeladen(&pool, vod.id).await.unwrap();
        let uploader = Arc::new(ZaehlenderHochlader::default());
        let worker = worker(&pool, config(&directory), &uploader);
        worker.raeume_auf().await.unwrap();
        assert!(path.exists());
        crate::youtube_check::test_confirm(&pool, "42", vod.id).await;
        worker.raeume_auf().await.unwrap();
        assert!(!path.exists());
        assert!(other.exists());
        let state: (String, Option<String>) =
            sqlx::query_as("SELECT status, local_path FROM twitch_vod_archive_vods WHERE id=$1")
                .bind(vod.id)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(state, (store::STATUS_ARCHIVIERT.into(), None));
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[tokio::test]
    async fn verworfene_uploads_fallen_nicht_unter_den_tisch() {
        let pool = pool("t_vod_verworfen")
            .await
            .expect("synthetic PostgreSQL required");
        let verzeichnis = temp_verzeichnis("verworfen");
        let pfad = verzeichnis.join("v1.mp4");
        std::fs::write(&pfad, b"videodaten").unwrap();

        store::merke_vod(&pool, "v1", "earlysalty", "42", "Langer Stream", 60_000)
            .await
            .unwrap();
        let vod = store::offene_vods(&pool, "42", 10).await.unwrap()[0].clone();
        store::setze_geladen(&pool, vod.id, &pfad.display().to_string(), None, 60_000)
            .await
            .unwrap();
        store::setze_teile(&pool, vod.id, "earlysalty", &[pfad.display().to_string()])
            .await
            .unwrap();

        let hochlader = Arc::new(ZaehlenderHochlader::default());
        let worker = worker(&pool, config(&verzeichnis), &hochlader);

        worker.lauf(&[einstellung("earlysalty")]).await.unwrap();
        assert_eq!(hochlader.uploads.load(Ordering::SeqCst), 1);
        assert_eq!(
            store::offene_vods(&pool, "42", 10).await.unwrap().len(),
            0,
            "der Lauf kennt nur erfolgreiche Uploads, das VOD ist abgeschlossen"
        );

        sqlx::query("INSERT INTO social_media_platform_auth(platform,twitch_user_id,platform_user_id) VALUES ('youtube','42','own')").execute(&pool).await.unwrap();
        crate::youtube_check::test_save_recovery(
            &pool,
            vod.id,
            &[crate::youtube_check::Beobachtung {
                video_id: "yt-0".into(),
                part_index: Some(0),
                part_total: Some(1),
                duration_sec: None,
                state: "rejected".into(),
                privacy: Some("private".into()),
                observed_at: chrono::Utc::now(),
            }],
            "rejected",
            false,
            None,
        )
        .await;
        let check: Value = sqlx::query_scalar("SELECT jsonb_build_array(state,observations) FROM twitch_vod_youtube_checks WHERE vod_id=$1").bind(vod.id).fetch_one(&pool).await.unwrap();
        assert_eq!(check[0], "rejected");
        assert_eq!(check[1][0]["state"], "rejected");
        for _ in 0..3 {
            worker.lauf(&[einstellung("earlysalty")]).await.unwrap();
        }
        let teile = store::teile(&pool, vod.id, &test_cipher()).await.unwrap();
        assert_eq!(teile[0].status, store::TEIL_FERTIG);
        assert!(teile[0].youtube_video_id.is_some());
        assert_eq!(hochlader.uploads.load(Ordering::SeqCst), 1);
        assert!(pfad.exists());
        let vod_status: String =
            sqlx::query_scalar("SELECT status FROM twitch_vod_archive_vods WHERE id=$1")
                .bind(vod.id)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(vod_status, store::STATUS_HOCHGELADEN);

        std::fs::remove_dir_all(verzeichnis).unwrap();
        pool.close().await;
        eprintln!("YOUTUBE_WORKER_DB_PROOF: reconciliation persists a real rejection, repeated archive execution uploads nothing and preserves the completed part and local source until explicit retry");
    }
}
