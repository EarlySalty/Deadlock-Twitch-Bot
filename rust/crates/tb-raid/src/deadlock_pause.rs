//! Deadlock-Pause: Mod-Rechte ruhen lassen, wenn ein Partner kein Deadlock mehr
//! streamt, und beim Comeback von allein zurückholen.
//!
//! Ein Partner, der seit Monaten etwas anderes streamt, soll nicht dauerhaft
//! einen fremden Moderator im Kanal behalten. Nach [`DEADLOCK_PAUSE_DAYS`] ohne
//! Deadlock-Stream gibt der Bot deshalb seine Mod-Rechte ab und schreibt dem
//! Streamer, wie er den Bot ganz trennt, falls das Dauerzustand bleibt. Streamt
//! derselbe Kanal wieder Deadlock, moddet sich der Bot wieder ein, sofern der
//! Streamer-Token noch gültig ist.
//!
//! Bewusst getrennt vom Bot-Ban-Lifecycle ([`crate::token_lifecycle`]): dort ist
//! der Kanal kaputt und muss pausieren, hier läuft die Partnerschaft normal
//! weiter und nur die Mod-Rechte ruhen. Beide teilen sich aber den
//! Discord-Port ([`TokenLifecycleNotifier`]) und die Ban-Probe
//! ([`BotBanStatusProbe`]) statt eigene Kanäle aufzumachen.

use std::sync::Arc;

use chrono::{DateTime, SecondsFormat, Utc};
use sqlx::PgPool;

use crate::token_lifecycle::{
    discord_user_id_for, BotBanStatus, BotBanStatusProbe, TokenLifecycleNotifier, BOT_SECTION_URL,
    TOKEN_ERROR_CHANNEL_ID,
};
use crate::util::mask_log_identifier as mask;

/// Ohne Deadlock-Stream über diese Dauer gibt der Bot seine Mod-Rechte ab.
pub const DEADLOCK_PAUSE_DAYS: i64 = 60;

/// Obergrenze an Unmods pro Lauf. Bewusst klein: beim ersten Sweep nach dem
/// Ausrollen ist der ganze Rückstand auf einmal fällig, und jeder Unmod schickt
/// eine DM an einen echten Streamer. Gestaffelt über mehrere Läufe bleibt Zeit,
/// einen Fehler zu bemerken, bevor alle Kanäle betroffen sind.
const MAX_UNMOD_PER_SWEEP: i64 = 5;

/// Obergrenze an Remods pro Lauf. Höher, weil ein Comeback nicht warten soll und
/// die Nachricht eine gute ist.
const MAX_REMOD_PER_SWEEP: i64 = 50;

/// Pause zwischen zwei Helix-Calls im Sweep.
const CALL_DELAY: std::time::Duration = std::time::Duration::from_millis(120);

/// Embed-Farbe für die Admin-Meldungen dieses Moduls (Markengold).
pub const DEADLOCK_PAUSE_COLOR: i64 = 0xC8_A8_6B;

// ---------------------------------------------------------------------------
// Ports
// ---------------------------------------------------------------------------

/// Ausgang eines Unmod-Versuchs, auf das reduziert, was der Sweep entscheiden
/// muss: Rechte weg (oder ohnehin nicht vorhanden) oder eben nicht.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnmodOutcome {
    /// Der Bot ist in diesem Kanal kein Moderator mehr.
    Done,
    /// Twitch hat den Entzug nicht ausgeführt (kein Token, Fehler). Der Kanal
    /// bleibt unmarkiert und wird beim nächsten Sweep erneut versucht.
    Failed,
}

/// Port für den Mod-Entzug. Die echte Implementierung lebt in tb-bot und ist
/// derselbe `HelixModeratorRemover`, den auch das bewusste Trennen im Dashboard
/// benutzt — hier wird kein zweiter Helix-Pfad aufgemacht.
#[async_trait::async_trait]
pub trait DeadlockPauseUnmodPort: Send + Sync {
    async fn unmod_bot(&self, broadcaster_id: &str, twitch_login: &str) -> UnmodOutcome;
}

// ---------------------------------------------------------------------------
// Texte (rein, ohne DB/Netz)
// ---------------------------------------------------------------------------

/// DM an den Streamer, wenn der Bot sich wegen Deadlock-Pause entmoddet hat.
///
/// Die Nachricht darf nicht wie ein Rauswurf klingen. Reihenfolge deshalb: was
/// passiert ist, dann sofort die Entwarnung (Status bleibt, kein Handlungsbedarf),
/// dann die Rückkehr-Zusage samt Bedingung. Das Trennen steht bewusst nur als
/// letzte Zeile: wer nichts tun will, soll es auch nicht lesen müssen.
pub fn user_dm_deadlock_pause_text(twitch_login: &str, pause_days: i64) -> String {
    let months = pause_days / 30;
    format!(
        "💤 **Der Bot ist bei dir kein Mod mehr**\n\n\
         Du hast seit {months} Monaten kein Deadlock gestreamt. Deshalb hat der Bot \
         seine Mod-Rechte in **{twitch_login}** abgegeben. Er soll keine Rechte in \
         deinem Kanal haben, wenn er dort gerade nichts tut.\n\n\
         **Du bleibst Partner und musst nichts machen.**\n\n\
         Streamst du wieder Deadlock, ist er von selbst zurück. Falls nicht, ist \
         meist die Verbindung zu deinem Twitch-Konto abgelaufen; die erneuerst du \
         im Dashboard.\n\n\
         Willst du ihn dauerhaft raus haben: {BOT_SECTION_URL}"
    )
}

/// Admin-Log-Meldung zur Deadlock-Pause. `zurueck` unterscheidet Unmod und Remod.
pub fn admin_deadlock_pause_text(
    twitch_login: &str,
    twitch_user_id: &str,
    zurueck: bool,
) -> (String, String) {
    if zurueck {
        (
            "🔄 Bot nach Deadlock-Comeback wieder gemoddet".to_string(),
            format!(
                "**{twitch_login}** streamt wieder Deadlock, die Mod-Rechte sind zurück.\n\n\
                 Streamer: [{twitch_login}](https://twitch.tv/{twitch_login})\n\
                 User ID: `{twitch_user_id}`"
            ),
        )
    } else {
        (
            "💤 Bot wegen Deadlock-Pause entmoddet".to_string(),
            format!(
                "In **{twitch_login}** lief seit {DEADLOCK_PAUSE_DAYS} Tagen kein \
                 Deadlock-Stream. Der Bot hat seine Mod-Rechte abgegeben; die \
                 Partnerschaft läuft weiter.\n\n\
                 Streamer: [{twitch_login}](https://twitch.tv/{twitch_login})\n\
                 User ID: `{twitch_user_id}`\n\
                 Bei einem Deadlock-Stream moddet er sich automatisch zurück."
            ),
        )
    }
}

// ---------------------------------------------------------------------------
// Reactor
// ---------------------------------------------------------------------------

/// Ergebnis eines Sweep-Laufs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct DeadlockPauseOutcome {
    /// Kanäle, in denen der Bot neu entmoddet wurde.
    pub unmodded: u64,
    /// Kanäle, in denen der Bot nach einem Deadlock-Comeback zurückgeholt wurde.
    pub remodded: u64,
}

impl DeadlockPauseOutcome {
    pub fn any(&self) -> bool {
        self.unmodded > 0 || self.remodded > 0
    }
}

/// Führt beide Richtungen der Deadlock-Pause aus.
pub struct DeadlockPauseReactor<N: TokenLifecycleNotifier> {
    pool: PgPool,
    notifier: N,
    unmod: Arc<dyn DeadlockPauseUnmodPort>,
    /// Remod läuft über die vorhandene Ban-Probe: die setzt den Bot im selben
    /// Call als Moderator ein und meldet `NotBanned`, wenn das geklappt hat.
    remod: Arc<dyn BotBanStatusProbe>,
    pause_days: i64,
}

impl<N: TokenLifecycleNotifier> DeadlockPauseReactor<N> {
    pub fn new(
        pool: PgPool,
        notifier: N,
        unmod: Arc<dyn DeadlockPauseUnmodPort>,
        remod: Arc<dyn BotBanStatusProbe>,
    ) -> Self {
        Self {
            pool,
            notifier,
            unmod,
            remod,
            pause_days: DEADLOCK_PAUSE_DAYS,
        }
    }

    /// Überschreibt die Pausendauer (Tests).
    #[must_use]
    pub fn with_pause_days(mut self, days: i64) -> Self {
        self.pause_days = days.max(1);
        self
    }

    fn iso(dt: DateTime<Utc>) -> String {
        dt.to_rfc3339_opts(SecondsFormat::Secs, false)
    }

    /// Ein kompletter Lauf: erst Comebacks zurückholen, dann Inaktive entmodden.
    ///
    /// Die Reihenfolge ist Absicht. Ein Kanal, der gerade wieder Deadlock
    /// streamt, verlässt die Pause noch im selben Lauf und kann danach nicht
    /// mehr als Unmod-Kandidat auftauchen.
    pub async fn sweep(&self) -> DeadlockPauseOutcome {
        let remodded = self.remod_returned_channels().await;
        let unmodded = self.unmod_idle_channels().await;
        DeadlockPauseOutcome { unmodded, remodded }
    }

    /// Entmoddet Partner ohne Deadlock-Stream innerhalb der Pausendauer.
    ///
    /// Kandidat ist nur, wer aktiv ist, einen gültigen Streamer-Token hat (ohne
    /// den kann Twitch den Entzug gar nicht ausführen) und nicht bereits wegen
    /// Ban oder Block pausiert. Als Referenzzeitpunkt zählt der letzte
    /// Deadlock-Stream.
    ///
    /// Wer in der erfassten Historie **nie** einen Deadlock-Stream hatte, bleibt
    /// bewusst außen vor. Für den ist die Pause die falsche Antwort: sie sagt
    /// "streamst du wieder Deadlock, ist der Bot zurück", obwohl es dort nie ein
    /// "wieder" gab. Solche Kanäle gehören getrennt, und das ist eine
    /// Entscheidung mit Ansage, kein Nebeneffekt eines Sweeps.
    pub async fn unmod_idle_channels(&self) -> u64 {
        let cutoff = (Utc::now() - chrono::Duration::days(self.pause_days))
            .format("%Y-%m-%d")
            .to_string();
        // Alle Zeitvergleiche laufen über `LEFT(x::text, 10)::date`. Grund: die
        // Zeitspalten sind je nach Umgebung `text` oder `timestamptz` und
        // `had_deadlock_in_session` je nach Umgebung `boolean` oder `integer`
        // (Prod führt boolean/timestamptz, das Baseline-Schema integer/text).
        // Ein direkter Vergleich bricht deshalb mit einem Typfehler ab. Der
        // Datumsschnitt ist bei einem 60-Tage-Fenster genau genug.
        let rows = match sqlx::query_as::<_, (String, String)>(
            r#"
            SELECT p.twitch_user_id,
                   LOWER(p.twitch_login) AS twitch_login
              FROM twitch_partners p
              JOIN twitch_raid_auth a
                ON a.twitch_user_id = p.twitch_user_id
             WHERE LOWER(TRIM(COALESCE(p.status, ''))) = 'active'
               AND p.deadlock_pause_unmodded_at IS NULL
               AND COALESCE(NULLIF(TRIM(p.twitch_login), ''), '') <> ''
               AND COALESCE(a.needs_reauth, TRUE) = FALSE
               AND a.access_token_enc IS NOT NULL
               AND OCTET_LENGTH(a.access_token_enc) > 0
               AND LOWER(TRIM(COALESCE(p.technical_pause_reason, '')))
                   NOT IN ('blocked', 'bot_banned')
               -- Der letzte Deadlock-Stream muss existieren UND alt genug sein.
               -- Kein NULL-Fallback auf partnered_at: ohne je einen
               -- Deadlock-Stream ist der Kanal kein Pausen-, sondern ein
               -- Trenn-Fall und bleibt hier unangetastet.
               AND (SELECT MAX(LEFT(s.started_at::text, 10)::date)
                      FROM twitch_stream_sessions s
                     WHERE LOWER(s.streamer_login) = LOWER(p.twitch_login)
                       AND (s.had_deadlock_in_session::text IN ('1', 't', 'true')
                            OR LOWER(COALESCE(s.game_name, '')) = 'deadlock'))
                   < $1::date
             ORDER BY p.twitch_login
             LIMIT $2
            "#,
        )
        .bind(&cutoff)
        .bind(MAX_UNMOD_PER_SWEEP)
        .fetch_all(&self.pool)
        .await
        {
            Ok(rows) => rows,
            Err(error) => {
                tracing::warn!(%error, "Deadlock-Pause: Unmod-Query fehlgeschlagen");
                return 0;
            }
        };

        let mut unmodded = 0u64;
        for (twitch_user_id, twitch_login) in rows {
            match self.unmod.unmod_bot(&twitch_user_id, &twitch_login).await {
                UnmodOutcome::Done => {}
                UnmodOutcome::Failed => {
                    // Nicht markieren: sonst gilt der Kanal als pausiert,
                    // obwohl der Bot seine Rechte behalten hat.
                    tracing::warn!(
                        login = %twitch_login,
                        "Deadlock-Pause: Mod-Entzug fehlgeschlagen, wird erneut versucht"
                    );
                    tokio::time::sleep(CALL_DELAY).await;
                    continue;
                }
            }
            if let Err(error) = self.mark_paused(&twitch_user_id).await {
                tracing::warn!(%error, user = %mask(&twitch_user_id), "Deadlock-Pause: Markierung fehlgeschlagen");
                tokio::time::sleep(CALL_DELAY).await;
                continue;
            }
            unmodded += 1;
            self.notify_pause(&twitch_user_id, &twitch_login, false)
                .await;
            tracing::info!(login = %twitch_login, "Deadlock-Pause: Bot entmoddet");
            tokio::time::sleep(CALL_DELAY).await;
        }
        unmodded
    }

    /// Holt die Mod-Rechte zurück, sobald ein pausierter Kanal wieder Deadlock
    /// gestreamt hat. Voraussetzung ist ein gültiger Streamer-Token; ohne den
    /// bleibt die Pause bestehen, bis der Streamer neu verbindet.
    pub async fn remod_returned_channels(&self) -> u64 {
        let rows = match sqlx::query_as::<_, (String, String)>(
            r#"
            SELECT p.twitch_user_id,
                   LOWER(p.twitch_login) AS twitch_login
              FROM twitch_partners p
              JOIN twitch_raid_auth a
                ON a.twitch_user_id = p.twitch_user_id
             WHERE LOWER(TRIM(COALESCE(p.status, ''))) = 'active'
               AND p.deadlock_pause_unmodded_at IS NOT NULL
               AND COALESCE(a.needs_reauth, TRUE) = FALSE
               AND a.access_token_enc IS NOT NULL
               AND OCTET_LENGTH(a.access_token_enc) > 0
               AND LOWER(TRIM(COALESCE(p.technical_pause_reason, '')))
                   NOT IN ('blocked', 'bot_banned')
               AND EXISTS (
                   SELECT 1
                     FROM twitch_stream_sessions s
                    WHERE LOWER(s.streamer_login) = LOWER(p.twitch_login)
                      AND (s.had_deadlock_in_session::text IN ('1', 't', 'true')
                           OR LOWER(COALESCE(s.game_name, '')) = 'deadlock')
                      -- `>=` statt `>`: der Vergleich läuft auf Tagesbasis
                      -- (siehe unmod_idle_channels), sonst fiele ein Comeback am
                      -- Tag des Unmods durch. Die Session, die den Unmod
                      -- ausgelöst hat, ist zwei Monate alt und trifft nie zu.
                      AND LEFT(s.started_at::text, 10)::date
                          >= LEFT(p.deadlock_pause_unmodded_at, 10)::date
               )
             ORDER BY p.twitch_login
             LIMIT $1
            "#,
        )
        .bind(MAX_REMOD_PER_SWEEP)
        .fetch_all(&self.pool)
        .await
        {
            Ok(rows) => rows,
            Err(error) => {
                tracing::warn!(%error, "Deadlock-Pause: Remod-Query fehlgeschlagen");
                return 0;
            }
        };

        let mut remodded = 0u64;
        for (twitch_user_id, twitch_login) in rows {
            match self
                .remod
                .bot_ban_status(&twitch_user_id, &twitch_login)
                .await
            {
                // Die Probe setzt den Bot im selben Call als Moderator ein.
                BotBanStatus::NotBanned => {}
                BotBanStatus::Banned => {
                    // Der Streamer hat den Bot in der Zwischenzeit gebannt. Das
                    // ist Sache des Bot-Ban-Lifecycles, nicht dieser Pause.
                    tracing::info!(
                        login = %twitch_login,
                        "Deadlock-Pause: Remod übersprungen, Bot ist im Kanal gebannt"
                    );
                    tokio::time::sleep(CALL_DELAY).await;
                    continue;
                }
                BotBanStatus::Unknown => {
                    tracing::info!(
                        login = %twitch_login,
                        "Deadlock-Pause: Remod verschoben, Status unklar"
                    );
                    tokio::time::sleep(CALL_DELAY).await;
                    continue;
                }
            }
            if let Err(error) = self.clear_paused(&twitch_user_id).await {
                tracing::warn!(%error, user = %mask(&twitch_user_id), "Deadlock-Pause: Aufhebung fehlgeschlagen");
                tokio::time::sleep(CALL_DELAY).await;
                continue;
            }
            remodded += 1;
            self.notify_pause(&twitch_user_id, &twitch_login, true)
                .await;
            tracing::info!(login = %twitch_login, "Deadlock-Pause: Bot wieder gemoddet");
            tokio::time::sleep(CALL_DELAY).await;
        }
        remodded
    }

    /// Meldet einen Pausen-Wechsel. Der Streamer bekommt nur beim Unmod eine DM:
    /// dass der Bot nach einem Deadlock-Stream wieder da ist, sieht er selbst,
    /// und eine Nachricht dafür wäre nur Rauschen.
    async fn notify_pause(&self, twitch_user_id: &str, twitch_login: &str, zurueck: bool) {
        if !zurueck {
            if let Some(discord_user_id) =
                discord_user_id_for(&self.pool, twitch_user_id, twitch_login).await
            {
                let text = user_dm_deadlock_pause_text(twitch_login, self.pause_days);
                self.notifier.send_user_dm(&discord_user_id, &text).await;
            }
        }
        let (title, description) = admin_deadlock_pause_text(twitch_login, twitch_user_id, zurueck);
        self.notifier
            .send_admin_embed(TOKEN_ERROR_CHANNEL_ID, &title, &description)
            .await;
    }

    async fn mark_paused(&self, twitch_user_id: &str) -> Result<(), sqlx::Error> {
        let now = Self::iso(Utc::now());
        sqlx::query(
            "UPDATE twitch_partners
                SET deadlock_pause_unmodded_at = $1
              WHERE twitch_user_id = $2",
        )
        .bind(&now)
        .bind(twitch_user_id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn clear_paused(&self, twitch_user_id: &str) -> Result<(), sqlx::Error> {
        sqlx::query(
            "UPDATE twitch_partners
                SET deadlock_pause_unmodded_at = NULL
              WHERE twitch_user_id = $1",
        )
        .bind(twitch_user_id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Die DM muss vier Fragen beantworten, bevor sie zum Trennen kommt: was ist
    /// passiert, verliere ich etwas, muss ich handeln, wie komme ich zurueck.
    /// Steht der Trenn-Link vorher, liest sie sich wie eine Kuendigung.
    #[test]
    fn pause_dm_beantwortet_erst_die_sorgen_dann_den_ausstieg() {
        let text = user_dm_deadlock_pause_text("foo", DEADLOCK_PAUSE_DAYS);
        assert!(text.contains("foo"));

        let bleibt = text
            .find("Du bleibst Partner und musst nichts machen")
            .expect("Entwarnung fehlt");
        let rueckkehr = text.find("von selbst zurück").expect("Rueckkehr fehlt");
        let trennen = text.find(BOT_SECTION_URL).expect("Trenn-Link fehlt");
        assert!(
            bleibt < rueckkehr && rueckkehr < trennen,
            "Reihenfolge muss Entwarnung, Rueckkehr, Trennen sein"
        );
    }

    /// Kein internes Vokabular in einer Streamer-DM: was fuer uns "archiviert",
    /// "Autorisierung" oder "entmoddet" heisst, sagt dem Streamer nichts.
    #[test]
    fn pause_dm_spricht_streamersprache() {
        let text = user_dm_deadlock_pause_text("foo", DEADLOCK_PAUSE_DAYS).to_lowercase();
        for begriff in [
            "archiviert",
            "autorisierung",
            "entmoddet",
            "opt-out",
            "technical",
            "token",
        ] {
            assert!(
                !text.contains(begriff),
                "internes Vokabular '{begriff}' gehoert nicht in die Streamer-DM"
            );
        }
    }

    /// Keine Dashes als Satzzeichen in Texten, die beim Nutzer landen.
    #[test]
    fn nutzertexte_ohne_dash_ersatzzeichen() {
        let texte = [
            user_dm_deadlock_pause_text("foo", DEADLOCK_PAUSE_DAYS),
            admin_deadlock_pause_text("foo", "42", false).1,
            admin_deadlock_pause_text("foo", "42", true).1,
        ];
        for text in texte {
            for zeichen in ['\u{2014}', '\u{2013}', '\u{2015}'] {
                assert!(
                    !text.contains(zeichen),
                    "Dash-Ersatzzeichen in Nutzertext: {text}"
                );
            }
            assert!(!text.contains(" - "), "Spaced Hyphen in Nutzertext: {text}");
            assert!(!text.contains("--"), "Doppel-Hyphen in Nutzertext: {text}");
        }
    }

    #[test]
    fn pause_dm_nennt_die_dauer_in_monaten() {
        let text = user_dm_deadlock_pause_text("foo", 60);
        assert!(text.contains("2 Monaten"), "Text war: {text}");
    }

    #[test]
    fn admin_text_unterscheidet_pause_und_rueckkehr() {
        let (pause_title, pause_body) = admin_deadlock_pause_text("foo", "42", false);
        assert!(pause_title.contains("entmoddet"));
        assert!(pause_body.contains("42"));
        let (back_title, _) = admin_deadlock_pause_text("foo", "42", true);
        assert!(back_title.contains("wieder gemoddet"));
    }

    #[test]
    fn pausendauer_ist_zwei_monate() {
        assert_eq!(DEADLOCK_PAUSE_DAYS, 60);
    }

    #[test]
    fn outcome_any_nur_bei_wirkung() {
        assert!(!DeadlockPauseOutcome::default().any());
        assert!(DeadlockPauseOutcome {
            unmodded: 1,
            remodded: 0
        }
        .any());
        assert!(DeadlockPauseOutcome {
            unmodded: 0,
            remodded: 3
        }
        .any());
    }

    // --- DB-Integration (env-gated via TB_TEST_DATABASE_URL) -----------------
    //
    // Gleiches Muster wie `token_lifecycle`: isoliertes Schema pro Test, ohne
    // gesetzte `TB_TEST_DATABASE_URL` werden die Tests übersprungen.

    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Mutex;

    #[derive(Default)]
    struct RecordingNotifier {
        dms: Mutex<Vec<String>>,
        admin_embeds: AtomicUsize,
    }

    #[async_trait::async_trait]
    impl TokenLifecycleNotifier for Arc<RecordingNotifier> {
        async fn send_admin_embed(
            &self,
            _channel_id: i64,
            _title: &str,
            _description: &str,
        ) -> bool {
            self.admin_embeds.fetch_add(1, Ordering::SeqCst);
            true
        }
        async fn send_user_dm(&self, _discord_user_id: &str, content: &str) -> bool {
            self.dms.lock().unwrap().push(content.to_string());
            true
        }
        async fn revoke_streamer_role(&self, _discord_user_id: &str, _reason: &str) -> bool {
            true
        }
    }

    /// Zählt Unmod-Aufrufe und liefert ein festes Ergebnis.
    struct FakeUnmod {
        outcome: UnmodOutcome,
        calls: Mutex<Vec<String>>,
    }

    #[async_trait::async_trait]
    impl DeadlockPauseUnmodPort for FakeUnmod {
        async fn unmod_bot(&self, _broadcaster_id: &str, twitch_login: &str) -> UnmodOutcome {
            self.calls.lock().unwrap().push(twitch_login.to_string());
            self.outcome
        }
    }

    struct FixedBanStatus(BotBanStatus);

    #[async_trait::async_trait]
    impl BotBanStatusProbe for FixedBanStatus {
        async fn bot_ban_status(&self, _uid: &str, _login: &str) -> BotBanStatus {
            self.0
        }
    }

    fn test_db_url() -> Option<String> {
        std::env::var("TB_TEST_DATABASE_URL")
            .ok()
            .filter(|v| !v.trim().is_empty())
    }

    async fn setup_db(schema: &str) -> PgPool {
        let url = test_db_url().expect("TB_TEST_DATABASE_URL muss gesetzt sein");
        let admin = PgPool::connect(&url).await.expect("Test-DB-Verbindung");
        sqlx::query(&format!("DROP SCHEMA IF EXISTS {schema} CASCADE"))
            .execute(&admin)
            .await
            .unwrap();
        sqlx::query(&format!("CREATE SCHEMA {schema}"))
            .execute(&admin)
            .await
            .unwrap();
        admin.close().await;

        let schema_owned = schema.to_string();
        let pool = sqlx::postgres::PgPoolOptions::new()
            .after_connect(move |conn, _| {
                let schema = schema_owned.clone();
                Box::pin(async move {
                    sqlx::query(&format!("SET search_path TO {schema}"))
                        .execute(conn)
                        .await?;
                    Ok(())
                })
            })
            .connect(&url)
            .await
            .expect("Schema-Pool");

        for ddl in [
            "CREATE TABLE twitch_partners (
                id bigserial PRIMARY KEY, twitch_user_id text, twitch_login text,
                status text DEFAULT 'active', technical_pause_reason text,
                partnered_at text, deadlock_pause_unmodded_at text)",
            "CREATE TABLE twitch_raid_auth (
                twitch_user_id text PRIMARY KEY, twitch_login text,
                needs_reauth boolean DEFAULT false, access_token_enc bytea)",
            // Typen bewusst wie in Prod: timestamptz + boolean. Das
            // Baseline-Schema fuehrt hier text + integer; die Queries muessen
            // mit beidem klarkommen, deshalb testet dieses Schema die Variante,
            // an der ein direkter Vergleich scheitern wuerde.
            "CREATE TABLE twitch_stream_sessions (
                id bigserial PRIMARY KEY, streamer_login text NOT NULL,
                started_at timestamptz NOT NULL, game_name text,
                had_deadlock_in_session boolean DEFAULT false)",
            "CREATE TABLE twitch_streamer_identities (
                twitch_user_id text, twitch_login text, discord_user_id text,
                updated_at timestamptz DEFAULT now())",
        ] {
            sqlx::query(ddl).execute(&pool).await.unwrap();
        }
        pool
    }

    /// Aktiver Partner mit gesundem Token und verknüpfter Discord-ID.
    async fn seed_partner(pool: &PgPool, uid: &str, login: &str, partnered_at: DateTime<Utc>) {
        sqlx::query(
            "INSERT INTO twitch_partners (twitch_user_id, twitch_login, partnered_at)
             VALUES ($1, $2, $3)",
        )
        .bind(uid)
        .bind(login)
        .bind(partnered_at.to_rfc3339_opts(SecondsFormat::Secs, false))
        .execute(pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO twitch_raid_auth (twitch_user_id, twitch_login, needs_reauth, access_token_enc)
             VALUES ($1, $2, false, $3)",
        )
        .bind(uid)
        .bind(login)
        .bind(vec![1_u8, 2, 3])
        .execute(pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO twitch_streamer_identities (twitch_user_id, twitch_login, discord_user_id)
             VALUES ($1, $2, '4711')",
        )
        .bind(uid)
        .bind(login)
        .execute(pool)
        .await
        .unwrap();
    }

    async fn seed_session(pool: &PgPool, login: &str, at: DateTime<Utc>, deadlock: bool) {
        sqlx::query(
            "INSERT INTO twitch_stream_sessions (streamer_login, started_at, game_name, had_deadlock_in_session)
             VALUES ($1, $2, $3, $4)",
        )
        .bind(login)
        .bind(at)
        .bind(if deadlock { "Deadlock" } else { "Dota 2" })
        .bind(deadlock)
        .execute(pool)
        .await
        .unwrap();
    }

    fn reactor_with(
        pool: PgPool,
        notifier: Arc<RecordingNotifier>,
        unmod: Arc<FakeUnmod>,
        status: BotBanStatus,
    ) -> DeadlockPauseReactor<Arc<RecordingNotifier>> {
        DeadlockPauseReactor::new(pool, notifier, unmod, Arc::new(FixedBanStatus(status)))
    }

    fn fake_unmod(outcome: UnmodOutcome) -> Arc<FakeUnmod> {
        Arc::new(FakeUnmod {
            outcome,
            calls: Mutex::new(Vec::new()),
        })
    }

    #[tokio::test]
    async fn unmod_greift_erst_nach_der_pausendauer() {
        if test_db_url().is_none() {
            eprintln!("SKIP: TB_TEST_DATABASE_URL nicht gesetzt");
            return;
        }
        let pool = setup_db("dlp_unmod").await;
        let long_ago = Utc::now() - chrono::Duration::days(400);

        // Kanal A: letzter Deadlock-Stream vor 90 Tagen → fällig.
        seed_partner(&pool, "1", "alt", long_ago).await;
        seed_session(&pool, "alt", Utc::now() - chrono::Duration::days(90), true).await;
        // Kanal B: letzter Deadlock-Stream vor 10 Tagen → bleibt Mod.
        seed_partner(&pool, "2", "frisch", long_ago).await;
        seed_session(
            &pool,
            "frisch",
            Utc::now() - chrono::Duration::days(10),
            true,
        )
        .await;
        // Kanal C: hatte frueher Deadlock, streamt jetzt taeglich etwas
        // anderes. Der Realfall, auf den die Pause zielt.
        seed_partner(&pool, "3", "andersspiel", long_ago).await;
        seed_session(
            &pool,
            "andersspiel",
            Utc::now() - chrono::Duration::days(120),
            true,
        )
        .await;
        seed_session(
            &pool,
            "andersspiel",
            Utc::now() - chrono::Duration::days(1),
            false,
        )
        .await;

        let notifier = Arc::new(RecordingNotifier::default());
        let unmod = fake_unmod(UnmodOutcome::Done);
        let reactor = reactor_with(
            pool.clone(),
            notifier.clone(),
            unmod.clone(),
            BotBanStatus::NotBanned,
        );

        assert_eq!(reactor.unmod_idle_channels().await, 2);
        let mut calls = unmod.calls.lock().unwrap().clone();
        calls.sort();
        assert_eq!(calls, vec!["alt".to_string(), "andersspiel".to_string()]);

        // Genau die beiden sind markiert.
        let marked: Vec<String> = sqlx::query_scalar(
            "SELECT twitch_login FROM twitch_partners
              WHERE deadlock_pause_unmodded_at IS NOT NULL ORDER BY twitch_login",
        )
        .fetch_all(&pool)
        .await
        .unwrap();
        assert_eq!(marked, vec!["alt".to_string(), "andersspiel".to_string()]);

        // Jeder bekommt genau eine DM, personalisiert und mit dem Trennweg.
        let dms = notifier.dms.lock().unwrap().clone();
        assert_eq!(dms.len(), 2);
        assert!(dms.iter().all(|d| d.contains(BOT_SECTION_URL)));
        assert!(dms.iter().any(|d| d.contains("alt")));
        assert!(dms.iter().any(|d| d.contains("andersspiel")));

        // Zweiter Lauf ist ein No-op: der Marker dedupliziert.
        assert_eq!(reactor.unmod_idle_channels().await, 0);
        assert_eq!(notifier.dms.lock().unwrap().len(), 2);
    }

    /// Ein Kanal ohne jeden Deadlock-Stream ist ein Trenn-Fall, kein Pausen-Fall.
    /// Die Pausen-DM verspricht "streamst du wieder Deadlock, ist der Bot
    /// zurueck" und waere dort schlicht falsch.
    #[tokio::test]
    async fn kanal_ohne_je_einen_deadlock_stream_wird_nicht_pausiert() {
        if test_db_url().is_none() {
            eprintln!("SKIP: TB_TEST_DATABASE_URL nicht gesetzt");
            return;
        }
        let pool = setup_db("dlp_nie_deadlock").await;
        let long_ago = Utc::now() - chrono::Duration::days(400);

        // Streamt aktiv, aber nie Deadlock.
        seed_partner(&pool, "1", "anderesspiel", long_ago).await;
        seed_session(
            &pool,
            "anderesspiel",
            Utc::now() - chrono::Duration::days(2),
            false,
        )
        .await;
        // Gar keine Sessions erfasst.
        seed_partner(&pool, "2", "niegesehen", long_ago).await;
        // Kontrolle: hatte Deadlock, aber vor langer Zeit.
        seed_partner(&pool, "3", "frueher", long_ago).await;
        seed_session(
            &pool,
            "frueher",
            Utc::now() - chrono::Duration::days(90),
            true,
        )
        .await;

        let notifier = Arc::new(RecordingNotifier::default());
        let unmod = fake_unmod(UnmodOutcome::Done);
        let reactor = reactor_with(
            pool.clone(),
            notifier.clone(),
            unmod.clone(),
            BotBanStatus::NotBanned,
        );

        assert_eq!(reactor.unmod_idle_channels().await, 1);
        assert_eq!(
            unmod.calls.lock().unwrap().clone(),
            vec!["frueher".to_string()],
            "nur der Kanal mit echter Deadlock-Historie wird pausiert"
        );
        assert_eq!(notifier.dms.lock().unwrap().len(), 1);
    }

    /// Ein fehlgeschlagener Entzug darf nicht als Pause gelten, sonst behält der
    /// Bot seine Rechte und niemand versucht es noch einmal.
    #[tokio::test]
    async fn fehlgeschlagener_unmod_markiert_nicht_und_meldet_nichts() {
        if test_db_url().is_none() {
            eprintln!("SKIP: TB_TEST_DATABASE_URL nicht gesetzt");
            return;
        }
        let pool = setup_db("dlp_unmod_fail").await;
        seed_partner(&pool, "1", "alt", Utc::now() - chrono::Duration::days(400)).await;
        seed_session(&pool, "alt", Utc::now() - chrono::Duration::days(90), true).await;

        let notifier = Arc::new(RecordingNotifier::default());
        let reactor = reactor_with(
            pool.clone(),
            notifier.clone(),
            fake_unmod(UnmodOutcome::Failed),
            BotBanStatus::NotBanned,
        );

        assert_eq!(reactor.unmod_idle_channels().await, 0);
        let marked: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM twitch_partners WHERE deadlock_pause_unmodded_at IS NOT NULL",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(marked, 0);
        assert!(notifier.dms.lock().unwrap().is_empty());
    }

    /// Ohne gültigen Token kann Twitch weder entziehen noch einsetzen. Solche
    /// Kanäle bleiben in beiden Richtungen unangetastet.
    #[tokio::test]
    async fn kanaele_ohne_gueltigen_token_bleiben_aussen_vor() {
        if test_db_url().is_none() {
            eprintln!("SKIP: TB_TEST_DATABASE_URL nicht gesetzt");
            return;
        }
        let pool = setup_db("dlp_token").await;
        seed_partner(
            &pool,
            "1",
            "kaputt",
            Utc::now() - chrono::Duration::days(400),
        )
        .await;
        seed_session(
            &pool,
            "kaputt",
            Utc::now() - chrono::Duration::days(90),
            true,
        )
        .await;
        sqlx::query("UPDATE twitch_raid_auth SET needs_reauth = true")
            .execute(&pool)
            .await
            .unwrap();

        let unmod = fake_unmod(UnmodOutcome::Done);
        let reactor = reactor_with(
            pool.clone(),
            Arc::new(RecordingNotifier::default()),
            unmod.clone(),
            BotBanStatus::NotBanned,
        );
        assert_eq!(reactor.unmod_idle_channels().await, 0);
        assert!(unmod.calls.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn deadlock_comeback_holt_die_mod_rechte_zurueck() {
        if test_db_url().is_none() {
            eprintln!("SKIP: TB_TEST_DATABASE_URL nicht gesetzt");
            return;
        }
        let pool = setup_db("dlp_remod").await;
        let unmodded_at = Utc::now() - chrono::Duration::days(5);
        seed_partner(
            &pool,
            "1",
            "zurueck",
            Utc::now() - chrono::Duration::days(400),
        )
        .await;
        seed_partner(
            &pool,
            "2",
            "immernoch",
            Utc::now() - chrono::Duration::days(400),
        )
        .await;
        sqlx::query("UPDATE twitch_partners SET deadlock_pause_unmodded_at = $1")
            .bind(unmodded_at.to_rfc3339_opts(SecondsFormat::Secs, false))
            .execute(&pool)
            .await
            .unwrap();
        // Nur "zurueck" hat nach dem Unmod wieder Deadlock gestreamt.
        seed_session(
            &pool,
            "zurueck",
            Utc::now() - chrono::Duration::days(1),
            true,
        )
        .await;
        // "immernoch" streamt, aber etwas anderes.
        seed_session(
            &pool,
            "immernoch",
            Utc::now() - chrono::Duration::days(1),
            false,
        )
        .await;

        let notifier = Arc::new(RecordingNotifier::default());
        let reactor = reactor_with(
            pool.clone(),
            notifier.clone(),
            fake_unmod(UnmodOutcome::Done),
            BotBanStatus::NotBanned,
        );

        assert_eq!(reactor.remod_returned_channels().await, 1);
        let still_paused: Vec<String> = sqlx::query_scalar(
            "SELECT twitch_login FROM twitch_partners
              WHERE deadlock_pause_unmodded_at IS NOT NULL",
        )
        .fetch_all(&pool)
        .await
        .unwrap();
        assert_eq!(still_paused, vec!["immernoch".to_string()]);
        // Die Rückkehr wird nicht angekündigt: dass der Bot wieder da ist, sieht
        // der Streamer selbst. Nur das Admin-Log bekommt eine Zeile.
        assert!(
            notifier.dms.lock().unwrap().is_empty(),
            "der Remod darf den Streamer nicht anschreiben"
        );
        assert_eq!(notifier.admin_embeds.load(Ordering::SeqCst), 1);
    }

    /// Wenn der Streamer den Bot in der Pause gebannt hat, ist das Sache des
    /// Bot-Ban-Lifecycles. Die Pause hebt sich dann nicht auf.
    #[tokio::test]
    async fn remod_bleibt_aus_wenn_der_bot_inzwischen_gebannt_ist() {
        if test_db_url().is_none() {
            eprintln!("SKIP: TB_TEST_DATABASE_URL nicht gesetzt");
            return;
        }
        let pool = setup_db("dlp_remod_ban").await;
        seed_partner(
            &pool,
            "1",
            "gebannt",
            Utc::now() - chrono::Duration::days(400),
        )
        .await;
        sqlx::query("UPDATE twitch_partners SET deadlock_pause_unmodded_at = $1")
            .bind(
                (Utc::now() - chrono::Duration::days(5))
                    .to_rfc3339_opts(SecondsFormat::Secs, false),
            )
            .execute(&pool)
            .await
            .unwrap();
        seed_session(
            &pool,
            "gebannt",
            Utc::now() - chrono::Duration::days(1),
            true,
        )
        .await;

        let notifier = Arc::new(RecordingNotifier::default());
        let reactor = reactor_with(
            pool.clone(),
            notifier.clone(),
            fake_unmod(UnmodOutcome::Done),
            BotBanStatus::Banned,
        );
        assert_eq!(reactor.remod_returned_channels().await, 0);
        let paused: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM twitch_partners WHERE deadlock_pause_unmodded_at IS NOT NULL",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(paused, 1);
        assert!(notifier.dms.lock().unwrap().is_empty());
    }

    /// Reihenfolge im Sweep: ein Comeback verlässt die Pause, bevor der
    /// Unmod-Teil läuft, und darf im selben Lauf nicht wieder entmoddet werden.
    #[tokio::test]
    async fn sweep_remoddet_vor_dem_unmodden() {
        if test_db_url().is_none() {
            eprintln!("SKIP: TB_TEST_DATABASE_URL nicht gesetzt");
            return;
        }
        let pool = setup_db("dlp_sweep").await;
        seed_partner(
            &pool,
            "1",
            "comeback",
            Utc::now() - chrono::Duration::days(400),
        )
        .await;
        sqlx::query("UPDATE twitch_partners SET deadlock_pause_unmodded_at = $1")
            .bind(
                (Utc::now() - chrono::Duration::days(5))
                    .to_rfc3339_opts(SecondsFormat::Secs, false),
            )
            .execute(&pool)
            .await
            .unwrap();
        seed_session(
            &pool,
            "comeback",
            Utc::now() - chrono::Duration::days(1),
            true,
        )
        .await;

        let unmod = fake_unmod(UnmodOutcome::Done);
        let reactor = reactor_with(
            pool.clone(),
            Arc::new(RecordingNotifier::default()),
            unmod.clone(),
            BotBanStatus::NotBanned,
        );
        let outcome = reactor.sweep().await;
        assert_eq!(outcome.remodded, 1);
        assert_eq!(outcome.unmodded, 0);
        assert!(unmod.calls.lock().unwrap().is_empty());
    }
}
