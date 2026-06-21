//! Raid-Arrival-Klassifikations-Engine — reine Entscheidungslogik ohne DB-Zugriff.
//!
//! Port von `bot/raid/arrival_confirmation.py` (179 Z.) und dem darin genutzten
//! `bot/raid/partner_resolution.py` (163 Z.).
//!
//! ## Überblick
//!
//! Gegeben ein [`PendingRaid`], Signal-Kontext und zwei Lookup-Traits ergibt sich
//! eine [`ArrivalConfirmationDecision`] mit:
//!
//! - `classification` — wie der Raid einzustufen ist (`ours_to_partner`,
//!   `external_to_partner`, …)
//! - diversen `should_*`-Flags — was die Ausführungsschicht danach tun soll
//!
//! Die DB-Lookups werden über [`PartnerLookup`] und [`KnownStreamerLookup`]
//! abstrahiert; die echten Implementierungen gehören in den Composition-Root.
//!
//! ## Klassifikations-Verzweigungen (partner_resolution.py Z. 83–152)
//!
//! | Konstellation                                | classification            | source_resolution           |
//! |----------------------------------------------|---------------------------|-----------------------------|
//! | Ziel ist kein Partner                        | `None`                    | `non_partner_target`        |
//! | Ziel=Partner + Quelle ist known_streamer (ID vorhanden) | `ours_to_partner` | `known_streamer_id`  |
//! | Ziel=Partner + Quelle ist known_streamer (kein ID) | `ours_to_partner`  | `known_streamer_login`      |
//! | Ziel=Partner + keine Quell-Identität         | `unknown_source_to_partner` | `missing_source_identity` |
//! | Ziel=Partner + Quelle nicht bekannt          | `external_to_partner`     | `unmatched_source`          |
//!
//! ## Arrival-Übersteuerung (arrival_confirmation.py Z. 115–117)
//!
//! Nach dem raw-resolution-Schritt: Wenn `pending_raid.is_partner_raid &&
//! target_is_partner && classification != "ours_to_partner"` → überschreibe
//! classification auf `"ours_to_partner"` und source_resolution auf
//! `"pending_partner_raid"`.
//!
//! ## follow_up_kind-Verzweigungen (Z. 119–130)
//!
//! | Bedingung                                         | follow_up_kind        | suppression_reason                             |
//! |---------------------------------------------------|-----------------------|------------------------------------------------|
//! | classification == "ours_to_partner"               | `partner`             | `None`                                         |
//! | target_is_partner (aber nicht ours_to_partner)    | `suppressed_external` | `partner_target_without_our_raid_confirmation` |
//! | !target_is_partner && !is_partner_raid            | `external`            | `None`                                         |
//! | !target_is_partner && is_partner_raid             | `suppressed_external` | `pending_partner_raid_later_resolved_non_partner` |

use crate::pending_raids::PendingRaid;

// ---------------------------------------------------------------------------
// Lookup-Traits (DB-Ports, Impl gehört in Composition-Root)
// ---------------------------------------------------------------------------

/// Sucht einen Partner-Eintrag anhand von Twitch-User-ID oder Login.
///
/// Port von `PartnerLookup` (partner_resolution.py Z. 8–14).
/// Gibt `true` zurück, wenn ein Eintrag gefunden wurde — der Aufrufer
/// entscheidet selbst, welchen Typ er zurückliefert; Rust modelliert
/// das als `bool` (Truthy-Semantik wie Python `bool(row)`).
pub trait PartnerLookup: Send + Sync {
    /// `twitch_user_id` und `twitch_login` sind optionale Suchschlüssel —
    /// mindestens einer muss belegt sein, damit die Suche sinnvoll ist.
    fn lookup_partner(&self, twitch_user_id: Option<&str>, twitch_login: Option<&str>) -> bool;
}

/// Sucht einen bekannten Streamer (our ecosystem) anhand von Broadcaster-ID
/// oder Login.
///
/// Port von `KnownStreamerLookup` (partner_resolution.py Z. 17–23).
/// Gibt `Some(has_id)` zurück wenn gefunden (`has_id=true` bedeutet der
/// Datensatz hat eine `twitch_user_id`/`user_id` gesetzt, `false` = nur Login
/// bekannt). `None` = nicht gefunden.
pub trait KnownStreamerLookup: Send + Sync {
    /// `broadcaster_id` und `broadcaster_login` sind optionale Suchschlüssel.
    /// Rückgabe: `None` wenn kein Datensatz, `Some(true)` wenn ID-Feld belegt,
    /// `Some(false)` wenn nur Login bekannt.
    fn lookup_known_streamer(
        &self,
        broadcaster_id: Option<&str>,
        broadcaster_login: Option<&str>,
    ) -> Option<bool>;
}

// ---------------------------------------------------------------------------
// PartnerRaidArrivalResolution
// ---------------------------------------------------------------------------

/// Ergebnis der Partner-Raid-Arrival-Klassifikation.
///
/// Port von `PartnerRaidArrivalResolution` (partner_resolution.py Z. 26–38).
#[derive(Debug, Clone)]
pub struct PartnerRaidArrivalResolution {
    /// Wie der Raid einzuklassifizieren ist. `None` = Ziel kein Partner.
    /// Mögliche Werte: `None`, `"ours_to_partner"`, `"unknown_source_to_partner"`,
    /// `"external_to_partner"`.
    pub classification: Option<String>,
    /// Woher die Klassifikation stammt.
    /// Werte: `"non_partner_target"`, `"known_streamer_id"`,
    /// `"known_streamer_login"`, `"missing_source_identity"`, `"unmatched_source"`.
    pub source_resolution: String,
    /// Ob der Ziel-Kanal ein Partner ist.
    pub target_is_partner: bool,
    pub from_broadcaster_id: Option<String>,
    pub from_broadcaster_login: String,
    pub to_broadcaster_id: String,
    pub to_broadcaster_login: String,
}

// ---------------------------------------------------------------------------
// classify_partner_raid_arrival (port partner_resolution.py Z. 83–152)
// ---------------------------------------------------------------------------

/// Klassifiziert einen eintreffenden Raid bezüglich Partner-Status von Quelle
/// und Ziel.
///
/// Port von `classify_partner_raid_arrival` (partner_resolution.py Z. 83–152).
///
/// Verzweigungslogik:
/// 1. Ziel ist kein Partner → classification=None, source_resolution="non_partner_target"
/// 2. Ziel=Partner, Quelle ist known_streamer:
///    - hat ID-Feld → source_resolution="known_streamer_id"
///    - nur Login → source_resolution="known_streamer_login"
///      → classification="ours_to_partner"
/// 3. Ziel=Partner, keine Quell-Identität → "unknown_source_to_partner" /
///    "missing_source_identity"
/// 4. Ziel=Partner, Quelle nicht bekannt → "external_to_partner" /
///    "unmatched_source"
pub fn classify_partner_raid_arrival(
    from_broadcaster_login: Option<&str>,
    from_broadcaster_id: Option<&str>,
    to_broadcaster_id: Option<&str>,
    to_broadcaster_login: Option<&str>,
    partner_lookup: &dyn PartnerLookup,
    known_streamer_lookup: &dyn KnownStreamerLookup,
) -> PartnerRaidArrivalResolution {
    classify_partner_raid_arrival_with_expectation(
        from_broadcaster_login,
        from_broadcaster_id,
        to_broadcaster_id,
        to_broadcaster_login,
        partner_lookup,
        known_streamer_lookup,
        false,
    )
}

/// Wie [`classify_partner_raid_arrival`], aber mit `expected_partner`-Override.
///
/// Port von `PartnerArrivalTrackingService.classify_partner_raid_arrival`
/// (partner_arrival_tracking.py Z. 116–152): Wenn der erste Klassifikations-Pass
/// `classification == None` liefert UND `expected_partner == true`, wird ein
/// zweiter Pass mit einem synthetischen Partner-Lookup (immer truthy, Quelle
/// `pending_partner_override`) durchgeführt. Damit wird ein als Partner-Raid
/// markiertes Ziel auch dann als Partner behandelt, wenn es (noch) nicht in der
/// Partner-Tabelle steht — Ergebnis ist ein `*_to_partner`-Wert mit
/// `target_is_partner == true` statt `non_partner_target`.
///
/// `expected_partner` entspricht `bool(pending_raid.is_partner_raid)`
/// (runtime_factories.py Z. 551).
pub fn classify_partner_raid_arrival_with_expectation(
    from_broadcaster_login: Option<&str>,
    from_broadcaster_id: Option<&str>,
    to_broadcaster_id: Option<&str>,
    to_broadcaster_login: Option<&str>,
    partner_lookup: &dyn PartnerLookup,
    known_streamer_lookup: &dyn KnownStreamerLookup,
    expected_partner: bool,
) -> PartnerRaidArrivalResolution {
    let result = classify_partner_raid_arrival_inner(
        from_broadcaster_login,
        from_broadcaster_id,
        to_broadcaster_id,
        to_broadcaster_login,
        partner_lookup,
        known_streamer_lookup,
    );

    // Zweiter Pass: kein Partner-Ziel erkannt, aber als Partner-Raid erwartet
    // → synthetischer Partner-Lookup erzwingt target_is_partner (Z. 140–151).
    if result.classification.is_none() && expected_partner {
        return classify_partner_raid_arrival_inner(
            from_broadcaster_login,
            from_broadcaster_id,
            to_broadcaster_id,
            to_broadcaster_login,
            &AlwaysPartnerOverride,
            known_streamer_lookup,
        );
    }
    result
}

/// Synthetischer Partner-Lookup, der immer `true` liefert — Python-Pendant
/// `lambda **_kwargs: {"source": "pending_partner_override"}`
/// (partner_arrival_tracking.py Z. 146).
struct AlwaysPartnerOverride;
impl PartnerLookup for AlwaysPartnerOverride {
    fn lookup_partner(&self, _id: Option<&str>, _login: Option<&str>) -> bool {
        true
    }
}

/// Reiner Klassifikations-Pass ohne `expected_partner`-Override.
fn classify_partner_raid_arrival_inner(
    from_broadcaster_login: Option<&str>,
    from_broadcaster_id: Option<&str>,
    to_broadcaster_id: Option<&str>,
    to_broadcaster_login: Option<&str>,
    partner_lookup: &dyn PartnerLookup,
    known_streamer_lookup: &dyn KnownStreamerLookup,
) -> PartnerRaidArrivalResolution {
    // Normalisierung identisch zu partner_resolution.py Z. 92–95
    let normalized_from_login = normalize_login(from_broadcaster_login);
    let normalized_to_login = normalize_login(to_broadcaster_login);
    let from_key = trim_opt(from_broadcaster_id);
    let to_key = trim_opt(to_broadcaster_id);

    // Ziel-Partner-Check (Z. 97–101)
    let target_is_partner =
        is_partner_target_channel(to_key.as_deref(), &normalized_to_login, partner_lookup);

    // Pfad 1: kein Partner-Ziel (Z. 102–111)
    if !target_is_partner {
        return PartnerRaidArrivalResolution {
            classification: None,
            source_resolution: "non_partner_target".to_string(),
            target_is_partner: false,
            from_broadcaster_id: from_key,
            from_broadcaster_login: normalized_from_login,
            to_broadcaster_id: to_key.unwrap_or_default(),
            to_broadcaster_login: normalized_to_login,
        };
    }

    // Pfad 2: Partner-Ziel + Quelle bekannt (Z. 113–131)
    let known = known_streamer_lookup.lookup_known_streamer(
        from_key.as_deref(),
        if normalized_from_login.is_empty() {
            None
        } else {
            Some(normalized_from_login.as_str())
        },
    );
    if let Some(has_id) = known {
        let source_resolution = if has_id {
            "known_streamer_id"
        } else {
            "known_streamer_login"
        };
        return PartnerRaidArrivalResolution {
            classification: Some("ours_to_partner".to_string()),
            source_resolution: source_resolution.to_string(),
            target_is_partner: true,
            from_broadcaster_id: from_key,
            from_broadcaster_login: normalized_from_login,
            to_broadcaster_id: to_key.unwrap_or_default(),
            to_broadcaster_login: normalized_to_login,
        };
    }

    // Pfad 3: Partner-Ziel + keine Quell-Identität (Z. 133–142)
    if normalized_from_login.is_empty() && from_key.is_none() {
        return PartnerRaidArrivalResolution {
            classification: Some("unknown_source_to_partner".to_string()),
            source_resolution: "missing_source_identity".to_string(),
            target_is_partner: true,
            from_broadcaster_id: None,
            from_broadcaster_login: String::new(),
            to_broadcaster_id: to_key.unwrap_or_default(),
            to_broadcaster_login: normalized_to_login,
        };
    }

    // Pfad 4: Partner-Ziel + Quelle nicht bekannt (Z. 144–152)
    PartnerRaidArrivalResolution {
        classification: Some("external_to_partner".to_string()),
        source_resolution: "unmatched_source".to_string(),
        target_is_partner: true,
        from_broadcaster_id: from_key,
        from_broadcaster_login: normalized_from_login,
        to_broadcaster_id: to_key.unwrap_or_default(),
        to_broadcaster_login: normalized_to_login,
    }
}

// ---------------------------------------------------------------------------
// ArrivalConfirmationDecision
// ---------------------------------------------------------------------------

/// Klassifikation des Raid-Arrivals.
///
/// Werte identisch zu Python-String-Literals in `arrival_confirmation.py`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FollowUpKind {
    /// Eigener Partner-Raid kam an.
    Partner,
    /// Externer Recruitment-Raid (nicht unterdrückt).
    External,
    /// Externer Raid, aber unterdrückt (z. B. Partner-Ziel ohne unsere Bestätigung).
    SuppressedExternal,
}

impl FollowUpKind {
    /// String-Repräsentation identisch zu Python-`Literal` (Z. 9).
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Partner => "partner",
            Self::External => "external",
            Self::SuppressedExternal => "suppressed_external",
        }
    }
}

/// Vollständige Entscheidung nach Bestätigung eines Pending-Raids.
///
/// Port von `ArrivalConfirmationDecision` (arrival_confirmation.py Z. 31–49).
/// Alle Felder 1:1 übernommen.
#[derive(Debug, Clone)]
pub struct ArrivalConfirmationDecision {
    /// Signaltyp, der die Bestätigung ausgelöst hat (Z. 33).
    pub signal_type: String,
    /// Der bestätigte Pending-Raid (Z. 34).
    pub pending_raid: PendingRaid,
    /// Rohes Klassifikationsergebnis aus `classify_partner_raid_arrival` (Z. 35).
    pub raw_resolution: PartnerRaidArrivalResolution,
    /// Finale Klassifikation (ggf. überschrieben, Z. 36).
    /// `None` = kein Partner-Bezug.
    pub classification: Option<String>,
    /// Woher die Klassifikation stammt (Z. 37).
    pub source_resolution: String,
    /// Folgeaktion-Kategorie (Z. 38).
    pub follow_up_kind: FollowUpKind,
    /// Ob der Ziel-Kanal ein Partner ist (Z. 39).
    pub target_is_partner: bool,
    /// Ob der Pending-Raid als Partner-Raid registriert war (Z. 40).
    pub pending_is_partner_raid: bool,
    /// Soll die letzte Raid-History als Referenz geladen werden? (Z. 41)
    ///
    /// `true` wenn `is_partner_raid || classification == "ours_to_partner"` (Z. 132).
    pub should_load_recent_raid_history_reference: bool,
    /// Soll ein externer Recruitment-Blacklist-Pending gelöscht werden? (Z. 42)
    ///
    /// `true` wenn `target_is_partner` (Z. 133).
    pub should_delete_external_recruitment_blacklist_pending: bool,
    /// Soll der Partner-Score-Cache neu geladen werden? (Z. 43)
    ///
    /// `true` wenn `classification == "ours_to_partner"` (Z. 134).
    pub should_refresh_partner_score_cache: bool,
    /// Soll ein bestätigter Partner-Raid getrackt werden? (Z. 44)
    ///
    /// `true` wenn `classification == "ours_to_partner"` (Z. 135).
    pub should_track_confirmed_partner_raid: bool,
    /// Soll eine Partner-Raid-Nachricht gesendet werden? (Z. 45)
    ///
    /// `true` wenn `classification == "ours_to_partner"` (Z. 136).
    pub should_send_partner_raid_message: bool,
    /// Soll ein bestätigter externer Recruitment-Raid persistiert werden? (Z. 46)
    ///
    /// `true` wenn `follow_up_kind == external` (Z. 137).
    pub should_persist_confirmed_external_recruitment_raid: bool,
    /// Soll ein externer Recruitment-Blacklist-Pending geplant werden? (Z. 47)
    ///
    /// `true` wenn `follow_up_kind == external` (Z. 138).
    pub should_schedule_external_recruitment_blacklist_pending: bool,
    /// Soll eine Recruitment-Nachricht gesendet werden? (Z. 48)
    ///
    /// `true` wenn `follow_up_kind == external` (Z. 139).
    pub should_send_recruitment_message: bool,
    /// Optionaler Unterdrückungsgrund (Z. 49).
    pub suppression_reason: Option<String>,
}

// ---------------------------------------------------------------------------
// ArrivalConfirmationService
// ---------------------------------------------------------------------------

/// Reine Entscheidungs-Engine für Raid-Arrival-Bestätigung.
///
/// Signal-Kontext für die Bestätigung — die Quell-/Ziel-Identität, die Python
/// vom Aufrufer an `classify_partner_raid_arrival` durchreicht (Z. 91–96).
/// `from_broadcaster_id` ist nötig für den `known_streamer_id`-Klassifikations-
/// zweig; das `PendingRaid` allein trägt diese Info NICHT.
pub struct ArrivalSignalContext<'a> {
    pub from_broadcaster_login: &'a str,
    pub from_broadcaster_id: Option<&'a str>,
    pub to_broadcaster_id: &'a str,
    pub to_broadcaster_login: Option<&'a str>,
}

/// Port von `ArrivalConfirmationService` (arrival_confirmation.py Z. 55–169).
/// Kein DB-Zugriff — die Lookups werden über Traits injiziert.
pub struct ArrivalConfirmationService {
    partner_lookup: Box<dyn PartnerLookup>,
    known_streamer_lookup: Box<dyn KnownStreamerLookup>,
}

impl ArrivalConfirmationService {
    /// Erstellt eine neue Instanz mit den gegebenen Lookup-Implementierungen.
    pub fn new(
        partner_lookup: Box<dyn PartnerLookup>,
        known_streamer_lookup: Box<dyn KnownStreamerLookup>,
    ) -> Self {
        Self {
            partner_lookup,
            known_streamer_lookup,
        }
    }

    /// Bestätigt einen ausstehenden Raid und gibt die Entscheidung zurück.
    ///
    /// Port von `confirm_pending_raid_arrival` (arrival_confirmation.py Z. 65–169).
    ///
    /// Gibt `None` zurück, wenn `pending_raid` keinen gültigen Raid enthält
    /// (analog zu Python Z. 87–88: `if raid is None: return None`).
    ///
    /// # Parameter
    ///
    /// - `pending_raid` — Der Pending-Raid-Kontext.
    /// - `signal_type` — Welches Signal die Bestätigung ausgelöst hat (Z. 69).
    /// - `classification_override` — Falls `Some(...)`, überschreibt die
    ///   `raw_resolution.classification` (Z. 75). `None` = kein Override.
    ///   Achtung: Python nutzt `_UNSET` als Sentinel-Objekt; hier ist `None`
    ///   der Nicht-Override-Zustand — ein explizites `Some(None)` entspricht
    ///   dem Python-`None`-Override (classification auf None setzen).
    /// - `source_resolution_override` — Analog zu `classification_override`.
    /// - `target_is_partner_override` — Falls `Some(bool)`, überschreibt den
    ///   Lookup-Wert (Z. 77). `None` = Lookup-Ergebnis verwenden.
    ///
    // WIRING-TODO(P1.13): bin/tb-bot raid_arrival_wiring.rs:704 ruft confirm aktuell
    // mit (None, None, None). Für den expected_partner-Override (Partner-Raid zu
    // noch nicht eingetragenem Partner-Ziel) muss der Aufrufer — wie
    // runtime_factories.py Z. 546–563 — vorab
    // `classify_partner_raid_arrival_with_expectation(.., expected_partner=pending.is_partner_raid)`
    // aufrufen und das Ergebnis als classification_override=Some(classification),
    // source_resolution_override=Some(Some(source_resolution)),
    // target_is_partner_override=Some(classification.is_some()) durchreichen.
    pub fn confirm_pending_raid_arrival(
        &self,
        pending_raid: PendingRaid,
        ctx: &ArrivalSignalContext<'_>,
        signal_type: &str,
        classification_override: Option<Option<String>>,
        source_resolution_override: Option<Option<String>>,
        target_is_partner_override: Option<bool>,
    ) -> Option<ArrivalConfirmationDecision> {
        // PendingRaid ist bereits ein konkreter Wert — kein from_payload nötig
        // (der from_payload-Pfad liegt in der Aufrufschicht; hier reinen Typ entgegennehmen).
        let raid = pending_raid;

        // raw_resolution durch Klassifikations-Funktion ermitteln (Z. 90–97).
        // WICHTIG: Quell-/Ziel-Identität kommt aus dem SIGNAL-Kontext (Python
        // reicht from_broadcaster_id/to_broadcaster_login vom Aufrufer durch) —
        // der `from_broadcaster_id`-Zweig (known_streamer_id) braucht das.
        //
        // Der `expected_partner`-Override (Partner-Raid zu noch NICHT
        // eingetragenem Partner-Ziel, P1.13) wird — wie in Python
        // (runtime_factories.py Z. 546–563) — vom AUFRUFER berechnet und über
        // `classification_override` / `source_resolution_override` /
        // `target_is_partner_override` durchgereicht. Der Aufrufer nutzt dafür
        // `classify_partner_raid_arrival_with_expectation(.., raid.is_partner_raid)`.
        // Siehe WIRING-TODO(P1.13).
        let raw_resolution = classify_partner_raid_arrival(
            Some(ctx.from_broadcaster_login),
            ctx.from_broadcaster_id,
            Some(ctx.to_broadcaster_id),
            ctx.to_broadcaster_login,
            self.partner_lookup.as_ref(),
            self.known_streamer_lookup.as_ref(),
        );

        // classification auflösen (Z. 99–103)
        let classification: Option<String> = match classification_override {
            Some(val) => val,
            None => raw_resolution.classification.clone(),
        };

        // source_resolution auflösen (Z. 104–108)
        let source_resolution: String = match source_resolution_override {
            Some(Some(val)) => val,
            Some(None) => String::new(),
            None => raw_resolution.source_resolution.clone(),
        };

        // target_is_partner auflösen (Z. 109–113)
        let target_is_partner =
            target_is_partner_override.unwrap_or(raw_resolution.target_is_partner);

        // Classification-Override: is_partner_raid + target_is_partner aber
        // noch nicht als ours_to_partner klassifiziert (Z. 115–117)
        let (classification, source_resolution) = if raid.is_partner_raid
            && target_is_partner
            && classification.as_deref() != Some("ours_to_partner")
        {
            (
                Some("ours_to_partner".to_string()),
                "pending_partner_raid".to_string(),
            )
        } else {
            (classification, source_resolution)
        };

        // follow_up_kind + suppression_reason (Z. 119–130)
        let (follow_up_kind, suppression_reason) = resolve_follow_up(
            classification.as_deref(),
            target_is_partner,
            raid.is_partner_raid,
        );

        // should_*-Flags (Z. 132–139)
        let is_ours_to_partner = classification.as_deref() == Some("ours_to_partner");
        let is_external = follow_up_kind == FollowUpKind::External;

        let decision = ArrivalConfirmationDecision {
            signal_type: signal_type.trim().to_string(),
            pending_raid: raid,
            raw_resolution,
            classification,
            source_resolution,
            follow_up_kind,
            target_is_partner,
            pending_is_partner_raid: false, // wird unten mit dem echten Wert überschrieben
            // Z. 132: is_partner_raid OR ours_to_partner
            should_load_recent_raid_history_reference: false,
            // Z. 133: target_is_partner
            should_delete_external_recruitment_blacklist_pending: target_is_partner,
            // Z. 134–136: alle drei nur bei ours_to_partner
            should_refresh_partner_score_cache: is_ours_to_partner,
            should_track_confirmed_partner_raid: is_ours_to_partner,
            should_send_partner_raid_message: is_ours_to_partner,
            // Z. 137–139: alle drei nur bei external
            should_persist_confirmed_external_recruitment_raid: is_external,
            should_schedule_external_recruitment_blacklist_pending: is_external,
            should_send_recruitment_message: is_external,
            suppression_reason,
        };

        // Flags die den raid-Wert brauchen, nachträglich korrekt setzen
        // (Rust erlaubt keine Teilkonstruktion + spätere Mutation über struct-literal)
        let pending_is_partner_raid = decision.pending_raid.is_partner_raid;
        let should_load = pending_is_partner_raid || is_ours_to_partner;

        Some(ArrivalConfirmationDecision {
            pending_is_partner_raid,
            should_load_recent_raid_history_reference: should_load,
            ..decision
        })
    }
}

// ---------------------------------------------------------------------------
// Interne Helfer
// ---------------------------------------------------------------------------

/// Normalisiert einen optionalen String: trim + lowercase, leer wenn None/leer.
///
/// Port von `normalize_broadcaster_login` (partner_resolution.py Z. 40–41).
fn normalize_login(raw: Option<&str>) -> String {
    raw.unwrap_or("").trim().to_lowercase()
}

/// Trimmt einen optionalen String, gibt None zurück wenn leer.
///
/// Port von `str(x or "").strip() or None` (partner_resolution.py Z. 94–95).
fn trim_opt(raw: Option<&str>) -> Option<String> {
    let s = raw.unwrap_or("").trim().to_string();
    if s.is_empty() {
        None
    } else {
        Some(s)
    }
}

/// Prüft ob der Ziel-Kanal ein Partner ist.
///
/// Port von `is_partner_target_channel` (partner_resolution.py Z. 44–58).
fn is_partner_target_channel(
    broadcaster_id: Option<&str>,
    broadcaster_login: &str,
    partner_lookup: &dyn PartnerLookup,
) -> bool {
    let id_key = broadcaster_id.unwrap_or("").trim();
    let login_key = broadcaster_login.trim();
    // Wenn beide Schlüssel leer → kein Lookup (Z. 52–53)
    if id_key.is_empty() && login_key.is_empty() {
        return false;
    }
    partner_lookup.lookup_partner(
        if id_key.is_empty() {
            None
        } else {
            Some(id_key)
        },
        if login_key.is_empty() {
            None
        } else {
            Some(login_key)
        },
    )
}

/// Leitet `follow_up_kind` und `suppression_reason` aus den aufgelösten Werten ab.
///
/// Port der elif-Kette (arrival_confirmation.py Z. 119–130).
fn resolve_follow_up(
    classification: Option<&str>,
    target_is_partner: bool,
    is_partner_raid: bool,
) -> (FollowUpKind, Option<String>) {
    if classification == Some("ours_to_partner") {
        (FollowUpKind::Partner, None)
    } else if target_is_partner {
        (
            FollowUpKind::SuppressedExternal,
            Some("partner_target_without_our_raid_confirmation".to_string()),
        )
    } else if !target_is_partner && !is_partner_raid {
        (FollowUpKind::External, None)
    } else {
        // !target_is_partner && is_partner_raid (Z. 128–130)
        (
            FollowUpKind::SuppressedExternal,
            Some("pending_partner_raid_later_resolved_non_partner".to_string()),
        )
    }
}

// ---------------------------------------------------------------------------
// Unit-Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pending_raids::PendingRaid;

    // --- Stub-Lookups ---

    struct AlwaysPartner;
    impl PartnerLookup for AlwaysPartner {
        fn lookup_partner(&self, _id: Option<&str>, _login: Option<&str>) -> bool {
            true
        }
    }

    struct NeverPartner;
    impl PartnerLookup for NeverPartner {
        fn lookup_partner(&self, _id: Option<&str>, _login: Option<&str>) -> bool {
            false
        }
    }

    /// Known-Streamer-Lookup der immer Some(true) zurückgibt (Quelle hat ID).
    struct AlwaysKnownWithId;
    impl KnownStreamerLookup for AlwaysKnownWithId {
        fn lookup_known_streamer(&self, _id: Option<&str>, _login: Option<&str>) -> Option<bool> {
            Some(true)
        }
    }

    /// Known-Streamer-Lookup der Some(false) zurückgibt (nur Login bekannt).
    struct AlwaysKnownLoginOnly;
    impl KnownStreamerLookup for AlwaysKnownLoginOnly {
        fn lookup_known_streamer(&self, _id: Option<&str>, _login: Option<&str>) -> Option<bool> {
            Some(false)
        }
    }

    /// Known-Streamer-Lookup der None zurückgibt (nicht bekannt).
    struct NeverKnown;
    impl KnownStreamerLookup for NeverKnown {
        fn lookup_known_streamer(&self, _id: Option<&str>, _login: Option<&str>) -> Option<bool> {
            None
        }
    }

    fn make_pending(from: &str, to_id: &str) -> PendingRaid {
        PendingRaid::new(from, to_id)
    }

    fn make_partner_pending(from: &str, to_id: &str) -> PendingRaid {
        let mut r = PendingRaid::new(from, to_id);
        r.is_partner_raid = true;
        r
    }

    /// Signal-Kontext aus Literalen (from_id/to_login default None) — entspricht
    /// dem bisherigen Verhalten (Service reichte None durch).
    fn sig_ctx<'a>(from_login: &'a str, to_id: &'a str) -> ArrivalSignalContext<'a> {
        ArrivalSignalContext {
            from_broadcaster_login: from_login,
            from_broadcaster_id: None,
            to_broadcaster_id: to_id,
            to_broadcaster_login: None,
        }
    }

    fn svc_always_partner_known_with_id() -> ArrivalConfirmationService {
        ArrivalConfirmationService::new(Box::new(AlwaysPartner), Box::new(AlwaysKnownWithId))
    }

    fn svc_never_partner() -> ArrivalConfirmationService {
        ArrivalConfirmationService::new(Box::new(NeverPartner), Box::new(NeverKnown))
    }

    // -----------------------------------------------------------------------
    // classify_partner_raid_arrival — alle vier Pfade (partner_resolution.py)
    // -----------------------------------------------------------------------

    #[test]
    fn resolution_pfad1_non_partner_target() {
        // partner_resolution.py Z. 102–111: Ziel ist kein Partner
        let res = classify_partner_raid_arrival(
            Some("raider"),
            Some("rid123"),
            Some("to_id"),
            Some("to_login"),
            &NeverPartner,
            &NeverKnown,
        );
        assert_eq!(res.classification, None);
        assert_eq!(res.source_resolution, "non_partner_target");
        assert!(!res.target_is_partner);
        // from_broadcaster_id muss trimmed zurückkommen
        assert_eq!(res.from_broadcaster_id.as_deref(), Some("rid123"));
        assert_eq!(res.from_broadcaster_login, "raider");
    }

    #[test]
    fn resolution_pfad2_ours_to_partner_mit_id() {
        // partner_resolution.py Z. 113–131: Ziel Partner + Quelle bekannt (ID vorhanden)
        let res = classify_partner_raid_arrival(
            Some("our_raider"),
            Some("oid"),
            Some("to_id"),
            Some("partner_login"),
            &AlwaysPartner,
            &AlwaysKnownWithId,
        );
        assert_eq!(res.classification.as_deref(), Some("ours_to_partner"));
        assert_eq!(res.source_resolution, "known_streamer_id");
        assert!(res.target_is_partner);
    }

    #[test]
    fn resolution_pfad2_ours_to_partner_login_only() {
        // partner_resolution.py Z. 118–119: known_source ohne ID-Feld
        let res = classify_partner_raid_arrival(
            Some("our_raider"),
            None,
            Some("to_id"),
            Some("partner_login"),
            &AlwaysPartner,
            &AlwaysKnownLoginOnly,
        );
        assert_eq!(res.classification.as_deref(), Some("ours_to_partner"));
        assert_eq!(res.source_resolution, "known_streamer_login");
        assert!(res.target_is_partner);
    }

    #[test]
    fn resolution_pfad3_unknown_source_to_partner() {
        // partner_resolution.py Z. 133–142: Ziel Partner, aber keine Quell-Identität
        let res = classify_partner_raid_arrival(
            None,
            None,
            Some("to_id"),
            Some("partner_login"),
            &AlwaysPartner,
            &NeverKnown,
        );
        assert_eq!(
            res.classification.as_deref(),
            Some("unknown_source_to_partner")
        );
        assert_eq!(res.source_resolution, "missing_source_identity");
        assert!(res.target_is_partner);
        assert!(res.from_broadcaster_id.is_none());
        assert!(res.from_broadcaster_login.is_empty());
    }

    #[test]
    fn resolution_pfad4_external_to_partner() {
        // partner_resolution.py Z. 144–152: Ziel Partner, Quelle nicht bekannt
        let res = classify_partner_raid_arrival(
            Some("external_raider"),
            Some("ext_id"),
            Some("to_id"),
            Some("partner_login"),
            &AlwaysPartner,
            &NeverKnown,
        );
        assert_eq!(res.classification.as_deref(), Some("external_to_partner"));
        assert_eq!(res.source_resolution, "unmatched_source");
        assert!(res.target_is_partner);
        assert_eq!(res.from_broadcaster_id.as_deref(), Some("ext_id"));
        assert_eq!(res.from_broadcaster_login, "external_raider");
    }

    #[test]
    fn resolution_normalisiert_eingaben() {
        // Login lowercase + trim, ID trim (partner_resolution.py Z. 92–95)
        let res = classify_partner_raid_arrival(
            Some("  RAIDER  "),
            Some("  id_x  "),
            Some("  to_id  "),
            Some("  PARTNER  "),
            &NeverPartner,
            &NeverKnown,
        );
        assert_eq!(res.from_broadcaster_login, "raider");
        assert_eq!(res.from_broadcaster_id.as_deref(), Some("id_x"));
        assert_eq!(res.to_broadcaster_id, "to_id");
        assert_eq!(res.to_broadcaster_login, "partner");
    }

    // -----------------------------------------------------------------------
    // ArrivalConfirmationService::confirm_pending_raid_arrival
    // -----------------------------------------------------------------------

    #[test]
    fn confirm_ours_to_partner_classification_flags() {
        // Konstellation: Ziel=Partner, Quelle bekannt (ID) → ours_to_partner
        // Flags: should_track_confirmed_partner_raid=true, should_load_recent_history=true
        let svc = svc_always_partner_known_with_id();
        let raid = make_pending("our_streamer", "partner_id");
        let dec = svc
            .confirm_pending_raid_arrival(
                raid,
                &sig_ctx("our_streamer", "partner_id"),
                "channel.raid",
                None,
                None,
                None,
            )
            .unwrap();
        assert_eq!(dec.classification.as_deref(), Some("ours_to_partner"));
        assert_eq!(dec.follow_up_kind, FollowUpKind::Partner);
        assert!(dec.should_track_confirmed_partner_raid);
        assert!(dec.should_send_partner_raid_message);
        assert!(dec.should_refresh_partner_score_cache);
        assert!(dec.should_load_recent_raid_history_reference);
        // AlwaysPartner → target_is_partner=true → delete_blacklist_pending=true (Z. 133)
        assert!(dec.target_is_partner);
        assert!(dec.should_delete_external_recruitment_blacklist_pending);
        assert!(!dec.should_persist_confirmed_external_recruitment_raid);
        assert!(!dec.should_send_recruitment_message);
        assert!(dec.suppression_reason.is_none());
        assert_eq!(dec.signal_type, "channel.raid");
    }

    #[test]
    fn confirm_target_is_partner_without_our_confirmation_suppressed_external() {
        // Konstellation: Ziel=Partner, Quelle NICHT bekannt → external_to_partner
        // follow_up_kind = suppressed_external (arrival_confirmation.py Z. 122–124)
        let svc = ArrivalConfirmationService::new(Box::new(AlwaysPartner), Box::new(NeverKnown));
        let mut raid = make_pending("external_streamer", "partner_id");
        raid.from_broadcaster_login = "external_streamer".to_string();
        let dec = svc
            .confirm_pending_raid_arrival(
                raid,
                &sig_ctx("external_streamer", "partner_id"),
                "channel.raid",
                None,
                None,
                None,
            )
            .unwrap();
        // external_to_partner → kein Override → kein is_partner_raid → kein Überschreiben
        assert_eq!(dec.classification.as_deref(), Some("external_to_partner"));
        assert_eq!(dec.follow_up_kind, FollowUpKind::SuppressedExternal);
        assert_eq!(
            dec.suppression_reason.as_deref(),
            Some("partner_target_without_our_raid_confirmation")
        );
        assert!(!dec.should_track_confirmed_partner_raid);
        assert!(!dec.should_send_recruitment_message);
        // target_is_partner=true → delete_blacklist_pending=true (Z. 133)
        assert!(dec.should_delete_external_recruitment_blacklist_pending);
    }

    #[test]
    fn confirm_non_partner_non_partner_raid_external() {
        // Konstellation: Ziel KEIN Partner, kein is_partner_raid → external (Z. 125–127)
        let svc = svc_never_partner();
        let raid = make_pending("external_raider", "normal_channel");
        let dec = svc
            .confirm_pending_raid_arrival(
                raid,
                &sig_ctx("external_raider", "normal_channel"),
                "channel.chat.notification",
                None,
                None,
                None,
            )
            .unwrap();
        assert_eq!(dec.classification, None);
        assert_eq!(dec.follow_up_kind, FollowUpKind::External);
        assert!(dec.suppression_reason.is_none());
        assert!(dec.should_persist_confirmed_external_recruitment_raid);
        assert!(dec.should_schedule_external_recruitment_blacklist_pending);
        assert!(dec.should_send_recruitment_message);
        assert!(!dec.should_track_confirmed_partner_raid);
        assert!(!dec.should_delete_external_recruitment_blacklist_pending);
    }

    #[test]
    fn confirm_partner_raid_flag_aber_non_partner_target_suppressed() {
        // Konstellation: is_partner_raid=true aber Ziel kein Partner
        // → suppressed_external, reason=pending_partner_raid_later_resolved_non_partner
        // (arrival_confirmation.py Z. 128–130)
        let svc = svc_never_partner();
        let raid = make_partner_pending("our_partner_src", "non_partner_target");
        let dec = svc
            .confirm_pending_raid_arrival(
                raid,
                &sig_ctx("our_partner_src", "non_partner_target"),
                "channel.raid",
                None,
                None,
                None,
            )
            .unwrap();
        assert!(dec.pending_is_partner_raid);
        assert!(!dec.target_is_partner);
        assert_eq!(dec.follow_up_kind, FollowUpKind::SuppressedExternal);
        assert_eq!(
            dec.suppression_reason.as_deref(),
            Some("pending_partner_raid_later_resolved_non_partner")
        );
        // is_partner_raid=true aber ours_to_partner=false → should_load=true (Z. 132)
        assert!(dec.should_load_recent_raid_history_reference);
        assert!(!dec.should_track_confirmed_partner_raid);
    }

    #[test]
    fn confirm_partner_raid_override_auf_ours_to_partner() {
        // Konstellation: is_partner_raid=true, target_is_partner=true, aber raw
        // classification wäre z.B. external_to_partner → Override auf ours_to_partner
        // (arrival_confirmation.py Z. 115–117)
        let svc = ArrivalConfirmationService::new(
            Box::new(AlwaysPartner),
            Box::new(NeverKnown), // → external_to_partner raw
        );
        let raid = make_partner_pending("src", "partner_id");
        // is_partner_raid=true + target_is_partner=true → forciert ours_to_partner
        let dec = svc
            .confirm_pending_raid_arrival(
                raid,
                &sig_ctx("src", "partner_id"),
                "channel.raid",
                None,
                None,
                None,
            )
            .unwrap();
        assert_eq!(dec.classification.as_deref(), Some("ours_to_partner"));
        assert_eq!(dec.source_resolution, "pending_partner_raid");
        assert_eq!(dec.follow_up_kind, FollowUpKind::Partner);
        assert!(dec.should_track_confirmed_partner_raid);
        assert!(dec.should_load_recent_raid_history_reference);
    }

    #[test]
    fn confirm_should_load_true_wenn_ours_to_partner_ohne_partner_raid_flag() {
        // should_load_recent_raid_history_reference = is_partner_raid OR ours_to_partner (Z. 132)
        // Hier: is_partner_raid=false aber ours_to_partner → should_load=true
        let svc = svc_always_partner_known_with_id();
        let raid = make_pending("src", "partner_id"); // is_partner_raid=false
        let dec = svc
            .confirm_pending_raid_arrival(
                raid,
                &sig_ctx("src", "partner_id"),
                "channel.raid",
                None,
                None,
                None,
            )
            .unwrap();
        assert!(!dec.pending_is_partner_raid);
        assert_eq!(dec.classification.as_deref(), Some("ours_to_partner"));
        // ours_to_partner → should_load=true
        assert!(dec.should_load_recent_raid_history_reference);
    }

    #[test]
    fn confirm_classification_override_funktioniert() {
        // classification_override = Some(Some("custom")) überschreibt raw_resolution
        let svc = svc_never_partner();
        let raid = make_pending("src", "target");
        let dec = svc
            .confirm_pending_raid_arrival(
                raid,
                &sig_ctx("src", "target"),
                "channel.raid",
                Some(Some("custom_classification".to_string())),
                None,
                None,
            )
            .unwrap();
        assert_eq!(dec.classification.as_deref(), Some("custom_classification"));
    }

    #[test]
    fn confirm_target_is_partner_override_true() {
        // target_is_partner_override=Some(true) überschreibt Lookup (Z. 109–113)
        // Lookup sagt NeverPartner, aber Override=true
        let svc = svc_never_partner();
        let raid = make_pending("src", "target");
        let dec = svc
            .confirm_pending_raid_arrival(
                raid,
                &sig_ctx("src", "target"),
                "channel.raid",
                None,
                None,
                Some(true),
            )
            .unwrap();
        assert!(dec.target_is_partner);
        // external_to_partner wäre nicht gesetzt (NeverKnown), classification bleibt None,
        // aber target_is_partner=true → suppressed_external
        assert_eq!(dec.follow_up_kind, FollowUpKind::SuppressedExternal);
    }

    #[test]
    fn confirm_signal_type_wird_getrimmt() {
        // signal_type: `str(signal_type or "").strip()` (Z. 142)
        let svc = svc_never_partner();
        let raid = make_pending("src", "target");
        let dec = svc
            .confirm_pending_raid_arrival(
                raid,
                &sig_ctx("src", "target"),
                "  channel.raid  ",
                None,
                None,
                None,
            )
            .unwrap();
        assert_eq!(dec.signal_type, "channel.raid");
    }

    #[test]
    fn confirm_follow_up_kind_as_str() {
        assert_eq!(FollowUpKind::Partner.as_str(), "partner");
        assert_eq!(FollowUpKind::External.as_str(), "external");
        assert_eq!(
            FollowUpKind::SuppressedExternal.as_str(),
            "suppressed_external"
        );
    }

    // -----------------------------------------------------------------------
    // should_delete_external_recruitment_blacklist_pending pinnen (Z. 133)
    // -----------------------------------------------------------------------

    #[test]
    fn delete_blacklist_pending_nur_wenn_target_is_partner() {
        // non_partner_target → false (Z. 133)
        let svc = svc_never_partner();
        let raid = make_pending("src", "target");
        let dec = svc
            .confirm_pending_raid_arrival(raid, &sig_ctx("src", "target"), "sig", None, None, None)
            .unwrap();
        assert!(!dec.should_delete_external_recruitment_blacklist_pending);

        // partner target → true
        let svc2 = svc_always_partner_known_with_id();
        let raid2 = make_pending("src", "target");
        let dec2 = svc2
            .confirm_pending_raid_arrival(raid2, &sig_ctx("src", "target"), "sig", None, None, None)
            .unwrap();
        assert!(dec2.should_delete_external_recruitment_blacklist_pending);
    }

    // -----------------------------------------------------------------------
    // P1.13: expected_partner-Override für (noch) nicht eingetragene Partner-Ziele
    // (partner_arrival_tracking.py Z. 76–90, 140–151)
    // -----------------------------------------------------------------------

    #[test]
    fn classify_expected_partner_override_macht_non_partner_zu_partner() {
        // Erster Pass: Ziel kein Partner (NeverPartner) + Quelle unbekannt
        // → classification=None. expected_partner=true erzwingt zweiten Pass mit
        // synthetischem Partner-Lookup → unknown_source_to_partner, target=true.
        let res = classify_partner_raid_arrival_with_expectation(
            None,
            None,
            Some("to_id"),
            Some("not_registered_partner"),
            &NeverPartner,
            &NeverKnown,
            true,
        );
        assert_eq!(
            res.classification.as_deref(),
            Some("unknown_source_to_partner")
        );
        assert!(res.target_is_partner);
    }

    #[test]
    fn classify_expected_partner_false_bleibt_non_partner_target() {
        // Ohne expected_partner bleibt es bei non_partner_target (kein zweiter Pass).
        let res = classify_partner_raid_arrival_with_expectation(
            None,
            None,
            Some("to_id"),
            Some("not_registered_partner"),
            &NeverPartner,
            &NeverKnown,
            false,
        );
        assert_eq!(res.classification, None);
        assert_eq!(res.source_resolution, "non_partner_target");
        assert!(!res.target_is_partner);
    }

    #[test]
    fn confirm_partner_raid_zu_nicht_eingetragenem_partner_ist_partner_nicht_suppressed() {
        // KERN-CONTRACT P1.13: Pending-Raid is_partner_raid=true, Ziel NICHT in
        // Partner-Tabelle (NeverPartner), Quelle unbekannt (NeverKnown).
        //
        // Dieser Test modelliert exakt den AUFRUFER-Pfad aus
        // runtime_factories.py Z. 546–563 (künftig bin/tb-bot raid_arrival_wiring,
        // siehe WIRING-TODO(P1.13)):
        //   1. classify_partner_raid_arrival_with_expectation(.., expected_partner=is_partner_raid)
        //   2. confirm_pending_raid_arrival(..,
        //        classification_override=Some(classification),
        //        source_resolution_override=Some(source_resolution),
        //        target_is_partner_override=Some(classification.is_some()))
        //
        // Erwartung: ours_to_partner, target_is_partner=true, follow_up_kind=Partner
        // — NICHT suppressed_external (der alte fehlerhafte Pfad).
        let svc = svc_never_partner();
        let raid = make_partner_pending("our_src", "unregistered_partner_id");

        // Schritt 1: Aufrufer-seitige Klassifikation mit expected_partner.
        let resolution = classify_partner_raid_arrival_with_expectation(
            Some("our_src"),
            None,
            Some("unregistered_partner_id"),
            Some("unregistered_partner_login"),
            &NeverPartner,
            &NeverKnown,
            raid.is_partner_raid, // expected_partner = is_partner_raid
        );
        let target_is_partner_override = resolution.classification.is_some();

        // Schritt 2: confirm mit den durchgereichten Overrides.
        let dec = svc
            .confirm_pending_raid_arrival(
                raid,
                &ArrivalSignalContext {
                    from_broadcaster_login: "our_src",
                    from_broadcaster_id: None,
                    to_broadcaster_id: "unregistered_partner_id",
                    to_broadcaster_login: Some("unregistered_partner_login"),
                },
                "channel.chat.notification",
                Some(resolution.classification.clone()),
                Some(Some(resolution.source_resolution.clone())),
                Some(target_is_partner_override),
            )
            .unwrap();

        assert!(target_is_partner_override, "Klassifikation darf nicht None sein");
        assert_ne!(dec.follow_up_kind, FollowUpKind::SuppressedExternal);
        assert!(dec.target_is_partner, "expected_partner muss target erzwingen");
        assert_eq!(
            dec.classification.as_deref(),
            Some("ours_to_partner"),
            "is_partner_raid + expected_partner-Pass + arrival-Override → ours_to_partner"
        );
        assert_eq!(dec.follow_up_kind, FollowUpKind::Partner);
        assert!(dec.should_track_confirmed_partner_raid);
        assert!(dec.should_send_partner_raid_message);
        assert!(dec.should_refresh_partner_score_cache);
    }

    #[test]
    fn confirm_reicht_from_broadcaster_id_an_klassifikation_durch() {
        // Beweist den Fix: mit from_broadcaster_id=Some(..) UND Ziel=Partner +
        // known-streamer-mit-ID klassifiziert der SERVICE als known_streamer_id
        // (nicht known_streamer_login). Vorher war from_id im Service hartcodiert None.
        let svc = svc_always_partner_known_with_id();
        let raid = make_pending("our_streamer", "partner_id");
        let ctx = ArrivalSignalContext {
            from_broadcaster_login: "our_streamer",
            from_broadcaster_id: Some("src_id_123"),
            to_broadcaster_id: "partner_id",
            to_broadcaster_login: Some("partner_login"),
        };
        let dec = svc
            .confirm_pending_raid_arrival(raid, &ctx, "channel.raid", None, None, None)
            .unwrap();
        assert_eq!(dec.classification.as_deref(), Some("ours_to_partner"));
        assert_eq!(
            dec.source_resolution, "known_streamer_id",
            "from_broadcaster_id muss durchgereicht werden → known_streamer_id-Zweig"
        );
    }
}
