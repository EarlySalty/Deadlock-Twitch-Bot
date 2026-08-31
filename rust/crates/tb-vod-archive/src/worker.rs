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

/// Wie weit zurueck fertige Uploads bei YouTube nachgeprueft werden. Der
/// Befund (abgelehnt, entfernt) liegt oft erst Stunden nach dem Upload vor,
/// also weit genug ueber den Tag hinaus, aber begrenzt, damit alte Laeufe
/// nicht ewig nachgefragt werden.
const PRUEF_FENSTER_TAGE: i32 = 14;

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
}

/// Woher der YouTube-Zugang eines Kanals kommt. `None` heisst: keine
/// Verbindung, es wird nur lokal archiviert.
#[async_trait]
pub trait HochladerQuelle: Send + Sync {
    async fn fuer(&self, streamer_login: &str) -> Option<Arc<dyn TeilHochlader>>;
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
    async fn fuer(&self, streamer_login: &str) -> Option<Arc<dyn TeilHochlader>> {
        let creds = self
            .credentials
            .get_credentials("youtube", Some(streamer_login))
            .await?;
        let gehoert_dem_streamer = creds
            .streamer_login
            .as_deref()
            .is_some_and(|login| login.eq_ignore_ascii_case(streamer_login));
        if !gehoert_dem_streamer {
            tracing::info!(
                kanal = %streamer_login,
                "Nur ein globaler YouTube-Zugang vorhanden, der gilt hier nicht"
            );
            return None;
        }
        if creds.access_token.is_empty() {
            return None;
        }
        Some(Arc::new(youtube_uploader(&creds)))
    }
}

pub struct VodArchiveWorker {
    pool: PgPool,
    config: VodArchiveConfig,
    zugang: Arc<dyn HochladerQuelle>,
    runner: Arc<dyn CommandRunner>,
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
    pub fn new(pool: PgPool, config: VodArchiveConfig, credentials: CredentialManager) -> Self {
        Self::mit_zugang(pool, config, Arc::new(StreamerZugang { credentials }))
    }

    /// Wie [`Self::new`], aber mit fertiger Upload-Quelle statt der
    /// Credential-Tabelle (Tests).
    pub fn mit_zugang(
        pool: PgPool,
        config: VodArchiveConfig,
        zugang: Arc<dyn HochladerQuelle>,
    ) -> Self {
        Self {
            pool,
            config,
            zugang,
            runner: Arc::new(twitch::TokioCommandRunner),
            laeufe: AtomicUsize::new(0),
        }
    }

    /// Tauscht den Prozess-Starter aus (Tests).
    pub fn with_runner(mut self, runner: Arc<dyn CommandRunner>) -> Self {
        self.runner = runner;
        self
    }

    pub async fn run(&self) {
        tokio::time::sleep(std::time::Duration::from_secs(INITIAL_DELAY_SECS)).await;
        loop {
            self.run_once().await;
            tokio::time::sleep(self.config.interval).await;
        }
    }

    /// Ein vollstaendiger Lauf. Faengt alle Fehler ab, weil der Worker sonst
    /// nach dem ersten kaputten VOD fuer immer schweigt.
    pub async fn run_once(&self) {
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
            if let Err(fehler) = self.entdecke(&einstellung.streamer_login).await {
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
            let zugang = self.zugang.fuer(&einstellung.streamer_login).await;
            if zugang.is_none() {
                tracing::info!(
                    kanal = %einstellung.streamer_login,
                    "Kein eigener YouTube-Zugang hinterlegt, es wird nur lokal archiviert. \
                     Der Upload startet, sobald die Verbindung im Dashboard steht."
                );
            }
            uploader.insert(einstellung.streamer_login.clone(), zugang);
        }

        // Der resumable Upload meldet nur angekommene Bytes, nicht den
        // Verbleib. Befunde wie "Video ist zu lang" liegen erst Stunden
        // spaeter vor und werden hier eingesammelt, bevor neue Uploads
        // starten.
        self.pruefe_fruehe_uploads(streamer, &uploader).await;

        // Reserve auf beide Grenzen, damit nach einem Download im selben Lauf
        // noch Uploads gefunden werden.
        let reserve =
            (self.config.max_downloads_per_run + self.config.max_uploads_per_run) as i64 + 10;
        let mut warteschlangen = Vec::with_capacity(streamer.len());
        for einstellung in streamer {
            let offen =
                store::offene_vods(&self.pool, &einstellung.streamer_login, reserve).await?;
            if !offen.is_empty() {
                warteschlangen.push((einstellung, offen));
            }
        }

        let versatz = self.laeufe.fetch_add(1, Ordering::Relaxed);
        for (einstellung, vod) in verschraenke(warteschlangen, versatz) {
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

            let zugang = uploader
                .get(&einstellung.streamer_login)
                .and_then(|u| u.clone());
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
                    break;
                }
                Err(fehler) => {
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
                    store::setze_fehler(&self.pool, vod.id, status, &fehler.to_string()).await?;
                }
            }
        }

        self.raeume_auf().await?;
        Ok(bilanz)
    }

    /// Traegt neue VODs eines Kanals ein. Ein laufender Stream wird
    /// ausgelassen, sein VOD waere sonst nur zur Haelfte im Archiv.
    async fn entdecke(&self, kanal: &str) -> Result<(), VodArchiveError> {
        let mut vods = twitch::liste_vods(self.runner.as_ref(), &self.config, kanal).await?;
        if vods.is_empty() {
            tracing::info!(kanal = %kanal, "Keine VODs gefunden");
            return Ok(());
        }
        if twitch::ist_live(self.runner.as_ref(), &self.config, kanal).await {
            tracing::info!(kanal = %kanal, "Kanal ist live, das neueste VOD wartet auf das Streamende");
            vods.remove(0);
        }
        for vod in &vods {
            if store::merke_vod(
                &self.pool,
                &vod.twitch_id,
                kanal,
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

    /// Holt den Befund ueber frische Uploads ein. YouTube nimmt einen
    /// fertigen resumable Upload erstmal an und wirft ihn spaeter wieder
    /// raus, etwa beim 15-Minuten-Limit eines nicht verifizierten Kanals.
    /// Ohne diese Nachfrage bliebe so ein Verlust als "Fertig" stehen, und
    /// das Aufraeumen wuerde irgendwann die letzte lokale Kopie loeschen.
    async fn pruefe_fruehe_uploads(
        &self,
        streamer: &[VodArchiveSettings],
        uploader: &HashMap<String, Option<Arc<dyn TeilHochlader>>>,
    ) {
        for einstellung in streamer {
            let login = einstellung.streamer_login.as_str();
            let Some(hochlader) = uploader.get(login).cloned().flatten() else {
                continue;
            };
            let frische =
                match store::frisch_hochgeladene_teile(&self.pool, login, PRUEF_FENSTER_TAGE).await
                {
                    Ok(frische) if frische.is_empty() => continue,
                    Ok(frische) => frische,
                    Err(fehler) => {
                        tracing::warn!(kanal = login, %fehler, "Upload-Nachpruefung nicht lesbar");
                        continue;
                    }
                };
            tracing::info!(kanal = login, teile = frische.len(), "Upload-Nachpruefung");
            for eintrag in frische {
                match hochlader.video_status(&eintrag.video_id).await {
                    Ok(None) => {
                        self.markiere_verworfen(
                            login,
                            &eintrag,
                            "YouTube kennt die Video-ID nicht mehr (entfernt oder abgelehnt)",
                        )
                        .await;
                    }
                    Ok(Some(stand))
                        if matches!(stand.upload_status.as_str(), "rejected" | "failed") =>
                    {
                        let grund = match stand.rejection_reason.as_deref() {
                            Some(reason) => format!(
                                "YouTube hat den Upload verworfen: uploadStatus={}, rejectionReason={}",
                                stand.upload_status, reason
                            ),
                            None => format!(
                                "YouTube hat den Upload verworfen: uploadStatus={}",
                                stand.upload_status
                            ),
                        };
                        self.markiere_verworfen(login, &eintrag, &grund).await;
                    }
                    Ok(Some(_)) => {}
                    Err(UploadError::QuotaExceeded(_)) => {
                        // Das Tageskontingent haengt am Google-Projekt: ist
                        // es leer, lohnt weder die Nachpruefung noch der
                        // restliche Lauf.
                        tracing::warn!(
                            kanal = login,
                            "YouTube-Tageskontingent erschoepft, Upload-Nachpruefung endet"
                        );
                        return;
                    }
                    Err(fehler) => {
                        tracing::warn!(
                            kanal = login,
                            video = %eintrag.video_id,
                            %fehler,
                            "Upload-Nachpruefung fehlgeschlagen"
                        );
                    }
                }
            }
        }
    }

    /// Trägt einen von YouTube wieder verworfenen Upload als solchen fest:
    /// das Teil geht in die Auszeit und das VOD aus "hochgeladen" zurueck in
    /// die Warteschlange, damit die lokale Kopie nicht aufgeräumt wird, solange
    /// bei YouTube nichts liegt.
    async fn markiere_verworfen(&self, kanal: &str, eintrag: &store::FrischerUpload, grund: &str) {
        tracing::error!(
            kanal = %kanal,
            video = %eintrag.video_id,
            teil = eintrag.part_index,
            grund,
            "Upload von YouTube verworfen; häufigste Ursache ist das 15-Minuten-Limit eines nicht verifizierten Kanals (youtube.com/verify). Das Teil pausiert und geht erneut hinaus, danach folgt der Upload von selbst."
        );
        if let Err(fehler) =
            store::setze_upload_abgelehnt(&self.pool, eintrag.teil_id, eintrag.vod_id, grund).await
        {
            tracing::error!(%fehler, "Verworfener Upload konnte nicht atomar zurückgesetzt werden");
        }
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
        let mut aufgenommen_am = vod.recorded_at;

        if vod.braucht_download() {
            tracing::info!(kanal = %kanal, vod = %vod.twitch_id, titel = %vod.title, "Lade VOD");
            store::setze_status(&self.pool, vod.id, store::STATUS_LAEDT).await?;
            let verzeichnis = self.config.verzeichnis_fuer(kanal);
            let download = twitch::lade_vod(
                self.runner.as_ref(),
                &self.config,
                &verzeichnis,
                &vod.twitch_id,
            )
            .await?;
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

        let Some(uploader) = uploader else {
            // Kein Login: das VOD bleibt offen und wartet auf die Verbindung.
            return Ok(());
        };

        let teile = store::teile(&self.pool, vod.id).await?;
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
        for teil in &teile {
            if teil.status == store::TEIL_FERTIG {
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
            self.lade_teil_hoch(vod, teil, anzahl, aufgenommen_am, uploader, einstellung)
                .await?;
            bilanz.hochgeladen += 1;
        }

        if offen > 0 {
            tracing::info!(
                kanal = %kanal,
                vod = %vod.twitch_id,
                offen,
                "Upload-Deckel erreicht, die restlichen Teile folgen beim naechsten Lauf"
            );
            return Ok(());
        }

        // Erst jetzt: jeder Teil liegt wirklich bei YouTube. Danach darf die
        // lokale Kopie weg.
        store::setze_hochgeladen(&self.pool, vod.id).await?;
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
        store::setze_teil_sitzung(&self.pool, teil.id, &uri, 0).await?;
        Ok(uri)
    }

    /// Loescht lokale Dateien nach der eingestellten Frist. 0 heisst nie.
    async fn raeume_auf(&self) -> Result<(), VodArchiveError> {
        if self.config.keep_local_days <= 0 {
            return Ok(());
        }
        for faellig in store::abgelaufen_lokal(&self.pool, self.config.keep_local_days).await? {
            let verzeichnis = self.config.verzeichnis_fuer(&faellig.streamer_login);
            loesche_dateien(&verzeichnis, &faellig.twitch_id);
            store::markiere_archiviert(&self.pool, faellig.id).await?;
            tracing::info!(
                kanal = %faellig.streamer_login,
                vod = %faellig.twitch_id,
                "Lokale Dateien geloescht"
            );
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
fn loesche_dateien(verzeichnis: &Path, twitch_id: &str) {
    let Ok(eintraege) = std::fs::read_dir(verzeichnis) else {
        return;
    };
    let praefix = format!("{twitch_id}.");
    for eintrag in eintraege.filter_map(|e| e.ok()) {
        if eintrag.file_name().to_string_lossy().starts_with(&praefix) {
            let _ = std::fs::remove_file(eintrag.path());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
        sqlx::query(&format!("DROP SCHEMA IF EXISTS {schema} CASCADE"))
            .execute(&admin)
            .await
            .unwrap();
        sqlx::query(&format!("CREATE SCHEMA {schema}"))
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
             streamer_login TEXT NOT NULL, title TEXT NOT NULL, duration_sec BIGINT NOT NULL DEFAULT 0, \
             recorded_at DATE, status TEXT NOT NULL DEFAULT 'new', local_path TEXT, last_error TEXT, \
             discovered_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP, downloaded_at TIMESTAMPTZ, \
             uploaded_at TIMESTAMPTZ, updated_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP)",
            "CREATE TABLE twitch_vod_archive_parts (id BIGSERIAL PRIMARY KEY, vod_id BIGINT NOT NULL \
             REFERENCES twitch_vod_archive_vods (id) ON DELETE CASCADE, streamer_login TEXT, \
             part_index INTEGER NOT NULL, \
             file_path TEXT NOT NULL, size_bytes BIGINT NOT NULL DEFAULT 0, status TEXT NOT NULL DEFAULT 'pending', \
             upload_session_uri TEXT, upload_offset BIGINT NOT NULL DEFAULT 0, youtube_video_id TEXT, \
             last_error TEXT, updated_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP, \
             UNIQUE (vod_id, part_index))",
        ] {
            sqlx::query(ddl).execute(&pool).await.unwrap();
        }
        Some(pool)
    }

    /// Zaehlt, wie oft wirklich ein Teil zu YouTube geschoben wurde. Genau
    /// diese Zahl kostet Tageskontingent. Mit `verwerfen` kann der Test
    /// simulieren, dass YouTube die fertigen Uploads wieder einsammelt.
    #[derive(Default)]
    struct ZaehlenderHochlader {
        uploads: AtomicUsize,
        verwerfen: Mutex<bool>,
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
            if *self.verwerfen.lock().unwrap() {
                return Ok(None);
            }
            Ok(Some(VideoZustand {
                upload_status: "processed".to_string(),
                rejection_reason: None,
            }))
        }
    }

    struct FesteQuelle(Arc<ZaehlenderHochlader>);

    #[async_trait]
    impl HochladerQuelle for FesteQuelle {
        async fn fuer(&self, _streamer_login: &str) -> Option<Arc<dyn TeilHochlader>> {
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
        VodArchiveWorker::mit_zugang(pool.clone(), cfg, Arc::new(FesteQuelle(hochlader.clone())))
            .with_runner(Arc::new(WerkzeugAttrappe::neu()))
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
            store::merke_vod(&pool, &format!("v{nummer}"), "earlysalty", "Stream", 60)
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
        let offen = store::offene_vods(&pool, "earlysalty", 50).await.unwrap();
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

        store::merke_vod(&pool, "v1", "earlysalty", "Langer Stream", 60_000)
            .await
            .unwrap();
        let vod = store::offene_vods(&pool, "earlysalty", 10).await.unwrap()[0].clone();
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
        let teile = store::teile(&pool, vod.id).await.unwrap();
        assert_eq!(
            teile
                .iter()
                .filter(|teil| teil.status == store::TEIL_FERTIG)
                .count(),
            2
        );
        // Ein Teil fehlt noch, also ist das VOD nicht fertig und bleibt in der
        // Warteschlange.
        let offen = store::offene_vods(&pool, "earlysalty", 10).await.unwrap();
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
        store::merke_vod(&pool, "v1", "earlysalty", "Abgebrochen", 60)
            .await
            .unwrap();
        let vod = store::offene_vods(&pool, "earlysalty", 10).await.unwrap()[0].clone();
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
        assert!(store::teile(&pool, vod.id).await.unwrap().is_empty());

        let hochlader = Arc::new(ZaehlenderHochlader::default());
        let bilanz = worker(&pool, config(&verzeichnis), &hochlader)
            .lauf(&[einstellung("earlysalty")])
            .await
            .unwrap();

        assert_eq!(hochlader.uploads.load(Ordering::SeqCst), 0);
        assert_eq!(bilanz.hochgeladen, 0);
        let offen = store::offene_vods(&pool, "earlysalty", 10).await.unwrap();
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

    /// Der resumable Upload meldet nur angekommene Bytes. Verwirft YouTube
    /// das Video erst spaeter (15-Minuten-Limit), darf das nicht als
    /// "Fertig" stehen bleiben: der Befund muss zum Teil und VOD durchschlagen
    /// und der Upload nach einer Auszeit erneut laufen, sobald der Grund weg
    /// ist.
    #[tokio::test]
    async fn verworfene_uploads_fallen_nicht_unter_den_tisch() {
        let Some(pool) = pool("t_vod_verworfen").await else {
            return;
        };
        let verzeichnis = temp_verzeichnis("verworfen");
        let pfad = verzeichnis.join("v1.mp4");
        std::fs::write(&pfad, b"videodaten").unwrap();

        store::merke_vod(&pool, "v1", "earlysalty", "Langer Stream", 60_000)
            .await
            .unwrap();
        let vod = store::offene_vods(&pool, "earlysalty", 10).await.unwrap()[0].clone();
        store::setze_geladen(&pool, vod.id, &pfad.display().to_string(), None, 60_000)
            .await
            .unwrap();
        store::setze_teile(&pool, vod.id, "earlysalty", &[pfad.display().to_string()])
            .await
            .unwrap();

        let hochlader = Arc::new(ZaehlenderHochlader::default());
        let worker = worker(&pool, config(&verzeichnis), &hochlader);

        // Lauf eins: der Upload geht raus, alles gilt als fertig.
        worker.lauf(&[einstellung("earlysalty")]).await.unwrap();
        assert_eq!(hochlader.uploads.load(Ordering::SeqCst), 1);
        assert_eq!(
            store::offene_vods(&pool, "earlysalty", 10)
                .await
                .unwrap()
                .len(),
            0,
            "der Lauf kennt nur erfolgreiche Uploads, das VOD ist abgeschlossen"
        );

        // Jetzt sammelt YouTube den Upload wieder ein.
        *hochlader.verwerfen.lock().unwrap() = true;
        worker.lauf(&[einstellung("earlysalty")]).await.unwrap();
        let teile = store::teile(&pool, vod.id).await.unwrap();
        assert_eq!(teile[0].status, store::TEIL_ABGELEHNT);
        assert!(teile[0].upload_session_uri.is_none());
        assert_eq!(teile[0].upload_offset, 0);
        assert!(teile[0].youtube_video_id.is_none());
        let vod_status: String =
            sqlx::query_scalar("SELECT status FROM twitch_vod_archive_vods WHERE id = $1")
                .bind(vod.id)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(
            vod_status,
            store::STATUS_UPLOAD_FEHLER,
            "das VOD muss aus 'uploaded' zurueckfallen, sonst raeumt das Archiv die letzte Kopie weg"
        );

        // Die Auszeit greift: kein neuer Upload, das VOD bleibt offen.
        worker.lauf(&[einstellung("earlysalty")]).await.unwrap();
        assert_eq!(hochlader.uploads.load(Ordering::SeqCst), 1);
        assert_eq!(
            store::offene_vods(&pool, "earlysalty", 10)
                .await
                .unwrap()
                .len(),
            1
        );

        // Auszeit vorbei: der Upload geht erneut hinaus, der Grund ist weg.
        sqlx::query(
            "UPDATE twitch_vod_archive_parts SET updated_at = CURRENT_TIMESTAMP - INTERVAL '13 hours'",
        )
        .execute(&pool)
        .await
        .unwrap();
        *hochlader.verwerfen.lock().unwrap() = false;
        worker.lauf(&[einstellung("earlysalty")]).await.unwrap();
        assert_eq!(
            hochlader.uploads.load(Ordering::SeqCst),
            2,
            "nach der Auszeit und ohne neuen Verwerfungsgrund muss erneut hochgeladen werden"
        );
        let teile = store::teile(&pool, vod.id).await.unwrap();
        assert_eq!(teile[0].status, store::TEIL_FERTIG);
        let offen = store::offene_vods(&pool, "earlysalty", 10).await.unwrap();
        assert_eq!(offen.len(), 0);

        let _ = std::fs::remove_dir_all(verzeichnis);
    }
}
