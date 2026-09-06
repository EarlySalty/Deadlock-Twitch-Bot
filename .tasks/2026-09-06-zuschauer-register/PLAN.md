# PLAN: Zuschauer-Register und Pitch nur an echte Neulinge

status: entwurf
datum: 2026-09-06
klasse: hoch
repo: Deadlock-Twitch-Bot
grundlage: CONTRACT.md und EVIDENCE.md in diesem Ordner

## Ausgangslage im Code (verifiziert gegen origin/main, nicht gegen den detached Checkout)

Der geteilte Checkout `~/repos/Deadlock-Twitch-Bot` ist detached auf `020f1c91` und liegt weit hinter `origin/main` (`542e6c17`). Die EVIDENCE-Zeilennummern passen zu `origin/main`, nicht zum Checkout. Alle folgenden Fundstellen sind gegen `origin/main` geprueft. Der Implementierer arbeitet in einem frischen Worktree von `origin/main`.

Kernfunde:

- `on_message_pitch` (`rust/crates/tb-chat/src/promos.rs:750`): Anlass-Pitch. Gate-Reihenfolge heute: Textlaenge < 25 oder `!`-Prefix (`:752`), Chatter == Broadcaster oder leere ID (`:755`), `pitch_semaphore` (`:764`), `pitch_judge_throttle_reserve` (`:769`), Partnerkanal (`:775`), Allowlist (`:784`), Werbefrei/Plan (`:789`), Suppression (`:793`), Startverzoegerung (`:798`), `partner_candidate` -> `run_partner_pitch` (`:805`), `pitch_user_limit_ok` (`:819`), `pitch_channel_limit_ok` (`:825`), dann Judge (`:840`), Filter `pitch_filter_reject` und `pitch_injection_reject` (`:849`, `:861`), Send-Lock (`:873`), Insert-Log-Pending, Send, `mark_pitch_log_sent`, `mark_promo_sent`, Review-Karte (`:919`).
- `run_partner_pitch` (`promos.rs:932`): Partner-Pitch an streamende Zuschauer, `pfad="partner"`, eigene Limits, Ledger, Review-Karte `PitchCardKind::Partner`. Bleibt unveraendert (INV-03).
- `maybe_send_targeted_promo` (`promos.rs:2438`): Timer-Pfad. `want_user`-Zweig (`:2467` bis `:2551`) sendet `pfad="targeted_user"` an einen per `pick_user_target` (`:2620`) zufaellig gewaehlten aktiven Chatter, danach faellt es auf `pfad="targeted_global"` (Announcement) zurueck. `pick_user_target` + `load_user_context_snippets` liefern Ziel und Kontext.
- `promo_pitch.rs` (existiert auf main, 775 Zeilen): `USE_CASE = "promo_pitch"`, `tb_llm::complete`, `.denken_aus()`. Traits `PitchJudge`, `PitchTextGen` (`channel_promo`, `targeted_pitch`), `PartnerPitchGen`. Funktionen `build_targeted_pitch_text(ctx: &TargetedPitchContext)` mit `TARGETED_PITCH_SYSTEM_PROMPT`, `pitch_filter_reject`, `pitch_injection_reject`, `finalize_targeted_pitch`. `TargetedPitchContext { target_login, target_messages: Vec<String>, game, title, recent_chat }`.
- Pitch-Log-Helfer in `promos.rs`: `PitchLogEntry` (`:513`), `record_pitch_log` (`:2994`), `insert_pitch_log_pending` (`:3016` RETURNING id), `mark_pitch_log_sent`, `mark_pitch_log_dropped`. Tabelle `twitch_promo_pitch_log` (Migration `20260905090000_twitch_promo_pitch_log.sql`).
- Review-Karte: Trait `PitchReviewSink::send_card(channel_login, target_login, trigger, reply, kind: PitchCardKind, candidate_hint: Option<&str>)` (`promos.rs:399`), Enum `PitchCardKind { Anlass, Partner }` (`:393`). Impl `DiscordPitchReviewSink` in `chat_wiring.rs:1945`, gesetzt per `set_pitch_review_sink` (`chat_wiring.rs:744`).
- Pipeline: `on_message_pitch` wird pro Nachricht per `tokio::spawn` aufgerufen, nur wenn Deadlock live (`pipeline.rs:1158`).
- PromoEngine-Bau: `chat_wiring.rs:731` bis `:748`. BrokerRelay ist dort ueber `ChatRuntimePorts.invite_relay` / `.review_relay` verfuegbar; konstruiert in `main.rs:1314-1315` aus `settings.broker`.
- Mitgliederliste: `BrokerRelay::list_members()` (`tb-transport-discord/src/relay.rs`) liefert `Vec<GuildMember { guild_id, id, name, global_name: Option, nick: Option }>`. Broker-Endpoint `GET /internal/master/v1/discord/members` in Deadlock-Bots `rust/crates/dl-broker/src/handlers.rs:355-385` liefert `{ok, members:[{id,name,global_name,nick}]}` fuer alle nicht-Bot-Mitglieder der Default-Gilde. **Alle drei Namensfelder sind vorhanden. Weder `relay.rs` noch der Broker muessen erweitert werden.**
- Streamer-Matcher: `streamer_link.rs` (`rust/bin/tb-bot/src/`): `norm_key` (`:181`, NFKD -> ASCII, Kleinschreibung, `leet_replace`, a-z0-9-Tokens, `AFFIXES`-Strip), `similarity` (`:220`, `strsim::jaro_winkler`, exakt = 1.0), `fallback_score` (`:230`, Baender 0.999/0.93/0.82), `MemberIndex { exact: HashMap<norm_key, Vec<GuildMember>> }` mit `build` und `best_match` (`:268`, exakt zuerst, `exact_unique = members.len()==1`, sonst Fuzzy). `FUZZY_FLOOR = 0.62`.
- Gate-Quelltabellen (Schema gegen `twitch_analytics` verifiziert): siehe REQ-05.
- Partnerkanal-Set: View `twitch_streamers_partner_state (twitch_login, twitch_user_id, is_partner, is_partner_active, ...)`. `dach_lock` ist `is_partner_active=1`; 53 aktive Partnerkanaele. `DbPartnerCheck` (`promos.rs:2244`) fragt `SELECT COALESCE(is_partner_active,0) FROM twitch_streamers_partner_state WHERE LOWER(twitch_login)=$1`.
- Session-Start: `twitch_live_state (streamer_login, active_session_id bigint, is_live integer)` -> `twitch_stream_sessions (id, started_at timestamptz)`. Muster `SELECT active_session_id FROM twitch_live_state WHERE streamer_login=$1` (`promos.rs:2251`).
- `WHITELISTED_BOTS = tb_analytics::bekannte_bots::KNOWN_CHAT_BOTS`, exportiert als `crate::mention_scoring::WHITELISTED_BOTS`.
- tb-chat/Cargo.toml hat bereits `unicode-normalization`, `serde_json`, `chrono`, `rand`, `sqlx`, `tb-llm`. `strsim` fehlt (nur in tb-bot) und wird ergaenzt.
- tb-bot/Cargo.toml hat genau ein `[[bin]]` (main.rs), kein clap. Backfill wird als zweites `[[bin]]` gefuehrt.
- Modul `promo_pitch` und `promos` sind in `tb-chat/src/lib.rs` deklariert (`:43`, `:44`). Neu: `pub mod zuschauer_register;`.

Annahme zum parallelen Branch `fix/targeted-user-pitch-aus`: er fuehrt eine Konstante `TARGETED_USER_PITCH_AKTIV = false` ein und klemmt damit den `want_user`-Zweig in `maybe_send_targeted_promo` (`if TARGETED_USER_PITCH_AKTIV && want_user && ...`) samt Regressionstest. Der lokale Branch-Zeiger steht heute noch auf `542e6c17` (identisch main), der Fix ist also noch nicht drin. Dieser Plan setzt darauf auf: REQ-07 entfernt die Konstante und den ganzen Zweig endgueltig. Vor dem Bau `origin/main` frisch holen; ist der Fix schon gemergt, entfaellt nur das Wiederfinden der Konstante, der Rest bleibt gleich.

## 1. Migration

Datei (Zeitstempel beim Bau nach dem Stand von `origin/main` waehlen, aktuell juengste ist `20260905130001`; damit z. B.):

`rust/migrations/20260906120000_twitch_zuschauer_register.sql`

Inhalt (ohne GRANTs, wie alle bestehenden Migrationen; GRANTs sind der manuelle postgres-Schritt, sonst bricht der Test-DB-Aufbau, weil die Rollen dort nicht existieren):

```sql
CREATE TABLE IF NOT EXISTS public.twitch_zuschauer_register (
    twitch_user_id        TEXT PRIMARY KEY,
    twitch_login          TEXT,
    discord_user_id       TEXT,
    community_probability  DOUBLE PRECISION NOT NULL,
    signals               JSONB NOT NULL DEFAULT '{}'::jsonb,
    first_partner_channel  TEXT,
    first_seen_at         TIMESTAMPTZ,
    computed_at           TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_twitch_zuschauer_register_discord
    ON public.twitch_zuschauer_register (discord_user_id);

CREATE INDEX IF NOT EXISTS idx_twitch_zuschauer_register_computed
    ON public.twitch_zuschauer_register (computed_at);
```

Spaltenbelegung nach REQ-01: `twitch_user_id` Schluessel, `twitch_login` nur Anzeige, `discord_user_id` beste Zuordnung (darf NULL sein), `community_probability` das p aus REQ-02 (0 bis 1), `signals` die Einzelsignale als JSON, `first_partner_channel` und `first_seen_at` das erste Auftauchen ueber alle Partnerkanaele, `computed_at` der Berechnungszeitpunkt.

Manuelle Anwendung (der Bot migriert nicht selbst, `TB_DB_MIGRATE=0`):

1. `origin/main` frisch holen, damit die Migrationsnummer eindeutig hinter der juengsten liegt.
2. Als `postgres` anwenden:
   `sudo -u postgres psql -d twitch_analytics -f rust/migrations/20260906120000_twitch_zuschauer_register.sql`
3. Rechte vergeben (INV-09, Least-Privilege, kein CREATE auf public):
   ```sql
   GRANT SELECT, INSERT, UPDATE, DELETE ON public.twitch_zuschauer_register TO twitchbot, twitchdash;
   ```
   (Kein Sequenz-GRANT noetig: kein BIGSERIAL, Schluessel ist der `twitch_user_id`-Text.)
4. Version in `_sqlx_migrations` eintragen. Checksumme = sha384 der Migrationsdatei als hex, Handeintrag:
   ```sql
   INSERT INTO _sqlx_migrations (version, description, installed_on, success, checksum, execution_time)
   VALUES (20260906120000, 'twitch zuschauer register', NOW(), TRUE, decode('<sha384-hex>','hex'), 0);
   ```
   sha384-hex per `sha384sum rust/migrations/20260906120000_twitch_zuschauer_register.sql`.

Der Schema-Snapshot `rust/crates/tb-db/tests/fresh_schema_snapshot.txt` aendert sich durch die Migration und wird mit dem generierten Stand aktualisiert (Teil des Contract-Scopes).

## 2. Modul `rust/crates/tb-chat/src/zuschauer_register.rs`

### 2.1 Datentypen

- `pub struct RegisterEntry { pub twitch_user_id: String, pub twitch_login: Option<String>, pub discord_user_id: Option<String>, pub p: f64, pub signals: serde_json::Value, pub first_partner_channel: Option<String>, pub first_seen_at: DateTime<Utc>, pub computed_at: DateTime<Utc> }`
- `pub struct MemberLite { pub id: String, pub name: String, pub global_name: Option<String>, pub nick: Option<String> }` (transport-freie Kopie von GuildMember, damit tb-chat nicht von tb-transport-discord abhaengt).
- `#[async_trait] pub trait MemberIndexSource: Send + Sync { async fn fetch_members(&self) -> Option<Vec<MemberLite>>; }` (Broker-Fehler -> `None`).
- `struct MemberIndex { exact: HashMap<String, Vec<MemberLite>> }` mit `build`, `best_match(login_key) -> Option<(MemberLite, f64 ratio, bool exact_unique)>`, exakt-1:1 wie in `streamer_link.rs:245-286`.
- `pub struct ZuschauerRegister { pool: PgPool, member_source: Arc<dyn MemberIndexSource>, cache: tokio::sync::Mutex<Option<MemberIndexCacheState>> }` mit `MemberIndexCacheState { built_at: Instant, index: Arc<MemberIndex> }`.
- `pub enum GateOutcome { Pass, Reject(&'static str) }` mit Gruenden `register_community`, `register_fehlt`, `kein_neuling`, `partner_oder_streamer`, `broadcaster_mod_bot` (die fuenf aus REQ-05).

### 2.2 Normalisierung und Aehnlichkeit (Muster aus streamer_link.rs, hier in tb-chat repliziert)

`fn leet_replace(char) -> char`, `static AFFIXES: &[&str]`, `fn norm_key(&str) -> String`, `fn similarity(&str,&str) -> f64 (strsim::jaro_winkler)` eins zu eins nach `streamer_link.rs:166-227`. Neu in tb-chat/Cargo.toml: `strsim = { workspace = true }`. Ein spaeteres Zusammenlegen in eine gemeinsame Crate ist nicht Teil dieses Contracts; die Duplizierung ist bewusst (Bin-Code ist fuer tb-chat nicht importierbar).

### 2.3 Score-Funktion (REQ-02)

Konstanten (im Code, keine Config, keine ENV):

```
PRIOR_COMMUNITY = 0.8            // dach_lock
PRIOR_PARTNER   = 0.2            // jeder andere Partnerkanal
FUZZY_FLOOR     = 0.62           // wie streamer_link.rs
NAMENS_SCORE_EXACT_UNIQUE = 0.90 // exakter, eindeutiger Namenstreffer
NAMENS_SCORE_EXACT_AMBIG  = 0.55 // exakter Treffer, aber mehrere Member mit gleichem Schluessel (Kollision)
NAMENS_SCORE_HIGH  = 0.60        // ratio >= 0.93
NAMENS_SCORE_MID   = 0.40        // ratio >= 0.82
NAMENS_SCORE_LOW   = 0.20        // ratio >= FUZZY_FLOOR
GATE_MAX_P = 0.35                // Schwelle aus REQ-05
```

Signale und Kombination:

1. Harte Zuordnung: existiert in `twitch_streamer_identities` eine Zeile mit `twitch_user_id` und gesetzter `discord_user_id`, ist `p = 1.0`, `discord_user_id` aus dieser Zeile, Signal `{"hard_match": true}`. Fertig, keine weitere Rechnung.
2. Sonst Namensabgleich: `login_key = norm_key(twitch_login)`. `index.best_match(login_key)`:
   - kein Treffer oder `ratio < FUZZY_FLOOR` -> `namens_score = 0.0`, `discord_user_id = None`.
   - exakt und `exact_unique` -> `NAMENS_SCORE_EXACT_UNIQUE`.
   - exakt und mehrdeutig -> `NAMENS_SCORE_EXACT_AMBIG`.
   - fuzzy nach Bandeinordnung `ratio` -> HIGH/MID/LOW.
   - Bei `namens_score > 0` wird die `id` des besten Members als `discord_user_id` uebernommen; das getroffene Feld (name/global_name/nick) kommt als `namens_quelle` ins Signal.
3. Prior: `prior = PRIOR_COMMUNITY` wenn die Person je in `dach_lock` aufgetaucht ist, sonst `PRIOR_PARTNER`. Beim Laufzeit-Anlegen (nur ein Kanal bekannt) `prior = PRIOR_COMMUNITY` falls aktueller Kanal `dach_lock`, sonst `PRIOR_PARTNER`.
4. Kombination: `p = 1.0 - (1.0 - prior) * (1.0 - namens_score)`.
5. `signals` (JSON): `{ "hard_match": bool, "namens_score": f, "namens_quelle": "username|global_name|nick|null", "match_ratio": f, "prior": f, "prior_quelle": "dach_lock|partner", "discord_user_id": "..."|null }`.

`pub fn score(twitch_login: &str, index: &MemberIndex, prior: f64, hard_discord_id: Option<&str>) -> (f64 p, Option<String> discord_id, serde_json::Value signals)`. Reine Funktion ohne DB und ohne LLM, damit Unit-testbar.

### 2.4 Mitgliederindex-Cache

`ZuschauerRegister::member_index(&self) -> Option<Arc<MemberIndex>>`: wenn `cache` gesetzt und `built_at.elapsed() < MEMBER_INDEX_TTL` (`MEMBER_INDEX_TTL = Duration::from_secs(3600)`, eine Stunde) -> Cache zurueckgeben. Sonst `member_source.fetch_members().await`; bei `Some(list)` `MemberIndex::build`, Cache setzen, `Arc` zurueckgeben; bei `None` (Broker nicht erreichbar) den alten Cache behalten falls vorhanden, sonst `None`. Kein Helix-Aufruf. Die Broker-Antwort liefert `name`, `global_name`, `nick` (verifiziert, siehe Abschnitt Ausgangslage), also ist keine Erweiterung an `relay.rs` oder am Broker noetig.

### 2.5 Register-Zugriffe (sqlx, Offline-Daten mit `cargo sqlx prepare` nachziehen)

- `async fn load(&self, twitch_user_id) -> Option<RegisterEntry>`: `SELECT ... FROM twitch_zuschauer_register WHERE twitch_user_id = $1`.
- `async fn upsert(&self, entry: &RegisterEntry)`: `INSERT ... ON CONFLICT (twitch_user_id) DO UPDATE SET twitch_login, discord_user_id, community_probability, signals, computed_at = EXCLUDED... ` und `first_partner_channel`/`first_seen_at` nur setzen wenn bisher NULL (`COALESCE(twitch_zuschauer_register.first_seen_at, EXCLUDED.first_seen_at)`), damit das historische erste Auftauchen nie ueberschrieben wird.
- `async fn ensure_current(&self, twitch_user_id, twitch_login, channel_login) -> Option<RegisterEntry>` (REQ-04-Kern): `load`; existiert Eintrag und `computed_at` juenger als 7 Tage -> zurueckgeben. Existiert er nicht oder ist er aelter als 7 Tage -> Index holen (`member_index`), harte Zuordnung lesen, `prior` aus aktuellem Kanal (`dach_lock`?), `score`, dann `upsert` mit `first_partner_channel = channel_login`, `first_seen_at = now` (nur wenn neu) und zurueckgeben. Ist der Index `None` und kein Alt-Eintrag da -> `None` (Gate lehnt dann mit `register_fehlt` ab, nicht durchlassen).

Alle sqlx-Fehler werden zu `None`/`warn!` degradiert; ein DB-Fehler darf nie zum Durchlassen fuehren (siehe Gate).

### 2.6 Gate-Funktion (REQ-05)

`pub async fn gate(&self, event: &ChatMessageEvent) -> GateOutcome`:

1. Mod- oder Bot-Ausschluss zuerst (kein DB noetig): `event.is_mod_or_broadcaster()` oder `WHITELISTED_BOTS` enthaelt `event.chatter_user_login.to_lowercase()` -> `Reject("broadcaster_mod_bot")`. (Broadcaster/leere ID sind in `on_message_pitch` bereits vorher abgefangen.)
2. Registereintrag sicherstellen: `entry = ensure_current(chatter_user_id, chatter_user_login, channel_login)`. `None` -> `Reject("register_fehlt")`.
3. `entry.p >= GATE_MAX_P` -> `Reject("register_community")`.
4. Neuling-Check: aktuelle Session-Start-Zeit des Kanals laden (`SELECT s.started_at FROM twitch_stream_sessions s JOIN twitch_live_state ls ON ls.active_session_id = s.id WHERE LOWER(ls.streamer_login)=LOWER($1) AND ls.is_live=1`). Wenn `entry.first_seen_at < session_start` -> `Reject("kein_neuling")` (die Person war schon vor dieser Session in einem Partnerkanal). Ist kein Session-Start ermittelbar, gilt das gerade angelegte `first_seen_at = now` als in der Session (frisch angelegte Neulinge bestehen; Alt-Eintraege scheitern an Schritt 3 oder hier).
5. Ausschluss ueber Twitch-User-ID in einer Query mit `EXISTS`-Zweigen (INV-04, nur ID, kein Login/Anzeigename):
   - `twitch_partners` mit gleicher `twitch_user_id` (Status egal, aktiv und departnered zaehlen beide).
   - `twitch_streamers` mit gleicher `twitch_user_id`.
   - `twitch_raid_auth` mit gleicher `twitch_user_id`.
   - `twitch_partner_signup_denylist` mit gleicher `twitch_user_id`.
   - `twitch_scout_pitch_blacklist` mit gleicher `twitch_user_id` (Spalte vorhanden, nicht der Login-PK).
   - `twitch_partner_outreach` mit gleicher `twitch_user_id` UND `contacted_at IS NOT NULL` (mit Kontakt).
   Trifft einer -> `Reject("partner_oder_streamer")`.
6. Sonst `Pass`.

Der Aufrufer (promos.rs) uebersetzt jedes `Reject(grund)` in eine `record_pitch_log`-Zeile mit `reject_reason = grund`, `pfad` je nach Pfad (`anlass` oder `gezielt`), `target_user_id`, `channel_login`, `trigger_text`; kein `generated_text`, kein Send.

`ZuschauerRegister::signals_for(twitch_user_id) -> Option<serde_json::Value>` als kleiner Helfer, damit die Review-Karte (REQ-08) p und Signale anhaengen kann.

## 3. Einbau in promos.rs und pipeline.rs

### 3.1 PromoEngine-Feld und Verdrahtung

- `promos.rs`: neues Feld `zuschauer_register: Option<Arc<crate::zuschauer_register::ZuschauerRegister>>`, Default `None` in `PromoEngine::new`, Setter `pub fn set_zuschauer_register(mut self, r: Arc<...>) -> Self`. Ohne Setter (Tests, kein Broker) verhaelt sich das Gate wie folgt: fehlt das Register komplett, wird jeder Zuschauer-Pitch mit `register_fehlt` abgelehnt (fail-closed). Damit bestehende Anlass-Tests gruen bleiben, bekommen sie im Testaufbau ein `ZuschauerRegister` mit einer Test-`MemberIndexSource` (leere Liste) und einer Test-DB-Zeile; siehe Abschnitt Tests.
- `chat_wiring.rs`: neue Impl `struct BrokerMemberSource { relay: BrokerRelay }` mit `MemberIndexSource`, die `relay.list_members().await` auf `Vec<MemberLite>` mappt (Broker-Fehler -> `None`). `build_runtime` erhaelt aus `ChatRuntimePorts` ein `member_relay: Option<BrokerRelay>`; wenn vorhanden, `engine.set_zuschauer_register(Arc::new(ZuschauerRegister::new(pool.clone(), Arc::new(BrokerMemberSource { relay }))))`.
- `main.rs`: in der `ChatRuntimePorts`-Konstruktion (`:1310`) `member_relay: BrokerRelay::new(&settings.broker).ok()` ergaenzen.

### 3.2 Gate vor den Zuschauer-Pitches (REQ-05)

In `on_message_pitch` das Gate genau nach dem `partner_candidate`-Zweig (`promos.rs:805`, der Partner-Pitch bleibt davor und unberuehrt) und vor `pitch_user_limit_ok` einsetzen:

```
let Some(register) = self.zuschauer_register.as_ref() else {
    self.log_zuschauer_reject(&login, &target_user_id, "register_fehlt", text, "anlass").await;
    self.pitch_judge_throttle_release(&login, &target_user_id);
    return;
};
match register.gate(event).await {
    GateOutcome::Pass => {}
    GateOutcome::Reject(grund) => {
        self.log_zuschauer_reject(&login, &target_user_id, grund, text, "anlass").await;
        self.pitch_judge_throttle_release(&login, &target_user_id);
        return;
    }
}
```

Neuer Helfer `log_zuschauer_reject(login, user_id, grund, trigger, pfad)` schreibt eine `record_pitch_log`-Zeile mit `pfad` und `reject_reason = grund`. Die bestehenden Anlass-Guards (INV-01) bleiben in Reihenfolge und Verhalten unveraendert; das Gate ist ein zusaetzlicher Schritt.

Die Textlaengen-Vorpruefung (`< 25`) am Funktionsanfang wird auf `< 15` gesenkt (Untergrenze fuer den gezielten Pitch aus REQ-06). Der Anlass-Judge wird weiterhin nur fuer Texte `>= 25` Zeichen befragt (bestehendes Anlass-Verhalten bleibt), der gezielte Pitch greift ab 15 Zeichen. Konkret: nach dem Gate

```
let occasion = if text.chars().count() >= 25 { self.pitch_judge.decide(...).await.and_then(|r| r.occasion.map(|o| (o, r.reply))) } else { None };
```

### 3.3 Neuer gezielter Pitch (REQ-06)

Nachrichtenzaehler je Session und Person: neues Feld in PromoEngine `gezielt_state: DashMap<String, GezieltPersonState>` mit Schluessel `"<channel_login>|<chatter_id>"` und `struct GezieltPersonState { session_id: i64, msgs: Vec<String> }`. In `on_message_pitch` (nach dem Gate `Pass`, wenn kein Anlass getroffen wurde) wird fuer qualifizierende Nachrichten (kein `!`-Prefix ist am Anfang schon gefiltert, `>= 15` Zeichen) der Zaehler gefuehrt: aktuelle `active_session_id` des Kanals laden; weicht sie vom gespeicherten `session_id` ab, State zuruecksetzen; aktuelle Nachricht an `msgs` anhaengen (Deckel z. B. letzte 8). Der gezielte Pitch feuert nur, wenn `msgs.len() >= 2` (zweite oder spaetere echte Nachricht) und mindestens eine eigene Nachricht vorliegt.

Ablauf des gezielten Pitch (nur wenn `occasion == None` und Zaehler-Bedingung erfuellt, damit Anlass-Pitch Vorrang hat, REQ-06):

1. Limits gegen `twitch_promo_pitch_log` mit `pfad='gezielt'`:
   - je Twitch-User-ID einmal fuer immer: `SELECT EXISTS(... WHERE target_user_id=$1 AND pfad='gezielt' AND sent_at IS NOT NULL)`; true -> `Reject`-Log `limit_user_ever`.
   - je Kanal zwei pro Stream: `COUNT(... WHERE channel_login=$1 AND pfad='gezielt' AND sent_at >= session_start) >= 2` -> `limit_channel_stream`.
   - fuenfzehn pro Tag gesamt: `COUNT(... WHERE pfad='gezielt' AND sent_at > NOW() - INTERVAL '1 day') >= 15` -> `limit_daily`.
2. Kontext bauen: `TargetedPitchContext { target_login, target_messages: state.msgs.clone(), game, title, recent_chat }` (game/title/recent ueber die vorhandenen `load_live_context` und `load_recent_channel_messages`). Sind `target_messages` leer -> kein Pitch (REQ-06), Log `kein_text`.
3. Text per `self.pitch_text_gen.targeted_pitch(&ctx).await` (das ist `build_targeted_pitch_text`, `tb_llm::complete`, USE_CASE `promo_pitch`, `denken_aus`, INV-02). `None` -> Log `kein_text`.
4. Harte Filter (INV): `pitch_filter_reject(&reply)` und `pitch_injection_reject(&reply, &target_login)`; bei Treffer Log mit Grund, kein Send.
5. Send-Lock des Kanals nehmen, Limits erneut pruefen (Race), `insert_pitch_log_pending(pfad="gezielt", occasion=None, trigger_text=letzte Nachricht, generated_text=out_text)`, `send_message`, bei Drop `mark_pitch_log_dropped("send_dropped")`, sonst `mark_pitch_log_sent` und `mark_promo_sent(login, ..., "gezielt_pitch", ...)`.
6. Review-Karte: `PitchCardKind::Gezielt` (neue Variante), `candidate_hint = Some("p=<..> Signale: <..>")` aus `register.signals_for(target_user_id)` (REQ-08). Anzeige in `DiscordPitchReviewSink` erweitern: `PitchCardKind::Gezielt => "Gezielter Pitch"`.

INV-01 gilt fuer beide Pfade: Werbefrei-Plan, Allowlist, Suppression, Startverzoegerung, Doppelsend-Lock, Judge-Drossel und Semaphore werden im gemeinsamen Kopf von `on_message_pitch` durchlaufen und decken auch den gezielten Pitch ab (er sitzt im selben Funktionskoerper, hinter denselben Guards). Der gezielte Pitch verbraucht dieselbe `pitch_semaphore`-Reservierung und dieselbe Judge-Drossel-Reservierung wie der Anlass-Pitch, also genau ein LLM-Aufruf pro Nachricht.

Enum-Erweiterung: `pub enum PitchCardKind { Anlass, Partner, Gezielt }`. Das ist der einzige Bruch an einem oeffentlichen Typ; alle `match`-Stellen sind in `promos.rs` und `chat_wiring.rs` (beide im Scope).

### 3.4 Rueckbau des alten Timer-Pfads (REQ-07)

In `maybe_send_targeted_promo` (`promos.rs:2438`):

- Den kompletten `want_user`-Zweig (heute `:2467` bis `:2551`, inklusive `pick_user_target`-Aufruf, `load_user_context_snippets`, `pfad="targeted_user"`-Logs und dem `channel_last_type="user"`-Set) loeschen. Ist der Fix-Branch schon gemergt, ist dieser Zweig bereits in `if TARGETED_USER_PITCH_AKTIV && ...` gekapselt; dann Konstante und Kapselung mit entfernen.
- Die Funktion behaelt nur noch den `targeted_global`-Announcement-Pfad. Der `cd_ok`-Check bleibt; die `want_user`-Berechnung und `channel_last_type`-Alternierung entfallen.
- `pick_user_target` (`:2620`) und `load_user_context_snippets` loeschen (nur vom entfernten Zweig genutzt).
- In `TargetedState` die Felder `user_last_pitched` und `channel_last_type` entfernen; nur `channel_last_targeted` bleibt.
- Ungenutzte Konstanten entfernen: `USER_PITCH_COOLDOWN_SEC`, `STAMMGAST_MIN_MESSAGES`, `STAMMGAST_DAYS` (nach dem Loeschen pruefen, dass sie nirgends sonst referenziert sind).
- `PitchTextGen::targeted_pitch` / `build_targeted_pitch_text` / `TargetedPitchContext` bleiben erhalten, weil der neue gezielte Pitch sie nutzt.
- `TARGETED_USER_PITCH_AKTIV` (falls durch den Fix-Branch vorhanden) restlos entfernen.

Bestehende Tests, die `targeted_user` erwarten, werden nicht abgeschwaecht (INV-07), sondern durch den neuen Nachweis ersetzt (Abschnitt Tests).

## 4. Backfill (REQ-03)

Neues Bin: Eintrag in `rust/bin/tb-bot/Cargo.toml`:

```
[[bin]]
name = "zuschauer-register-backfill"
path = "src/zuschauer_register_backfill.rs"
```

Datei `rust/bin/tb-bot/src/zuschauer_register_backfill.rs`:

- `#[tokio::main]`, `tracing_subscriber::fmt::init()`, `Settings::from_env()`, `tb_db::connect(&settings.db)`, `BrokerRelay::new(&settings.broker)` (wie main.rs). Laeuft im selben Umfeld wie der Dienst (DB-DSN und Broker aus Infisical ueber das Start-Wrapper-Env). Kein neues Secret.
- Mitgliederindex einmal ueber `list_members()` laden und `MemberIndex::build`. Ist der Broker nicht erreichbar, mit Fehlercode abbrechen (kein Register ohne Namensdaten).
- Vorberechnungen als Aggregat-Queries (nicht pro Person):
  - Partnerkanal-Set: `SELECT LOWER(twitch_login) FROM twitch_streamers_partner_state WHERE is_partner_active = 1` plus `dach_lock`.
  - Erstes Auftauchen je Twitch-User-ID ueber Partnerkanaele: eine `GROUP BY chatter_id`-Query ueber `twitch_session_chatters` gejoint auf das Partnerkanal-Set: `MIN(first_message_at) AS first_seen`, `MIN(streamer_login) KEEP` bzw. per Fensterfunktion den Kanal des Minimums als `first_partner_channel`, plus `BOOL_OR(LOWER(streamer_login)='dach_lock') AS in_community`.
  - Login-only-Zeilen (50 Prozent ohne `chatter_id`): ueber `twitch_login_aliases` nachschluesseln (`SELECT twitch_user_id, login FROM twitch_login_aliases`), Login -> ID-Map im Speicher; Zeilen, deren Login dort nicht auffloest, bleiben ohne Registereintrag (REQ-03).
  - Grundgesamtheit der IDs: distinct `chatter_id` aus `twitch_session_chatters` und `twitch_chat_messages`, plus die per Alias aufgeloesten IDs.
  - Harte Zuordnungen: `SELECT twitch_user_id, discord_user_id FROM twitch_streamer_identities WHERE discord_user_id IS NOT NULL` als Map.
- Pro ID: `score(login, &index, prior, hard)` mit `prior = 0.8` falls `in_community`, sonst `0.2`; `first_partner_channel`/`first_seen_at` aus dem Aggregat; `upsert`. Batch-Groesse 500 pro Transaktion (`INSERT ... ON CONFLICT DO UPDATE`), idempotent: ein zweiter Lauf liefert dieselben Zeilen (first_seen bleibt via COALESCE stehen, computed_at wird erneuert).
- Log am Ende nur Zahlen (INV-08): Anzahl Eintraege gesamt, davon mit Discord-Zuordnung, und die Verteilung von p in vier Stufen (`< 0.35`, `0.35..0.6`, `0.6..0.8`, `>= 0.8`). Keine Logins, keine Namen, keine IDs.

Laufzeitabschaetzung: 14398 distinct IDs, alle rechenintensiven Teile sind im Speicher (Index-Lookup je ID O(1) bis O(Fuzzy-Menge)). Die Fuzzy-Suche laeuft nur bei Fehlschlag der exakten Suche; sie iteriert ueber rund 2500 Member-Keys je Miss. Worst case 14398 x 2500 Jaro-Winkler-Vergleiche sind wenige Sekunden CPU. DB: drei Aggregat-Queries plus rund 29 Batch-Upserts. Gesamt deutlich unter 10 Minuten, realistisch ein bis drei Minuten. Alias-Aufloesung ist ein einzelner Table-Scan von `twitch_login_aliases`.

## 5. Laufende Pflege (REQ-04)

- "Erstes Auftauchen in einem Partnerkanal" ist im tb-chat-Pfad ohne `tb-monitoring` (verboten) genau der Moment, in dem `on_message_pitch` fuer eine `chatter_user_id` laeuft: die Funktion wird pro Nachricht in Deadlock-Live-Partnerkanaelen aufgerufen (`pipeline.rs:1158`), der Partnerkanal-Check sitzt im Kopf. `ZuschauerRegister::ensure_current` legt beim ersten `load`-Miss den Eintrag sofort aus dem gecachten Mitgliederindex an (kein Helix pro Nachricht, INV-06). Damit ist das Anlegen an den bestehenden Nachrichtenpfad gekoppelt, ohne `tb-monitoring` anzufassen.
- Kein Nachtrag pro Poll, keine Reparaturschleife: `ensure_current` schreibt genau einmal beim ersten Sehen und danach nur, wenn `computed_at` aelter als 7 Tage ist (dann Neuberechnung von p, discord_user_id und signals; first_seen/first_partner_channel bleiben stehen).
- Cache-TTL des Mitgliederindex: `MEMBER_INDEX_TTL = 3600s`. Bei Broker-Ausfall bleibt der letzte Index gueltig bis zum naechsten erfolgreichen Refresh; ist noch nie einer geladen worden, liefert das Gate `register_fehlt` (fail-closed).

## 6. Tests

Testumgebung: tb-chat-Tests laufen gegen den Docker-Test-Container aus `rust/scripts/test_db.sh` (Container `tb-test-postgres`, nur per `sudo docker`), `SQLX_OFFLINE=1`, Toolchain rustc 1.97 aus `~/.rustup/toolchains` (Toolchain-`bin/` vorn im PATH). Die DB-Tests nutzen das vorhandene `pool_or_skip!`-Muster.

Unit-Tests (`zuschauer_register.rs`, ohne DB):

- `norm_key_gleich_wie_streamer_link`: `norm_key("EarlySaltyTTV")` liefert denselben Schluessel wie in streamer_link (Affix-Strip, Kleinschreibung).
- `score_harte_zuordnung_ist_eins`: mit `hard_discord_id = Some(..)` ist `p == 1.0` und `discord_id` gesetzt, unabhaengig vom Namen.
- `score_exakter_eindeutiger_treffer_hoch`: exakter, eindeutiger Namenstreffer auf Partnerkanal (prior 0.2) liefert p ueber der Schwelle 0.35 (also spaeter Ablehnung).
- `score_kein_treffer_partnerkanal_niedrig`: kein Namenstreffer, prior 0.2 -> p == 0.2 (< 0.35, Neuling).
- `score_community_prior_hoch`: prior 0.8 (dach_lock), kein Namenstreffer -> p == 0.8 (>= 0.35).
- `score_kombination_formel`: prueft `p = 1 - (1-prior)*(1-namens_score)` an einem Zahlenbeispiel.
- `member_index_exakt_mehrdeutig_gibt_niedrigeren_score`: zwei Member mit gleichem norm_key -> `NAMENS_SCORE_EXACT_AMBIG` statt `EXACT_UNIQUE`.

DB-Tests (`tb-chat/tests/` oder `#[cfg(test)]` in promos.rs, `pool_or_skip!`):

- `gate_lehnt_ohne_register_eintrag_ab`: kein Registereintrag, kein Index -> `register_fehlt`, kein Send, Log-Zeile mit dem Grund.
- `gate_lehnt_bei_hoher_wahrscheinlichkeit_ab`: Eintrag mit p >= 0.35 -> `register_community`.
- `gate_lehnt_alt_bekannten_ab`: Eintrag mit `first_seen_at` vor Session-Start -> `kein_neuling`.
- `gate_lehnt_partner_streamer_raid_denylist_blacklist_outreach_ab`: je eine Zeile in `twitch_partners`, `twitch_streamers`, `twitch_raid_auth`, `twitch_partner_signup_denylist`, `twitch_scout_pitch_blacklist`, `twitch_partner_outreach (contacted_at gesetzt)` -> jeweils `partner_oder_streamer` (parametrisiert, je Tabelle ein Fall).
- `gate_lehnt_mod_und_bot_ab`: Event mit Moderator-Badge bzw. Login in `WHITELISTED_BOTS` -> `broadcaster_mod_bot`.
- `gate_laesst_frischen_neuling_durch`: neuer Eintrag, p < 0.35, first_seen in Session, keine Ausschlusszeile -> `Pass`.
- `gezielter_pitch_erst_ab_zweiter_nachricht`: eine Nachricht -> kein gezielter Pitch; zweite Nachricht -> gezielter Pitch (Log `pfad='gezielt'`, `sent_at` gesetzt, Review-Karte `Gezielt`).
- `gezielter_pitch_ohne_eigene_nachrichten_kein_pitch`: leere `target_messages` -> `kein_text`, kein Send.
- `gezielter_pitch_limit_einmal_pro_user`: bestehende gesendete `gezielt`-Zeile fuer die User-ID -> zweiter Versuch `limit_user_ever`.
- `gezielter_pitch_limit_zwei_pro_kanal_und_stream` und `gezielter_pitch_limit_fuenfzehn_pro_tag`: Grenzwerte.
- `anlass_hat_vorrang_vor_gezielt`: trifft der Judge einen Anlass, geht nur der Anlass-Pitch, kein `gezielt`-Eintrag fuer dieselbe Nachricht.
- `ensure_current_erneuert_nach_sieben_tagen_ohne_first_seen_zu_aendern`: `computed_at` alt, `first_seen_at` bleibt beim Neuberechnen unveraendert, `computed_at` wird neu.

Rueckbau-Nachweis (REQ-07, INV-07), ersetzt den Regressionstest des Abschalt-Fixes:

- `kein_timer_pitch_an_einzelpersonen`: `maybe_send_targeted_promo` mit gefuellter aktiver Chatterliste sendet nie `pfad='targeted_user'`; nach dem Lauf existiert keine `twitch_promo_pitch_log`-Zeile mit `pfad='targeted_user'`, und der einzige gesendete Pfad ist `targeted_global`. Beweist, dass der Timer keinen Einzelpersonen-Pitch mehr erzeugt.
- Die vorhandenen `targeted_global`-Tests (`targeted_global_haengt_invite_an` u. a.) bleiben unveraendert gruen (INV-03).

INV-03-Tests (Partner-Pitch, `targeted_global`, Scout, Outreach): unveraendert, muessen gruen bleiben.

## 7. Schritte in Reihenfolge (jeder Schritt baut und testet gruen)

1. Frischen Worktree von `origin/main` anlegen; pruefen, ob `fix/targeted-user-pitch-aus` und der Partner-Pitch-Branch schon gemergt sind. (10 min)
2. Migration schreiben, als postgres anwenden, GRANTs setzen, `_sqlx_migrations` eintragen, `fresh_schema_snapshot.txt` neu erzeugen. Commit "migration: zuschauer-register". (45 min)
3. Modul `zuschauer_register.rs`: Datentypen, norm_key/similarity, `score`, `MemberIndex`, `MemberIndexSource`, Cache, DB-Zugriffe, `gate`. `strsim` in tb-chat/Cargo.toml. `pub mod zuschauer_register` in lib.rs. Unit-Tests aus Abschnitt 6 (ohne DB) gruen. `cargo sqlx prepare`, `.sqlx` committen. Commit "zuschauer-register: modul, score, gate". (4 h)
4. DB-Tests des Gates (`pool_or_skip!`) gegen den Test-Container gruen. Commit "zuschauer-register: gate-tests". (2 h)
5. Einbau in promos.rs: Feld, Setter, Gate vor Zuschauer-Pitches, Laengengrenze 15, `log_zuschauer_reject`. chat_wiring.rs und main.rs verdrahten (`BrokerMemberSource`, `member_relay`). Bestehende Anlass-Tests mit Test-Register versorgen, gruen. Commit "promos: gate vor anlass-pitch". (3 h)
6. Gezielter Pitch: `gezielt_state`, Zaehler, Limits, LLM ueber `targeted_pitch`, Filter, Send, Log, Review-Karte `PitchCardKind::Gezielt`. Tests aus Abschnitt 6 gruen. `cargo sqlx prepare`. Commit "promos: gezielter pitch (req-06)". (4 h)
7. Rueckbau Timer-Pfad (REQ-07): `want_user`-Zweig, `pick_user_target`, `load_user_context_snippets`, TargetedState-Felder, tote Konstanten, ggf. `TARGETED_USER_PITCH_AKTIV`. Neuer Test `kein_timer_pitch_an_einzelpersonen`. Commit "promos: timer-einzelpitch entfernt (req-07)". (2 h)
8. Backfill-Bin: Cargo.toml-Eintrag, `zuschauer_register_backfill.rs`, `cargo sqlx prepare`. Trockenlauf gegen Test-DB (kleine Fixtures). Commit "backfill: zuschauer-register". (3 h)
9. FAQ `rust/knowledge/bot/faq-werbung.md` wahrheitsgemaess ergaenzen: der Bot spricht nur neue Zuschauer an (REQ-08). Commit "faq: nur neue zuschauer". (30 min)
10. Selbstpruefung (`gate_hook.py --review` gegen die eigene Arbeit), dann frischer Reviewer gegen Diff + Contract. Danach Merge-Gate, merge nach main, push. (nach Auftrag)
11. Deploy: Backfill einmalig als `postgres`-Umfeld-Dienstlauf ausfuehren (`zuschauer-register-backfill`), Zahlen pruefen. Bot-Release bauen und `deadlock-twitch-bot-rust` neu starten. Live-Pruefung: eine Reject-Zeile je Grund in `twitch_promo_pitch_log` beobachten, ein echter Neuling-Pitch. (nach Auftrag)

Summe reine Implementierung (Schritte 1 bis 9): rund 22 bis 24 Stunden Agentenarbeit.

## 8. Risiken und Bedenken

- Zuschauer ohne Twitch-User-ID: 50 Prozent der historischen Chatter-Zeilen tragen keine ID. Der Backfill schluesselt ueber `twitch_login_aliases` nach; was dort nie bekannt war, bekommt keinen Eintrag. Im Laufzeitpfad ist die ID aus dem EventSub-Event immer gesetzt (`on_message_pitch` bricht bei leerer ID ohnehin ab), das betrifft also nur die Historie, nicht das Gate.
- Namenskollisionen: mehrere Discord-Member mit gleichem norm_key liefern `NAMENS_SCORE_EXACT_AMBIG` statt vollem Treffer, damit ein haeufiger Name nicht faelschlich eine hohe Community-Wahrscheinlichkeit erzeugt. Trotzdem bleibt Jaro-Winkler ein Naeherungsmass; die Schwelle 0.35 ist bewusst so gewaehlt, dass ein einzelner schwacher Namenstreffer (LOW 0.20) auf einem Partnerkanal (prior 0.2, kombiniert 0.36) knapp ueber der Schwelle liegt und den Pitch verhindert. Ob das zu streng ist, zeigen die Backfill-Zahlen (offene Produktfrage im Contract, Anpassung als Amendment).
- Last durch die Mitgliederliste: `list_members()` zieht rund 2500 Member; mit TTL 1h im Speicher gehalten, ein Broker-Aufruf pro Stunde und Prozess. Der `MemberIndex`-Aufbau ist O(Member). Kein Aufruf pro Nachricht.
- Broker nicht erreichbar: Cache haelt den letzten Index; ohne je geladenen Index lehnt das Gate mit `register_fehlt` ab (fail-closed, nie durchlassen). Das ist konservativ korrekt: im Zweifel kein Pitch.
- DB-Fehler im Gate: jede Query degradiert zu Ablehnung, nicht zu Durchlass.
- Contract-Bedenken zum GRANT-Ort (INV-09): INV-09 verlangt, dass "die Migration" die Rechte vergibt. Alle bestehenden Migrationen enthalten aber keine GRANTs, weil der Test-DB-Aufbau die Rollen `twitchbot`/`twitchdash` nicht kennt und an einem GRANT scheitern wuerde. Der Plan haelt sich an die Repo-Konvention: CREATE/INDEX in der `.sql`, GRANT im manuellen postgres-Schritt. Das erfuellt INV-09 sinngemaess (die Migration wird als postgres angewendet und die Rechte im selben Vorgang gesetzt), weicht aber vom Wortlaut "die Migration legt an und vergibt" ab. Falls der Nutzer den Wortlaut streng will, muessten die GRANTs in die `.sql` und der Test-Schema-Aufbau die Rollen vorab anlegen; das beruehrt Test-Infrastruktur ausserhalb des Scopes und ist deshalb nicht eingeplant.
- Scope: `PitchCardKind` bekommt die Variante `Gezielt`; alle Matcher liegen in `promos.rs` und `chat_wiring.rs` (im Scope). `rust/crates/tb-transport-discord/src/relay.rs` und `lib.rs` stehen zwar im erlaubten Bereich, werden aber voraussichtlich nicht angefasst (GuildMember ist bereits `pub` mit allen Feldern, Broker liefert alle Namensfelder). Sollte der Import von `GuildMember` in chat_wiring wider Erwarten ein Re-Export brauchen, ist der Pfad abgedeckt.
- Migrationsnummer: erst nach frischem `origin/main` vergeben (Merge-Schleuse-Regel), sonst Kollision mit einer zwischenzeitlich gemergten Migration.
