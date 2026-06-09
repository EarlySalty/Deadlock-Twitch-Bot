# Schritt 6 — Raid (OAuth, Auto-Raid, Scoring, Blacklist, Recruitment)

> Status: Planung · Vorgezogen vor den Monitoring-Cutover (Schritt 4f), weil die
> Monitoring-EventSub-Hooks (`on_channel_raid`, `on_channel_moderate`,
> `on_score_refresh`, `on_stream_offline`) ohne Raid Noop bleiben — ein
> Monitoring-Flip allein würde Auto-Raid live abwürgen. Deshalb: **Raid bauen,
> dann Monitoring + Raid atomar zusammen flippen.**

## Ausgangslage (Python)

`bot/raid/` — 47 Dateien, ~17.743 Zeilen (größer als Monitoring). Größte Brocken:
`auth.py` (1797, OAuth + Token-Store), `partner_scores.py` (915) +
`partner_raid_score_tracking.py` (634, Scoring), `recruitment_messaging.py` (894),
`raid_blacklist.py` (749), `raid_tracking_runtime.py` (704) +
`raid_arrival_runtime.py` (690) + `signal_correlation.py` (500, Arrival),
`raid_pipeline.py` (508, Ausführung), `pending_raids.py` (485),
`offline_raid_orchestrator.py` (Auto-Raid-Trigger), `candidate_selection.py` (463).

**Öffentliche Oberfläche** (was der Rest des Bots ruft): `auth_manager`,
`start_manual_raid`, `handle_streamer_offline` (Auto-Raid bei Offline),
`on_raid_arrival` / `on_chat_raid_notification` / `on_chat_unraid_notification`
(Arrival), Blacklist (`_is_blacklisted`, `_add_to_blacklist`,
`_schedule_external_target_ban_check`), `partner_raid_score_service`,
`raid_executor` / `execute_raid_pipeline`, Lifecycle (`start`/`cleanup`).

## Geklärte Make-or-Break-Risiken

1. **AES-256-GCM-Interop = grün und bidirektional** (`tb-crypto/tests/interop.rs`,
   3 Tests: Python→Rust, Rust→Python, byte-identisch bei fixer Nonce). Rust liest
   die bestehenden `twitch_raid_auth`-Blobs **und** Python liest Rust-geschriebene
   (rollback-sicher nach Token-Refresh). **Kein Re-Auth, keine Runtime-Migration** —
   beide Prozesse teilen den verschlüsselten Store zur Laufzeit. Per-Feld-AAD gegen
   echte (kopierte) Prod-Blobs wird beim Portieren des Stores (6a) verifiziert.

## Architektur-Fallen (würde ein naiver Port brechen)

1. **`oauth_state_tokens` ist von raid UND social-media geteilt** und hat einen
   `platform`-Discriminator (social-media indiziert `(platform, expires_at)`).
   Rust-Raid muss konsequent nach `platform` filtern/schreiben, sonst stören sich
   Rust-raid und Python-social-media beim getrennten Cutover. Plattform-Wert vor 6a
   verifizieren.
2. **`twitch_raid_auth` ist AES-256-GCM-verschlüsselt** — jeder Token-Zugriff geht
   über `tb-crypto::FieldCipher` mit korrekter per-Feld-AAD; kein Klartext in Log/
   Stacktrace.
3. **Token-Refresh-DB-Lock** (`_acquire_refresh_db_lock`) verhindert paralleles
   Refreshen desselben Tokens — als Advisory-Lock nachbauen.

## Saubere Trennung statt 1:1 (auth.py 1797 Z. → 4 Strukturen)

- **StateStore** — OAuth-State-Token-Lifecycle (`oauth_state_tokens`, platform-gated):
  persist/lookup/consume/verify/cleanup.
- **OAuthFlow** — Authorize-URL + PKCE + Scope-Profile (`_build_authorize_url`,
  `_resolve_scope_profile`, `_build_state_info`).
- **TokenRefresher** — Crypto + Exchange/Refresh (`exchange_code_for_token`,
  `refresh_token`, `refresh_all_tokens`, `_write_token_refresh`), nutzt FieldCipher
  + Advisory-Lock.
- **TokenStore** — Lese-API (`get_valid_token`, `get_valid_token_for_login`,
  `get_tokens_for_user`, `get_scopes`) + Token-Blacklist (`twitch_token_blacklist`).

## Prod-Schema-Befunde 6a (read-only verifiziert 2026-06-09)

- **`oauth_state_tokens`**: text-Spalten + timestamptz (`expires_at`, `consumed_at`).
  `platform`-Wert von raid = **`twitch_raid`** (`_OAUTH_STATE_PLATFORM_RAID`).
  Spalte `pkce_verifier` speichert in Wahrheit serialisierte State-Meta (Alt-Last,
  Spaltenname lügt) — Rust-Feld heißt ehrlich `state_meta`, Spalte bleibt.
- **`twitch_raid_auth`**: Token-Blobs liegen in **`access_token_enc`/`refresh_token_enc`
  (bytea)** + `enc_version` (int) + `enc_kid` (text) — das ist der AES-256-GCM-Pfad
  via `tb-crypto::FieldCipher`. Klartext `access_token`/`refresh_token` (text) sind
  Legacy/Migration → NICHT verwenden. `scopes` text, `raid_enabled`/`needs_reauth`
  boolean, Timestamps timestamptz.
- **`twitch_token_blacklist`**: Alt-Stil, abweichende Konventionen —
  `first_error_at`/`last_error_at`/`grace_expires_at` als **text** (nicht timestamptz!),
  Flags `notified`/`user_dm_sent`/`reminder_sent`/`role_removed` als **integer** (nicht
  boolean). Pro Tabelle die echten Typen binden, kein uniformer Port.

## Slice-Plan

| Slice | Inhalt | Schaltet Monitoring-Hook scharf |
|---|---|---|
| **6a** | RaidAuth-Fundament: StateStore ✅ + scope_profiles/OAuthFlow ✅ + TokenStore (Lese-/Entschlüsselungspfad, **Prod-Interop bewiesen**) ✅ + TokenRefresher (Refresh-Schreibpfad, Advisory-Lock byte-identisch) ✅. **Offen: `exchange_code_for_token`** (Onboarding/Initial-Auth, legt Auth-Zeile an). | — (Dependency für alles) |
| **6b** | Blacklist + Raid-Guard (`twitch_raid_blacklist`, external-recruitment-blacklist-pending, `channel.moderate`-Guard) | `on_channel_moderate` |
| **6c** | Scoring (`twitch_partner_raid_scores`/`_score_tracking`, Score-Berechnung + Refresh) | `on_score_refresh` |
| **6d** | Raid-Ausführung: Candidate-Selection + Executor + Pipeline + Pending (`twitch_raid_history`, `twitch_auto_raid_pause`, `twitch_raid_disabled_strikes`) | — |
| **6e** | Arrival-Tracking + Signal-Correlation (`on_raid_arrival`, Chat-Raid/Unraid, `twitch_raid_arrival_tracking`, confirmed-external-recruitment) | `on_channel_raid` |
| **6f** | Auto-Raid-Orchestrator (`handle_streamer_offline`, Followers, Offline-Trigger) | `on_stream_offline` |
| **6g** | Recruitment + Outreach (`recruitment_messaging`, `twitch_partner_outreach`/`_conversations`/`_audit`) | — |
| **6h** | Chat-Commands + Partner-Setup-Service | — |
| **6i** | **Atomarer Cutover Monitoring + Raid** zusammen (Python beide AUS, Rust beide AN) — **user-gated** | alle |

Reihenfolge ist dependency-getrieben: 6a (Auth) ist das Fundament; 6b/6c/6e/6f
schalten je einen Monitoring-Hook scharf; 6i flippt erst, wenn alle Hooks echt sind.

## Slices 6a–6h laufen ohne Prod-Berührung

Wie bei Monitoring: gegen isolierte Test-Schemas gebaut + getestet, Python bleibt
alleiniger Live-Writer bis 6i. Das gesamte Risiko sitzt im gemeinsamen Cutover 6i
(Wartungsfenster) — dort gehen Python-Monitoring **und** Python-Raid AUS, Rust
übernimmt beide; die Bridge liefert weiter an 8776 (jetzt Rust), das jetzt sowohl
Monitoring- als auch Raid-EventSub-Typen echt verarbeitet.

## Delegation

Implementierung der Slices an native Sonnet-Sub-Agents (CLAUDE.md), Orchestrator
reviewt jeden Block zeilengenau. Krypto-/Token-Kern (6a) wird besonders eng
geprüft (Security-sensibel). Doku in `rust/docs/` laufend mitschreiben, interne
Infra → kein Discord/Changelog.
