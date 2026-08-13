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

use std::collections::VecDeque;

/// Blocklaenge der Aufnahme. Der Wert kommt aus der Python-Fassung
/// (`DEFAULT_CHUNK_SECONDS`) und passt zu dem, was das Modell am Stueck gut
/// verarbeitet.
pub const BLOCK_SEKUNDEN: u64 = 10 * 60;

/// Obergrenze je Kanal und Sendung, aus `MAX_LIVE_SECONDS`. Ohne Deckel laeuft
/// ein Dauerstream die Platte voll.
pub const MAX_SEKUNDEN_JE_SENDUNG: u64 = 6 * 60 * 60;

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
}

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
        format!("{}-{}-t{:06}", self.kanal, self.lauf, self.versatz_sekunden)
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
        self.eintraege.remove(stelle)
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

    /// Dateien aller wartenden Bloecke.
    ///
    /// Die Aufbewahrung braucht sie, um nicht genau die Aufnahme zu loeschen,
    /// die gleich ausgewertet wird.
    pub fn dateien(&self) -> Vec<String> {
        self.eintraege.iter().map(|b| b.datei.clone()).collect()
    }

    pub fn laenge(&self) -> usize {
        self.eintraege.len()
    }

    pub fn ist_leer(&self) -> bool {
        self.eintraege.is_empty()
    }

    /// Bloecke eines Kanals verwerfen, etwa wenn eine Sendung abgebrochen ist
    /// und die Reste nichts mehr aussagen.
    pub fn kanal_verwerfen(&mut self, kanal: &str) -> usize {
        let vorher = self.eintraege.len();
        self.eintraege.retain(|b| b.kanal != kanal);
        vorher - self.eintraege.len()
    }
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
        }
    }

    /// Wie weit die Sendung an dieser Stelle gelaufen ist.
    pub fn sendungssekunden(&self) -> u64 {
        self.versatz_basis + self.aufgenommen_sekunden
    }

    /// Wie lang der naechste Block werden darf. `None` heisst: Deckel erreicht,
    /// nicht weiter aufnehmen.
    /// Der Deckel zaehlt Sendungszeit, nicht Aufnahmezeit. Ein Neustart des
    /// Dienstes in Stunde sieben faengt damit nicht wieder von vorn an.
    pub fn naechste_blocklaenge(&self) -> Option<u64> {
        let rest = MAX_SEKUNDEN_JE_SENDUNG.saturating_sub(self.sendungssekunden());
        match rest {
            r if r < MIND_BLOCK_SEKUNDEN => None,
            r if r < BLOCK_SEKUNDEN => Some(r),
            _ => Some(BLOCK_SEKUNDEN),
        }
    }

    /// Aufgenommenen Block verbuchen und den Warteschlangen-Eintrag bauen.
    pub fn block_fertig(&mut self, datei: impl Into<String>, dauer_sekunden: u64) -> Block {
        let versatz = self.sendungssekunden();
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
                }
                .bezeichnung()
            })
            .collect();
        namen.sort();
        assert_eq!(
            namen,
            vec!["kanal-L1-t000600", "kanal-L1-t001200", "kanal-L1-t006000"]
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
            });
        }
        assert_eq!(w.laenge(), 3);
        assert_eq!(w.naechster().unwrap().nummer, 1);
        assert_eq!(w.naechster().unwrap().nummer, 2);
        assert_eq!(w.naechster().unwrap().nummer, 3);
        assert!(w.ist_leer());
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
            });
        }
        let reihenfolge: Vec<_> = std::iter::from_fn(|| w.naechster())
            .map(|b| b.kanal)
            .collect();
        assert_eq!(reihenfolge, vec!["a", "b", "c", "a"]);
    }

    #[test]
    fn kanal_verwerfen_laesst_andere_stehen() {
        let mut w = Warteschlange::new();
        for kanal in ["a", "b", "a"] {
            w.einreihen(Block {
                kanal: kanal.to_owned(),
                lauf: "L1".to_owned(),
                nummer: 1,
                versatz_sekunden: 0,
                datei: "x.ts".to_owned(),
                versuche: 0,
                frueherstens: 0,
            });
        }
        assert_eq!(w.kanal_verwerfen("a"), 2);
        assert_eq!(w.laenge(), 1);
        assert_eq!(w.naechster().unwrap().kanal, "b");
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
        assert_eq!(a.naechste_blocklaenge(), Some(90));
    }

    #[test]
    fn am_deckel_wird_nicht_weiter_aufgenommen() {
        let mut a = Aufnahme::starten("k", "L1");
        a.aufgenommen_sekunden = MAX_SEKUNDEN_JE_SENDUNG;
        assert_eq!(a.naechste_blocklaenge(), None);
    }

    #[test]
    fn ueberschreitung_kippt_nicht_ins_negative() {
        let mut a = Aufnahme::starten("k", "L1");
        a.aufgenommen_sekunden = MAX_SEKUNDEN_JE_SENDUNG + 500;
        assert_eq!(a.naechste_blocklaenge(), None);
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
        };
        assert_eq!(block.segment_id(7), "deadlockgermany-L1-t001200-s00007");
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
    fn deckel_zaehlt_sendungszeit() {
        // Neustart in Stunde sieben: es wird nicht noch einmal aufgenommen.
        let spaet = Aufnahme::starten_bei("ricky", "42", MAX_SEKUNDEN_JE_SENDUNG + 60);
        assert_eq!(spaet.naechste_blocklaenge(), None);

        // Kurz vor dem Deckel bleibt nur der Rest.
        let knapp = Aufnahme::starten_bei("ricky", "42", MAX_SEKUNDEN_JE_SENDUNG - 120);
        assert_eq!(knapp.naechste_blocklaenge(), Some(120));
    }
}
