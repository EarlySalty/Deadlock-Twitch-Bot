//! Last-Gate fuer die Auswertung.
//!
//! Der Waechter und seine Messung liegen seit dem Zusammenlegen mit dem
//! Smalltalk-Test in der reinen Leaf-Crate [`tb_load`]. Dieses Modul reicht
//! ihn nur weiter, damit die bestehenden `crate::last::...`-Pfade dieses
//! Dienstes unveraendert bleiben. Es gibt genau einen `Lastwaechter` im Repo.

pub use tb_load::last::*;
