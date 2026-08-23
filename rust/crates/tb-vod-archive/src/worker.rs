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

use sqlx::PgPool;
use tb_social_media::credentials::CredentialManager;
use tb_social_media::upload_worker::youtube_uploader;
use tb_social_media::uploaders::youtube::{ChunkOutcome, ResumeStand, YouTubeUploader};
use tb_social_media::vod_archive::{aktive_vod_archive_streamer, VodArchiveSettings};

use crate::config::{wurzel_oder_elternteil, VodArchiveConfig};
use crate::error::VodArchiveError;
use crate::metadata::baue_metadaten;
use crate::store;
use crate::twitch::{self, CommandRunner};

/// Erster Lauf erst nach dieser Frist, damit der Bot-Start nicht sofort einen
/// mehrstuendigen Download anwirft.
const INITIAL_DELAY_SECS: u64 = 300;

pub struct VodArchiveWorker {
    pool: PgPool,
    config: VodArchiveConfig,
    credentials: CredentialManager,
    runner: Arc<dyn CommandRunner>,
    /// Zaehlt die Laeufe, damit der Startplatz der Warteschlange wandert.
    laeufe: AtomicUsize,
}

/// Was ein Lauf bewegt hat. Nur fuer Log und Tests.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct LaufBilanz {
    pub geladen: usize,
    pub hochgeladen: usize,
    pub uebersprungen: usize,
}

impl VodArchiveWorker {
    pub fn new(pool: PgPool, config: VodArchiveConfig, credentials: CredentialManager) -> Self {
        Self {
            pool,
            config,
            credentials,
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
        let mut uploader: HashMap<String, Option<YouTubeUploader>> = HashMap::new();
        for einstellung in streamer {
            let zugang = self.baue_uploader(&einstellung.streamer_login).await;
            if zugang.is_none() {
                tracing::info!(
                    kanal = %einstellung.streamer_login,
                    "Kein eigener YouTube-Zugang hinterlegt, es wird nur lokal archiviert. \
                     Der Upload startet, sobald die Verbindung im Dashboard steht."
                );
            }
            uploader.insert(einstellung.streamer_login.clone(), zugang);
        }

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
            if braucht_download && bilanz.geladen >= self.config.max_downloads_per_run {
                bilanz.uebersprungen += 1;
                continue;
            }
            if !braucht_download && bilanz.hochgeladen >= self.config.max_uploads_per_run {
                bilanz.uebersprungen += 1;
                continue;
            }
            if bilanz.geladen >= self.config.max_downloads_per_run
                && bilanz.hochgeladen >= self.config.max_uploads_per_run
            {
                tracing::info!("Grenzen erreicht, der Rest folgt beim naechsten Lauf");
                break;
            }
            if !self.platz_reicht() {
                tracing::warn!("Plattenplatz aufgebraucht, der Rest folgt spaeter");
                break;
            }

            let zugang = uploader
                .get(&einstellung.streamer_login)
                .and_then(|u| u.as_ref());
            match self.bearbeite(&vod, zugang, einstellung).await {
                Ok((geladen, hochgeladen)) => {
                    bilanz.geladen += usize::from(geladen);
                    bilanz.hochgeladen += usize::from(hochgeladen);
                }
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

    /// Liefert `(geladen, hochgeladen)`.
    async fn bearbeite(
        &self,
        vod: &store::Vod,
        uploader: Option<&YouTubeUploader>,
        einstellung: &VodArchiveSettings,
    ) -> Result<(bool, bool), VodArchiveError> {
        let kanal = einstellung.streamer_login.as_str();
        let mut geladen = false;
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
            geladen = true;
            tracing::info!(kanal = %kanal, vod = %vod.twitch_id, teile = dateien.len(), "VOD liegt lokal");
        }

        let Some(uploader) = uploader else {
            // Kein Login: das VOD bleibt offen und wartet auf die Verbindung.
            return Ok((geladen, false));
        };

        let teile = store::teile(&self.pool, vod.id).await?;
        let anzahl = teile.len();
        let mut hochgeladen = false;
        for teil in &teile {
            if teil.status == store::TEIL_FERTIG {
                continue;
            }
            self.lade_teil_hoch(vod, teil, anzahl, aufgenommen_am, uploader, einstellung)
                .await?;
            hochgeladen = true;
        }

        store::setze_hochgeladen(&self.pool, vod.id).await?;
        Ok((geladen, hochgeladen))
    }

    async fn lade_teil_hoch(
        &self,
        vod: &store::Vod,
        teil: &store::Teil,
        anzahl: usize,
        aufgenommen_am: Option<chrono::NaiveDate>,
        uploader: &YouTubeUploader,
        einstellung: &VodArchiveSettings,
    ) -> Result<(), VodArchiveError> {
        let pfad = PathBuf::from(&teil.file_path);
        if !pfad.is_file() {
            return Err(VodArchiveError::DateiFehlt(teil.file_path.clone()));
        }
        let groesse = tokio::fs::metadata(&pfad).await?.len();

        // Wiederaufnahme: gibt es eine Sitzung, entscheidet YouTube, wie weit
        // sie gekommen ist. Der eigene Stand aus der Datenbank ist nur der
        // Rueckfall, falls die Nachfrage scheitert.
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
        uploader: &YouTubeUploader,
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

    /// Baut den YouTube-Uploader aus den Credentials **dieses** Streamers.
    /// `None` heisst: keine Verbindung, es wird nur lokal archiviert.
    ///
    /// Der globale Rueckfall von [`CredentialManager::get_credentials`] wird
    /// hier bewusst verworfen: er wuerde das VOD eines Partners auf den
    /// YouTube-Kanal des Betreibers schieben.
    async fn baue_uploader(&self, streamer_login: &str) -> Option<YouTubeUploader> {
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
        Some(youtube_uploader(&creds))
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
}
