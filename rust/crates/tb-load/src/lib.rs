//! Geteiltes Last-Gate: eine Zustandsmaschine plus die reine `/proc`-Messung.
//!
//! Frueher lag beides im `tb-stream-audit`-Dienst. Der Smalltalk-Test im
//! `tb-bot` erzeugt aber dieselbe Dauerlast (lokales Whisper, keine GPU) und
//! braucht denselben Waechter. Damit `tb-bot` nicht die ganze Audit-Crate
//! (llm/report/sqlx) nur fuer das Gate hereinzieht, liegt der Waechter hier in
//! einer reinen Leaf-Crate: nur `std` und `tracing`. So gibt es genau **einen**
//! `Lastwaechter` und genau **eine** Messung, geteilt von Audit und Bot.

pub mod last;
pub mod messung;

pub use last::Lastwaechter;
pub use messung::{cpu_prozent, cpu_stand, ram_prozent, CpuStand};
