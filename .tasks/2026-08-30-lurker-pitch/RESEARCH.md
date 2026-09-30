status: aktiv
Datum: 2026-08-30

# Research: Lurker-Discord-Pitch + LFG-Pitch-Kreis

## Beobachtet (Fakten mit Fundstelle)

- LFG-Pitch (`rust/crates/tb-chat/src/lfg_pitch.rs`): antwortet auf LFG-Signale in JEDEM Kanal
  des Bots. Kette in `decide()` (lfg_pitch.rs:492-587): enabled-Schalter (`LFG_PITCH_ENABLED`,
  lfg_pitch.rs:259) → Regex (`classify_lfg`, :86) → Promo-Block durch Plan
  (`PromoBlockCheck`, :535) → Invite-URL (:545) → Cooldowns (:645: 120 s Kanal, 6 h User,
  30 s Judge-Bremse) → MiniMax-Judge (:562, confidence >= 0.7). Kein Kanal-Kreis-Kriterium.
  Text: `LFG_PITCH_REPLY` (lfg_pitch.rs:23). Silent-Grund-Enum: `SilentReason` (:277) mit
  `as_str`-Mapping (:300).
- Konstruktion: `bin/tb-bot/src/chat_wiring.rs:834` (Produktivpfad, `bot_user_id` ist im
  Scope, :801 an ChatPipelineParts übergeben) und `chat_wiring.rs:2828` (zweite, reduzierte
  Verdrahtung, enabled=false). `ChatApi::bot_user_id()` existiert (crates/tb-chat/src/api.rs:101).
- Lurker-Tax (`rust/crates/tb-chat/src/promos.rs`): Tick-Loop `send_promo_if_due` (:976)
  holt Live-Kanäle ohne Plan-Filter (:1004, `get_live_channels_for_lurker_tax` :2351), gate
  über Partner-Check (:1014) und ruft `maybe_send_lurker_tax_reminder` (:1679):
  Overall-Promo-Cooldown (:1684) → `streamer_plans.lurker_tax_enabled` Opt-in (:1696, Default 0,
  Spalte baseline_schema.sql:371) → Paid-Plan mit `chat.lurker_tax`-Entitlement (:1735) →
  `moderator:read:chatters`-Scope-Gate (:1745, :1840) → Kandidaten (:1757, Query :1852:
  stille Lurker, `messages = 0`, `seen_via_chatters_api`, >= 3 Sessions, >= 240 Lurk-Minuten,
  Identity-Key `id:<id>` sonst `login:<login>`, Bots ausgeschlossen, Sortierung nach
  Lurk-Minuten) → Pro-Session-Dedup (:1770-1793, `LURKER_TAX_MAX_MENTIONS = 2`, :289) →
  Send als Announcement "orange" (:1798) → `mark_promo_sent` (:1818).
- Textbau: `build_lurker_tax_text` (promos.rs:1938). Schwellen: :283-289.
- Dashboard: `handlers/lurker_tax_settings.rs` (GET :110, POST :159) setzt
  `lurker_tax_enabled` auf `streamer_plans`; Partner darf nur eigenen Kanal (:37-47).
- Chat-Command: `!lurkersteuer_off` (commands.rs:537) setzt dieselbe Spalte auf 0.
- Entitlement `chat.lurker_tax` im Billing-Katalog (tb-analytics/src/billing/catalog.rs:125,153).
- Migrationen: additive SQL-Dateien `rust/migrations/<jjjjmmtthhmmss>_<name>.sql`
  (z. B. 20260829090000_twitch_scout_candidates.sql).
- Testpfad Repo: `verify-change.sh Deadlock-Twitch-Bot` → `cd rust && eval
  "$(./scripts/test_db.sh env)" && cargo test --workspace -j 2`; sqlx-Offline-Cache in
  `rust/.sqlx`, neue `query!`-Stellen erfordern Neupräparieren gegen die Test-DB.

## Hypothesen (noch zu verifizieren)

- `bot_user_id` ist in chat_wiring.rs:834 bereits als Bindung vorhanden (laut :801-Zuweisung);
  exakter Bezeichner beim Implement prüfen.
- Invite-Auflösung im Promo-Engine-Kontext: `promo_invite_fallback` (promos.rs:2590) +
  konfigurierter Invite; genaue Wiederverwendungsstelle beim Implement suchen
  (Targeted-Promo-Pfad, promos.rs:1951 ff.).
