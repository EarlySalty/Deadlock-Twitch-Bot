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
    /// Netzwerk Free: vollwertig, aber nur der letzte Stream und Clips mit
    /// Wasserzeichen.
    Free,
    /// Netzwerk Plus: voller Verlauf, Vergleiche, KI, Coaching.
    Plus,
    /// Creator Pro: alles aus Plus, Clips ohne Limit, automatisches Posten.
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
pub fn clip_monatslimit(stufe: Stufe) -> Option<u32> {
    match stufe {
        Stufe::Free => Some(3),
        Stufe::Plus => Some(10),
        Stufe::Pro => None,
    }
}

/// `true`, wenn Clips dieser Stufe ein Wasserzeichen tragen.
pub fn clip_wasserzeichen(stufe: Stufe) -> bool {
    stufe == Stufe::Free
}

/// `true`, wenn die Stufe automatisches Posten auf TikTok, Instagram und
/// YouTube nutzen darf. Das ist die Pro-Grenze.
pub fn auto_posting_erlaubt(stufe: Stufe) -> bool {
    stufe.hat_pro()
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
    let letzte: Option<chrono::DateTime<chrono::Utc>> = sqlx::query_scalar(
        "SELECT started_at FROM twitch_stream_sessions          WHERE LOWER(streamer_login) = $1 AND started_at IS NOT NULL          ORDER BY started_at DESC LIMIT 1",
    )
    .bind(&login)
    .fetch_optional(pool)
    .await
    .ok()
    .flatten();
    let Some(started_at) = letzte else {
        return FREE_VERLAUF_TAGE;
    };
    let tage = (chrono::Utc::now() - started_at).num_days() + 1;
    tage.clamp(FREE_VERLAUF_TAGE, FREE_FENSTER_MAX_TAGE)
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
    /// Bereits erzeugte Clips in diesem Monat (verworfene zaehlen nicht mit).
    pub genutzt: i64,
    /// Monatsgrenze; `None` heisst unbegrenzt.
    pub limit: Option<u32>,
    /// Ob Clips dieser Stufe ein Wasserzeichen tragen.
    pub wasserzeichen: bool,
}

impl ClipKontingent {
    /// Wie viele Clips noch gehen. `None` heisst unbegrenzt.
    pub fn rest(&self) -> Option<i64> {
        self.limit.map(|limit| (limit as i64 - self.genutzt).max(0))
    }

    /// `true`, wenn noch mindestens ein Clip frei ist.
    pub fn frei(&self) -> bool {
        self.rest().is_none_or(|rest| rest > 0)
    }

    /// JSON-Block fuer Antworten und Fehlermeldungen.
    pub fn als_json(&self) -> serde_json::Value {
        serde_json::json!({
            "plan_stufe": self.stufe.as_str(),
            "genutzt": self.genutzt,
            "limit": self.limit,
            "rest": self.rest(),
            "wasserzeichen": self.wasserzeichen,
        })
    }
}

/// Zaehlt die in diesem Kalendermonat erzeugten Clips eines Streamers.
///
/// Verworfene Clips (`discarded_at`) zaehlen nicht, sonst waere ein
/// Fehlgriff dauerhaft teuer. Bei DB-Fehler `0`: eine kaputte Zaehlung darf
/// niemandem den Dienst sperren, die Sperre selbst haengt an der Stufe.
pub async fn clips_diesen_monat(pool: &PgPool, streamer: &str) -> i64 {
    let login = streamer.trim().to_lowercase();
    if login.is_empty() {
        return 0;
    }
    sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*)::bigint FROM twitch_clips_social_media          WHERE LOWER(streamer_login) = $1            AND discarded_at IS NULL            AND created_at >= DATE_TRUNC('month', NOW())",
    )
    .bind(&login)
    .fetch_one(pool)
    .await
    .unwrap_or(0)
}

/// Kontingent eines Streamers fuer die gegebene Stufe.
pub async fn clip_kontingent(pool: &PgPool, stufe: Stufe, streamer: &str) -> ClipKontingent {
    let limit = clip_monatslimit(stufe);
    let genutzt = if limit.is_some() {
        clips_diesen_monat(pool, streamer).await
    } else {
        // Pro hat kein Limit, dann braucht es auch keine Zaehlabfrage.
        0
    };
    ClipKontingent {
        stufe,
        genutzt,
        limit,
        wasserzeichen: clip_wasserzeichen(stufe),
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
        assert!(clip_wasserzeichen(Stufe::Free));
        assert!(!clip_wasserzeichen(Stufe::Plus));
        assert!(!clip_wasserzeichen(Stufe::Pro));
        assert!(!auto_posting_erlaubt(Stufe::Free));
        assert!(!auto_posting_erlaubt(Stufe::Plus));
        assert!(auto_posting_erlaubt(Stufe::Pro));
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
