//! Signal-Korrelations-Planer für Raid-Signale — reine Planungs-Engine ohne DB-Zugriff.
//!
//! Port von `bot/raid/signal_correlation.py` (500 Z., Schritt 6e-Logik).
//!
//! ## Aufgabe
//!
//! Die Engine empfängt Raid-Signale (EventSub `channel.raid`, Chat-Notification,
//! Chat-Unraid) zusammen mit dem aktuellen Pending-Raid-Zustand und gibt einen
//! [`RaidSignalPlan`] mit typisierten Actions zurück. **Keine Seiteneffekte,
//! kein DB-Zugriff** — die Ausführung der Actions liegt beim Aufrufer.
//!
//! ## Drei Planer
//!
//! | Methode                  | Signal                           | Python-Herkunft |
//! |--------------------------|----------------------------------|-----------------|
//! | `plan_raid_arrival`      | EventSub `channel.raid`          | Z. 85–186       |
//! | `plan_chat_notification` | Chat `channel.chat.notification` | Z. 188–307      |
//! | `plan_chat_unraid`       | Chat Unraid                      | Z. 309–377      |

use crate::pending_raids::{normalize_broadcaster_login, PendingRaid};

// ---------------------------------------------------------------------------
// Signal-Typ / Outcome / ActionKind
// ---------------------------------------------------------------------------

/// Welcher Twitch-Signal-Typ das Ereignis ausgelöst hat.
///
/// Port von `RaidSignalType` (signal_correlation.py Z. 9–13).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RaidSignalType {
    /// EventSub `channel.raid`.
    ChannelRaid,
    /// Chat-Notification `channel.chat.notification`.
    ChannelChatNotification,
    /// Chat-Unraid `channel.chat.notification.unraid`.
    ChannelChatNotificationUnraid,
}

impl RaidSignalType {
    /// String-Repräsentation identisch zum Python-`Literal`.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::ChannelRaid => "channel.raid",
            Self::ChannelChatNotification => "channel.chat.notification",
            Self::ChannelChatNotificationUnraid => "channel.chat.notification.unraid",
        }
    }
}

/// Klassifiziert das Ergebnis des Planungs-Durchlaufs.
///
/// Port von `RaidSignalOutcome` (signal_correlation.py Z. 15–23).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RaidSignalOutcome {
    /// Signal ist ein Folgesignal zu einem bereits verarbeiteten Arrival.
    SecondarySignalHandled,
    /// Pending-Raid und Signal-Quelle stimmen überein.
    PendingMatched,
    /// Pending-Raid existiert, aber Quelle stimmt nicht überein.
    PendingMismatch,
    /// Chat-Notification ohne korrespondierenden Pending-Raid.
    OrphanChatNotification,
    /// Kein Pending-Raid vorhanden, aber unabhängiger manueller Raid erkannt.
    IndependentManualArrival,
    /// Unraid beobachtet, Pending-Raid existiert.
    PendingUnraidObserved,
    /// Kein Pending-Raid vorhanden.
    NoPending,
}

impl RaidSignalOutcome {
    /// String-Repräsentation identisch zum Python-`Literal`.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::SecondarySignalHandled => "secondary_signal_handled",
            Self::PendingMatched => "pending_matched",
            Self::PendingMismatch => "pending_mismatch",
            Self::OrphanChatNotification => "orphan_chat_notification",
            Self::IndependentManualArrival => "independent_manual_arrival",
            Self::PendingUnraidObserved => "pending_unraid_observed",
            Self::NoPending => "no_pending",
        }
    }
}

/// Typisierter Action-Bezeichner.
///
/// Port von `RaidSignalActionKind` (signal_correlation.py Z. 25–33).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RaidSignalActionKind {
    /// Sekundäres Signal in der DB vermerken (kein neuer Raid).
    RecordSecondarySignal,
    /// Beobachtung zu einem Pending-Raid eintragen (diagnostisch).
    RecordPendingObservation,
    /// Pending-Raid speichern oder aktualisieren.
    StorePendingRaid,
    /// Pending-Raid als bestätigten Arrival abschließen.
    ConfirmPendingRaid,
    /// Verwaiste Chat-Notification ohne Pending-Kontext speichern.
    StoreOrphanChatNotification,
    /// Manuellen Raid als gestartet markieren (setzt TTL-Lock).
    MarkManualRaidStarted,
    /// Unabhängigen Raid-Arrival ohne Pending-Kontext aufzeichnen.
    RecordIndependentRaidArrival,
}

impl RaidSignalActionKind {
    /// String-Repräsentation identisch zum Python-`Literal`.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::RecordSecondarySignal => "record_secondary_signal",
            Self::RecordPendingObservation => "record_pending_observation",
            Self::StorePendingRaid => "store_pending_raid",
            Self::ConfirmPendingRaid => "confirm_pending_raid",
            Self::StoreOrphanChatNotification => "store_orphan_chat_notification",
            Self::MarkManualRaidStarted => "mark_manual_raid_started",
            Self::RecordIndependentRaidArrival => "record_independent_raid_arrival",
        }
    }
}

// ---------------------------------------------------------------------------
// ActionData — typisierte Nutzdaten je Action-Variante
// ---------------------------------------------------------------------------

/// Typisierte Nutzdaten zu einer Action.
///
/// In Python ist `data` ein freies `dict[str, Any]`. Rust modelliert jeden
/// strukturell unterscheidbaren Fall als eigene Variante.
#[derive(Debug, Clone)]
pub enum ActionData {
    /// `record_secondary_signal` (signal_correlation.py Z. 400–413).
    SecondarySignal {
        signal_type: &'static str,
        from_broadcaster_login: String,
        from_broadcaster_id: Option<String>,
        to_broadcaster_login: String,
        to_broadcaster_id: String,
        viewer_count: i32,
        unraid_seen: bool,
    },
    /// `record_pending_observation` (Z. 141–150, 260–270, 363–375).
    PendingObservation {
        pending_raid: PendingRaid,
        signal_type: &'static str,
        status: &'static str,
        reason: Option<&'static str>,
        detail: Option<String>,
    },
    /// `store_pending_raid` (Z. 150, 173, 270, 294, 374).
    StorePendingRaid { pending_raid: PendingRaid },
    /// `confirm_pending_raid` (Z. 166–184, Z. 295–306).
    ConfirmPendingRaid {
        signal_type: &'static str,
        to_broadcaster_id: String,
        to_broadcaster_login: String,
        from_broadcaster_login: String,
        from_broadcaster_id: Option<String>,
        viewer_count: i32,
    },
    /// `store_orphan_chat_notification` (Z. 231–244).
    OrphanChatNotification {
        to_broadcaster_id: String,
        to_broadcaster_login: String,
        from_broadcaster_id: Option<String>,
        from_broadcaster_login: String,
        viewer_count: i32,
        message_id: Option<String>,
        event_timestamp: Option<String>,
    },
    /// `mark_manual_raid_started` (Z. 444–451).
    MarkManualRaidStarted {
        source_key: String,
        ttl_seconds: f64,
    },
    /// `record_independent_raid_arrival` (Z. 452–477).
    IndependentRaidArrival {
        signal_type: &'static str,
        from_broadcaster_login: String,
        from_broadcaster_id: Option<String>,
        to_broadcaster_login: String,
        to_broadcaster_id: String,
        viewer_count: i32,
    },
}

/// Eine einzelne typisierte Action im Plan.
///
/// Port von `RaidSignalAction` (signal_correlation.py Z. 36–39).
#[derive(Debug, Clone)]
pub struct RaidSignalAction {
    pub kind: RaidSignalActionKind,
    pub data: ActionData,
}

/// Ergebnis eines Planungs-Durchlaufs — reine Datenstruktur, keine Seiteneffekte.
///
/// Port von `RaidSignalPlan` (signal_correlation.py Z. 42–57).
#[derive(Debug, Clone)]
pub struct RaidSignalPlan {
    pub signal_type: RaidSignalType,
    pub outcome: RaidSignalOutcome,
    pub from_broadcaster_login: String,
    pub from_broadcaster_id: Option<String>,
    pub to_broadcaster_login: String,
    pub to_broadcaster_id: String,
    pub viewer_count: i32,
    /// Pending-Raid-Kontext, falls relevant für diesen Plan.
    pub pending_raid: Option<PendingRaid>,
    pub actions: Vec<RaidSignalAction>,
    /// Optionaler Grund-String (Python `reason`, Z. 53).
    pub reason: Option<String>,
}

impl RaidSignalPlan {
    /// `true` wenn das Signal als Folgesignal klassifiziert wurde und die Pipeline
    /// kurz geschlossen werden kann.
    ///
    /// Port von `RaidSignalPlan.is_short_circuit` (signal_correlation.py Z. 56–57).
    pub fn is_short_circuit(&self) -> bool {
        self.outcome == RaidSignalOutcome::SecondarySignalHandled
    }
}

// ---------------------------------------------------------------------------
// Request-Structs — vermeiden zu viele Argumente an den öffentlichen Methoden
// ---------------------------------------------------------------------------

/// Eingaben für [`RaidSignalCorrelationService::plan_raid_arrival`].
///
/// Gruppiert die Parameter von `plan_raid_arrival`
/// (signal_correlation.py Z. 85–97).
pub struct RaidArrivalInput {
    pub to_broadcaster_id: String,
    pub to_broadcaster_login: String,
    pub from_broadcaster_login: String,
    pub from_broadcaster_id: Option<String>,
    pub viewer_count: i32,
    pub pending_raid: Option<PendingRaid>,
    pub recent_arrival_present: bool,
    /// Ob ein unabhängiger manueller Raid erkannt wurde.
    /// Python: `independent_manual_detected` (Z. 95), Default `False`.
    pub independent_manual_detected: bool,
    /// Optionaler Source-Key für manuellen Raid (Python Z. 96).
    pub manual_raid_source_key: Option<String>,
}

/// Eingaben für [`RaidSignalCorrelationService::plan_chat_notification`].
///
/// Gruppiert die Parameter von `plan_chat_notification`
/// (signal_correlation.py Z. 188–200).
pub struct ChatNotificationInput {
    pub to_broadcaster_id: String,
    pub to_broadcaster_login: String,
    pub from_broadcaster_login: String,
    pub from_broadcaster_id: Option<String>,
    pub viewer_count: i32,
    pub message_id: Option<String>,
    pub event_timestamp: Option<String>,
    pub pending_raid: Option<PendingRaid>,
    pub recent_arrival_present: bool,
}

/// Eingaben für [`RaidSignalCorrelationService::plan_chat_unraid`].
///
/// Gruppiert die Parameter von `plan_chat_unraid`
/// (signal_correlation.py Z. 309–319).
pub struct ChatUnraidInput {
    pub to_broadcaster_id: String,
    pub to_broadcaster_login: String,
    pub from_broadcaster_login: String,
    pub from_broadcaster_id: Option<String>,
    pub pending_raid: Option<PendingRaid>,
    pub recent_arrival_present: bool,
    pub event_timestamp: Option<String>,
}

// ---------------------------------------------------------------------------
// Helfer-Funktionen
// ---------------------------------------------------------------------------

/// Normalisiert eine rohe Broadcaster-ID: trim.
///
/// Port von `_normalize_target_id` (signal_correlation.py Z. 60–61).
fn normalize_target_id(raw: &str) -> String {
    raw.trim().to_string()
}

/// Normalisiert einen optionalen Detail-String: trim + `None` wenn leer.
///
/// Port von `_normalize_detail` (signal_correlation.py Z. 64–66).
fn normalize_detail(raw: Option<&str>) -> Option<String> {
    let text = raw.unwrap_or("").trim().to_string();
    if text.is_empty() {
        None
    } else {
        Some(text)
    }
}

/// Koerciert einen optionalen `PendingRaid` — gibt ihn normalisiert zurück oder `None`.
///
/// Port von `_coerce_pending_raid` (signal_correlation.py Z. 69–79).
///
/// **Einschränkung gegenüber Python:** Python akzeptiert zusätzlich `Mapping[str, Any]`
/// (DB-Deserialisierings-Pfad). In Rust wird nur `Option<PendingRaid>` unterstützt, da
/// dieser Zweig in der Lade-Schicht liegt, nicht im Planer.
fn coerce_pending_raid(
    pending_raid: Option<PendingRaid>,
    to_broadcaster_id: &str,
    from_broadcaster_login: &str,
) -> Option<PendingRaid> {
    let mut raid = pending_raid?;
    // Fehlende Felder nachfüllen — identisch zum PendingRaid-Zweig in from_payload (Z. 197–207).
    if raid.to_broadcaster_id.is_empty() {
        raid.to_broadcaster_id = to_broadcaster_id.trim().to_string();
    }
    if raid.from_broadcaster_login.is_empty() {
        raid.from_broadcaster_login = normalize_broadcaster_login(from_broadcaster_login);
    }
    Some(raid)
}

// ---------------------------------------------------------------------------
// Kern-Planer
// ---------------------------------------------------------------------------

/// Reine Planungs-Engine für Raid-Signal-Korrelation.
///
/// Port von `RaidSignalCorrelationService` (signal_correlation.py Z. 82–490).
/// Alle Methoden sind zustandslos — Eingaben rein, Plan raus.
pub struct RaidSignalCorrelationService;

impl RaidSignalCorrelationService {
    // -----------------------------------------------------------------------
    // plan_raid_arrival
    // -----------------------------------------------------------------------

    /// Erstellt einen Plan für ein eintreffendes EventSub `channel.raid`-Signal.
    ///
    /// Port von `plan_raid_arrival` (signal_correlation.py Z. 85–186).
    ///
    /// **Klassifikations-Pfade:**
    ///
    /// 1. `recent_arrival_present` → `secondary_signal_handled` (short-circuit)
    /// 2. `pending=None` → `_independent_or_empty_plan`
    ///    (`no_pending` oder `independent_manual_arrival`)
    /// 3. `pending.from != normalized_from` → `pending_mismatch`
    ///    (`record_pending_observation` + `store_pending_raid`)
    /// 4. Match → `pending_matched`
    ///    (`record_pending_observation` + `store_pending_raid` + `confirm_pending_raid`)
    pub fn plan_raid_arrival(&self, input: RaidArrivalInput) -> RaidSignalPlan {
        let RaidArrivalInput {
            to_broadcaster_id,
            to_broadcaster_login,
            from_broadcaster_login,
            from_broadcaster_id,
            viewer_count,
            pending_raid,
            recent_arrival_present,
            independent_manual_detected,
            manual_raid_source_key,
        } = input;

        let normalized_from = normalize_broadcaster_login(&from_broadcaster_login);
        let normalized_to = normalize_broadcaster_login(&to_broadcaster_login);
        let target_id = normalize_target_id(&to_broadcaster_id);

        // Pfad 1: Sekundärsignal (Z. 102–110)
        if recent_arrival_present {
            return self.secondary_signal_plan(
                RaidSignalType::ChannelRaid,
                normalized_from,
                from_broadcaster_id,
                normalized_to,
                target_id,
                viewer_count,
                false,
            );
        }

        // Pending koercieren (Z. 112–116)
        let pending = coerce_pending_raid(pending_raid, &target_id, &normalized_from);

        // Pfad 2: kein Pending (Z. 117–127)
        if pending.is_none() {
            return self.independent_or_empty_plan(
                RaidSignalType::ChannelRaid,
                normalized_from,
                from_broadcaster_id,
                normalized_to,
                target_id,
                viewer_count,
                independent_manual_detected,
                manual_raid_source_key,
            );
        }
        let pending = pending.unwrap();

        // Pfad 3: Mismatch (Z. 129–153)
        if pending.from_broadcaster_login != normalized_from {
            let detail = format!(
                "expected={} actual={}",
                pending.from_broadcaster_login, normalized_from
            );
            return RaidSignalPlan {
                signal_type: RaidSignalType::ChannelRaid,
                outcome: RaidSignalOutcome::PendingMismatch,
                from_broadcaster_login: normalized_from,
                from_broadcaster_id,
                to_broadcaster_login: normalized_to,
                to_broadcaster_id: target_id,
                viewer_count,
                pending_raid: Some(pending.clone()),
                actions: vec![
                    RaidSignalAction {
                        kind: RaidSignalActionKind::RecordPendingObservation,
                        data: ActionData::PendingObservation {
                            pending_raid: pending.clone(),
                            signal_type: "channel.raid",
                            status: "ignored",
                            reason: Some("source_target_mismatch"),
                            detail: Some(detail),
                        },
                    },
                    RaidSignalAction {
                        kind: RaidSignalActionKind::StorePendingRaid,
                        data: ActionData::StorePendingRaid {
                            pending_raid: pending,
                        },
                    },
                ],
                reason: Some("source_target_mismatch".to_string()),
            };
        }

        // Pfad 4: Match (Z. 155–186)
        RaidSignalPlan {
            signal_type: RaidSignalType::ChannelRaid,
            outcome: RaidSignalOutcome::PendingMatched,
            from_broadcaster_login: normalized_from.clone(),
            from_broadcaster_id: from_broadcaster_id.clone(),
            to_broadcaster_login: normalized_to.clone(),
            to_broadcaster_id: target_id.clone(),
            viewer_count,
            pending_raid: Some(pending.clone()),
            actions: vec![
                RaidSignalAction {
                    kind: RaidSignalActionKind::RecordPendingObservation,
                    data: ActionData::PendingObservation {
                        pending_raid: pending.clone(),
                        signal_type: "channel.raid",
                        status: "matched_pending",
                        reason: None,
                        detail: None,
                    },
                },
                RaidSignalAction {
                    kind: RaidSignalActionKind::StorePendingRaid,
                    data: ActionData::StorePendingRaid {
                        pending_raid: pending,
                    },
                },
                RaidSignalAction {
                    kind: RaidSignalActionKind::ConfirmPendingRaid,
                    data: ActionData::ConfirmPendingRaid {
                        signal_type: "channel.raid",
                        to_broadcaster_id: target_id,
                        to_broadcaster_login: normalized_to,
                        from_broadcaster_login: normalized_from,
                        from_broadcaster_id,
                        viewer_count,
                    },
                },
            ],
            reason: None,
        }
    }

    // -----------------------------------------------------------------------
    // plan_chat_notification
    // -----------------------------------------------------------------------

    /// Erstellt einen Plan für ein `channel.chat.notification`-Signal.
    ///
    /// Port von `plan_chat_notification` (signal_correlation.py Z. 188–307).
    ///
    /// **Klassifikations-Pfade:**
    ///
    /// 1. `recent_arrival_present` → `secondary_signal_handled` (short-circuit)
    /// 2. `pending=None` → `orphan_chat_notification`
    ///    (`store_orphan_chat_notification` mit vollem Payload)
    /// 3. `pending.from != normalized_from` → `pending_mismatch`
    ///    (`record_pending_observation` + `store_pending_raid`)
    /// 4. Match → `pending_matched`
    ///    (`record_pending_observation` mit `message_id` als detail
    ///    + `store_pending_raid` + `confirm_pending_raid`)
    pub fn plan_chat_notification(&self, input: ChatNotificationInput) -> RaidSignalPlan {
        let ChatNotificationInput {
            to_broadcaster_id,
            to_broadcaster_login,
            from_broadcaster_login,
            from_broadcaster_id,
            viewer_count,
            message_id,
            event_timestamp,
            pending_raid,
            recent_arrival_present,
        } = input;

        let normalized_from = normalize_broadcaster_login(&from_broadcaster_login);
        let normalized_to = normalize_broadcaster_login(&to_broadcaster_login);
        let target_id = normalize_target_id(&to_broadcaster_id);

        // Pfad 1: Sekundärsignal (Z. 205–213)
        if recent_arrival_present {
            return self.secondary_signal_plan(
                RaidSignalType::ChannelChatNotification,
                normalized_from,
                from_broadcaster_id,
                normalized_to,
                target_id,
                viewer_count,
                false,
            );
        }

        // Pending koercieren (Z. 215–219)
        let pending = coerce_pending_raid(pending_raid, &target_id, &normalized_from);

        // Pfad 2: Orphan (Z. 220–247)
        if pending.is_none() {
            // from_broadcaster_id im Orphan-Payload: `str(x or "").strip() or None` (Z. 237)
            let fbi = from_broadcaster_id.as_deref().and_then(|s| {
                let t = s.trim().to_string();
                if t.is_empty() {
                    None
                } else {
                    Some(t)
                }
            });
            return RaidSignalPlan {
                signal_type: RaidSignalType::ChannelChatNotification,
                outcome: RaidSignalOutcome::OrphanChatNotification,
                from_broadcaster_login: normalized_from.clone(),
                from_broadcaster_id,
                to_broadcaster_login: normalized_to.clone(),
                to_broadcaster_id: target_id.clone(),
                viewer_count,
                pending_raid: None,
                actions: vec![RaidSignalAction {
                    kind: RaidSignalActionKind::StoreOrphanChatNotification,
                    data: ActionData::OrphanChatNotification {
                        to_broadcaster_id: target_id,
                        to_broadcaster_login: normalized_to,
                        from_broadcaster_id: fbi,
                        from_broadcaster_login: normalized_from,
                        viewer_count,
                        message_id: normalize_detail(message_id.as_deref()),
                        event_timestamp: normalize_detail(event_timestamp.as_deref()),
                    },
                }],
                reason: Some("no_pending_raid".to_string()),
            };
        }
        let pending = pending.unwrap();

        // Pfad 3: Mismatch (Z. 249–273)
        if pending.from_broadcaster_login != normalized_from {
            let detail = format!(
                "expected={} actual={}",
                pending.from_broadcaster_login, normalized_from
            );
            return RaidSignalPlan {
                signal_type: RaidSignalType::ChannelChatNotification,
                outcome: RaidSignalOutcome::PendingMismatch,
                from_broadcaster_login: normalized_from,
                from_broadcaster_id,
                to_broadcaster_login: normalized_to,
                to_broadcaster_id: target_id,
                viewer_count,
                pending_raid: Some(pending.clone()),
                actions: vec![
                    RaidSignalAction {
                        kind: RaidSignalActionKind::RecordPendingObservation,
                        data: ActionData::PendingObservation {
                            pending_raid: pending.clone(),
                            signal_type: "channel.chat.notification",
                            status: "ignored",
                            reason: Some("source_target_mismatch"),
                            detail: Some(detail),
                        },
                    },
                    RaidSignalAction {
                        kind: RaidSignalActionKind::StorePendingRaid,
                        data: ActionData::StorePendingRaid {
                            pending_raid: pending,
                        },
                    },
                ],
                reason: Some("source_target_mismatch".to_string()),
            };
        }

        // Pfad 4: Match (Z. 275–307)
        // Detail für record_pending_observation = message_id (Z. 291)
        let msg_detail = normalize_detail(message_id.as_deref());
        RaidSignalPlan {
            signal_type: RaidSignalType::ChannelChatNotification,
            outcome: RaidSignalOutcome::PendingMatched,
            from_broadcaster_login: normalized_from.clone(),
            from_broadcaster_id: from_broadcaster_id.clone(),
            to_broadcaster_login: normalized_to.clone(),
            to_broadcaster_id: target_id.clone(),
            viewer_count,
            pending_raid: Some(pending.clone()),
            actions: vec![
                RaidSignalAction {
                    kind: RaidSignalActionKind::RecordPendingObservation,
                    data: ActionData::PendingObservation {
                        pending_raid: pending.clone(),
                        signal_type: "channel.chat.notification",
                        status: "matched_pending",
                        reason: None,
                        detail: msg_detail,
                    },
                },
                RaidSignalAction {
                    kind: RaidSignalActionKind::StorePendingRaid,
                    data: ActionData::StorePendingRaid {
                        pending_raid: pending,
                    },
                },
                RaidSignalAction {
                    kind: RaidSignalActionKind::ConfirmPendingRaid,
                    data: ActionData::ConfirmPendingRaid {
                        signal_type: "channel.chat.notification",
                        to_broadcaster_id: target_id,
                        to_broadcaster_login: normalized_to,
                        from_broadcaster_login: normalized_from,
                        from_broadcaster_id,
                        viewer_count,
                    },
                },
            ],
            reason: None,
        }
    }

    // -----------------------------------------------------------------------
    // plan_chat_unraid
    // -----------------------------------------------------------------------

    /// Erstellt einen Plan für ein `channel.chat.notification.unraid`-Signal.
    ///
    /// Port von `plan_chat_unraid` (signal_correlation.py Z. 309–377).
    ///
    /// **Klassifikations-Pfade:**
    ///
    /// 1. `recent_arrival_present` → `secondary_signal_handled` mit `unraid_seen=true`,
    ///    `viewer_count=0` (short-circuit)
    /// 2. `pending=None` → `no_pending`, leere Actions, `reason=event_timestamp`
    /// 3. Pending vorhanden → `pending_unraid_observed`
    ///    (`record_pending_observation` status=`diagnostic_only` + `store_pending_raid`)
    ///
    /// **Wichtig:** Kein Mismatch-Pfad! Unraid bestätigt keinen Raid — es wird nur
    /// diagnostisch notiert (Z. 370: `unraid_does_not_confirm`). Im Gegensatz zu
    /// `plan_raid_arrival` und `plan_chat_notification` wird bei vorhandenem Pending
    /// IMMER `pending_unraid_observed` zurückgegeben, unabhängig ob from_login passt.
    pub fn plan_chat_unraid(&self, input: ChatUnraidInput) -> RaidSignalPlan {
        let ChatUnraidInput {
            to_broadcaster_id,
            to_broadcaster_login,
            from_broadcaster_login,
            from_broadcaster_id,
            pending_raid,
            recent_arrival_present,
            event_timestamp,
        } = input;

        let normalized_from = normalize_broadcaster_login(&from_broadcaster_login);
        let normalized_to = normalize_broadcaster_login(&to_broadcaster_login);
        let target_id = normalize_target_id(&to_broadcaster_id);

        // Pfad 1: Sekundärsignal (Z. 324–333), viewer_count=0, unraid_seen=true
        if recent_arrival_present {
            return self.secondary_signal_plan(
                RaidSignalType::ChannelChatNotificationUnraid,
                normalized_from,
                from_broadcaster_id,
                normalized_to,
                target_id,
                0,
                true,
            );
        }

        // Pending koercieren (Z. 335–339)
        let pending = coerce_pending_raid(pending_raid, &target_id, &normalized_from);

        // Pfad 2: kein Pending (Z. 340–352), reason = event_timestamp
        if pending.is_none() {
            return RaidSignalPlan {
                signal_type: RaidSignalType::ChannelChatNotificationUnraid,
                outcome: RaidSignalOutcome::NoPending,
                from_broadcaster_login: normalized_from,
                from_broadcaster_id,
                to_broadcaster_login: normalized_to,
                to_broadcaster_id: target_id,
                viewer_count: 0,
                pending_raid: None,
                actions: vec![],
                reason: normalize_detail(event_timestamp.as_deref()),
            };
        }
        let pending = pending.unwrap();

        // Pfad 3: Pending vorhanden (Z. 354–377), immer diagnostic_only
        let ts_detail = normalize_detail(event_timestamp.as_deref());
        RaidSignalPlan {
            signal_type: RaidSignalType::ChannelChatNotificationUnraid,
            outcome: RaidSignalOutcome::PendingUnraidObserved,
            from_broadcaster_login: normalized_from,
            from_broadcaster_id,
            to_broadcaster_login: normalized_to,
            to_broadcaster_id: target_id,
            viewer_count: 0,
            pending_raid: Some(pending.clone()),
            actions: vec![
                RaidSignalAction {
                    kind: RaidSignalActionKind::RecordPendingObservation,
                    data: ActionData::PendingObservation {
                        pending_raid: pending.clone(),
                        signal_type: "channel.chat.notification.unraid",
                        status: "diagnostic_only",
                        reason: Some("unraid_does_not_confirm"),
                        detail: ts_detail,
                    },
                },
                RaidSignalAction {
                    kind: RaidSignalActionKind::StorePendingRaid,
                    data: ActionData::StorePendingRaid {
                        pending_raid: pending,
                    },
                },
            ],
            reason: Some("unraid_does_not_confirm".to_string()),
        }
    }

    // -----------------------------------------------------------------------
    // Privater Helfer: secondary_signal_plan
    // -----------------------------------------------------------------------

    /// Short-Circuit-Plan für Folgesignale.
    ///
    /// Port von `_secondary_signal_plan` (signal_correlation.py Z. 379–414).
    #[allow(clippy::too_many_arguments)]
    fn secondary_signal_plan(
        &self,
        signal_type: RaidSignalType,
        from_broadcaster_login: String,
        from_broadcaster_id: Option<String>,
        to_broadcaster_login: String,
        to_broadcaster_id: String,
        viewer_count: i32,
        unraid_seen: bool,
    ) -> RaidSignalPlan {
        let sig_str = signal_type.as_str();
        RaidSignalPlan {
            signal_type,
            outcome: RaidSignalOutcome::SecondarySignalHandled,
            from_broadcaster_login: from_broadcaster_login.clone(),
            from_broadcaster_id: from_broadcaster_id.clone(),
            to_broadcaster_login: to_broadcaster_login.clone(),
            to_broadcaster_id: to_broadcaster_id.clone(),
            viewer_count,
            pending_raid: None,
            actions: vec![RaidSignalAction {
                kind: RaidSignalActionKind::RecordSecondarySignal,
                data: ActionData::SecondarySignal {
                    signal_type: sig_str,
                    from_broadcaster_login,
                    from_broadcaster_id,
                    to_broadcaster_login,
                    to_broadcaster_id,
                    viewer_count,
                    unraid_seen,
                },
            }],
            reason: Some("recent_arrival_present".to_string()),
        }
    }

    // -----------------------------------------------------------------------
    // Privater Helfer: independent_or_empty_plan
    // -----------------------------------------------------------------------

    /// Plan für Arrivals ohne Pending-Kontext.
    ///
    /// Port von `_independent_or_empty_plan` (signal_correlation.py Z. 416–490).
    ///
    /// **Klassifikations-Pfade:**
    ///
    /// - `!independent_manual_detected` → `no_pending`, leere Actions
    /// - `independent_manual_detected && source_key.is_some()` →
    ///   `independent_manual_arrival` mit
    ///   [`record_independent_raid_arrival`, `mark_manual_raid_started`]
    /// - `independent_manual_detected && source_key.is_none()` →
    ///   `independent_manual_arrival` mit [`record_independent_raid_arrival`]
    #[allow(clippy::too_many_arguments)]
    fn independent_or_empty_plan(
        &self,
        signal_type: RaidSignalType,
        from_broadcaster_login: String,
        from_broadcaster_id: Option<String>,
        to_broadcaster_login: String,
        to_broadcaster_id: String,
        viewer_count: i32,
        independent_manual_detected: bool,
        manual_raid_source_key: Option<String>,
    ) -> RaidSignalPlan {
        let sig_str = signal_type.as_str();

        // Pfad: kein manueller Raid erkannt (Z. 428–440)
        if !independent_manual_detected {
            return RaidSignalPlan {
                signal_type,
                outcome: RaidSignalOutcome::NoPending,
                from_broadcaster_login,
                from_broadcaster_id,
                to_broadcaster_login,
                to_broadcaster_id,
                viewer_count,
                pending_raid: None,
                actions: vec![],
                reason: Some("no_pending_raid".to_string()),
            };
        }

        // Arrival-Action (in beiden manuellen Pfaden vorhanden)
        let arrival_action = RaidSignalAction {
            kind: RaidSignalActionKind::RecordIndependentRaidArrival,
            data: ActionData::IndependentRaidArrival {
                signal_type: sig_str,
                from_broadcaster_login: from_broadcaster_login.clone(),
                from_broadcaster_id: from_broadcaster_id.clone(),
                to_broadcaster_login: to_broadcaster_login.clone(),
                to_broadcaster_id: to_broadcaster_id.clone(),
                viewer_count,
            },
        };

        let actions = if let Some(source_key) = manual_raid_source_key {
            // Pfad: manuell mit Source-Key (Z. 443–463)
            // source_key trimmen (Z. 448: `str(manual_raid_source_key or "").strip()`)
            let trimmed = source_key.trim().to_string();
            vec![
                arrival_action,
                RaidSignalAction {
                    kind: RaidSignalActionKind::MarkManualRaidStarted,
                    data: ActionData::MarkManualRaidStarted {
                        source_key: trimmed,
                        ttl_seconds: 180.0,
                    },
                },
            ]
        } else {
            // Pfad: manuell ohne Source-Key (Z. 464–477)
            vec![arrival_action]
        };

        RaidSignalPlan {
            signal_type,
            outcome: RaidSignalOutcome::IndependentManualArrival,
            from_broadcaster_login,
            from_broadcaster_id,
            to_broadcaster_login,
            to_broadcaster_id,
            viewer_count,
            pending_raid: None,
            actions,
            reason: Some("independent_or_manual_raid_detected".to_string()),
        }
    }
}

// ---------------------------------------------------------------------------
// Unit-Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pending_raids::PendingRaid;

    fn svc() -> RaidSignalCorrelationService {
        RaidSignalCorrelationService
    }

    fn make_pending(from: &str, to_id: &str) -> PendingRaid {
        PendingRaid::new(from, to_id)
    }

    // -----------------------------------------------------------------------
    // Helfer-Funktionen
    // -----------------------------------------------------------------------

    #[test]
    fn normalize_target_id_trimmt() {
        assert_eq!(normalize_target_id("  123  "), "123");
    }

    #[test]
    fn normalize_detail_leer_gibt_none() {
        assert_eq!(normalize_detail(None), None);
        assert_eq!(normalize_detail(Some("")), None);
        assert_eq!(normalize_detail(Some("  ")), None);
    }

    #[test]
    fn normalize_detail_trimmt_und_gibt_some() {
        assert_eq!(
            normalize_detail(Some("  hallo  ")),
            Some("hallo".to_string())
        );
    }

    #[test]
    fn coerce_none_gibt_none() {
        assert!(coerce_pending_raid(None, "id", "login").is_none());
    }

    #[test]
    fn coerce_fuellt_leere_felder() {
        let mut raid = PendingRaid::new("", "");
        raid.to_broadcaster_id = String::new();
        raid.from_broadcaster_login = String::new();
        let result = coerce_pending_raid(Some(raid), "target_123", "StreamerA").unwrap();
        assert_eq!(result.to_broadcaster_id, "target_123");
        assert_eq!(result.from_broadcaster_login, "streamera");
    }

    #[test]
    fn coerce_behaelt_bestehende_felder() {
        let raid = make_pending("existing_streamer", "existing_id");
        let result = coerce_pending_raid(Some(raid), "other_id", "other_streamer").unwrap();
        // Bestehende nicht-leere Felder werden NICHT überschrieben
        assert_eq!(result.to_broadcaster_id, "existing_id");
        assert_eq!(result.from_broadcaster_login, "existing_streamer");
    }

    // -----------------------------------------------------------------------
    // is_short_circuit
    // -----------------------------------------------------------------------

    #[test]
    fn is_short_circuit_nur_bei_secondary_signal_handled() {
        let plan = svc().plan_raid_arrival(RaidArrivalInput {
            to_broadcaster_id: "id".to_string(),
            to_broadcaster_login: "to".to_string(),
            from_broadcaster_login: "from".to_string(),
            from_broadcaster_id: None,
            viewer_count: 10,
            pending_raid: None,
            recent_arrival_present: true,
            independent_manual_detected: false,
            manual_raid_source_key: None,
        });
        assert!(plan.is_short_circuit());
        assert_eq!(plan.outcome, RaidSignalOutcome::SecondarySignalHandled);
    }

    // -----------------------------------------------------------------------
    // plan_raid_arrival — alle vier Pfade
    // -----------------------------------------------------------------------

    #[test]
    fn raid_arrival_secondary_signal() {
        // Python Z. 102–110: recent_arrival_present=true → secondary_signal_handled
        let plan = svc().plan_raid_arrival(RaidArrivalInput {
            to_broadcaster_id: "to_id".to_string(),
            to_broadcaster_login: "to_login".to_string(),
            from_broadcaster_login: "from_login".to_string(),
            from_broadcaster_id: Some("from_id".to_string()),
            viewer_count: 42,
            pending_raid: None,
            recent_arrival_present: true,
            independent_manual_detected: false,
            manual_raid_source_key: None,
        });
        assert_eq!(plan.outcome, RaidSignalOutcome::SecondarySignalHandled);
        assert_eq!(plan.actions.len(), 1);
        assert_eq!(
            plan.actions[0].kind,
            RaidSignalActionKind::RecordSecondarySignal
        );
        assert_eq!(plan.reason.as_deref(), Some("recent_arrival_present"));
        if let ActionData::SecondarySignal {
            unraid_seen,
            signal_type,
            ..
        } = &plan.actions[0].data
        {
            assert!(!unraid_seen, "channel.raid hat unraid_seen=false");
            assert_eq!(*signal_type, "channel.raid");
        } else {
            panic!("Falsche ActionData-Variante");
        }
    }

    #[test]
    fn raid_arrival_no_pending_kein_manual() {
        // Python Z. 428–440: kein Pending, kein manueller Raid → no_pending, leere Actions
        let plan = svc().plan_raid_arrival(RaidArrivalInput {
            to_broadcaster_id: "to_id".to_string(),
            to_broadcaster_login: "to_login".to_string(),
            from_broadcaster_login: "from_login".to_string(),
            from_broadcaster_id: None,
            viewer_count: 5,
            pending_raid: None,
            recent_arrival_present: false,
            independent_manual_detected: false,
            manual_raid_source_key: None,
        });
        assert_eq!(plan.outcome, RaidSignalOutcome::NoPending);
        assert!(plan.actions.is_empty());
        assert_eq!(plan.reason.as_deref(), Some("no_pending_raid"));
    }

    #[test]
    fn raid_arrival_independent_manual_mit_source_key() {
        // Manuell + source_key: erst Arrival speichern, danach Suppression setzen.
        let plan = svc().plan_raid_arrival(RaidArrivalInput {
            to_broadcaster_id: "to_id".to_string(),
            to_broadcaster_login: "to_login".to_string(),
            from_broadcaster_login: "from_login".to_string(),
            from_broadcaster_id: None,
            viewer_count: 15,
            pending_raid: None,
            recent_arrival_present: false,
            independent_manual_detected: true,
            manual_raid_source_key: Some("  src_key  ".to_string()),
        });
        assert_eq!(plan.outcome, RaidSignalOutcome::IndependentManualArrival);
        assert_eq!(plan.actions.len(), 2);
        assert_eq!(
            plan.actions[0].kind,
            RaidSignalActionKind::RecordIndependentRaidArrival
        );
        assert_eq!(
            plan.actions[1].kind,
            RaidSignalActionKind::MarkManualRaidStarted
        );
        // source_key wird getrimmt (Z. 448)
        if let ActionData::MarkManualRaidStarted {
            source_key,
            ttl_seconds,
        } = &plan.actions[1].data
        {
            assert_eq!(source_key, "src_key");
            assert_eq!(*ttl_seconds, 180.0);
        } else {
            panic!("Falsche ActionData-Variante");
        }
        assert_eq!(
            plan.reason.as_deref(),
            Some("independent_or_manual_raid_detected")
        );
    }

    #[test]
    fn raid_arrival_independent_manual_ohne_source_key() {
        // Python Z. 464–477: manuell ohne source_key → [record_independent_raid_arrival]
        let plan = svc().plan_raid_arrival(RaidArrivalInput {
            to_broadcaster_id: "to_id".to_string(),
            to_broadcaster_login: "to_login".to_string(),
            from_broadcaster_login: "from_login".to_string(),
            from_broadcaster_id: None,
            viewer_count: 7,
            pending_raid: None,
            recent_arrival_present: false,
            independent_manual_detected: true,
            manual_raid_source_key: None,
        });
        assert_eq!(plan.outcome, RaidSignalOutcome::IndependentManualArrival);
        assert_eq!(plan.actions.len(), 1);
        assert_eq!(
            plan.actions[0].kind,
            RaidSignalActionKind::RecordIndependentRaidArrival
        );
    }

    #[test]
    fn raid_arrival_pending_mismatch() {
        // Python Z. 129–153: pending.from != normalized_from → mismatch
        let pending = make_pending("other_streamer", "to_id");
        let plan = svc().plan_raid_arrival(RaidArrivalInput {
            to_broadcaster_id: "to_id".to_string(),
            to_broadcaster_login: "to_login".to_string(),
            from_broadcaster_login: "actual_streamer".to_string(),
            from_broadcaster_id: None,
            viewer_count: 20,
            pending_raid: Some(pending),
            recent_arrival_present: false,
            independent_manual_detected: false,
            manual_raid_source_key: None,
        });
        assert_eq!(plan.outcome, RaidSignalOutcome::PendingMismatch);
        assert_eq!(plan.actions.len(), 2);
        assert_eq!(
            plan.actions[0].kind,
            RaidSignalActionKind::RecordPendingObservation
        );
        assert_eq!(plan.actions[1].kind, RaidSignalActionKind::StorePendingRaid);
        assert_eq!(plan.reason.as_deref(), Some("source_target_mismatch"));
        if let ActionData::PendingObservation {
            status,
            reason,
            detail,
            ..
        } = &plan.actions[0].data
        {
            assert_eq!(*status, "ignored");
            assert_eq!(*reason, Some("source_target_mismatch"));
            let d = detail.as_ref().unwrap();
            assert!(
                d.contains("other_streamer"),
                "Detail muss expected enthalten"
            );
            assert!(
                d.contains("actual_streamer"),
                "Detail muss actual enthalten"
            );
        } else {
            panic!("Falsche ActionData-Variante");
        }
    }

    #[test]
    fn raid_arrival_pending_matched() {
        // Python Z. 155–186: Match → [record_pending_observation, store_pending_raid,
        //   confirm_pending_raid]
        let pending = make_pending("the_raider", "to_id");
        let plan = svc().plan_raid_arrival(RaidArrivalInput {
            to_broadcaster_id: "to_id".to_string(),
            to_broadcaster_login: "to_login".to_string(),
            from_broadcaster_login: "THE_RAIDER".to_string(),
            from_broadcaster_id: Some("raider_id".to_string()),
            viewer_count: 100,
            pending_raid: Some(pending),
            recent_arrival_present: false,
            independent_manual_detected: false,
            manual_raid_source_key: None,
        });
        assert_eq!(plan.outcome, RaidSignalOutcome::PendingMatched);
        assert_eq!(plan.actions.len(), 3);
        assert_eq!(
            plan.actions[0].kind,
            RaidSignalActionKind::RecordPendingObservation
        );
        assert_eq!(plan.actions[1].kind, RaidSignalActionKind::StorePendingRaid);
        assert_eq!(
            plan.actions[2].kind,
            RaidSignalActionKind::ConfirmPendingRaid
        );
        assert!(plan.reason.is_none());
        if let ActionData::PendingObservation {
            status,
            signal_type,
            reason,
            ..
        } = &plan.actions[0].data
        {
            assert_eq!(*status, "matched_pending");
            assert_eq!(*signal_type, "channel.raid");
            assert!(reason.is_none());
        } else {
            panic!("Falsche ActionData-Variante");
        }
        if let ActionData::ConfirmPendingRaid {
            viewer_count,
            signal_type,
            ..
        } = &plan.actions[2].data
        {
            assert_eq!(*viewer_count, 100);
            assert_eq!(*signal_type, "channel.raid");
        } else {
            panic!("Falsche ActionData-Variante");
        }
    }

    #[test]
    fn raid_arrival_normalisiert_eingaben() {
        // normalize_broadcaster_login + normalize_target_id auf alle Eingaben (Z. 98–100)
        let pending = make_pending("streamer_x", "id_99");
        let plan = svc().plan_raid_arrival(RaidArrivalInput {
            to_broadcaster_id: "  id_99  ".to_string(),
            to_broadcaster_login: "  TO_Login  ".to_string(),
            from_broadcaster_login: "  STREAMER_X  ".to_string(),
            from_broadcaster_id: None,
            viewer_count: 0,
            pending_raid: Some(pending),
            recent_arrival_present: false,
            independent_manual_detected: false,
            manual_raid_source_key: None,
        });
        assert_eq!(plan.outcome, RaidSignalOutcome::PendingMatched);
        assert_eq!(plan.from_broadcaster_login, "streamer_x");
        assert_eq!(plan.to_broadcaster_login, "to_login");
        assert_eq!(plan.to_broadcaster_id, "id_99");
    }

    // -----------------------------------------------------------------------
    // plan_chat_notification — alle vier Pfade
    // -----------------------------------------------------------------------

    #[test]
    fn chat_notification_secondary_signal() {
        // Python Z. 205–213
        let plan = svc().plan_chat_notification(ChatNotificationInput {
            to_broadcaster_id: "to_id".to_string(),
            to_broadcaster_login: "to_login".to_string(),
            from_broadcaster_login: "from_login".to_string(),
            from_broadcaster_id: None,
            viewer_count: 5,
            message_id: None,
            event_timestamp: None,
            pending_raid: None,
            recent_arrival_present: true,
        });
        assert_eq!(plan.outcome, RaidSignalOutcome::SecondarySignalHandled);
        assert!(plan.is_short_circuit());
        if let ActionData::SecondarySignal { signal_type, .. } = &plan.actions[0].data {
            assert_eq!(*signal_type, "channel.chat.notification");
        } else {
            panic!("Falsche ActionData-Variante");
        }
    }

    #[test]
    fn chat_notification_orphan() {
        // Python Z. 220–247: kein Pending → orphan_chat_notification mit Payload
        let plan = svc().plan_chat_notification(ChatNotificationInput {
            to_broadcaster_id: "to_id".to_string(),
            to_broadcaster_login: "to_login".to_string(),
            from_broadcaster_login: "from_login".to_string(),
            from_broadcaster_id: Some("fid".to_string()),
            viewer_count: 3,
            message_id: Some("  msg123  ".to_string()),
            event_timestamp: Some("  2026-06-01T10:00:00Z  ".to_string()),
            pending_raid: None,
            recent_arrival_present: false,
        });
        assert_eq!(plan.outcome, RaidSignalOutcome::OrphanChatNotification);
        assert_eq!(plan.actions.len(), 1);
        assert_eq!(
            plan.actions[0].kind,
            RaidSignalActionKind::StoreOrphanChatNotification
        );
        assert_eq!(plan.reason.as_deref(), Some("no_pending_raid"));
        if let ActionData::OrphanChatNotification {
            message_id,
            event_timestamp,
            ..
        } = &plan.actions[0].data
        {
            assert_eq!(message_id.as_deref(), Some("msg123"));
            assert_eq!(event_timestamp.as_deref(), Some("2026-06-01T10:00:00Z"));
        } else {
            panic!("Falsche ActionData-Variante");
        }
    }

    #[test]
    fn chat_notification_orphan_leere_from_id_wird_none() {
        // Python Z. 237: `str(from_broadcaster_id or "").strip() or None`
        let plan = svc().plan_chat_notification(ChatNotificationInput {
            to_broadcaster_id: "to_id".to_string(),
            to_broadcaster_login: "to_login".to_string(),
            from_broadcaster_login: "from_login".to_string(),
            from_broadcaster_id: Some("  ".to_string()),
            viewer_count: 0,
            message_id: None,
            event_timestamp: None,
            pending_raid: None,
            recent_arrival_present: false,
        });
        if let ActionData::OrphanChatNotification {
            from_broadcaster_id,
            ..
        } = &plan.actions[0].data
        {
            assert!(
                from_broadcaster_id.is_none(),
                "Leere ID (Whitespace) muss None ergeben"
            );
        } else {
            panic!("Falsche ActionData-Variante");
        }
    }

    #[test]
    fn chat_notification_mismatch() {
        // Python Z. 249–273
        let pending = make_pending("other_streamer", "to_id");
        let plan = svc().plan_chat_notification(ChatNotificationInput {
            to_broadcaster_id: "to_id".to_string(),
            to_broadcaster_login: "to_login".to_string(),
            from_broadcaster_login: "actual_streamer".to_string(),
            from_broadcaster_id: None,
            viewer_count: 0,
            message_id: None,
            event_timestamp: None,
            pending_raid: Some(pending),
            recent_arrival_present: false,
        });
        assert_eq!(plan.outcome, RaidSignalOutcome::PendingMismatch);
        assert_eq!(plan.actions.len(), 2);
        if let ActionData::PendingObservation {
            signal_type,
            status,
            ..
        } = &plan.actions[0].data
        {
            assert_eq!(*signal_type, "channel.chat.notification");
            assert_eq!(*status, "ignored");
        } else {
            panic!("Falsche ActionData-Variante");
        }
    }

    #[test]
    fn chat_notification_matched_message_id_als_detail() {
        // Python Z. 275–307: detail = message_id (Z. 291)
        let pending = make_pending("raider", "to_id");
        let plan = svc().plan_chat_notification(ChatNotificationInput {
            to_broadcaster_id: "to_id".to_string(),
            to_broadcaster_login: "to_login".to_string(),
            from_broadcaster_login: "RAIDER".to_string(),
            from_broadcaster_id: None,
            viewer_count: 25,
            message_id: Some("msg_abc".to_string()),
            event_timestamp: None,
            pending_raid: Some(pending),
            recent_arrival_present: false,
        });
        assert_eq!(plan.outcome, RaidSignalOutcome::PendingMatched);
        assert_eq!(plan.actions.len(), 3);
        if let ActionData::PendingObservation { detail, status, .. } = &plan.actions[0].data {
            assert_eq!(detail.as_deref(), Some("msg_abc"));
            assert_eq!(*status, "matched_pending");
        } else {
            panic!("Falsche ActionData-Variante");
        }
        if let ActionData::ConfirmPendingRaid { signal_type, .. } = &plan.actions[2].data {
            assert_eq!(*signal_type, "channel.chat.notification");
        } else {
            panic!("Falsche ActionData-Variante");
        }
    }

    #[test]
    fn chat_notification_matched_ohne_message_id_detail_none() {
        // detail = None wenn message_id fehlt
        let pending = make_pending("raider", "to_id");
        let plan = svc().plan_chat_notification(ChatNotificationInput {
            to_broadcaster_id: "to_id".to_string(),
            to_broadcaster_login: "to_login".to_string(),
            from_broadcaster_login: "raider".to_string(),
            from_broadcaster_id: None,
            viewer_count: 5,
            message_id: None,
            event_timestamp: None,
            pending_raid: Some(pending),
            recent_arrival_present: false,
        });
        assert_eq!(plan.outcome, RaidSignalOutcome::PendingMatched);
        if let ActionData::PendingObservation { detail, .. } = &plan.actions[0].data {
            assert!(detail.is_none());
        } else {
            panic!("Falsche ActionData-Variante");
        }
    }

    // -----------------------------------------------------------------------
    // plan_chat_unraid — alle drei Pfade
    // -----------------------------------------------------------------------

    #[test]
    fn chat_unraid_secondary_signal_unraid_seen_true() {
        // Python Z. 324–333: recent_arrival=true → unraid_seen=true, viewer_count=0
        let plan = svc().plan_chat_unraid(ChatUnraidInput {
            to_broadcaster_id: "to_id".to_string(),
            to_broadcaster_login: "to_login".to_string(),
            from_broadcaster_login: "from_login".to_string(),
            from_broadcaster_id: None,
            pending_raid: None,
            recent_arrival_present: true,
            event_timestamp: None,
        });
        assert_eq!(plan.outcome, RaidSignalOutcome::SecondarySignalHandled);
        assert_eq!(plan.viewer_count, 0);
        assert!(plan.is_short_circuit());
        if let ActionData::SecondarySignal {
            unraid_seen,
            viewer_count,
            signal_type,
            ..
        } = &plan.actions[0].data
        {
            assert!(*unraid_seen, "unraid_seen muss true sein");
            assert_eq!(*viewer_count, 0);
            assert_eq!(*signal_type, "channel.chat.notification.unraid");
        } else {
            panic!("Falsche ActionData-Variante");
        }
    }

    #[test]
    fn chat_unraid_no_pending_leere_actions_reason_timestamp() {
        // Python Z. 340–352: kein Pending → no_pending, leere Actions, reason=timestamp
        let plan = svc().plan_chat_unraid(ChatUnraidInput {
            to_broadcaster_id: "to_id".to_string(),
            to_broadcaster_login: "to_login".to_string(),
            from_broadcaster_login: "from_login".to_string(),
            from_broadcaster_id: None,
            pending_raid: None,
            recent_arrival_present: false,
            event_timestamp: Some("2026-06-01T10:00:00Z".to_string()),
        });
        assert_eq!(plan.outcome, RaidSignalOutcome::NoPending);
        assert!(plan.actions.is_empty());
        assert_eq!(plan.reason.as_deref(), Some("2026-06-01T10:00:00Z"));
    }

    #[test]
    fn chat_unraid_no_pending_kein_timestamp_reason_none() {
        let plan = svc().plan_chat_unraid(ChatUnraidInput {
            to_broadcaster_id: "to_id".to_string(),
            to_broadcaster_login: "to_login".to_string(),
            from_broadcaster_login: "from_login".to_string(),
            from_broadcaster_id: None,
            pending_raid: None,
            recent_arrival_present: false,
            event_timestamp: None,
        });
        assert_eq!(plan.outcome, RaidSignalOutcome::NoPending);
        assert!(plan.reason.is_none());
    }

    #[test]
    fn chat_unraid_pending_vorhanden_diagnostic_only() {
        // Python Z. 354–377: pending vorhanden → pending_unraid_observed, diagnostic_only
        let pending = make_pending("raider", "to_id");
        let plan = svc().plan_chat_unraid(ChatUnraidInput {
            to_broadcaster_id: "to_id".to_string(),
            to_broadcaster_login: "to_login".to_string(),
            from_broadcaster_login: "raider".to_string(),
            from_broadcaster_id: None,
            pending_raid: Some(pending),
            recent_arrival_present: false,
            event_timestamp: Some("2026-06-10T08:00:00Z".to_string()),
        });
        assert_eq!(plan.outcome, RaidSignalOutcome::PendingUnraidObserved);
        assert_eq!(plan.viewer_count, 0);
        assert_eq!(plan.actions.len(), 2);
        assert_eq!(
            plan.actions[0].kind,
            RaidSignalActionKind::RecordPendingObservation
        );
        assert_eq!(plan.actions[1].kind, RaidSignalActionKind::StorePendingRaid);
        assert_eq!(plan.reason.as_deref(), Some("unraid_does_not_confirm"));
        if let ActionData::PendingObservation {
            status,
            reason,
            detail,
            signal_type,
            ..
        } = &plan.actions[0].data
        {
            assert_eq!(*status, "diagnostic_only");
            assert_eq!(*reason, Some("unraid_does_not_confirm"));
            assert_eq!(detail.as_deref(), Some("2026-06-10T08:00:00Z"));
            assert_eq!(*signal_type, "channel.chat.notification.unraid");
        } else {
            panic!("Falsche ActionData-Variante");
        }
    }

    #[test]
    fn chat_unraid_kein_mismatch_pfad_pending_wird_immer_observed() {
        // Wichtig: plan_chat_unraid hat KEINEN Mismatch-Pfad (Python Z. 309–377).
        // Bei vorhandenem Pending immer pending_unraid_observed — unabhängig von from_login.
        let pending = make_pending("other_streamer", "to_id");
        let plan = svc().plan_chat_unraid(ChatUnraidInput {
            to_broadcaster_id: "to_id".to_string(),
            to_broadcaster_login: "to_login".to_string(),
            from_broadcaster_login: "actual_streamer".to_string(),
            from_broadcaster_id: None,
            pending_raid: Some(pending),
            recent_arrival_present: false,
            event_timestamp: None,
        });
        assert_eq!(
            plan.outcome,
            RaidSignalOutcome::PendingUnraidObserved,
            "Unraid hat keinen Mismatch-Pfad — jeder Pending → observed"
        );
    }

    // -----------------------------------------------------------------------
    // String-Repräsentationen — 1:1 zu Python-Literals
    // -----------------------------------------------------------------------

    #[test]
    fn action_kind_as_str_identisch_zu_python() {
        use RaidSignalActionKind::*;
        assert_eq!(RecordSecondarySignal.as_str(), "record_secondary_signal");
        assert_eq!(
            RecordPendingObservation.as_str(),
            "record_pending_observation"
        );
        assert_eq!(StorePendingRaid.as_str(), "store_pending_raid");
        assert_eq!(ConfirmPendingRaid.as_str(), "confirm_pending_raid");
        assert_eq!(
            StoreOrphanChatNotification.as_str(),
            "store_orphan_chat_notification"
        );
        assert_eq!(MarkManualRaidStarted.as_str(), "mark_manual_raid_started");
        assert_eq!(
            RecordIndependentRaidArrival.as_str(),
            "record_independent_raid_arrival"
        );
    }

    #[test]
    fn outcome_as_str_identisch_zu_python() {
        use RaidSignalOutcome::*;
        assert_eq!(SecondarySignalHandled.as_str(), "secondary_signal_handled");
        assert_eq!(PendingMatched.as_str(), "pending_matched");
        assert_eq!(PendingMismatch.as_str(), "pending_mismatch");
        assert_eq!(OrphanChatNotification.as_str(), "orphan_chat_notification");
        assert_eq!(
            IndependentManualArrival.as_str(),
            "independent_manual_arrival"
        );
        assert_eq!(PendingUnraidObserved.as_str(), "pending_unraid_observed");
        assert_eq!(NoPending.as_str(), "no_pending");
    }

    #[test]
    fn signal_type_as_str_identisch_zu_python() {
        use RaidSignalType::*;
        assert_eq!(ChannelRaid.as_str(), "channel.raid");
        assert_eq!(
            ChannelChatNotification.as_str(),
            "channel.chat.notification"
        );
        assert_eq!(
            ChannelChatNotificationUnraid.as_str(),
            "channel.chat.notification.unraid"
        );
    }
}
