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
}

impl Block {
    /// Name im Bericht: Kanal, Sendung und Blocknummer. Dreistellig, damit die
    /// Sortierung nach Name auch der Reihenfolge entspricht.
    pub fn bezeichnung(&self) -> String {
        format!("{}-{}-block{:03}", self.kanal, self.lauf, self.nummer)
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

impl Warteschlange {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn einreihen(&mut self, block: Block) {
        self.eintraege.push_back(block);
    }

    pub fn naechster(&mut self) -> Option<Block> {
        self.eintraege.pop_front()
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
}

impl Aufnahme {
    pub fn starten(kanal: impl Into<String>, lauf: impl Into<String>) -> Self {
        Self {
            kanal: kanal.into(),
            lauf: lauf.into(),
            bloecke: 0,
            aufgenommen_sekunden: 0,
        }
    }

    /// Wie lang der naechste Block werden darf. `None` heisst: Deckel erreicht,
    /// nicht weiter aufnehmen.
    pub fn naechste_blocklaenge(&self) -> Option<u64> {
        let rest = MAX_SEKUNDEN_JE_SENDUNG.saturating_sub(self.aufgenommen_sekunden);
        match rest {
            0 => None,
            r if r < BLOCK_SEKUNDEN => Some(r),
            _ => Some(BLOCK_SEKUNDEN),
        }
    }

    /// Aufgenommenen Block verbuchen und den Warteschlangen-Eintrag bauen.
    pub fn block_fertig(&mut self, datei: impl Into<String>, dauer_sekunden: u64) -> Block {
        let versatz = self.aufgenommen_sekunden;
        self.bloecke += 1;
        self.aufgenommen_sekunden += dauer_sekunden;
        Block {
            kanal: self.kanal.clone(),
            lauf: self.lauf.clone(),
            nummer: self.bloecke,
            versatz_sekunden: versatz,
            datei: datei.into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bezeichnung_sortiert_nach_nummer() {
        let mut namen: Vec<_> = [10u32, 2, 1]
            .iter()
            .map(|n| {
                Block {
                    kanal: "kanal".to_owned(),
                    lauf: "L1".to_owned(),
                    nummer: *n,
                    versatz_sekunden: 0,
                    datei: "x.ts".to_owned(),
                }
                .bezeichnung()
            })
            .collect();
        namen.sort();
        assert_eq!(
            namen,
            vec![
                "kanal-L1-block001",
                "kanal-L1-block002",
                "kanal-L1-block010"
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
            });
        }
        assert_eq!(w.kanal_verwerfen("a"), 2);
        assert_eq!(w.laenge(), 1);
        assert_eq!(w.naechster().unwrap().kanal, "b");
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
        };
        assert_eq!(block.segment_id(7), "deadlockgermany-L1-block003-s00007");
    }
}
