//! Die eine Plan-Stufe, an der alle Sperren haengen.
//!
//! Vorher lag die Paywall in zwei Formen vor: das Entitlement-Flag `"analytics"`
//! (serverseitig, aber nur an einem Teil der Handler) und der Rest nur im
//! Frontend, also per direktem API-Aufruf umgehbar. Hier steht jetzt ein
//! einziges Praedikat: [`plan_stufe`] loest den Plan eines Streamers auf und
//! liefert die Stufe. Ein Handler fragt `stufe >= Stufe::Plus` und ist fertig.
//!
//! Kein Entitlement-Graph: die Entitlement-Strings aus [`crate::plan`] bleiben
//! bestehen (sie stehen so in der DB und im Chat-Pfad), sind aber nur noch die
//! Ableitung, nicht die Entscheidungsgrundlage.
//!
//! Spec: `.tasks/2026-08-23-pricing-drei-stufen/SPEC.md` (M1).

use sqlx::PgPool;

/// Die drei Stufen des Katalogs, aufsteigend geordnet.
///
/// `PartialOrd`/`Ord` folgen der Deklarationsreihenfolge, deshalb reicht
/// `stufe >= Stufe::Plus` als Gate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Stufe {
    /// Netzwerk Free: vollwertig, aber nur der letzte Stream.
    Free,
    /// Netzwerk Plus: voller Verlauf, Vergleiche, KI, Coaching.
    Plus,
    /// Creator Pro: alles aus Plus, dazu Vorrang bei Support und neuen
    /// Funktionen. Steht im Katalog auf `buchbar = false`.
    Pro,
}

impl Stufe {
    /// Maschinenlesbarer Name fuer JSON-Antworten.
    pub fn as_str(self) -> &'static str {
        match self {
            Stufe::Free => "free",
            Stufe::Plus => "plus",
            Stufe::Pro => "pro",
        }
    }

    /// Anzeigename fuer den Nutzer.
    pub fn anzeigename(self) -> &'static str {
        match self {
            Stufe::Free => "Netzwerk Free",
            Stufe::Plus => "Netzwerk Plus",
            Stufe::Pro => "Creator Pro",
        }
    }

    /// `true` ab Netzwerk Plus.
    pub fn hat_plus(self) -> bool {
        self >= Stufe::Plus
    }

    /// `true` ab Creator Pro.
    pub fn hat_pro(self) -> bool {
        self >= Stufe::Pro
    }
}

/// Stufe einer Plan-ID.
///
/// Die drei aktuellen IDs stehen im Katalog; die acht Vorgaenger-IDs stehen noch
/// in der DB und werden hier eingeordnet, damit niemand durch den Umbau Zugang
/// verliert. Zuordnung laut Spec M4: `raid_free` bleibt Free, alles was frueher
/// Geld gekostet hat (inklusive `analytics_trial`) wird Plus. Unbekanntes faellt
/// auf Free.
pub fn stufe_fuer_plan(plan_id: &str) -> Stufe {
    match plan_id.trim() {
        "pro" => Stufe::Pro,
        "plus"
        | "chat_quiet"
        | "raid_boost"
        | "analysis_dashboard"
        | "bundle_chat_quiet_raid_boost"
        | "bundle_werbefrei_analyse"
        | "bundle_komplett"
        | "bundle_analysis_raid_boost"
        | "analytics_trial" => Stufe::Plus,
        _ => Stufe::Free,
    }
}

/// `true`, wenn die Plan-ID einen **bezahlten** Zugang bezeichnet.
///
/// Eine Stelle statt zweier Namenslisten: gefragt wird der Katalog
/// ([`crate::billing::catalog::is_paid_plan_id`], also jeder Eintrag mit Preis),
/// und zusaetzlich gilt jede Alt-ID als bezahlt, die [`stufe_fuer_plan`] auf
/// mindestens Plus hebt. Die acht Vorgaenger-Plaene stehen nicht mehr im
/// Katalog, haben aber Geld gekostet. Kommt eine neue Stufe in den Katalog, ist
/// sie hier sofort mit drin, ohne dass jemand eine Liste nachpflegen muss.
///
/// Der Trial ist ausdruecklich **nicht** bezahlt: er ist ein befristetes
/// Geschenk, sonst wuerde er sich selbst blockieren.
pub fn ist_bezahlter_plan(plan_id: &str) -> bool {
    let id = plan_id.trim();
    if id.is_empty() || id == crate::trial::ANALYTICS_TRIAL_PLAN_ID {
        return false;
    }
    crate::billing::catalog::is_paid_plan_id(id) || stufe_fuer_plan(id).hat_plus()
}

/// `true`, wenn eine Sperre auf `verlangt` heute ueberhaupt greifen darf.
///
/// **Das ist die eine Stelle, an der jede Stufen-Sperre haengt.** Eine Sperre
/// auf eine Stufe, die niemand kaufen kann, nimmt bestehenden Partnern eine
/// heute funktionierende Funktion weg und laesst ihnen keinen Weg zurueck:
/// `checkout_start_handler` schickt jeden Kaufversuch fuer eine nicht buchbare
/// Stufe auf `/twitch/pricing` zurueck, nur ein Admin-Geschenk kaeme noch durch.
/// Deshalb fragt die Sperre den Katalog, ob die verlangte Stufe `buchbar` ist.
/// Solange Creator Pro dort `false` steht, laeuft alles wie vor dem Umbau;
/// sobald die Stufe kaufbar wird, schaltet sich jede daran haengende Sperre von
/// allein scharf, ohne Codeaenderung, ohne Flag und ohne Konfiguration.
pub fn sperre_greift(verlangt: Stufe) -> bool {
    crate::billing::catalog::find_plan(verlangt.as_str()).is_some_and(|plan| plan.buchbar)
}

/// Loest die Stufe eines Streamers aus der DB auf.
///
/// Nutzt denselben Resolver wie das Dashboard
/// ([`crate::plan::resolve_plan_snapshot`]): Manual-Override vor Stripe-Abo vor
/// Default, inklusive Ablaufpruefung und Trial-Grant. `user_id` darf leer sein,
/// dann laeuft die Aufloesung nur ueber den Login.
pub async fn plan_stufe(pool: &PgPool, streamer: &str) -> Result<Stufe, sqlx::Error> {
    plan_stufe_mit_user_id(pool, streamer, "").await
}

/// Wie [`plan_stufe`], aber mit bekannter `twitch_user_id`.
///
/// Wer die User-ID hat, sollte sie mitgeben: ein Manual-Override, der nur per
/// User-ID eingetragen ist, wird sonst nicht gefunden.
pub async fn plan_stufe_mit_user_id(
    pool: &PgPool,
    login: &str,
    user_id: &str,
) -> Result<Stufe, sqlx::Error> {
    let snapshot = crate::plan::resolve_plan_snapshot(pool, login, user_id).await?;
    Ok(stufe_fuer_plan(snapshot.plan_id))
}

// ── Abgeleitete Grenzen ─────────────────────────────────────────────────────

/// Wie viele Clips die Stufe im Monat erlaubt. `None` heisst unbegrenzt.
///
/// Die Zahlen sind der Unterbau fuer spaeter. Ob sie jemanden **sperren**,
/// entscheidet [`sperre_greift`] beim Bau des [`ClipKontingent`]: der Ausweg aus
/// dem Kontingent ist Creator Pro, und solange die Stufe nicht buchbar ist,
/// wird nur gezaehlt.
pub fn clip_monatslimit(stufe: Stufe) -> Option<u32> {
    match stufe {
        Stufe::Free => Some(3),
        Stufe::Plus => Some(10),
        Stufe::Pro => None,
    }
}

/// `true`, wenn die Stufe automatisches Posten auf TikTok, Instagram und
/// YouTube nutzen darf.
///
/// Die Grenze ist Creator Pro, aber sie greift nur, wenn Pro auch kaufbar ist
/// (siehe [`sperre_greift`]). Automatisches Posten laeuft heute fuer jeden
/// freigeschalteten Partner, und ein Partner, der es verliert, koennte es nicht
/// zurueckkaufen. Wird Pro buchbar, gilt die Pro-Grenze von allein.
pub fn auto_posting_erlaubt(stufe: Stufe) -> bool {
    !sperre_greift(Stufe::Pro) || stufe.hat_pro()
}

/// Wie viele Tage Verlauf die Stufe sieht.
///
/// Free bekommt keine 403-Wand, sondern ein kurzes Fenster: der letzte Stream.
/// Praktisch sind das die letzten `FREE_VERLAUF_TAGE` Tage; wer seltener
/// streamt, sieht seinen letzten Stream trotzdem, weil die Handler das Fenster
/// zusaetzlich bis zum letzten Stream aufziehen. Ab Plus ist der Verlauf voll.
pub fn verlauf_tage_limit(stufe: Stufe) -> Option<i64> {
    if stufe.hat_plus() {
        None
    } else {
        Some(FREE_VERLAUF_TAGE)
    }
}

/// Mindestfenster der Gratis-Stufe in Tagen (ein Stream-Tag plus Puffer, damit
/// eine ueber Mitternacht laufende Session komplett drin ist).
pub const FREE_VERLAUF_TAGE: i64 = 2;

/// Obergrenze des Gratis-Fensters. Wer ein halbes Jahr nicht gestreamt hat,
/// bekommt kein halbes Jahr Verlauf geschenkt.
pub const FREE_FENSTER_MAX_TAGE: i64 = 90;

/// Wie viele Tage zurueck die Gratis-Stufe fuer diesen Streamer sehen darf.
///
/// Free bekommt "den letzten Stream", nicht "die letzten zwei Tage": wer
/// unregelmaessig streamt, saehe sonst eine leere Seite. Deshalb zieht das
/// Fenster bis zum Beginn der letzten Session auf, mindestens
/// [`FREE_VERLAUF_TAGE`], hoechstens [`FREE_FENSTER_MAX_TAGE`]. Ohne Session
/// oder bei DB-Fehler gilt das Mindestfenster.
pub async fn freies_fenster_tage(pool: &PgPool, streamer: &str) -> i64 {
    let login = streamer.trim().to_lowercase();
    if login.is_empty() {
        return FREE_VERLAUF_TAGE;
    }
    let Some((_, started_at)) = letzte_beendete_session(pool, &login).await else {
        return FREE_VERLAUF_TAGE;
    };
    let tage = (chrono::Utc::now() - started_at).num_days() + 1;
    tage.clamp(FREE_VERLAUF_TAGE, FREE_FENSTER_MAX_TAGE)
}

/// Die eine Definition von "letzter Stream" fuer die ganze Paywall. Sie haengt
/// an zwei Stellen: am Gratis-Fenster ([`freies_fenster_tage`]) und an der
/// Klemme der Session-Detailansicht (`session_detail::letzte_session_klemme`
/// ueber `last_session::latest_ended_session`). Vorher nahm das Fenster die
/// zuletzt **begonnene** Session und die Klemme die zuletzt **beendete**; laeuft
/// gerade ein Stream, liess die Klemme damit eine Session durch, die ausserhalb
/// des Fensters lag. Laufende Sessions zaehlen hier nicht, sie liegen ohnehin
/// immer im Fenster (das reicht bis jetzt).
///
/// Leerer `login` heisst Wildcard (global letzte beendete Session); das nutzt
/// nur der privilegierte Overview-Pfad. `None`, wenn es keine beendete Session
/// gibt oder die Abfrage scheitert.
pub async fn letzte_beendete_session(
    pool: &PgPool,
    login: &str,
) -> Option<(i64, chrono::DateTime<chrono::Utc>)> {
    let login = login.trim().to_lowercase();
    let sql = format!(
        "SELECT s.id, s.started_at \
         FROM twitch_stream_sessions s \
         WHERE s.ended_at IS NOT NULL AND s.started_at IS NOT NULL \
           AND ($1 = '' OR LOWER(s.streamer_login) = $1){} \
         ORDER BY s.started_at DESC \
         LIMIT 1",
        crate::overview::GEISTER_FILTER
    );
    match sqlx::query_as::<_, (i64, chrono::DateTime<chrono::Utc>)>(&sql)
        .bind(&login)
        .fetch_optional(pool)
        .await
    {
        Ok(row) => row,
        Err(error) => {
            tracing::warn!(%error, login, "letzte beendete Session nicht ladbar");
            None
        }
    }
}

/// Fenster der Stufe fuer diesen Streamer und die Nachfrage nach `angefragt`
/// Tagen.
///
/// Gibt `(effektive_tage, gekuerzt)` zurueck. Ab Plus bleibt die Nachfrage
/// unangetastet; unter Plus wird auf [`freies_fenster_tage`] geklemmt.
pub async fn verlauf_tage_fuer(
    pool: &PgPool,
    stufe: Stufe,
    streamer: &str,
    angefragt: i64,
) -> (i64, bool) {
    if stufe.hat_plus() {
        return (angefragt, false);
    }
    let fenster = freies_fenster_tage(pool, streamer).await;
    if angefragt > fenster {
        (fenster, true)
    } else {
        (angefragt, false)
    }
}

/// Klemmt eine angefragte Tageszahl auf das Fenster der Stufe.
///
/// Gibt `(effektive_tage, wurde_gekuerzt)` zurueck. Ab Plus bleibt der Wert
/// unveraendert.
pub fn verlauf_tage_klemmen(stufe: Stufe, angefragt: i64) -> (i64, bool) {
    match verlauf_tage_limit(stufe) {
        Some(limit) if angefragt > limit => (limit, true),
        _ => (angefragt, false),
    }
}

/// Klemmt eine angefragte Monatszahl auf das Fenster der Stufe.
pub fn verlauf_monate_klemmen(stufe: Stufe, angefragt: i64) -> (i64, bool) {
    if stufe.hat_plus() || angefragt <= 1 {
        (angefragt, false)
    } else {
        (1, true)
    }
}

/// Clip-Kontingent eines Streamers im laufenden Kalendermonat.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClipKontingent {
    /// Stufe, aus der die Grenze stammt.
    pub stufe: Stufe,
    /// Bereits verbrauchte Clips in diesem Monat (verworfene zaehlen nicht mit).
    pub genutzt: i64,
    /// Monatsgrenze; `None` heisst unbegrenzt.
    pub limit: Option<u32>,
    /// Ob die Grenze heute jemanden sperrt.
    ///
    /// `false`, solange der Ausweg (Creator Pro) nicht kaufbar ist. Gezaehlt
    /// wird trotzdem weiter, damit die Zahlen stimmen, wenn das Kontingent
    /// spaeter verkauft wird.
    pub erzwungen: bool,
}

impl ClipKontingent {
    /// Wie viele Clips noch gehen. `None` heisst unbegrenzt.
    ///
    /// Auch `None`, solange die Grenze nicht erzwungen wird: Aufrufer klemmen
    /// damit nichts (z. B. die Fetch-Menge) und sperren nichts.
    pub fn rest(&self) -> Option<i64> {
        if !self.erzwungen {
            return None;
        }
        self.limit.map(|limit| (limit as i64 - self.genutzt).max(0))
    }

    /// `true`, wenn noch mindestens ein Clip frei ist.
    pub fn frei(&self) -> bool {
        self.rest().is_none_or(|rest| rest > 0)
    }

    /// JSON-Block fuer Antworten und Fehlermeldungen.
    ///
    /// `limit` und `rest` bleiben `null`, solange die Grenze nicht erzwungen
    /// wird: das Dashboard soll keine Schranke anzeigen, die es nicht gibt.
    /// `genutzt` steht immer da, die Zaehlung laeuft weiter.
    pub fn als_json(&self) -> serde_json::Value {
        serde_json::json!({
            "plan_stufe": self.stufe.as_str(),
            "genutzt": self.genutzt,
            "limit": self.erzwungen.then_some(self.limit).flatten(),
            "rest": self.rest(),
            "erzwungen": self.erzwungen,
        })
    }
}

/// Zaehlt, was ein Streamer in diesem Kalendermonat vom Kontingent verbraucht hat.
///
/// Gezaehlt wird die Aufnahme in unsere DB (`kontingent_verbraucht_at`), nicht
/// `created_at`: das ist bei gefetchten Clips der Zeitstempel von Twitch. Ein
/// Streamer mit lauter Clips aus dem Vormonat haette sonst dauerhaft 0, und
/// Zuschauer-Clips aus diesem Monat haetten sein Kontingent aufgebraucht, bevor
/// er selbst etwas angeklickt hat.
///
/// Gesetzt wird die Spalte nur dort, wo der Streamer die Aufnahme selbst
/// ausloest (eigener Upload, eigener Clip-Fetch im Dashboard). Der
/// Hintergrund-Fetcher laesst sie NULL, seine Clips zaehlen nicht.
///
/// Verworfene Clips (`discarded_at`) zaehlen nicht, sonst waere ein
/// Fehlgriff dauerhaft teuer. Bei DB-Fehler `0`: eine kaputte Zaehlung darf
/// niemandem den Dienst sperren, die Sperre selbst haengt an der Stufe.
pub async fn clip_verbrauch_diesen_monat(pool: &PgPool, streamer: &str) -> i64 {
    let login = streamer.trim().to_lowercase();
    if login.is_empty() {
        return 0;
    }
    sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*)::bigint FROM twitch_clips_social_media \
           WHERE LOWER(streamer_login) = $1 \
             AND discarded_at IS NULL \
             AND kontingent_verbraucht_at >= DATE_TRUNC('month', NOW())",
    )
    .bind(&login)
    .fetch_one(pool)
    .await
    .unwrap_or(0)
}

/// Kontingent eines Streamers fuer die gegebene Stufe.
///
/// Ob die Grenze sperrt, entscheidet [`sperre_greift`] fuer die Stufe, die aus
/// dem Kontingent herausfuehrt (Creator Pro, unbegrenzt). Solange Pro nicht
/// buchbar ist, waere die Grenze eine Einbahnstrasse: bestehende, heute
/// unbegrenzte Funktionen wuerden zugemacht, ohne dass jemand sie aufkaufen
/// koennte. Gezaehlt wird trotzdem.
pub async fn clip_kontingent(pool: &PgPool, stufe: Stufe, streamer: &str) -> ClipKontingent {
    let limit = clip_monatslimit(stufe);
    let genutzt = if limit.is_some() {
        clip_verbrauch_diesen_monat(pool, streamer).await
    } else {
        // Pro hat kein Limit, dann braucht es auch keine Zaehlabfrage.
        0
    };
    ClipKontingent {
        stufe,
        genutzt,
        limit,
        erzwungen: sperre_greift(Stufe::Pro),
    }
}

/// Hinweisblock, der einer gekuerzten Antwort beiliegt, damit das Dashboard die
/// Sperre anzeigen kann, ohne selbst zu rechnen.
pub fn kuerzungs_hinweis(stufe: Stufe, effektive_tage: i64) -> serde_json::Value {
    serde_json::json!({
        "plan_stufe": stufe.as_str(),
        "gekuerzt": true,
        "fenster_tage": effektive_tage,
        "benoetigte_stufe": Stufe::Plus.as_str(),
        "hinweis": "Netzwerk Free zeigt deinen letzten Stream. Mit Netzwerk Plus siehst du deinen vollen Verlauf.",
    })
}

/// Haengt [`kuerzungs_hinweis`] an eine JSON-Antwort, wenn gekuerzt wurde.
///
/// Objekt-Antworten bekommen das Feld `plan_limit`; alles andere (Arrays,
/// Skalare) bleibt unveraendert, weil dort kein Platz dafuer ist.
pub fn hinweis_anhaengen(
    mut payload: serde_json::Value,
    stufe: Stufe,
    effektive_tage: i64,
    gekuerzt: bool,
) -> serde_json::Value {
    if gekuerzt {
        if let Some(map) = payload.as_object_mut() {
            map.insert(
                "plan_limit".to_string(),
                kuerzungs_hinweis(stufe, effektive_tage),
            );
        }
    }
    payload
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stufen_sind_aufsteigend_geordnet() {
        assert!(Stufe::Free < Stufe::Plus);
        assert!(Stufe::Plus < Stufe::Pro);
        assert!(Stufe::Pro >= Stufe::Plus);
        assert!(!Stufe::Free.hat_plus());
        assert!(Stufe::Plus.hat_plus());
        assert!(!Stufe::Plus.hat_pro());
        assert!(Stufe::Pro.hat_pro());
    }

    #[test]
    fn katalog_ids_bilden_die_drei_stufen() {
        assert_eq!(stufe_fuer_plan("free"), Stufe::Free);
        assert_eq!(stufe_fuer_plan("plus"), Stufe::Plus);
        assert_eq!(stufe_fuer_plan("pro"), Stufe::Pro);
        // Jede Katalog-ID muss eine Stufe haben.
        for plan in crate::billing::catalog::BILLING_PLANS {
            let stufe = stufe_fuer_plan(plan.id);
            assert_eq!(
                stufe.hat_plus(),
                plan.monthly_gross_cents > 0,
                "bezahlte Stufe muss mindestens Plus sein: {}",
                plan.id
            );
        }
    }

    /// Zuordnung alt zu neu laut Spec: raid_free bleibt Free, alles frueher
    /// Bezahlte wird Plus, der Trial ebenfalls.
    #[test]
    fn alte_plan_ids_behalten_ihren_zugang() {
        assert_eq!(stufe_fuer_plan("raid_free"), Stufe::Free);
        for alt in [
            "chat_quiet",
            "raid_boost",
            "analysis_dashboard",
            "bundle_chat_quiet_raid_boost",
            "bundle_werbefrei_analyse",
            "bundle_komplett",
            "bundle_analysis_raid_boost",
            "analytics_trial",
        ] {
            assert_eq!(stufe_fuer_plan(alt), Stufe::Plus, "{alt} muss Plus sein");
        }
        // Unbekanntes und Leeres faellt auf Free, nie nach oben.
        assert_eq!(stufe_fuer_plan(""), Stufe::Free);
        assert_eq!(stufe_fuer_plan("garbage"), Stufe::Free);
        assert_eq!(stufe_fuer_plan("Pro"), Stufe::Free);
    }

    /// Die Stufe deckt sich mit dem alten `analytics`-Flag: wer das Flag traegt,
    /// ist mindestens Plus, und umgekehrt.
    #[test]
    fn stufe_deckt_sich_mit_analytics_flag() {
        for id in [
            "free",
            "plus",
            "pro",
            "raid_free",
            "chat_quiet",
            "raid_boost",
            "analysis_dashboard",
            "bundle_komplett",
            "analytics_trial",
        ] {
            let flag = crate::plan::plan_has_analytics(id);
            let stufe = stufe_fuer_plan(id);
            if flag {
                assert!(stufe.hat_plus(), "{id}: analytics-Flag ohne Plus-Stufe");
            }
        }
    }

    #[test]
    fn clip_grenzen_je_stufe() {
        assert_eq!(clip_monatslimit(Stufe::Free), Some(3));
        assert_eq!(clip_monatslimit(Stufe::Plus), Some(10));
        assert_eq!(clip_monatslimit(Stufe::Pro), None);
    }

    /// Die eine Stelle, an der Sperren scharf werden: der Katalog.
    #[test]
    fn sperre_haengt_am_katalog_flag() {
        for stufe in [Stufe::Free, Stufe::Plus, Stufe::Pro] {
            let buchbar = crate::billing::catalog::find_plan(stufe.as_str())
                .expect("jede Stufe steht im Katalog")
                .buchbar;
            assert_eq!(
                sperre_greift(stufe),
                buchbar,
                "{}: Sperre muss dem buchbar-Flag folgen",
                stufe.as_str()
            );
        }
    }

    /// Solange Creator Pro nicht buchbar ist, darf niemand aus dem
    /// automatischen Posten fallen. Die Gegenprobe steht direkt daneben: waere
    /// Pro buchbar, greift die Pro-Grenze.
    #[test]
    fn auto_posting_sperrt_nur_bei_buchbarem_pro() {
        let pro_buchbar = sperre_greift(Stufe::Pro);
        assert!(
            !pro_buchbar,
            "Erwartung dieses Umbaus: Creator Pro ist noch nicht buchbar"
        );
        assert!(auto_posting_erlaubt(Stufe::Free));
        assert!(auto_posting_erlaubt(Stufe::Plus));
        assert!(auto_posting_erlaubt(Stufe::Pro));
        // Gegenprobe zur Formel selbst: mit scharfer Sperre bliebe nur Pro uebrig.
        let mit_sperre = |stufe: Stufe| stufe.hat_pro();
        assert!(!mit_sperre(Stufe::Free));
        assert!(!mit_sperre(Stufe::Plus));
        assert!(mit_sperre(Stufe::Pro));
    }

    /// Das Kontingent zaehlt, sperrt aber nicht, solange Pro nicht buchbar ist.
    #[test]
    fn clip_kontingent_zaehlt_ohne_zu_sperren() {
        let offen = ClipKontingent {
            stufe: Stufe::Free,
            genutzt: 99,
            limit: clip_monatslimit(Stufe::Free),
            erzwungen: false,
        };
        assert!(offen.frei(), "ohne buchbaren Ausweg darf nichts sperren");
        assert_eq!(offen.rest(), None, "kein Rest heisst: nichts klemmen");
        let json = offen.als_json();
        assert_eq!(json["genutzt"], 99, "gezaehlt wird weiter");
        assert!(json["limit"].is_null(), "keine Schranke behaupten");
        assert!(json["rest"].is_null());
        assert_eq!(json["erzwungen"], false);

        // Gegenprobe: mit erzwungener Grenze sperrt derselbe Stand.
        let scharf = ClipKontingent {
            erzwungen: true,
            ..offen
        };
        assert!(!scharf.frei());
        assert_eq!(scharf.rest(), Some(0));
        assert_eq!(scharf.als_json()["limit"], 3);
    }

    /// Bezahlt-Erkennung kommt aus dem Katalog, nicht aus einer Namensliste.
    #[test]
    fn bezahlte_plan_ids_kommen_aus_dem_katalog() {
        for plan in crate::billing::catalog::BILLING_PLANS {
            assert_eq!(
                ist_bezahlter_plan(plan.id),
                plan.monthly_gross_cents > 0,
                "{} muss dem Katalogpreis folgen",
                plan.id
            );
        }
        // Alt-IDs haben Geld gekostet und gelten weiter als bezahlt.
        for alt in [
            "chat_quiet",
            "raid_boost",
            "analysis_dashboard",
            "bundle_chat_quiet_raid_boost",
            "bundle_werbefrei_analyse",
            "bundle_komplett",
            "bundle_analysis_raid_boost",
        ] {
            assert!(ist_bezahlter_plan(alt), "{alt} muss bezahlt bleiben");
        }
        // Gratis, Trial, Unbekanntes und Leeres sind nicht bezahlt.
        assert!(!ist_bezahlter_plan("free"));
        assert!(!ist_bezahlter_plan("raid_free"));
        assert!(!ist_bezahlter_plan("analytics_trial"));
        assert!(!ist_bezahlter_plan("garbage"));
        assert!(!ist_bezahlter_plan(""));
        assert!(!ist_bezahlter_plan("  "));
    }

    #[test]
    fn verlauf_wird_nur_unter_plus_geklemmt() {
        assert_eq!(verlauf_tage_klemmen(Stufe::Free, 365), (2, true));
        assert_eq!(verlauf_tage_klemmen(Stufe::Free, 1), (1, false));
        assert_eq!(verlauf_tage_klemmen(Stufe::Plus, 365), (365, false));
        assert_eq!(verlauf_tage_klemmen(Stufe::Pro, 365), (365, false));
        assert_eq!(verlauf_monate_klemmen(Stufe::Free, 12), (1, true));
        assert_eq!(verlauf_monate_klemmen(Stufe::Plus, 12), (12, false));
    }

    #[test]
    fn hinweis_haengt_nur_bei_kuerzung_an_objekten() {
        let voll = hinweis_anhaengen(serde_json::json!({"a": 1}), Stufe::Plus, 365, false);
        assert!(voll.get("plan_limit").is_none());

        let gekuerzt = hinweis_anhaengen(serde_json::json!({"a": 1}), Stufe::Free, 2, true);
        assert_eq!(gekuerzt["plan_limit"]["gekuerzt"], true);
        assert_eq!(gekuerzt["plan_limit"]["plan_stufe"], "free");
        assert_eq!(gekuerzt["plan_limit"]["fenster_tage"], 2);
        assert_eq!(gekuerzt["plan_limit"]["benoetigte_stufe"], "plus");

        // Arrays bleiben unveraendert (kein Platz fuer das Feld).
        let liste = hinweis_anhaengen(serde_json::json!([1, 2]), Stufe::Free, 2, true);
        assert!(liste.is_array());
    }
}
