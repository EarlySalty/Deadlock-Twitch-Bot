use chrono::{NaiveDate, Utc};
use tb_analytics::chat_typen::{
    klassifiziere_modell, klassifiziere_regel, lade_unlabelte, speichere_labels, Nachrichtentyp,
};

use crate::task_supervisor::TaskSupervisor;

const LAUF_INTERVALL_SEKUNDEN: u64 = 60 * 60;
const MAX_NACHRICHTEN_PRO_LAUF: i64 = 20_000;
const PAKET_GROESSE: usize = 40;
const TAGES_KAPPE_MODELLAUFRUFE: u32 = 2_000;

struct TagesZaehler {
    tag: NaiveDate,
    aufrufe: u32,
}

impl TagesZaehler {
    fn new(tag: NaiveDate) -> Self {
        Self { tag, aufrufe: 0 }
    }

    fn rest(&mut self, heute: NaiveDate) -> u32 {
        if heute != self.tag {
            self.tag = heute;
            self.aufrufe = 0;
        }
        TAGES_KAPPE_MODELLAUFRUFE.saturating_sub(self.aufrufe)
    }

    fn zaehle(&mut self) {
        self.aufrufe = self.aufrufe.saturating_add(1);
    }
}

pub fn spawn(supervisor: &TaskSupervisor, pool: sqlx::PgPool) {
    supervisor.spawn("twitch_chat_typen", async move {
        let mut zaehler = TagesZaehler::new(Utc::now().date_naive());
        let mut tick =
            tokio::time::interval(std::time::Duration::from_secs(LAUF_INTERVALL_SEKUNDEN));
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            tick.tick().await;
            if let Err(error) = lauf(&pool, &mut zaehler).await {
                tracing::error!(%error, "Chat-Typen: Lauf fehlgeschlagen");
            }
        }
    });
}

async fn lauf(pool: &sqlx::PgPool, zaehler: &mut TagesZaehler) -> Result<(), sqlx::Error> {
    let unlabelte = lade_unlabelte(pool, MAX_NACHRICHTEN_PRO_LAUF).await?;
    let geladen = unlabelte.len();

    let mut regel_rows: Vec<(i64, Nachrichtentyp, &str, Option<String>)> = Vec::new();
    let mut offen: Vec<(i64, String)> = Vec::new();
    for (id, content, login) in &unlabelte {
        let typ = klassifiziere_regel(content, login);
        if typ == Nachrichtentyp::Other && !content.trim().is_empty() {
            offen.push((*id, content.clone()));
        } else {
            regel_rows.push((*id, typ, "regel", None));
        }
    }
    let regel_anzahl = regel_rows.len();
    if !regel_rows.is_empty() {
        speichere_labels(pool, &regel_rows).await?;
    }

    let mut modell_anzahl = 0usize;
    let mut modell_aufrufe = 0u32;
    for paket in offen.chunks(PAKET_GROESSE) {
        if zaehler.rest(Utc::now().date_naive()) == 0 {
            break;
        }
        match klassifiziere_modell(paket).await {
            Ok((ergebnis, modell)) => {
                zaehler.zaehle();
                modell_aufrufe += 1;
                let rows: Vec<(i64, Nachrichtentyp, &str, Option<String>)> = ergebnis
                    .into_iter()
                    .map(|(id, typ)| (id, typ, "modell", Some(modell.clone())))
                    .collect();
                match speichere_labels(pool, &rows).await {
                    Ok(_) => modell_anzahl += rows.len(),
                    Err(error) => {
                        tracing::warn!(%error, "Chat-Typen: Modell-Labels konnten nicht gespeichert werden")
                    }
                }
            }
            Err(error) => {
                tracing::warn!(%error, "Chat-Typen: Modell-Paket fehlgeschlagen, bleibt für den nächsten Lauf offen")
            }
        }
    }

    let offen_rest = offen.len().saturating_sub(modell_anzahl);
    tracing::info!(
        geladen,
        regel = regel_anzahl,
        modell = modell_anzahl,
        modell_aufrufe,
        offen = offen_rest,
        "Chat-Typen: Lauf abgeschlossen"
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tageskappe_setzt_bei_tageswechsel_zurueck() {
        let heute = NaiveDate::from_ymd_opt(2026, 9, 5).unwrap();
        let mut z = TagesZaehler::new(heute);
        assert_eq!(z.rest(heute), TAGES_KAPPE_MODELLAUFRUFE);
        for _ in 0..TAGES_KAPPE_MODELLAUFRUFE {
            z.zaehle();
        }
        assert_eq!(z.rest(heute), 0);
        let morgen = NaiveDate::from_ymd_opt(2026, 9, 6).unwrap();
        assert_eq!(z.rest(morgen), TAGES_KAPPE_MODELLAUFRUFE);
    }

    #[test]
    fn konstanten_bleiben_im_vertraglichen_rahmen() {
        assert_eq!(PAKET_GROESSE, 40);
        assert_eq!(MAX_NACHRICHTEN_PRO_LAUF, 20_000);
        assert_eq!(TAGES_KAPPE_MODELLAUFRUFE, 2_000);
        assert_eq!(LAUF_INTERVALL_SEKUNDEN, 3_600);
    }
}
