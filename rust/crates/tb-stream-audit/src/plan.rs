//! Aufnahmeplan und Warteschlange.
//!
//! Aufnehmen und Auswerten sind getrennt: aufgenommen wird parallel je Kanal,
//! ausgewertet seriell aus einer gemeinsamen Warteschlange. Ohne GPU teilten
//! sich drei gleichzeitig transkribierte Streams dieselben Kerne wie das
//! Modell, und der Rueckstand waechst schneller, als er abgebaut wird.
//!
//! Diese Datei enthaelt bewusst keine Prozesse und kein Netz, nur die
//! Entscheidungen: welcher Block als naechstes drankommt, wann ein Kanal als
//! offline gilt, wie ein Block heisst. Damit ist die Logik testbar, ohne einen
//! Stream zu brauchen.

use std::collections::{HashSet, VecDeque};

/// Blocklaenge der Aufnahme.
///
/// Zwei Minuten, nicht zehn: der lokale STT-Dienst ist derselbe, den Reaktionen
/// und Smalltalk benutzen, und er arbeitet eine Anfrage nach der anderen ab.
/// Ein Zehn-Minuten-Block haette ihn rund zwei Minuten belegt - laenger als die
/// Zeitgrenze der anderen Aufrufer, die dann reihenweise abgebrochen waeren.
/// Zwei Minuten Ton sind in etwa einer halben Minute transkribiert, und
/// dazwischen kommt jeder andere Aufrufer dran.
pub const BLOCK_SEKUNDEN: u64 = 2 * 60;

/// Obergrenze je Kanal an **aufgenommener** Zeit, nicht an Sendungszeit.
///
/// Der alte Deckel zaehlte ab Helix `started_at`. Wer mitten in einer langen
/// Sendung dazukam, hoerte nach wenigen Minuten still auf. Der Deckel begrenzt
/// jetzt nur unseren Mitschnitt; die Platte begrenzt `MAX_AUFNAHME_BYTES`.
pub const MAX_SEKUNDEN_JE_SENDUNG: u64 = 24 * 60 * 60;

/// Solange die Aufnahmen unter dieser Groesse bleiben, wird weiter
/// mitgeschnitten. Darueber pausiert nur die Aufnahme, nie die Auswertung.
pub const MAX_AUFNAHME_BYTES: u64 = 12 * 1024 * 1024 * 1024;

/// Anteil der Kerne, bis zu dem die Auswertung STT belasten darf.
pub const LAST_KERNE_ANTEIL: f64 = 0.85;

/// So oft wird geprueft, ob ein Kanal sendet.
pub const LIVE_PRUEFUNG_SEKUNDEN: u64 = 60;

/// Ein aufgenommener, noch nicht ausgewerteter Block.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Block {
    pub kanal: String,
    /// Kennung der Sendung, aus der dieser Block stammt. Ohne sie
    /// ueberschreibt der naechste Stream desselben Kanals die Berichte des
    /// vorigen - gleicher Kanal, gleiche Blocknummer, gleicher Dateiname.
    pub lauf: String,
    /// Laufende Nummer innerhalb der Sendung, ab 1.
    pub nummer: u32,
    /// Sekunden seit Beginn der Sendung, an denen dieser Block anfaengt.
    pub versatz_sekunden: u64,
    pub datei: String,
    /// Wie oft die Auswertung dieses Blocks schon fehlschlug.
    #[doc(hidden)]
    pub versuche: u32,
    /// Unix-Zeit, vor der dieser Block nicht wieder drankommt.
    #[doc(hidden)]
    pub frueherstens: i64,
    /// Bericht liegt schon auf der Platte, es fehlt nur die Meldung.
    ///
    /// Ein zweiter Durchlauf durch Transkription und Modell koennte anders
    /// ausfallen als der erste - und ein spaeteres "nichts gefunden" wuerde
    /// den frueheren Fund ueberschreiben und seine Aufnahme loeschen.
    #[doc(hidden)]
    pub nur_melden: bool,
    /// Wie oft die Meldung dieses Blocks schon scheiterte.
    #[doc(hidden)]
    pub meldeversuche: u32,
    /// Zeiten zaehlen ab Aufnahmebeginn statt ab Sendungsbeginn.
    ///
    /// Passiert, wenn Twitch kein brauchbares `started_at` liefert. Der Bericht
    /// sagt es dann, statt eine Stelle im VOD zu behaupten, die nicht stimmt.
    pub zeit_unsicher: bool,
    /// Absoluter Start der Twitch-Sendung, sofern Helix ihn geliefert hat.
    pub stream_start_utc: Option<String>,
    /// Absoluter Beginn dieses Aufnahmeprozesses als Ersatzbasis.
    pub aufnahme_beginn_utc: Option<String>,
}

/// Ab diesem Zaehlerstand ist der halbstuendige Takt vorbei: gemeldet wird ein
/// Block, dessen Bericht schon steht, also viermal mit halbstuendigem Abstand
/// (Versuch 1 bis 4). Danach wechselt der Aufrufer auf sechsstuendige Abstaende und
/// gibt nach insgesamt zwoelf Anlaeufen auf; die Marke `meldung_offen.json`
/// bleibt liegen, sodass der stuendliche Aufraeumtakt es weiter versucht. Ein
/// Fund verschwindet also nicht, er wird nur seltener angeboten.
pub const MAX_MELDEVERSUCHE: u32 = 5;

/// Grundpause vor der Wiederholung, mit dem Versuch multipliziert. Der
/// haeufigste Grund fuer einen Fehlschlag ist ein STT-Dienst, der gerade neu
/// startet; der braucht laenger als drei Anlaeufe am Stueck.
pub const PAUSE_SEKUNDEN: i64 = 120;

/// So oft wird ein Block nach einem Fehlschlag erneut eingereiht. Ein
/// Aussetzer der Transkription darf die einzige Aufnahme nicht verbrennen,
/// ein dauerhaft kaputter Block aber auch nicht ewig kreisen.
pub const MAX_VERSUCHE: u32 = 3;

impl Block {
    /// Name im Bericht: Kanal, Sendung und Sekunde in der Sendung.
    ///
    /// Frueher stand hier die laufende Blocknummer. Startete der Dienst
    /// mitten in derselben Sendung neu, begann sie wieder bei 1 - und die
    /// neuen Berichte ueberschrieben die alten unter demselben Namen. Der
    /// Zeitversatz ist innerhalb einer Sendung eindeutig und sortiert
    /// nebenbei richtig.
    pub fn bezeichnung(&self) -> String {
        // Zeit **und** laufende Nummer: der Versatz zaehlt in ganzen Sekunden,
        // und eine Aufnahme kann in weniger als einer Sekunde zurueckkommen -
        // etwa wenn streamlink sofort abbricht und trotzdem ein paar Kilobyte
        // dalaesst. Zwei Bloecke haetten dann denselben Namen, denselben
        // Ordner und denselben Idempotenzschluessel.
        format!(
            "{}-{}-t{:06}-b{:04}",
            self.kanal, self.lauf, self.versatz_sekunden, self.nummer
        )
    }

    /// Segment-ID-Praefix fuer diesen Block.
    pub fn segment_id(&self, laufend: usize) -> String {
        format!("{}-s{:05}", self.bezeichnung(), laufend)
    }
}

/// Warteschlange der aufgenommenen Bloecke.
///
/// Bewusst FIFO und ohne Prioritaet: ein spaeterer Block eines Kanals sagt
/// nichts darueber aus, wie dringend er ist, und eine Prioritaet nach Kanal
/// wuerde bei drei gleichzeitig sendenden Kanaelen einen davon aushungern.
#[derive(Debug, Default)]
pub struct Warteschlange {
    eintraege: VecDeque<Block>,
    /// Dateien der Bloecke, die gerade ausgewertet werden.
    ///
    /// Ein Block ist zwischen [`Warteschlange::naechster`] und
    /// [`Warteschlange::freigeben`] in keiner Liste und traegt auch noch keine
    /// Fertig-Marke. Ohne diesen Zwischenspeicher hielt ihn der Aufnahme-Task
    /// fuer liegengeblieben und reihte ihn ein zweites Mal ein: die zweite
    /// Auswertung ueberschrieb dann den Bericht des ersten Durchlaufs und
    /// loeschte im Zweifel die Aufnahme, die den Fund belegt.
    in_arbeit: std::collections::HashSet<String>,
}

/// Mindestlaenge eines Blocks.
///
/// Der `AudioCapturer` lehnt alles unter fuenf Sekunden ab. Ein Restblock von
/// zwei Sekunden waere also kein Block, sondern eine Aufnahme, die jede
/// Minute neu startet und jedes Mal scheitert.
pub const MIND_BLOCK_SEKUNDEN: u64 = 5;

impl Warteschlange {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn einreihen(&mut self, block: Block) {
        self.eintraege.push_back(block);
    }

    /// Naechster faelliger Block.
    ///
    /// Ein Block, der nach einem Fehlschlag wartet, blockiert die anderen
    /// nicht: er wird uebersprungen, bis seine Wartezeit um ist.
    pub fn naechster(&mut self) -> Option<Block> {
        self.naechster_um(chrono::Utc::now().timestamp())
    }

    /// Wie [`Warteschlange::naechster`], aber mit vorgegebener Uhrzeit.
    pub fn naechster_um(&mut self, jetzt: i64) -> Option<Block> {
        let stelle = self
            .eintraege
            .iter()
            .position(|b| b.frueherstens <= jetzt)?;
        let block = self.eintraege.remove(stelle)?;
        self.in_arbeit.insert(block.datei.clone());
        Some(block)
    }

    /// Nimmt einen Block aus der Arbeit. Gehoert ans Ende jedes Durchlaufs,
    /// egal ob er sauber durchlief, wieder eingereiht wurde oder aufgegeben
    /// ist: danach schuetzt ihn seine Fertig-Marke oder sein neuer Platz in
    /// der Schlange.
    pub fn freigeben(&mut self, datei: &str) {
        self.in_arbeit.remove(datei);
    }

    /// Reiht einen fehlgeschlagenen Block hinten wieder ein, bis die Versuche
    /// aufgebraucht sind. Hinten, damit ein kaputter Block die anderen nicht
    /// blockiert.
    ///
    /// Gibt `false` zurueck, wenn aufgegeben wurde - dann bleibt die Aufnahme
    /// liegen und muss von Hand angesehen werden.
    pub fn erneut_versuchen(&mut self, block: Block) -> bool {
        self.erneut_versuchen_um(block, chrono::Utc::now().timestamp())
    }

    /// Wie [`Warteschlange::erneut_versuchen`], aber mit vorgegebener Uhrzeit.
    ///
    /// Die Pause steht am Block, nicht im Arbeiter. Ein Schlaf im Arbeiter
    /// haette alle anderen Kanaele mit angehalten - wegen eines Blocks, der
    /// gerade nicht geht.
    pub fn erneut_versuchen_um(&mut self, mut block: Block, jetzt: i64) -> bool {
        block.versuche += 1;
        if block.versuche >= MAX_VERSUCHE {
            return false;
        }
        block.frueherstens = jetzt + (PAUSE_SEKUNDEN * i64::from(block.versuche));
        self.eintraege.push_back(block);
        true
    }

    /// Dateien aller wartenden Bloecke - und der gerade laufenden Auswertung.
    ///
    /// Die Aufbewahrung braucht sie, um nicht genau die Aufnahme zu loeschen,
    /// die gleich ausgewertet wird; das Wiedereinreihen liegengebliebener
    /// Aufnahmen braucht sie, um den Block in Arbeit nicht doppelt zu starten.
    pub fn dateien(&self) -> Vec<String> {
        self.eintraege
            .iter()
            .map(|b| b.datei.clone())
            .chain(self.in_arbeit.iter().cloned())
            .collect()
    }

    /// Reiht einen Block mit fester Pause wieder ein, ohne Versuche zu
    /// zaehlen. Fuer den Fall, dass die Meldung eines aufgegebenen Blocks
    /// nicht rausging - der Block darf nicht einfach verschwinden.
    pub fn spaeter_einreihen(&mut self, mut block: Block, pause_sekunden: i64) {
        block.frueherstens = chrono::Utc::now().timestamp() + pause_sekunden;
        self.eintraege.push_back(block);
    }

    /// Bloecke, die noch transkribiert werden muessen.
    ///
    /// Der Gegendruck haengt an dieser Zahl, nicht an der Gesamtlaenge: sonst
    /// stoppt eine Discord-Stoerung die Aufnahme. Bloecke, die nur noch auf
    /// ihre Meldung warten, kosten keine Rechenzeit.
    pub fn offene_auswertungen(&self) -> usize {
        self.eintraege.iter().filter(|b| !b.nur_melden).count()
    }

    pub fn laenge(&self) -> usize {
        self.eintraege.len()
    }

    pub fn ist_leer(&self) -> bool {
        self.eintraege.is_empty()
    }

    /// Naechster Block, dessen Lauf nicht mehr aufgenommen wird.
    ///
    /// Waehrend der Sendung liegen die Bloecke schon in der Warteschlange,
    /// werden aber nicht transkribiert: STT teilt sich mit Reaktionen, und
    /// drei parallele Auswertungen wuerden den Rechner tragen. Nach dem
    /// Freigeben kommt der aelteste freie Block zuerst.
    pub fn naechster_ohne_sperre_um(&mut self, jetzt: i64, sperre: &LaufSperre) -> Option<Block> {
        let stelle = self
            .eintraege
            .iter()
            .position(|b| b.frueherstens <= jetzt && !sperre.ist_gesperrt(&b.kanal, &b.lauf))?;
        self.eintraege.remove(stelle)
    }

    pub fn naechster_ohne_sperre(&mut self, sperre: &LaufSperre) -> Option<Block> {
        self.naechster_ohne_sperre_um(chrono::Utc::now().timestamp(), sperre)
    }

    /// Offene Bloecke eines Laufs, Auswertung und Meldung zusammen.
    pub fn offene_fuer_lauf(&self, kanal: &str, lauf: &str) -> usize {
        self.eintraege
            .iter()
            .filter(|b| b.kanal == kanal && b.lauf == lauf)
            .count()
    }
}

/// Sendungen, deren Bloecke noch nicht ausgewertet werden duerfen.
#[derive(Debug, Default, Clone)]
pub struct LaufSperre {
    gesperrt: HashSet<(String, String)>,
}

impl LaufSperre {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn sperren(&mut self, kanal: impl Into<String>, lauf: impl Into<String>) {
        self.gesperrt.insert((kanal.into(), lauf.into()));
    }

    pub fn freigeben(&mut self, kanal: &str, lauf: &str) {
        self.gesperrt.remove(&(kanal.to_owned(), lauf.to_owned()));
    }

    pub fn ist_gesperrt(&self, kanal: &str, lauf: &str) -> bool {
        self.gesperrt.contains(&(kanal.to_owned(), lauf.to_owned()))
    }

    pub fn eintraege(&self) -> Vec<(String, String)> {
        self.gesperrt.iter().cloned().collect()
    }
}

/// Ob ein gesperrter Lauf noch die aktuelle Sendung ist.
///
/// `aktuelle_id` ist die Helix-Stream-ID, falls der Kanal sendet. `None`
/// heisst offline: der Lauf ist zu Ende, Auswertung darf loslegen.
pub fn lauf_ist_aktuelle_sendung(lauf: &str, aktuelle_id: Option<&str>) -> bool {
    let Some(id) = aktuelle_id.filter(|s| !s.is_empty()) else {
        return false;
    };
    lauf == id || lauf.starts_with(&format!("{id}-"))
}

/// Abschluss-DM nur, wenn nichts mehr aufgenommen wird und nichts mehr wartet.
pub fn ende_dm_faellig(gesperrt: bool, offene_bloecke: usize) -> bool {
    !gesperrt && offene_bloecke == 0
}

/// Ob noch Platz fuer weitere Aufnahmen ist.
pub fn platte_reicht(belegt: u64, grenze: u64) -> bool {
    belegt < grenze
}

/// Ob die Auswertung STT starten darf, ohne den Rechner zu traegen.
pub fn last_erlaubt(load1: f64, kerne: u32) -> bool {
    if kerne == 0 {
        return true;
    }
    load1 < f64::from(kerne) * LAST_KERNE_ANTEIL
}

/// Laufender Aufnahmezustand eines Kanals.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Aufnahme {
    pub kanal: String,
    pub lauf: String,
    pub bloecke: u32,
    pub aufgenommen_sekunden: u64,
    /// Sekunden, die die Sendung schon lief, als diese Aufnahme begann.
    ///
    /// Ohne sie sind alle Zeiten im Bericht relativ zum Aufnahmebeginn. Wer
    /// dann "Minute 12" im VOD sucht, sucht an der falschen Stelle - und nach
    /// einem Neustart des Dienstes faengt die Zaehlung erneut bei null an,
    /// samt Sechs-Stunden-Deckel.
    pub versatz_basis: u64,
    /// Ob die Zeitbasis geraten ist, weil `started_at` fehlte.
    pub zeit_unsicher: bool,
    /// Absoluter Start der Twitch-Sendung, sofern bekannt.
    pub stream_start_utc: Option<String>,
    /// Unix-Zeit, zu der diese Aufnahme angelegt wurde.
    ///
    /// Die Sendungszeit laeuft an der Uhr, nicht an der Summe der
    /// Blocklaengen: zwischen zwei Bloecken liegen Pausen - Rueckstau,
    /// Neustart eines abgebrochenen streamlink, Wartetakte. Wer sie nicht
    /// mitzaehlt, zeigt spaeter auf die falsche Stelle im VOD.
    pub gestartet_um: i64,
}

impl Aufnahme {
    pub fn starten(kanal: impl Into<String>, lauf: impl Into<String>) -> Self {
        Self::starten_bei(kanal, lauf, 0)
    }

    /// Wie [`Aufnahme::starten`], aber mitten in einer laufenden Sendung.
    pub fn starten_bei(
        kanal: impl Into<String>,
        lauf: impl Into<String>,
        versatz_basis: u64,
    ) -> Self {
        Self {
            kanal: kanal.into(),
            lauf: lauf.into(),
            bloecke: 0,
            aufgenommen_sekunden: 0,
            versatz_basis,
            zeit_unsicher: false,
            stream_start_utc: None,
            gestartet_um: chrono::Utc::now().timestamp(),
        }
    }

    /// Wie weit die Sendung jetzt gelaufen ist - nach der Uhr.
    pub fn sendungssekunden(&self) -> u64 {
        self.sendungssekunden_um(chrono::Utc::now().timestamp())
    }

    /// Wie [`Aufnahme::sendungssekunden`], mit vorgegebener Uhrzeit.
    pub fn sendungssekunden_um(&self, jetzt: i64) -> u64 {
        self.versatz_basis + (jetzt - self.gestartet_um).max(0) as u64
    }

    /// Wie weit die Sendung nach der Summe der Bloecke gelaufen waere.
    ///
    /// Nur fuer den Wiederaufnahme-Pfad, in dem es keine laufende Sendung und
    /// keine Uhr gibt, an der sich etwas messen liesse.
    pub fn gezaehlte_sekunden(&self) -> u64 {
        self.versatz_basis + self.aufgenommen_sekunden
    }

    /// Wie lang der naechste Block werden darf. `None` heisst: Deckel erreicht,
    /// nicht weiter aufnehmen.
    ///
    /// Der Deckel zaehlt aufgenommene Zeit, nicht Sendungszeit. Wer mitten in
    /// einer langen Sendung dazukommt, bekommt trotzdem den vollen Mitschnitt
    /// bis zur Grenze; die Sendungszeiten im Bericht bleiben ueber
    /// `versatz_basis` korrekt.
    pub fn naechste_blocklaenge(&self) -> Option<u64> {
        self.naechste_blocklaenge_um(chrono::Utc::now().timestamp())
    }

    /// Wie [`Aufnahme::naechste_blocklaenge`], mit vorgegebener Uhrzeit.
    pub fn naechste_blocklaenge_um(&self, _jetzt: i64) -> Option<u64> {
        let rest = MAX_SEKUNDEN_JE_SENDUNG.saturating_sub(self.aufgenommen_sekunden);
        match rest {
            r if r < MIND_BLOCK_SEKUNDEN => None,
            r if r < BLOCK_SEKUNDEN => Some(r),
            _ => Some(BLOCK_SEKUNDEN),
        }
    }

    /// Aufgenommenen Block verbuchen und den Warteschlangen-Eintrag bauen.
    pub fn block_fertig(&mut self, datei: impl Into<String>, dauer_sekunden: u64) -> Block {
        let versatz = self.gezaehlte_sekunden();
        self.block_fertig_bei(datei, dauer_sekunden, versatz)
    }

    /// Wie [`Aufnahme::block_fertig`], aber mit dem Versatz, der beim Start
    /// dieses Blocks galt. Der Live-Pfad nimmt diesen Weg, weil zwischen zwei
    /// Bloecken Zeit vergeht, die keine Aufnahme ist.
    pub fn block_fertig_bei(
        &mut self,
        datei: impl Into<String>,
        dauer_sekunden: u64,
        versatz: u64,
    ) -> Block {
        self.bloecke += 1;
        self.aufgenommen_sekunden += dauer_sekunden;
        Block {
            kanal: self.kanal.clone(),
            lauf: self.lauf.clone(),
            nummer: self.bloecke,
            versatz_sekunden: versatz,
            datei: datei.into(),
            versuche: 0,
            frueherstens: 0,
            nur_melden: false,
            meldeversuche: 0,
            zeit_unsicher: self.zeit_unsicher,
            stream_start_utc: self.stream_start_utc.clone(),
            aufnahme_beginn_utc: chrono::DateTime::<chrono::Utc>::from_timestamp(
                self.gestartet_um,
                0,
            )
            .map(|zeitpunkt| zeitpunkt.to_rfc3339()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bezeichnung_sortiert_nach_sendungszeit() {
        let mut namen: Vec<_> = [6000u64, 1200, 600]
            .iter()
            .map(|sekunde| {
                Block {
                    kanal: "kanal".to_owned(),
                    lauf: "L1".to_owned(),
                    nummer: 1,
                    versatz_sekunden: *sekunde,
                    datei: "x.ts".to_owned(),
                    versuche: 0,
                    frueherstens: 0,
                    nur_melden: false,
                    meldeversuche: 0,
                    zeit_unsicher: false,
                    stream_start_utc: None,
                    aufnahme_beginn_utc: None,
                }
                .bezeichnung()
            })
            .collect();
        namen.sort();
        assert_eq!(
            namen,
            vec![
                "kanal-L1-t000600-b0001",
                "kanal-L1-t001200-b0001",
                "kanal-L1-t006000-b0001"
            ]
        );
    }

    #[test]
    fn warteschlange_ist_fifo() {
        let mut w = Warteschlange::new();
        for n in 1..=3 {
            w.einreihen(Block {
                kanal: "a".to_owned(),
                lauf: "L1".to_owned(),
                nummer: n,
                versatz_sekunden: 0,
                datei: format!("{n}.ts"),
                versuche: 0,
                frueherstens: 0,
                nur_melden: false,
                meldeversuche: 0,
                zeit_unsicher: false,
                stream_start_utc: None,
                aufnahme_beginn_utc: None,
            });
        }
        assert_eq!(w.laenge(), 3);
        assert_eq!(w.naechster().unwrap().nummer, 1);
        assert_eq!(w.naechster().unwrap().nummer, 2);
        assert_eq!(w.naechster().unwrap().nummer, 3);
        assert!(w.ist_leer());
    }

    #[test]
    fn block_in_arbeit_bleibt_sichtbar() {
        // Der Aufnahme-Task fragt `dateien()`, bevor er liegengebliebene
        // Aufnahmen einreiht. Waere der Block in Arbeit dort unsichtbar, liefe
        // er ein zweites Mal - und der zweite Bericht ueberschriebe den
        // ersten samt seinem Beleg.
        let mut w = Warteschlange::new();
        w.einreihen(Block {
            kanal: "a".to_owned(),
            lauf: "L1".to_owned(),
            nummer: 1,
            versatz_sekunden: 0,
            datei: "/pfad/a.ts".to_owned(),
            versuche: 0,
            frueherstens: 0,
            nur_melden: false,
            meldeversuche: 0,
            zeit_unsicher: false,
            stream_start_utc: None,
            aufnahme_beginn_utc: None,
        });
        let block = w.naechster().expect("Block");
        assert!(w.ist_leer(), "der Block ist aus der Schlange raus");
        assert_eq!(
            w.dateien(),
            vec!["/pfad/a.ts".to_owned()],
            "in Arbeit heisst weiterhin belegt"
        );
        w.freigeben(&block.datei);
        assert!(w.dateien().is_empty(), "nach der Freigabe ist er weg");
    }

    #[test]
    fn kanaele_hungern_sich_nicht_gegenseitig_aus() {
        // Drei Kanaele reihen abwechselnd ein; die Reihenfolge bleibt gemischt.
        let mut w = Warteschlange::new();
        for (i, kanal) in ["a", "b", "c", "a"].iter().enumerate() {
            w.einreihen(Block {
                kanal: (*kanal).to_owned(),
                lauf: "L1".to_owned(),
                nummer: i as u32 + 1,
                versatz_sekunden: 0,
                datei: "x.ts".to_owned(),
                versuche: 0,
                frueherstens: 0,
                nur_melden: false,
                meldeversuche: 0,
                zeit_unsicher: false,
                stream_start_utc: None,
                aufnahme_beginn_utc: None,
            });
        }
        let reihenfolge: Vec<_> = std::iter::from_fn(|| w.naechster())
            .map(|b| b.kanal)
            .collect();
        assert_eq!(reihenfolge, vec!["a", "b", "c", "a"]);
    }

    #[test]
    fn fehlgeschlagener_block_kommt_hinten_wieder_rein() {
        let mut w = Warteschlange::new();
        let mut a = Aufnahme::starten("k", "L1");
        let erster = a.block_fertig("1.ts", 60);
        let zweiter = a.block_fertig("2.ts", 60);
        w.einreihen(zweiter);
        assert!(w.erneut_versuchen_um(erster, 1000));
        // Der wartende Block blockiert die anderen nicht: der gesunde kommt
        // sofort, der kaputte erst nach seiner Pause.
        assert_eq!(w.naechster_um(1000).unwrap().datei, "2.ts");
        assert!(w.naechster_um(1000).is_none(), "Pause laeuft noch");
        assert_eq!(w.naechster_um(1000 + PAUSE_SEKUNDEN).unwrap().datei, "1.ts");
    }

    #[test]
    fn nach_drei_versuchen_wird_aufgegeben() {
        let mut w = Warteschlange::new();
        let mut a = Aufnahme::starten("k", "L1");
        let mut block = a.block_fertig("1.ts", 60);
        let mut jetzt = 1000;
        for _ in 0..MAX_VERSUCHE - 1 {
            assert!(w.erneut_versuchen_um(block.clone(), jetzt));
            jetzt += PAUSE_SEKUNDEN * i64::from(MAX_VERSUCHE);
            block = w.naechster_um(jetzt).unwrap();
        }
        assert!(!w.erneut_versuchen(block));
        assert!(w.ist_leer());
    }

    #[test]
    fn erster_block_hat_versatz_null_und_nummer_eins() {
        let mut a = Aufnahme::starten("kanal", "L1");
        let block = a.block_fertig("a.ts", BLOCK_SEKUNDEN);
        assert_eq!(block.nummer, 1);
        assert_eq!(block.versatz_sekunden, 0);
    }

    #[test]
    fn versatz_waechst_mit_der_tatsaechlichen_dauer() {
        let mut a = Aufnahme::starten("kanal", "L1");
        a.block_fertig("a.ts", 600);
        // Zweiter Block bricht frueh ab, etwa weil der Stream endete.
        let zweiter = a.block_fertig("b.ts", 120);
        assert_eq!(zweiter.versatz_sekunden, 600);
        let dritter = a.block_fertig("c.ts", 600);
        assert_eq!(dritter.versatz_sekunden, 720);
    }

    #[test]
    fn blocklaenge_ist_normal_der_volle_block() {
        assert_eq!(
            Aufnahme::starten("k", "L1").naechste_blocklaenge(),
            Some(BLOCK_SEKUNDEN)
        );
    }

    #[test]
    fn letzter_block_wird_am_deckel_gekuerzt() {
        let mut a = Aufnahme::starten("k", "L1");
        a.aufgenommen_sekunden = MAX_SEKUNDEN_JE_SENDUNG - 90;
        assert_eq!(a.naechste_blocklaenge_um(a.gestartet_um), Some(90));
    }

    #[test]
    fn am_deckel_wird_nicht_weiter_aufgenommen() {
        let mut a = Aufnahme::starten("k", "L1");
        a.aufgenommen_sekunden = MAX_SEKUNDEN_JE_SENDUNG;
        assert_eq!(a.naechste_blocklaenge_um(a.gestartet_um), None);
    }

    #[test]
    fn pausen_zaehlen_zur_sendungszeit() {
        // Zwischen zwei Bloecken vergeht Zeit, die keine Aufnahme ist:
        // Rueckstau, Wartetakte, ein abgebrochener streamlink. Frueher zaehlte
        // nur die Summe der Blocklaengen, und jeder spaetere Zeitstempel zeigte
        // im VOD auf die falsche Stelle.
        let a = Aufnahme::starten_bei("k", "L1", 0);
        assert_eq!(a.sendungssekunden_um(a.gestartet_um + 1800), 1800);
        assert_eq!(a.gezaehlte_sekunden(), 0, "aufgenommen wurde noch nichts");
    }

    #[test]
    fn ueberschreitung_kippt_nicht_ins_negative() {
        let mut a = Aufnahme::starten("k", "L1");
        a.aufgenommen_sekunden = MAX_SEKUNDEN_JE_SENDUNG + 500;
        assert_eq!(a.naechste_blocklaenge_um(a.gestartet_um), None);
    }

    #[test]
    fn segment_id_enthaelt_kanal_und_block() {
        let block = Block {
            kanal: "deadlockgermany".to_owned(),
            lauf: "L1".to_owned(),
            nummer: 3,
            versatz_sekunden: 1200,
            datei: "x.ts".to_owned(),
            versuche: 0,
            frueherstens: 0,
            nur_melden: false,
            meldeversuche: 0,
            zeit_unsicher: false,
            stream_start_utc: None,
            aufnahme_beginn_utc: None,
        };
        assert_eq!(
            block.segment_id(7),
            "deadlockgermany-L1-t001200-b0003-s00007"
        );
    }
}

#[cfg(test)]
mod tests_versatz {
    use super::*;

    #[test]
    fn zeiten_zaehlen_ab_sendungsbeginn() {
        // Der Dienst startet mitten im Stream: eine Stunde lief schon.
        let mut zustand = Aufnahme::starten_bei("ricky", "42", 3600);
        let block = zustand.block_fertig("/tmp/a.ts", BLOCK_SEKUNDEN);
        assert_eq!(block.versatz_sekunden, 3600);
        let zweiter = zustand.block_fertig("/tmp/b.ts", BLOCK_SEKUNDEN);
        assert_eq!(zweiter.versatz_sekunden, 3600 + BLOCK_SEKUNDEN);
    }

    #[test]
    fn deckel_zaehlt_aufgenommene_zeit_nicht_sendungsbeginn() {
        // Mitten in einer langen Sendung dazukommen darf den Mitschnitt
        // nicht nach wenigen Minuten beenden.
        let spaet = Aufnahme::starten_bei("ricky", "42", MAX_SEKUNDEN_JE_SENDUNG + 60);
        assert_eq!(
            spaet.naechste_blocklaenge_um(spaet.gestartet_um),
            Some(BLOCK_SEKUNDEN)
        );

        let mut knapp = Aufnahme::starten("ricky", "42");
        knapp.aufgenommen_sekunden = MAX_SEKUNDEN_JE_SENDUNG - 120;
        assert_eq!(knapp.naechste_blocklaenge_um(knapp.gestartet_um), Some(120));
    }
}

#[cfg(test)]
mod tests_gegendruck {
    use super::*;

    #[test]
    fn wartende_meldungen_zaehlen_nicht_als_rueckstand() {
        // Eine Discord-Stoerung darf die Aufnahme nicht stoppen.
        let mut w = Warteschlange::new();
        let mut a = Aufnahme::starten("k", "L1");
        let auszuwerten = a.block_fertig("1.ts", 120);
        let mut nur_meldung = a.block_fertig("2.ts", 120);
        nur_meldung.nur_melden = true;
        w.einreihen(auszuwerten);
        w.einreihen(nur_meldung);
        assert_eq!(w.laenge(), 2);
        assert_eq!(w.offene_auswertungen(), 1);
    }
}

#[cfg(test)]
mod tests_lauf_sperre {
    use super::*;

    fn block(kanal: &str, lauf: &str, nummer: u32) -> Block {
        Block {
            kanal: kanal.to_owned(),
            lauf: lauf.to_owned(),
            nummer,
            versatz_sekunden: 0,
            datei: format!("{nummer}.ts"),
            versuche: 0,
            frueherstens: 0,
            nur_melden: false,
            meldeversuche: 0,
            zeit_unsicher: false,
            stream_start_utc: None,
            aufnahme_beginn_utc: None,
        }
    }

    #[test]
    fn gesperrter_lauf_wird_nicht_ausgewertet() {
        let mut w = Warteschlange::new();
        w.einreihen(block("ricky", "live1", 1));
        w.einreihen(block("ski", "live2", 1));
        let mut sperre = LaufSperre::new();
        sperre.sperren("ricky", "live1");
        let naechster = w.naechster_ohne_sperre_um(0, &sperre).unwrap();
        assert_eq!(naechster.kanal, "ski");
        assert!(w.naechster_ohne_sperre_um(0, &sperre).is_none());
        assert_eq!(w.offene_fuer_lauf("ricky", "live1"), 1);
    }

    #[test]
    fn nach_freigabe_kommt_der_gesperrte_block() {
        let mut w = Warteschlange::new();
        w.einreihen(block("ricky", "live1", 1));
        let mut sperre = LaufSperre::new();
        sperre.sperren("ricky", "live1");
        assert!(w.naechster_ohne_sperre_um(0, &sperre).is_none());
        sperre.freigeben("ricky", "live1");
        assert_eq!(
            w.naechster_ohne_sperre_um(0, &sperre).unwrap().kanal,
            "ricky"
        );
    }

    #[test]
    fn zweiter_kanal_wird_nicht_von_ricky_gesperrt() {
        let mut sperre = LaufSperre::new();
        sperre.sperren("ricky", "1");
        assert!(!sperre.ist_gesperrt("ski", "1"));
        assert!(sperre.ist_gesperrt("ricky", "1"));
        assert!(!sperre.ist_gesperrt("ricky", "2"));
    }

    #[test]
    fn streamlink_hiccup_gibt_die_sperre_nicht_frei() {
        assert!(lauf_ist_aktuelle_sendung("42", Some("42")));
        assert!(lauf_ist_aktuelle_sendung("42-99", Some("42")));
        assert!(!lauf_ist_aktuelle_sendung("42", Some("99")));
        assert!(!lauf_ist_aktuelle_sendung("42", None));
    }

    #[test]
    fn ende_dm_wartet_auf_letzte_auswertung() {
        assert!(!ende_dm_faellig(true, 0));
        assert!(!ende_dm_faellig(false, 3));
        assert!(ende_dm_faellig(false, 0));
    }
}

#[cfg(test)]
mod tests_last_und_platte {
    use super::*;

    #[test]
    fn platte_reicht_unter_der_grenze() {
        assert!(platte_reicht(0, MAX_AUFNAHME_BYTES));
        assert!(platte_reicht(MAX_AUFNAHME_BYTES - 1, MAX_AUFNAHME_BYTES));
        assert!(!platte_reicht(MAX_AUFNAHME_BYTES, MAX_AUFNAHME_BYTES));
        assert!(!platte_reicht(MAX_AUFNAHME_BYTES + 1, MAX_AUFNAHME_BYTES));
    }

    #[test]
    fn last_erlaubt_unter_85_prozent() {
        assert!(last_erlaubt(13.5, 16));
        assert!(!last_erlaubt(13.7, 16));
        assert!(last_erlaubt(100.0, 0));
    }
}
