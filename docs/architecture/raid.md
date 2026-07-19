# raid/ — Architektur & Funktionsreferenz

> Pfad: `bot/raid/` · Stand: 2026-06-08 · 47 Dateien, ~17.770 Zeilen
>
> Teil der [Architektur-Doku](README.md). Verwandt: [monitoring.md](monitoring.md) (channel.raid-Events, Score-Refresh), [api.md](api.md) (Helix-Raid/Token), [storage.md](storage.md) (Auth-IDs, Partner-Lifecycle), [external-recruitment-blacklist.md](../external-recruitment-blacklist.md), [PROJ-1-partner-raid-score-cache.md](../../features/PROJ-1-partner-raid-score-cache.md).

## 1. Zweck & Abgrenzung

`raid/` ist das **Auto-Raid-System**: Wenn ein Partner offline geht, sucht der Bot einen passenden anderen live-Partner aus, raidet ihn (über das User-OAuth-Token des offline gehenden Streamers) und **misst den Erfolg** (kamen Zuschauer an, blieben sie?). Daraus entsteht ein **Fairness-/Readiness-Score**, der die nächste Raid-Verteilung steuert. Dazu kommen: OAuth-Autorisierung pro Streamer, Recruitment-Nachrichten an externe Deadlock-Streamer und eine Blacklist gegen Missbrauch.

Architektonisch ist das Subsystem **service-orientiert und dependency-injiziert**: Ein Host-Mixin (`TwitchRaidMixin`) instanziiert lazy ~20 Services über Factory-Funktionen (`runtime_factories.py`). Jeder Service hat eine klare Verantwortung und explizite Abhängigkeiten — gut testbar, klar abgegrenzt.

Abgrenzung: Die channel.raid-EventSub-Subscriptions kommen aus [monitoring.md](monitoring.md); die reinen Helix-Calls aus [api.md](api.md). `raid/` enthält die **Entscheidungs- und Orchestrierungslogik**.

## 2. Einordnung & Abhängigkeiten

| Richtung | Beziehung |
|----------|-----------|
| **Wird genutzt von** | `TwitchStreamCog` (`TwitchRaidMixin` + `RaidCommandsMixin`); getriggert von Monitoring (Offline-Event, Raid-Arrival). |
| **Nutzt** | `api/` (Helix-Raid, OAuth-Token), `storage/` (Auth-IDs, Partner-Lifecycle, Score-Cache), `core/` (Partner-Gate, Login-Norm), `chat/` (Recruitment-/Chat-Targets), FieldCrypto (Token-Verschlüsselung). |
| **DB-Tabellen** | `twitch_raid_auth` (verschlüsselte User-Tokens), Raid-History, Partner-Score-Cache/-Tracking, Raid-Blacklist + External-Recruitment-Pending, `oauth_state_tokens`. |
| **Externe Dienste** | Twitch-OAuth (Authorize/Token), Twitch-Helix (`/raids`, `/streams`), Discord (Commands, Auth-Links). |
| **Secret-Namen** | Twitch-Client-ID/-Secret, FieldCrypto-Schlüssel; `TWITCH_RAID_REDIRECT_URI` (Konstante). |

## 3. Dateien im Überblick (Auswahl)

| Datei | Zeilen | Rolle |
|-------|-------:|-------|
| `auth.py` | 1798 | `RaidAuthManager` — OAuth-User-Token-Verwaltung, State-Tokens, Scope-Profile, Token-Verschlüsselung. |
| `partner_scores.py` | 916 | Vorberechneter Partner-Raid-Score-Cache (Readiness/Fairness/New-Partner-Multiplikator). |
| `services/recruitment_messaging.py` | 895 | Recruitment-Nachrichten an externe Deadlock-Streamer (Stufen). |
| `services/raid_blacklist.py` | 750 | External-Recruitment-Blacklist + Ban-Check-Scheduling. |
| `commands.py` | 714 | `RaidCommandsMixin` — Discord-Commands für Streamer/Admins. |
| `raid_tracking_runtime.py` | 705 | `RaidTrackingRuntimeService` — Tracking offener/erwarteter Raids. |
| `raid_arrival_runtime.py` | 691 | `RaidArrivalRuntime` — Ankunft bestätigen, Signal-Pläne ausführen. |
| `runtime_factories.py` | 662 | Composition-Root: `make_*`-Factories für alle Services. |
| `partner_raid_score_tracking.py` | 635 | Bestätigte Raids + Post-Raid-Deadlock-Dauer tracken. |
| `services/raid_data_sources.py` | 564 | Datenquellen: Deadlock-Eligibility, Partner-Roster, Online-Kandidaten. |
| `facades/tracking_arrival.py` | 546 | `RaidTrackingArrivalFacadeMixin` — Runtime-State an den Cog binden. |
| `services/partner_setup_service.py` | 525 | Partner-Setup nach Auth: Rolle, Trial, First-Login, Aktivierung. |
| `raid_pipeline.py` | 509 | `RaidPipelineService` — Kandidaten → Auswahl → Ausführung. |
| `signal_correlation.py` | 501 | `RaidSignalCorrelationService` — mehrere Raid-Signale korrelieren. |
| `services/candidate_selection.py` | 464 | Score-basierte + faire Kandidatenwahl. |
| `executor.py` | 275 | `RaidExecutor` — Raid via Helix starten/abbrechen + History. |
| `facades/data_setup.py` | 258 | `RaidDataSetupFacadeMixin` — Datenaufbau-Helfer. |
| `partner_resolution.py` | 163 | Partner-Lookup-Protokolle + Arrival-Klassifizierung. |
| `scope_profiles.py` | 97 | OAuth-Scope-Profile (welche Scopes je Profil). |
| `chat_targets.py` | 53 | `ChatTarget` + Outbound-Suppression-Lookup. |
| `mixin.py` | (Host) | `TwitchRaidMixin` — instanziiert + exponiert alle Services. |
| `services/` (weitere) | — | RaidStateStore, ManualRaidSuppression, PartnerArrivalTracking, RaidMetricsStore, CandidateFollowers, OfflineRaidOrchestrator, ExternalRecruitment, ArrivalConfirmation, PartnerRaidDelivery, RaidObservability. |

## 4. Datenfluss / Lebenszyklus

**A) Autorisierung (einmalig je Streamer):** Streamer ruft im Chat/Dashboard den Auth-Flow auf → `RaidAuthManager._build_authorize_url` (mit Scope-Profil) → Twitch-OAuth → Callback. Der `state` wird vorab in der DB persistiert (`_persist_state_token`) und beim Callback **atomar konsumiert** (`_consume_state_token`). Die erhaltenen User-Tokens werden **verschlüsselt** (FieldCrypto) in `twitch_raid_auth` abgelegt. Danach läuft `PartnerSetupService.complete_setup_for_streamer` (Rolle vergeben, 45-Tage-Trial prüfen, First-Login vermerken, Partner-Features aktivieren).

**B) Auto-Raid bei Offline:** Geht ein Partner offline (`handle_streamer_offline`), prüft `RaidDataSourceService`, ob die Session Deadlock-relevant war (`evaluate_deadlock_raid_source`), lädt das Partner-Roster und baut die Online-Kandidatenliste. `CandidateSelectionService` wählt anhand des **vorberechneten Scores** (`partner_scores`) und einer **Fairness-Regel** (Recent-Raid-Cooldown, `select_fairest_candidate`) das Ziel. `RaidExecutor.start_raid` führt den Raid über Helix mit dem User-Token des offline gehenden Streamers aus und schreibt die Raid-History.

**C) Ankunft & Scoring:** Der Erfolg wird über **mehrere Signale** bestätigt (`RaidSignalCorrelationService`): das channel.raid-Event, die Chat-`/raid`-Notice und die tatsächliche Zuschauer-Ankunft. `RaidArrivalRuntime.confirm_pending_raid_arrival` bestätigt; `partner_raid_score_tracking.track_confirmed_partner_raid` speichert den Snapshot und misst die **Post-Raid-Deadlock-Dauer** (blieben die Zuschauer im Deadlock-Kontext?). Daraus werden Readiness-/Fairness-Scores abgeleitet und der Score-Cache aktualisiert (getriggert über `monitoring/partner_ops`).

**D) Recruitment & Blacklist:** Raidet ein **externer** (Nicht-Partner-)Deadlock-Streamer wiederholt herein, schickt `RecruitmentMessagingService` gestufte Einladungs-Nachrichten. Überschreitet die Zahl bestätigter externer Raids ein Limit (`external_recruitment_raid_limit`), plant `RaidBlacklistService` einen Eintrag (mit Grace-Period) und einen Ban-Check (siehe [external-recruitment-blacklist.md](../external-recruitment-blacklist.md)).

## 5. Funktionsreferenz pro Bereich

### auth.py — `RaidAuthManager`
OAuth-User-Token-Verwaltung für Raid-Autorisierung.
- `__init__(client_id, client_secret, redirect_uri)`.
- `_build_authorize_url(*, state, scope_profile)` / `_resolve_scope_profile(twitch_login, requested_profile)` — Authorize-URL + Scope-Profil bestimmen.
- State-Handling: `_persist_state_token(...)`, `_lookup_state_token(state)` (nicht konsumierend), `_consume_state_token(state)` (atomar), `_build_state_info(...)`, `_serialize_state_meta`/`_parse_state_meta`, `_parse_expiry_ts`. Datentyp `RaidOAuthState`.
- Client-Auth-Schutz: `is_client_auth_blocked()`, `_block_client_auth(reason, *, cooldown_seconds=900.0)`, `_raise_if_client_auth_blocked()`, `_ensure_client_credentials()`.
- Token-Verschlüsselung: `_get_crypto_optional()`, `_try_encrypt(plaintext, aad, context)`, `_try_decrypt(blob, aad, context)` (FieldCrypto, AAD-gebunden).
- Cross-Worker-Serialisierung: `_acquire_refresh_db_lock(conn, user_id)` + `_refresh_advisory_lock_pair(user_id)` — Refreshs eines Broadcasters über Prozesse hinweg serialisieren.
- Discord-Verknüpfung: `_linked_twitch_login_for_discord_user(...)`, `_linked_twitch_identity_for_discord_user(...)`, `_has_existing_auth_row(...)`, `_has_existing_streamer_context(login)`.

### scope_profiles.py
- `normalize_scope_profile(raw)` / `scopes_for_profile(scope_profile)` — Profil normalisieren, Scope-Liste liefern.
- `serialize_scope_profile_meta(...)` / `parse_scope_profile_meta[_details](...)` — Profil in die State-Meta ein-/auspacken.

### executor.py — `RaidExecutor`
- `start_raid(from_broadcaster_id, from_broadcaster_login, to_broadcaster_id, to_broadcaster_login, viewer_count, stream_duration_sec, target_stream_started_at, candidates_count, session, reason="auto_raid_on_offline") -> (success, error)` — Raid via Helix starten.
- `cancel_raid(from_broadcaster_id, from_broadcaster_login, session) -> (success, error)` — ausstehenden Raid abbrechen (DELETE).
- `_save_raid_history(...)` — Raid-Metadaten in die DB schreiben.

### partner_scores.py
Vorberechneter Score-Cache (`_PreparedScore`, `_PartnerRow`).
- Score-Komponenten (privat): `_readiness_score(duration_score, time_pattern_score)`, `_new_partner_multiplier(received_successful_raids_total)`, `_clamp`, `_round_score`, `_today_in_berlin(now_utc)`.
- Lade-/Speicher-Helfer für den Cache (`as_db_tuple`, `as_dict`, `_normalized_ids`, `_placeholders`, `_column_exists`).

### partner_raid_score_tracking.py
- `track_confirmed_partner_raid(*, to_broadcaster_id, to_broadcaster_login, from_broadcaster_login, from_broadcaster_id=None, viewer_count=0, score_snapshot=None, confirmed_at=None) -> int | None` — bestätigten Raid + Snapshot speichern.
- `resolve_partner_raid_tracking_for_session(*, twitch_user_id, streamer_login, session_id, session_ended_at) -> int` — offene Tracking-Zeilen einer Session auflösen (Post-Raid-Dauer).
- Score-Ableitung: `_derive_readiness_score`, `_derive_fairness_score`; Schema: `_ensure_tracking_schema(conn)`.

### partner_resolution.py
- `is_partner_target_channel(*, broadcaster_id, broadcaster_login, partner_lookup) -> bool` — ist das Ziel ein Partner?
- `classify_partner_raid_arrival(*, from_…, to_…, partner_lookup, known_streamer_lookup) -> PartnerRaidArrivalResolution` — klassifiziert eine eingehende Raid-Ankunft (Partner/extern/unbekannt). Protokolle `PartnerLookup`, `KnownStreamerLookup`.

### raid_pipeline.py / raid_tracking_runtime.py / raid_arrival_runtime.py
- `RaidPipelineService` — orchestriert Kandidatenfindung → Auswahl → `RaidExecutor`.
- `RaidTrackingRuntimeService` — hält den Laufzeit-Zustand offener/erwarteter Raids (Readiness, Suppressions).
- `RaidArrivalRuntime` — `on_raid_arrival(*, to_broadcaster_id, to_broadcaster_login, from_broadcaster_login, viewer_count, from_broadcaster_id=None)`, `confirm_pending_raid_arrival(...)`, `_handle_secondary_confirmed_signal(...)`, `_execute_signal_plan_actions(actions)`; `RaidArrivalRuntimeDependencies` als DI-Bundle. `PendingRaid`-Verwaltung (`supersede_from_source`).

### signal_correlation.py — `RaidSignalCorrelationService`
Korreliert die unabhängigen Raid-Signale (channel.raid-Event, Chat-`/raid`-Notice, Viewer-Ankunft, `/unraid`), um echte Raids von Fehlsignalen zu trennen, und liefert einen Aktionsplan.

### services/candidate_selection.py — `CandidateSelectionService`
- `load_prepared_partner_scores(twitch_user_ids) -> ScoreMap` — Score-Cache laden.
- `refresh_partner_score_cache_if_available(twitch_user_id, *, reason)` — Cache bei Bedarf auffrischen.
- `get_recent_raid_targets(from_broadcaster_id, days) -> set[str]` — kürzlich beraidete Ziele (Cooldown).
- `select_partner_candidate_by_score(candidates, from_broadcaster_id)` / `select_fairest_candidate(...)` — Auswahl nach Score bzw. Fairness. Im live geschalteten Rust-Pfad stellt `select_fairest` den Nicht-Partner `edoeasy` hinter alle anderen zulässigen Kandidaten, lässt ihn als einziges verbleibendes Ziel aber zu. `PreparedPartnerScore`.

### services/raid_data_sources.py — `RaidDataSourceService`
- `evaluate_deadlock_raid_source(*, current_game, had_deadlock_session, last_deadlock_seen_at)` / `is_deadlock_raid_source_eligible(...)` / `is_deadlock_partner_candidate_eligible(...)` — Deadlock-Relevanz prüfen.
- `load_partner_roster_for_raid(source_user_id)` / `load_partner_live_state_map(...)` / `build_online_partner_candidates(...)` / `filter_deadlock_eligible_partner_candidates(...)` — Kandidaten aufbauen.
- `fetch_streams_by_logins_for_raid(logins, *, api=None)` / `load_broadcaster_live_state(...)` / `calculate_stream_duration_sec(started_at)` / `raid_language_filters()`.

### services/partner_setup_service.py — `PartnerSetupService`
- `complete_setup_for_streamer(twitch_user_id, twitch_login, state_discord_user_id=None, activate_partner_features=True)` — kompletter Post-Auth-Setup-Lauf.
- `sync_partner_state_after_auth(...)` — Partner-Status nach gültiger Auth synchronisieren.
- `apply_streamer_role(discord_user_id, *, should_have_role, reason)` — Live-Rolle vergeben/entziehen.
- `check_and_grant_trial_eligibility(twitch_user_id, twitch_login) -> bool` — 45-Tage-Analytics-Trial prüfen/gewähren.
- `_record_first_login(...)` — `first_login_at` setzen.

### services/raid_blacklist.py — `RaidBlacklistService`
- `is_blacklisted(target_id, target_login)` / `load_raid_blacklist() -> (ids, logins)` / `add_to_blacklist(target_id, target_login, reason)`.
- `increment_raid_disabled_strikes(target_id, target_login, reason) -> int`.
- External-Recruitment: `schedule_external_recruitment_blacklist_pending(*, target_id, target_login, confirmed_raid_count, raid_flow_id)`, `delete_…`, `process_due_external_recruitment_blacklist_pending()`.
- Ban-Check: `schedule_external_target_ban_check(*, target_id, target_login, source)`, `reschedule_…(delay_seconds=900)`, `process_due_external_target_ban_checks()`. Config `RaidBlacklistConfig` (Limit, Grace, Delay), DI über `RaidBlacklistDependencies`.

### services/recruitment_messaging.py — `RecruitmentMessagingService`
Verschickt gestufte Recruitment-Nachrichten an externe Deadlock-Streamer (mit Follower-Auflösung, Outbound-Suppression-Check). Enthält die Erklär-Substanz der Stufen s1–s3 (proaktiv gegen Scam-Verdacht).

### commands.py — `RaidCommandsMixin`
Discord-Commands für Streamer/Admins:
- `cmd_twitch_raid_auth(ctx)` — OAuth-Link für Raid/Follower/Chat-Scopes senden.
- `cmd_raid_enable` / `cmd_raid_disable` / `cmd_raid_status` — Auto-Raid-Bot steuern.
- `cmd_raid_history(ctx, limit=10)` — Raid-History (max 20).
- `cmd_check_scopes` / `cmd_check_auth` — OAuth-Scopes prüfen.
- `cmd_sendchatpromo(ctx, streamer)` — Test-Chat-Promo.
- `cmd_reauth_all(ctx)` (Admin) — alle Streamer zur Neu-Auth auffordern.
- `cmd_test_token_error(ctx, target=None, mode="initial")` (Owner) — Token-Error-DM testen.

### runtime_factories.py
Composition-Root: `make_raid_state_store`, `make_manual_raid_suppression_service`, `make_partner_arrival_tracking_service`, `make_raid_data_source_service`, `make_partner_setup_service`, `make_offline_raid_orchestrator`, `make_raid_metrics_store`, `make_candidate_followers_service`, `make_candidate_selection_service`, `make_raid_blacklist_service`, `make_raid_pipeline_service`, `make_raid_tracking_runtime_service`, `make_raid_arrival_runtime`, `make_recruitment_messaging_service`, `make_raid_observability_service`, `make_partner_raid_delivery_service`, `make_external_recruitment_service`, `make_arrival_confirmation_service`. Jede Factory injiziert `readonly_connection`/`transaction`, Lookups und Config — so sind die Services in Tests austauschbar.

### mixin.py — `TwitchRaidMixin` (+ Facades)
Host-Mixin im Cog: lazy `@property`-Zugriffe (`_raid_pipeline_service`, `_candidate_selection_service`, `_raid_blacklist_service`, …) bauen die Services beim ersten Zugriff. Öffentliche Einstiegspunkte u. a. `has_enabled_auth(twitch_user_id)`, `start_manual_raid(*, broadcaster_id, broadcaster_login)`, `handle_streamer_offline(...)`, `complete_setup_for_streamer(...)`, `confirm_pending_raid_arrival(...)`. Bindet die Facades `RaidTrackingArrivalFacadeMixin` (`facades/tracking_arrival.py`) und `RaidDataSetupFacadeMixin` (`facades/data_setup.py`) ein, die Runtime-State an den Cog koppeln.

## 6. Datenbank & externe Schnittstellen

- **DB:** `twitch_raid_auth` (FieldCrypto-verschlüsselte User-Tokens, `needs_reauth`), Raid-History, Partner-Score-Cache + Tracking, Raid-Blacklist + External-Recruitment/Ban-Check-Pending, `oauth_state_tokens`.
- **Twitch:** OAuth (Authorize + Token), Helix `/raids` (start/cancel), `/streams` (Kandidaten).
- **Discord:** Commands, Auth-Links, Status.

## 7. Stolperfallen / Besonderheiten

- **Service-Verdrahtung über Factories:** Wer einen Service ändert, fasst meist `runtime_factories.py` (DI) **und** den Service an. Die `_*_service`-Properties im Mixin sind lazy — Reihenfolge der Instanziierung kann relevant sein.
- **State-Token wird atomar konsumiert:** Der OAuth-`state` ist einmalig (`_consume_state_token`). Doppelte Callbacks/Replays laufen ins Leere — gewollt (CSRF-/Replay-Schutz).
- **Tokens sind AAD-gebunden verschlüsselt:** `_try_encrypt/_try_decrypt` binden den Klartext an einen Kontext (AAD). Ein Blob lässt sich nicht in einem anderen Kontext entschlüsseln.
- **Refresh ist cross-worker serialisiert:** `_acquire_refresh_db_lock` (Advisory-Lock) verhindert, dass zwei Prozesse dasselbe Broadcaster-Token gleichzeitig refreshen (sonst Token-Invalidierung).
- **Fairness schlägt reinen Score:** `select_fairest_candidate` + Recent-Raid-Cooldown sorgen dafür, dass nicht immer derselbe Top-Score beraidet wird — sonst würde die Verteilung kippen (vgl. CHANGELOG #93/#94).
- **Erfolg braucht Mehrfach-Signal:** Ein einzelnes Signal (nur das channel.raid-Event) reicht nicht; erst die Korrelation (`signal_correlation`) bestätigt einen Raid, sonst werden Phantom-Raids gezählt.
- **External-Recruitment-Blacklist hat Grace + Ban-Check:** Ein externer Vielraider wird nicht sofort gebannt, sondern erst nach Limit + Grace-Period + separatem Ban-Check (offline-orientiert) — siehe [external-recruitment-blacklist.md](../external-recruitment-blacklist.md).
