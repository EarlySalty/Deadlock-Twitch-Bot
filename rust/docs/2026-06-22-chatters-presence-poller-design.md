# #11 Chatters/Presence-Poller + Raid-Retention — Design & Implementierungs-Spec

Stand 2026-06-22. Schließt den letzten Audit-Rest des PY→Rust-Cutovers (P2.64, P2.61,
P2.65, P1.23, P1.24). Quelle: Python-Legacy `bot/analytics/mixin.py`
(`collect_chatters_data`, `_poll_chatters_single`, `_attempt_bot_moderator_self_heal`,
`compute_raid_retention`). User-Entscheidung: **voller Helix-Poller** (NICHT IRC).

## 1. Ziel & Abgrenzung

In Rust läuft heute KEIN produktiver Chatters-Poll. Der message-getriebene
`ChatterTracker` (tb-chat) erfasst nur tippende Chatter. **Stille Lurker, Presence-/
Watchtime-Timelines und das Raid-Retention-Dashboard sind tot.** Dieses Feature baut den
Helix-Poller nach, der pro 30s alle live Streamer abfragt und ALLE Anwesenden (inkl.
stiller Lurker) erfasst, plus einen unabhängigen 1h-Loop für Raid-Retention.

**Kein DB-Snapshot-Loop.** Nur ein echter `GET /chat/chatters`-Call entdeckt stille
Lurker — vorhandene `twitch_session_chatters` zu re-ticken erfasst nichts Neues.

## 2. Architektur (Crate-Grenzen)

- **`tb-monitoring`** besitzt Algorithmus + DB-Writes. Hat bereits `tb-transport-twitch`
  (→ `HelixClient::get_chatters`, `HelixError`, `Chatter`) und `tb-chat` als Dep.
  **Keine neue Crate-Dep** (insb. NICHT `tb-raid` — sonst Zyklusrisiko).
- **Token-Plumbing wird per Trait/Arc aus dem Binary injiziert** (genau wie
  `HelixModeratorProvisioner` in `bin/tb-bot/src/eventsub_hooks.rs` den tb-monitoring-
  Trait `ModeratorProvisioner` implementiert):
  - Bot-Token/-User-ID/-Scopes ← `tb_chat::token::BotTokenManager` (via `ChatApiHandle::bot_token_manager()`)
  - Streamer-OAuth-Token ← `tb_raid::TokenProvider::get_valid_token` (raid_enabled-gated)
  - Mod-Self-Heal ← bestehender Trait `tb_monitoring::ModeratorProvisioner` (`HelixModeratorProvisioner`)
- **`bin/tb-bot`** baut die konkreten Adapter + spawnt die 2 Loops (`chatters_wiring.rs`,
  Aufruf in `main.rs` — macht Claude zentral).

## 3. Prod-Schema (Ground Truth, verifiziert gg. `twitch_analytics`-DB 2026-06-22)

```
twitch_live_state            PK(twitch_user_id)
  twitch_user_id text NN, streamer_login text NN, is_live int4 d0,
  active_session_id bigint, ...                       -- Query: is_live=1 AND active_session_id IS NOT NULL

twitch_session_chatters      PK(session_id, chatter_login)
  session_id bigint NN, streamer_login text NN, chatter_login text NN, chatter_id text,
  first_message_at timestamptz NN, messages int4 d0, is_first_time_streamer bool d false,
  seen_via_chatters_api bool d false, last_seen_at timestamptz, confirmed_first_ever bool d false

twitch_chatter_rollup        PK(streamer_login, chatter_login)
  streamer_login text NN, chatter_login text NN, chatter_id text,
  first_seen_at timestamptz NN, last_seen_at timestamptz NN,    -- baseline war text → Migration 20260622150000 korrigiert
  total_messages int4 d0, total_sessions int4 d0

twitch_viewer_presence_ticks PK(session_id, viewer_login, tick_at)
  session_id bigint NN, streamer_login text NN, viewer_login text NN, tick_at timestamptz NN

twitch_raid_history          (Quelle für Retention)
  id bigint NN, from_broadcaster_login text NN, to_broadcaster_login text NN,
  viewer_count int4 d0, executed_at timestamptz NN, ...

twitch_stream_sessions       PK(id)
  id bigint NN, streamer_login text NN, started_at timestamptz NN, ended_at timestamptz, ...

twitch_raid_retention        PK(raid_id, executed_at)
  raid_id bigint NN, from_broadcaster_login text NN, to_broadcaster_login text NN,
  viewer_count_sent int4 NN, executed_at timestamptz NN,
  target_session_id int4,   -- ACHTUNG: int4, Session-id ist bigint → beim Insert casten
  chatters_at_plus5m/15m/30m int4, known_from_raider int4, new_to_target int4, new_chatters int4,
  computed_at timestamptz d now()
```

**Schema-Treue (PFLICHT, #12-Lektion):** Binds als `i64`→bigint, `&str`→text,
`DateTime<Utc>`→timestamptz, `i32`→int4. Die Migration `20260622150000_chatter_rollup_timestamptz_contract.sql`
(idempotent, schon angelegt) richtet rollup-Timestamps für Neudeploys/Tests an Prod aus.

## 4. Modul A — `tb-monitoring/src/chatters_poller.rs`

### 4.1 Injizierte Ports (Traits in tb-monitoring definieren)
```rust
#[async_trait] pub trait BotChatterAuth: Send + Sync {
    async fn bot_token(&self) -> Option<String>;       // BotTokenManager::access_token
    async fn bot_user_id(&self) -> Option<String>;     // ::bot_user_id  (== moderator_id)
    async fn bot_login(&self) -> Option<String>;        // ::bot_login   (Self-Exclude)
    async fn has_chatters_scope(&self) -> bool;          // 'moderator:read:chatters' in scopes (leer ⇒ true, wie Python)
}
#[async_trait] pub trait StreamerTokenSource: Send + Sync {
    async fn streamer_token(&self, twitch_user_id: &str) -> Option<String>; // TokenProvider::get_valid_token (raid_enabled-gated)
}
// ModeratorProvisioner: bestehender Trait aus subscriptions.rs wiederverwenden.
```

### 4.2 Roster (pro 30s-Tick)
```sql
SELECT ls.twitch_user_id, ls.streamer_login, ls.active_session_id,
       COALESCE(ps.is_partner_active, 0) AS is_partner_active
FROM twitch_live_state ls
LEFT JOIN twitch_streamers_partner_state ps ON LOWER(ps.twitch_login) = LOWER(ls.streamer_login)
WHERE ls.is_live = 1 AND ls.active_session_id IS NOT NULL
```
`is_partner_active` (int, =1) gatet ausschließlich den **Self-Heal** (P2.61), NICHT den
Poll selbst — Python pollt ALLE live. Streamer-Token-Fallback gatet sich selbst über
`get_valid_token` (raid_enabled). `streamer_login`/`chatter_login` IMMER `lower().trim()`.

### 4.3 Pro-Streamer-Poll `_poll_chatters_single` (P2.64)
Token-Reihenfolge exakt wie Python:
1. **Bot-Pfad zuerst:** wenn `bot_token` vorhanden UND `has_chatters_scope()`:
   `helix.get_chatters(broadcaster_id, moderator_id = bot_user_id, &bot_token)`.
   - `Err(HelixError::NotModerator)` (403) → **Self-Heal** (4.4); bei `true` GENAU EIN Retry desselben Bot-Calls.
   - Erfolg → Chatter-Liste; Counter `chatters_bot_path_success_total`.
2. **Streamer-OAuth-Fallback:** NUR wenn Bot-Pfad nicht erfolgreich UND keine Chatter UND
   `streamer_tokens.streamer_token(user_id)` = `Some` (= raid_enabled): `get_chatters(broadcaster_id, moderator_id = broadcaster_id, &streamer_token)`.
   Counter `chatters_reason_fallback_to_streamer_token_total`.
3. Beides fehlend/fehlerhaft → kein Write für diesen Streamer; Counter `chatters_bot_path_failure_total` + `chatters_reason_<reason>_total`.

Concurrency: Polls der Streamer nebenläufig (z.B. `buffer_unordered(8)` als Rate-Schutz —
`get_chatters` paginiert ohne eigenes Retry/Backoff), Ergebnisse sammeln, dann Writes.
`tick_at` = EIN gemeinsamer `Utc::now()` pro Zyklus (truncate auf Sekunde, wie Python
`timespec='seconds'`) für alle Streamer/Tabellen → idempotent gegen Doppellauf.

### 4.4 Bot-Mod-Self-Heal (P2.61)
Trigger: Bot-Pfad-403 `HelixError::NotModerator`. Ablauf:
- Key = `(broadcaster_login_lower)`. **Cooldown-Check** (`HashMap<String, Instant>` hinter
  `Mutex`/`tokio::Mutex`): wenn `now < cooldown` → `false` (kein Heal).
- **Partner-Gate:** nur wenn `is_partner_active == 1` (aus Roster). Sonst `false`.
- `mod_provisioner.ensure_bot_is_mod(broadcaster_id, login).await` (löst intern Streamer-
  Token via `get_valid_token_unrestricted` + `add_channel_moderator`; 422/„already mod" = Erfolg).
- Erfolg → Cooldown-Eintrag entfernen, Counter `chatters_moderator_self_heal_success_total`, `true`.
- Fehler → `cooldown[key] = now + 600s`, Counter `chatters_moderator_self_heal_failure_total`, `false`.

### 4.5 Batch-Write pro Streamer (Reihenfolge ZWINGEND)
Chatter filtern: `is_known_chat_bot(login)` (bestehende KNOWN_CHAT_BOTS aus irc_lurker/tb-chat
wiederverwenden, NICHT duplizieren) **und** `login == bot_login` ausschließen
(saubere Korrektur — der Bot ist kein Viewer; Python tat das nicht, dokumentierte Abweichung).

1. **Pre-Read** (für `is_first_time_streamer`):
   `SELECT chatter_login FROM twitch_chatter_rollup WHERE streamer_login=$1 AND chatter_login = ANY($2)`
   → Set `seen_before`. `is_first_time_streamer = login NOT IN seen_before`.
2. **session_chatters** je Chatter:
   ```sql
   INSERT INTO twitch_session_chatters
     (session_id, streamer_login, chatter_login, chatter_id, first_message_at,
      messages, is_first_time_streamer, seen_via_chatters_api, last_seen_at)
     VALUES ($1, $2, $3, $4, $5, 0, $6, TRUE, $7)
   ON CONFLICT (session_id, chatter_login) DO UPDATE SET last_seen_at = EXCLUDED.last_seen_at
   ```
   $4=chatter_id (leer→NULL), $5/$7=tick_at, $6=is_first_time_streamer. **Nur last_seen_at**
   im Conflict — messages/seen_via_chatters_api/is_first_time_streamer/first_message_at NICHT
   überschreiben (Message-Pfad bleibt erhalten).
3. **rollup** je Chatter:
   ```sql
   INSERT INTO twitch_chatter_rollup
     (streamer_login, chatter_login, chatter_id, first_seen_at, last_seen_at, total_messages, total_sessions)
     VALUES ($1, $2, $3, $4, $5, 0, 1)
   ON CONFLICT (streamer_login, chatter_login) DO UPDATE SET
     last_seen_at = EXCLUDED.last_seen_at,
     chatter_id   = COALESCE(twitch_chatter_rollup.chatter_id, EXCLUDED.chatter_id)
   ```
   $4/$5=tick_at. **total_messages/total_sessions NIE inkrementieren** (Insert 0/1, Conflict no-op) — Message-Zählung passiert nur im Chat-Pfad.
4. **presence_ticks**: `tb_monitoring::record_presence_ticks(&pool, session_id, &streamer_login, &viewer_logins, tick_at)` — fertige fn (irc_lurker.rs:204), schema-treu (schreibt streamer_login), idempotent. 1 Tick/Chatter/Zyklus.

Writes pro Streamer in eigener Transaktion (ein fehlerhafter Streamer rollt nicht alle zurück). Fehler nur loggen, Loop läuft weiter.

## 5. Modul B — `tb-monitoring/src/raid_retention.rs` (P1.24)

Unabhängiger 1h-Loop, reines SQL gegen den Pool (kein Token).
```sql
SELECT id, from_broadcaster_login, to_broadcaster_login, viewer_count, executed_at
FROM twitch_raid_history WHERE executed_at >= NOW() - INTERVAL '7 days' ORDER BY executed_at DESC
```
Pro Raid:
- Skip wenn `(raid_id, executed_at)` schon in `twitch_raid_retention`.
- Ziel-Session: `SELECT id FROM twitch_stream_sessions WHERE LOWER(streamer_login)=$to AND started_at<=$executed AND (ended_at IS NULL OR ended_at>=$executed) ORDER BY started_at DESC LIMIT 1` (timestamptz↔timestamptz, kein Cast gg. Prod) — Skip wenn keine.
- Fenster 5/15/30 (über **session_chatters.last_seen_at**, NICHT presence_ticks):
  `COUNT(DISTINCT COALESCE(NULLIF(chatter_login,''), chatter_id)) FROM twitch_session_chatters WHERE session_id=$target AND last_seen_at>=$executed AND last_seen_at <= $executed + (offset||' minutes')::interval AND <bot-clause>`.
- `known_from_raider` = COUNT(DISTINCT chatter_login) im Ziel-Fenster, die im **rollup des FROM-Streamers** stehen.
- `new_to_target` = COUNT(DISTINCT COALESCE(NULLIF(login,''),id)), die NICHT im rollup des TO-Streamers mit `first_seen_at<executed_at` stehen.
- `new_chatters` = wie new_to_target, aber zusätzlich `first_message_at>=executed_at AND messages>0` (echte Erst-Schreiber; Lurker zählen NICHT).
- bot-clause = `build_known_chat_bot_not_in_clause` (SQL NOT IN, NULL/''-logins bleiben).
- Insert (target_session_id **int4-Cast**):
  ```sql
  INSERT INTO twitch_raid_retention (raid_id, from_broadcaster_login, to_broadcaster_login,
    viewer_count_sent, executed_at, target_session_id, chatters_at_plus5m, chatters_at_plus15m,
    chatters_at_plus30m, known_from_raider, new_to_target, new_chatters)
    VALUES (..., $target_session_id::int4, ...)
  ON CONFLICT (raid_id, executed_at) DO NOTHING
  ```
  DO NOTHING (KEIN UPDATE) — einmal berechnet bleibt fix (Python-Parität).

## 6. Binary-Wiring (`bin/tb-bot`, macht Claude zentral)
- Vor `main.rs:880` (Chat-Handle-Konsum) Bot-Token-Adapter aus `chat_api_handle` ziehen
  (Muster `recruit_chat_api`): `chat_api_handle.as_ref().map(|h| h.bot_token_manager())`.
- Streamer-Token-Adapter aus `build_moderator_token_provider(...)` (main.rs:1510) → `Arc<TokenProvider>`.
- `ModeratorProvisioner` = `HelixModeratorProvisioner::new(token_provider, helix, bot_user_id)` (Muster main.rs:437).
- Neues Modul `chatters_wiring.rs` mit `pub fn spawn_chatters_schedulers(...)` (2 `tokio::spawn`,
  `interval` + `MissedTickBehavior::Delay`, Muster B / Subs-Collector). 30s collect + 1h retention.
- Aufruf bei ~`main.rs:1373` (nach Streamer-Link-Spawn, `pool` noch im Scope → `pool.clone()`).
- Fehlt Bot-Token-Adapter (TB_CHAT_ENABLED aus) → collect-Loop nicht spawnen (kein Poll möglich);
  retention-Loop läuft unabhängig (braucht keinen Token). Sauber loggen.

## 7. Tests (TDD, eigenes prod-treues Fixture)
- **NICHT** das geteilte `support::pool_in_schema` mutieren (bricht Geschwister-Tests).
  Neue fn z.B. `support::pool_with_chatters_schema(schema)` die das geteilte Fixture um
  prod-treue DDL erweitert: `twitch_session_chatters` (volle Spalten + PK), `twitch_chatter_rollup`
  (timestamptz + PK), `twitch_viewer_presence_ticks` (volle Spalten + PK), `twitch_raid_history`,
  `twitch_raid_retention`, `twitch_stream_sessions` (timestamptz started_at/ended_at!).
- Token-Ports + ModeratorProvisioner per Fake-Impls (kein Netz). `get_chatters` über einen
  injizierbaren Helix-Seam ODER die Poll-Logik so kapseln, dass die Chatter-Liste als
  Test-Input einspeisbar ist (Helix-HTTP nicht im Test).
- Fälle: Lurker-Insert (messages=0/seen_via_chatters_api=TRUE), Conflict aktualisiert nur
  last_seen_at, is_first_time_streamer korrekt (pre-read), rollup kein +1 im Conflict,
  presence-tick idempotent (gleicher tick_at), bot/self-Filter, 403→Self-Heal→Retry (Fake-
  Provisioner true/false + Cooldown + Partner-Gate), Streamer-Fallback nur raid_enabled,
  Retention: Fenster-Counts, known_from_raider/new_to_target/new_chatters, skip-if-exists,
  target_session-Auflösung, DO NOTHING.
- `TB_TEST_DATABASE_URL=postgres://postgres:tbtest@127.0.0.1:5434/postgres` + `TB_TEST_REQUIRE_DB=1`.

## 8. Counter / Observability
Python-Counter (über bestehende Metrik-/Observability-Fassade emittieren falls vorhanden,
sonst strukturierte `tracing`-Events für Journal-Verifikation):
`chatters_bot_path_attempt/success/failure_total`, `chatters_reason_fallback_to_streamer_token_total`,
`chatters_reason_<final_reason>_total`, `chatters_moderator_self_heal_success/failure_total`.
`run_cycle` gibt eine Stats-Struktur zurück; die Wiring-fn loggt 1 Zusammenfassung/Tick
(live-Verifikation: „N live Streamer, M Chatter, K Lurker neu").

## 9. Footguns (aus Python-Audit)
1. logins immer `lower().trim()`; Self-Heal-Key zusätzlich führendes `#` strippen.
2. `is_live=1` ist INT-Vergleich, `raid_enabled IS TRUE` Boolean.
3. rollup total_messages/total_sessions im Presence-Poll NIE +1.
4. is_first_time_streamer braucht Pre-Read VOR den Inserts (Reihenfolge!).
5. tick_at = ein gemeinsamer Sekunden-Timestamp pro Zyklus (Idempotenz).
6. target_session_id int4 < session.id bigint → Cast beim Insert.
7. Retention-Fenster über session_chatters.last_seen_at, NICHT presence_ticks.
8. Retention `ON CONFLICT DO NOTHING` (kein UPDATE).
9. eigener Bot ist nicht in KNOWN_CHAT_BOTS → hier zusätzlich per bot_login ausschließen.
