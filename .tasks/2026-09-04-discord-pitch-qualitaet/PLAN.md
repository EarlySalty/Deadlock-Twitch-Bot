status: aktiv
datum: 2026-09-05

# Plan: Discord-Pitches mit Qualität statt Preset-Sprüchen

Maßstab ist der Contract `.tasks/2026-09-04-discord-pitch-qualitaet/CONTRACT.md` (unveränderlich, inklusive Amendment vom 2026-09-04: der Anlass-Pitch geht als Nachricht mit `@login`-Anrede statt als echter Twitch-Reply, weil `ChatApi::send_message` kein `reply_parent` kennt). Belege in `RESEARCH.md` und `EVIDENCE.md` im selben Ordner.

Alle Pfade relativ zu `rust/` im Repo `Deadlock-Twitch-Bot`. Cargo ist immer `/home/nathanael/.cargo/bin/cargo` (1.97.1); `/usr/bin/cargo` ist 1.75 und bricht. Keine Code-Kommentare im neuen Code, bestehende Kommentare in angefassten Funktionen löschen statt erweitern. Nutzersichtbare Texte mit echten Umlauten, keine Gedankenstriche.

Vorbild für den ganzen Anlass-Pfad ist das bereits gemergte `lfg_pitch.rs` (Responder-Struct, `maybe_respond`/`decide`, injizierbarer `LfgJudge`-Trait, Notifier-Seam, In-Memory-Cooldowns). Wo möglich dieselbe Bauform übernehmen.

## Baseline zuerst messen (vor M1)

- `/home/nathanael/.cargo/bin/cargo test -p tb-chat 2>&1 | tail -40` einmal laufen lassen und den roten Stand (Testnamen, Fehlermeldung, Zählung) in `EVIDENCE.md` unter einem neuen Abschnitt `## Rote Baseline <datum>` festhalten. DB-Tests ohne `TB_TEST_DATABASE_URL` gelten als SKIP, nicht als rot.
- Danach `git rev-parse HEAD` notieren. Ab jetzt gilt: neue rote Tests, die nicht in der Baseline stehen, sind ein Fehler.

Stop-Regel: Läuft die Baseline nicht bis zum Ende durch (Compile-Fehler in main), erst diesen Stand klären, nicht bauen.

---

## M1 - tb-llm-Use-Case `promo_pitch` in der Nur-Fireworks-Liste

Änderungen:
- `crates/tb-llm/src/selection.rs:14`: `FIREWORKS_ONLY_USE_CASES` von `&["ricky_crew_review", "outreach_shadow"]` auf `&["ricky_crew_review", "outreach_shadow", "promo_pitch"]` erweitern. Keine weitere Logik, `endpoint_for` bindet ohnehin jeden Use-Case an Deepseek V4 Flash (`selection.rs:26`).

Erwarteter Zwischenzustand: tb-llm baut, die Liste enthält den neuen Use-Case. Der Guard-Test dazu entsteht in M7.

Validierung: `/home/nathanael/.cargo/bin/cargo build -p tb-llm`

Stop-Regel: Kein Modellname und keine Base-URL ändern (INV-02). Bei Build-Fehler abbrechen und klären.

STATUS M1 erledigt 2026-09-05: selection.rs:14 erweitert um "promo_pitch". `cargo build -p tb-llm` EXIT=0.

---

## M2 - Neues Modul `promo_pitch.rs` (reine Logik, ohne DB, ohne Verdrahtung)

Änderungen:
- Neue Datei `crates/tb-chat/src/promo_pitch.rs`, in `crates/tb-chat/src/lib.rs` als `pub mod promo_pitch;` eintragen (Block bei `lib.rs:43`, alphabetisch nach `pipeline`).
- Konstanten:
  - `pub const USE_CASE: &str = "promo_pitch";`
  - `const PITCH_TIMEOUT: Duration = Duration::from_secs(20);`
  - `pub const PITCH_SYSTEM_PROMPT: &str = r#"..."#;` als Kopie des Stilvertrags aus `crates/tb-engagement/src/outreach_shadow.rs:23` (`OUTREACH_SYSTEM_PROMPT`), zugeschnitten auf den Anlass-Pitch im eigenen Partnerkanal: kein Import aus tb-engagement (die Filter dort sind privat, INV-08 verbietet Änderungen). Der Prompt schreibt fest: Deutsch, kurz, locker, Kleinschreibung erlaubt, kein Emoji außer `:)`, keine Superlative, keine Mitgliederzahlen, kein Gedankenstrich, kein Link, keine Ausrufezeichen-Werbung, keine Wendungen wie "komm auf"/"join"/"tritt bei". Zwei Teile in fester Reihenfolge: erst echte Antwort auf das Gesagte, dann höchstens ein Satz zur Community in dritter Person. Anlass-Bindung: nur bei `no_mates`, `game_unpopular`, `too_tryhard`, `solo_queue`, `new_player`, `wants_help`; sonst `occasion: null` und leer.
  - Antwortformat im Prompt: `{"occasion": null|"no_mates"|"game_unpopular"|"too_tryhard"|"solo_queue"|"new_player"|"wants_help", "reply": "...", "confidence": 0.0}`.
- Typen:
  - `#[derive(Deserialize, Serialize, ...)] pub enum PitchOccasion` mit `serde(rename_all = "snake_case")` und den sechs Werten.
  - `pub struct PitchResponse { pub occasion: Option<PitchOccasion>, pub reply: String, pub confidence: f32 }`.
- Parser `pub fn parse_pitch_response(raw: &str) -> Option<PitchResponse>`: JSON-Objekt aus dem Rohtext ziehen (Muster `extract_json_object` aus `lfg_pitch.rs:212` als Kopie, da privat) und deserialisieren; bei fehlendem `occasion` oder leerem `reply` gibt der Anlass-Pfad nichts (occasion=null bedeutet: kein Pitch).
- Harte Filter, alle privat, kopiert und angepasst aus `outreach_shadow.rs` (`contains_link` :410, `contains_member_count` :424, `contains_superlative` :461, `contains_forbidden_emoji` :478) plus drei zusätzliche Regeln:
  - `contains_hard_dash(text) -> bool`: `\u{2014}`, `\u{2013}`, `\u{2015}`, ` -- `, ` - ` (identisch zum Test-Helfer `hat_gedankenstrich` in `promos.rs:2508`; `contains_forbidden_emoji` in outreach whitelistet Striche und reicht hier nicht).
  - Längengrenze über 400 Zeichen.
  - Wendungen `"komm auf"`, `"join"`, `"tritt bei"` (lowercase).
  - `pub enum PitchRejectReason { Link, MemberCount, Superlative, Dash, Emoji, TooLong, JoinPhrase }` (fürs Log).
  - `pub fn pitch_filter_reject(text: &str) -> Option<PitchRejectReason>`: prüft in fester Reihenfolge und gibt den ersten Verstoß zurück, sonst `None`.
- Injizierbarer Judge (Bauform wie `LfgJudge` `lfg_pitch.rs:139`):
  - `#[async_trait] pub trait PitchJudge: Send + Sync { async fn decide(&self, input: PitchJudgeInput) -> Option<PitchResponse>; }`
  - `pub struct PitchJudgeInput { pub trigger_text: String, pub game: Option<String>, pub title: Option<String>, pub recent_chat: Vec<String>, pub target_login: String }` (JSON-serialisierbar als User-Message).
  - `pub struct FireworksPitchJudge;` mit `impl PitchJudge`: baut `tb_llm::Request::simple(PITCH_SYSTEM_PROMPT, serde_json::to_string(&input))` `.temperature(0.0).json_object().timeout(PITCH_TIMEOUT)`, ruft `tb_llm::complete(USE_CASE, request)` (`hub.rs:279`), parst mit `parse_pitch_response`. Bei Timeout/Fehler/leer `None`. Muster wörtlich aus `outreach_shadow.rs:225` (`decide`).
- Textgenerierung für die beiden Timer-Pfade (eigene Funktionen, direkter `tb_llm::complete`-Aufruf, kein Judge-Trait nötig, geben `Option<String>` zurück; bei fehlendem/ungültigem Text `None`):
  - `pub async fn build_channel_promo_text(ctx: &ChannelPromoContext, invite: &str) -> Option<String>`: periodische Kanal-Promo, hängt den Invite-Link ans Ende. Vor dem Rückgabewert `pitch_filter_reject` OHNE Link-Regel prüfen (der Invite ist erlaubt, deshalb den Link erst nach dem Filter anfügen). Eigener Prompt-Teil im Systemprompt oder ein zweiter `pub const CHANNEL_PROMO_SYSTEM_PROMPT`.
  - `pub async fn build_targeted_pitch_text(ctx: &TargetedPitchContext) -> Option<String>`: personenbezogen, NIE mit Link, `pitch_filter_reject` inklusive Link-Regel.
  - Kontext-Structs `ChannelPromoContext { game, title, recent_chat: Vec<String> }`, `TargetedPitchContext { target_login, target_messages: Vec<String>, game, title, recent_chat: Vec<String> }`.
- Unit-Tests in `promo_pitch.rs` (`#[cfg(test)]`, kein DB): Parser (gültiges JSON mit occasion, occasion=null, kaputtes JSON), jeder harte Filter positiv und negativ, `contains_hard_dash` gegen die Strich-Menge, 400-Zeichen-Grenze, Join-Wendungen, Reihenfolge von `pitch_filter_reject`.

Erwarteter Zwischenzustand: tb-chat baut, das Modul ist eigenständig testbar, noch nirgends verdrahtet.

Validierung: `/home/nathanael/.cargo/bin/cargo test -p tb-chat promo_pitch::`

Stop-Regel: Kein `pub`-Machen der outreach-Filter, kein Import aus tb-engagement, keine Änderung an `outreach_shadow.rs`. Filter sind Kopie.

STATUS M2 erledigt 2026-09-05: promo_pitch.rs angelegt (Prompts, PitchOccasion/PitchResponse, Parser, harte Filter inkl. contains_hard_dash/400-Zeichen/Join, PitchJudge+FireworksPitchJudge, ChannelPromo/Targeted-Textpfade mit reinen finalize-Helfern). lib.rs eingetragen und re-exportiert. `cargo test -p tb-chat promo_pitch::` = 18 passed, 0 failed.

---

## M3 - Migration `twitch_promo_pitch_log`, DDL im Test, Log-Helfer, sqlx-Offline

Änderungen:
- Neue Datei `rust/migrations/20260905090000_twitch_promo_pitch_log.sql`, additiv, Muster `20260903090000_twitch_moderation_settings.sql` (reines `CREATE TABLE IF NOT EXISTS public.twitch_promo_pitch_log`, KEIN GRANT im File, INV-09). Spalten nach REQ-07:
  - `id bigserial primary key`
  - `channel_login text not null`
  - `target_user_id text` (nullbar: Timer-Global und periodische Promo haben keine Zielperson)
  - `pfad text not null` (`anlass` | `periodic` | `targeted_user` | `targeted_global`)
  - `occasion text` (nullbar)
  - `trigger_text text` (nullbar)
  - `generated_text text` (nullbar)
  - `reject_reason text` (nullbar; NULL = gesendet)
  - `sent_at timestamptz` (NULL = verworfen)
  - `created_at timestamptz not null default now()`
  - Index auf `(target_user_id, sent_at)` und `(channel_login, sent_at)` für die Limit-Abfragen.
- Log-Helfer in `crates/tb-chat/src/promos.rs`: `async fn record_pitch_log(&self, entry: PitchLogEntry)` mit `sqlx::query!`-INSERT gegen `twitch_promo_pitch_log`. `PitchLogEntry`-Struct trägt alle Spalten. Fehler beim Insert nur `tracing::warn!`, nie den Sendepfad blockieren.
- DB-Test-DDL: in `apply_ddl` (`promos.rs:3288`) das `CREATE TABLE`-Statement für `twitch_promo_pitch_log` ergänzen, damit `mod db_tests` (`promos.rs:3233`) die Tabelle im eigenen Schema hat.
- sqlx-Offline: nach dem Anlegen der `query!`-Aufrufe (auch die aus M4/M5) einmal `cargo sqlx prepare` gegen eine DB mit der neuen Tabelle laufen lassen, damit `rust/.sqlx/` die neuen `query-*.json` bekommt. Reihenfolge: erst die Tabelle in der Prepare-DB anlegen (Migration einspielen), dann prepare. Dieser Schritt gehört ans Ende von M5 (wenn alle `query!` stehen), hier nur vormerken.

Erwarteter Zwischenzustand: Migration liegt vor, Helfer und DDL sind da, Offline-Daten folgen in M5.

Validierung (nach M5-Prepare): `/home/nathanael/.cargo/bin/cargo build -p tb-chat` (Offline-Build grün). DB-Test: `TB_TEST_DATABASE_URL=... /home/nathanael/.cargo/bin/cargo test -p tb-chat db_tests`.

Stop-Regel: Kein GRANT ins Migrations-File. Kein `alter` an bestehenden Migrationen. Zeitstempel muss echt größer sein als `20260903090000`.

STATUS M3 erledigt 2026-09-05: Migration `20260905090000_twitch_promo_pitch_log.sql` (additiv, ohne GRANT), `PitchLogEntry`-Struct, `record_pitch_log` und `load_recent_channel_messages` in promos.rs, DB-Test-DDL für `twitch_promo_pitch_log` und `twitch_chat_messages` in `apply_ddl`. Migration ist bereits im Testcontainer eingespielt (Port 33097). `cargo build -p tb-chat` (SQLX_OFFLINE=false gegen die migrierte Test-DB) EXIT=0, nur erwartete dead_code-Warnung für die noch ungenutzten Helfer (verschwindet in M4/M5). Der sqlx-Offline-Cache `.sqlx` folgt am Ende von M5, wenn alle `query!` stehen.

---

## M4 - Presets löschen, beide Timer-Pfade auf LLM-Text umstellen

Änderungen in `crates/tb-chat/src/promos.rs`:
- Löschen: `promo_messages_hype()` (:339), `all_promo_messages()` (:355), `activity_promo_messages()` (:367), die Kategorie-Helfer `promo_messages_*` (competitive/community/growth/coaching/partner, `promos.rs:308` ff, soweit nur von den gelöschten Listen genutzt), `PromoPreset` (:489), `global_presets()` (:497), `user_presets()` (:545), `PresetPicker`-Trait `pick_preset` (:465) und `RandomPresetPicker` (:592). Prüfen mit `graphify affected` bzw. `grep`, dass keine Fremdnutzung (z. B. Sonder-Event-Texte, INV) an den Kategorie-Helfern hängt; was ausschließlich Sonder-Events dient, bleibt (Nicht-Ziel: Sonder-Event-Texte).
- `build_promo_text` (:1240) umbauen: Reihenfolge bleibt (INV-03): erst `load_global_promo_message` (:1307), dann `load_streamer_promo_message` (:1322); wenn beide leer, statt Pool-Zufallswahl `promo_pitch::build_channel_promo_text(ctx, invite)` aufrufen. Kontext (`ChannelPromoContext`) aus Spiel/Titel (`twitch_live_state.last_game`/`last_title`) und den letzten N Kanalnachrichten aus `twitch_chat_messages.content` (neuer Loader `load_recent_channel_messages(login, N)` in promos.rs, `query!`, LIMIT ~8 ORDER BY ts DESC). Liefert der LLM-Pfad `None`, sendet `maybe_send_promo_with_stats` nichts (Rückgabe so führen, dass kein Announcement rausgeht und kein Cooldown verbrannt wird). Jeder Ausgang (gesendet/`None`) über `record_pitch_log` mit `pfad='periodic'`.
- `maybe_send_targeted_promo` (:1952) umbauen: den PresetPicker-Weg (`user_presets`/`global_presets` plus `preset_picker.pick_preset` mit 5s-Timeout, :1988/:2036) ersetzen durch `promo_pitch::build_targeted_pitch_text(ctx)`. User-Zweig: Kontext aus `load_user_context_snippets` (:2146) plus Spiel/Titel; Text NIE mit Link; Senden weiter über `guarded_api_for("promo", login).send_message` (:2006) plus `record_suppression_on_drop`; State-Mutation und `mark_promo_sent("targeted_promo")` nur bei erfolgreichem Send (unverändert). Global-Zweig analog ohne Zielperson, weiter `send_announcement("purple")`. Liefert der LLM-Pfad `None`, kein Pitch, kein Cooldown. Jeder Ausgang über `record_pitch_log` (`pfad='targeted_user'`/`'targeted_global'`). Stammgast-Ausnahme (`pick_user_target`, :2122) bleibt für Timer-Pitches (REQ-05).
- `MINIMAX_TIMEOUT_SEC` (:294) entfällt, wenn nirgends mehr genutzt.

Änderungen in `bin/tb-bot/src/chat_wiring.rs`:
- Löschen: `MINIMAX_PRESET_SYSTEM_PROMPT` (:79), `struct MinimaxPresetPicker` und `impl` (:1871 ff), `fn minimax_preset_user_prompt` (:1929), `fn match_preset_id` (:1951), der `.set_preset_picker(Arc::new(MinimaxPresetPicker::new(...)))`-Aufruf (:739). Import `PresetPicker, PromoPreset, RandomPresetPicker` aus `use tb_chat::promos::{...}` (:35) entfernen, soweit ungenutzt.
- `set_preset_picker` (`promos.rs:843`) und das zugehörige Feld/Default in `PromoEngine::new` (:762) entfernen, da der Trait weg ist.

Tests ersetzen (`promos.rs:2517` bis `:2571`): die reinen Preset-/Pool-Tests (`alle_sichtbaren_promo_texte`, `jeder_promo_text_traegt_einen_link_und_ist_einzigartig`, `promo_texte_haben_keine_gedankenstriche`, `partner_texte_liegen_im_chat_activity_pool`, `coaching_texte_liegen_im_chat_activity_pool`, `promo_pools_sind_nicht_leer`, `community_texte_liegen_im_chat_activity_pool`) löschen. Ersatz: neue Tests des LLM-Pfads (Filter-Abdeckung liegt schon in `promo_pitch.rs` aus M2; hier zusätzlich ein Test, dass `build_channel_promo_text` bei Judge-`None` kein Announcement erzeugt und dass der periodische Text den Invite am Ende trägt, mit gemocktem tb_llm-Pfad bzw. über den Judge-Seam). `hat_gedankenstrich` (:2508) als Helfer behalten.

Erwarteter Zwischenzustand: keine Preset-Listen mehr im Code; beide Timer-Pfade erzeugen freien Text über tb_llm oder senden nichts; chat_wiring ohne PresetPicker.

Validierung: `/home/nathanael/.cargo/bin/cargo build -p tb-chat -p tb-bot` und `/home/nathanael/.cargo/bin/cargo test -p tb-chat`.

Stop-Regel: `send_timeout_pitch` (:1210), Cooldown-Persistenz `twitch_promo_cooldowns` (:2427/:2379), Doppelsend-Lock (:965) und die Dashboard-Override-Loader (:1307/:1322) bleiben unverändert (INV-03/INV-04). Sonder-Event-Texte nicht anfassen.

STATUS M4 erledigt 2026-09-05: alle Preset-Listen (`promo_messages_*`, `all_promo_messages`, `activity_promo_messages`, `global_presets`, `user_presets`), `PromoPreset`/`PresetType`, der `PresetPicker`-Trait und `RandomPresetPicker` in promos.rs gelöscht; in chat_wiring.rs `MinimaxPresetPicker`, `MINIMAX_PRESET_SYSTEM_PROMPT`, `minimax_preset_user_prompt`, `match_preset_id`, der `set_preset_picker`-Aufruf und die zugehörigen Imports entfernt. `build_promo_text` gibt jetzt `Option<String>` und holt den Text bei fehlenden Overrides über den LLM-Pfad; `maybe_send_targeted_promo` erzeugt User- und Global-Text über den LLM-Pfad (User nie mit Link, `@login`-Anrede), sendet nichts bei `None`, protokolliert jeden Ausgang über `record_pitch_log` (`periodic`/`targeted_user`/`targeted_global`). Neuer `load_live_context`-Loader für Spiel/Titel. Technische Entscheidung (Files im Scope, kein Contract-Amendment nötig): die Timer-Textpfade laufen über einen injizierbaren `PitchTextGen`-Seam (Default `FireworksPitchTextGen`, Setter `set_pitch_text_gen`), analog zum Judge-Seam, damit die beiden Suppression-Guard-DB-Tests weiter mit gemocktem Text senden können. Preset-/Pool-Tests und der Anti-Repeat-Test gelöscht, Ersatz: `periodischer_promo_text_traegt_invite_am_ende_ohne_strich` und `periodischer_promo_text_leer_gibt_keinen_text` (nutzen `hat_gedankenstrich` weiter). `cargo build -p tb-chat -p tb-bot` (SQLX_OFFLINE=false gegen die Test-DB) EXIT=0, keine Warnungen.

---

## M5 - Anlass-Pitch-Pfad in promos.rs, Einhängung in pipeline.rs

Änderungen in `crates/tb-chat/src/promos.rs`:
- Neue Methode `pub async fn on_message_pitch(&self, event: &ChatMessageEvent)`, aufgebaut nach `LfgPitchResponder::maybe_respond`/`decide` (`lfg_pitch.rs:471`/:490). Ablauf mit Vorfilter VOR jedem LLM-Aufruf (spart Kosten):
  1. `event.text()` mindestens 25 Zeichen, kein `!`-Befehl, `event.chatter_user_id != event.broadcaster_user_id` (kein Broadcaster). Sonst still zurück (kein Log, da kein Pitch-Versuch), Kurznachrichten laufen nicht ins Modell.
  2. Bestehende Gates wie im Timer-Pfad (REQ-06): Partner-Check (`is_partner_channel_for_chat_tracking`), Allowlist (`promo_channel_allowed_db` :2214), Werbefrei/Plan (`promo_blocked_by_plan_or_flag` :2259), Outbound-Suppression (`suppression.is_muted`), Startverzögerung 10 min (`stream_start_delay_ok` :2184). Bei Block: `record_pitch_log` mit `reject_reason` und `sent_at=NULL`, dann zurück.
  3. Limits (REQ-05) per DB-Abfrage auf `twitch_promo_pitch_log`:
     - User-Limit: höchstens ein gesendeter Anlass-Pitch je `target_user_id` in 7 Tagen über alle Kanäle. `query!` `SELECT max(sent_at)` where `target_user_id=$1 AND pfad='anlass' AND sent_at IS NOT NULL`.
     - Kanal-Limit: höchstens 3 gesendete Anlass-Pitches pro laufendem Stream mit mindestens 10 min Abstand. Stream-Start aus `twitch_live_state` (Go-Live-Zeitpunkt) bzw. `twitch_stream_sessions`; ist keiner ermittelbar, Fenster auf die letzten 3 Stunden. `count(*)` und `max(sent_at)` where `channel_login=$1 AND pfad='anlass' AND sent_at >= <stream_start>`.
     - Bei Limit-Block: `record_pitch_log` mit `reject_reason='limit_user'`/`'limit_channel'`, zurück.
  4. Doppelsend-Lock `get_send_lock` (:965) halten (wie `on_message`).
  5. Judge fragen: `self.pitch_judge.decide(PitchJudgeInput{...})`. Kontext: `trigger_text=event.text()`, `game`/`title` aus `twitch_live_state`, `recent_chat` aus `load_recent_channel_messages`, `target_login=event.chatter_user_login`. Bei `None` oder `occasion=None`: `record_pitch_log` mit `reject_reason='kein_anlass'`, zurück.
  6. Harte Filter: `promo_pitch::pitch_filter_reject(&resp.reply)`. Bei Verstoß: `record_pitch_log` mit dem `PitchRejectReason`, KEIN Fallback-Text (REQ-03), zurück.
  7. Senden als `@login`-Antwort: Text `format!("@{} {}", event.chatter_user_login, resp.reply)`, über `guarded_api_for("promo", login).send_message(channel_id, &text)` plus `record_suppression_on_drop`. Nur bei `SendOutcome::Sent`:
     - `record_pitch_log` mit `sent_at=now`, `occasion`, `generated_text`.
     - Cooldown koppeln (REQ-05): `mark_promo_sent(login, now, "anlass_pitch", ts)` wie `send_timeout_pitch`, damit danach keine Timer-Promo obendrauf kommt.
     - Discord-Review-Karte auslösen (M6): `pitch_review_sink.send_card(...)` mit Auslöser-Zitat und Antwort.
  8. Der Anlass-Pitch läuft asynchron und blockiert die Pipeline nicht (siehe Einhängung).
- Neue Felder in `PromoEngine`: `pitch_judge: Arc<dyn PitchJudge>` (Default `FireworksPitchJudge`) und `pitch_review_sink: Option<Arc<dyn PitchReviewSink>>` (M6). Setter `set_pitch_judge` und `set_pitch_review_sink` nach dem Muster der bestehenden `set_*` (`promos.rs:789` ff). Default-Judge im Konstruktor `PromoEngine::new` (:762) setzen.
- Loader `load_recent_channel_messages(login, n)` (aus M4 wiederverwenden).

Änderungen in `crates/tb-chat/src/pipeline.rs`:
- Im `class.is_deadlock_live`-Block (Schritt 13/14, `pipeline.rs:1146`) direkt bei/nach `promos.on_message` einen zusätzlichen, asynchron gespawnten Aufruf `promos.on_message_pitch(&event)` einhängen. Umsetzung wie der LFG-Step (`pipeline.rs:1288`): `run_pipeline_step("promos.on_message_pitch", ...)` mit `tokio::spawn`, damit der 20-s-LLM-Aufruf die Chat-Pipeline nicht blockiert. Derselbe `Arc::clone(&p.promos)`, eigener `event.clone()`.

Erwarteter Zwischenzustand: eingehende Anlass-Nachrichten in Partnerkanälen führen zu einer `@login`-Antwort, alle Ausgänge stehen in `twitch_promo_pitch_log`.

Nach diesem Milestone: `cargo sqlx prepare` (M3) gegen eine DB mit `twitch_promo_pitch_log` laufen, alle neuen `query!` aufnehmen.

Validierung: `/home/nathanael/.cargo/bin/cargo build -p tb-chat -p tb-bot` und `/home/nathanael/.cargo/bin/cargo test -p tb-chat`.

Stop-Regel: kein zweiter OAuth-Weg, kein neues Secret (INV-07). Keine Änderung an `commands.rs` (Verbotene Änderungen); `!discord`/`!invite` bleiben unberührt (INV-05).

---

## M6 - Discord-Review-Karte für gesendete Anlass-Pitches

Änderungen:
- In `crates/tb-chat/src/promos.rs` (oder `promo_pitch.rs`) schmaler Trait `#[async_trait] pub trait PitchReviewSink: Send + Sync { async fn send_card(&self, channel_login: &str, target_login: &str, trigger: &str, reply: &str); }`. Bauform wie die injizierten Traits (`InviteReplyNotifier` `commands.rs:213`, `OutboundSuppressionCheck`). tb-chat kennt tb-transport-discord NICHT und bekommt keine neue Crate-Abhängigkeit.
- Implementierung in `bin/tb-bot/src/chat_wiring.rs` (oder `main.rs`): Struct, das einen `Arc<dyn DiscordBackend>` via `BrokerRelay::new(&settings.broker)` (Muster `main.rs:861`/`smalltalk_loop_wiring.rs:274`) hält und in `send_card` ein `SendRichMessage { channel_id: DEFAULT_REVIEW_CHANNEL_ID, ... }` (`smalltalk_loop_wiring.rs:37`, Payload `tb-transport-discord/src/backend.rs:7`, Methode `send_rich_message` `backend.rs:110`) mit Auslöser-Zitat und Antwort baut und über `discord.send_rich_message` schickt. Kartenaufbau angelehnt an `build_discord_card` (`smalltalk_loop_wiring.rs:584`), aber schlank (Kanal, Zielperson, Zitat, gesendeter Text).
- Verdrahtung: `.set_pitch_review_sink(Arc::new(...))` an der `PromoEngine`-Konstruktion in `chat_wiring.rs:729` ff anhängen. `DEFAULT_REVIEW_CHANNEL_ID` als Konstante in chat_wiring spiegeln oder aus smalltalk_loop_wiring exportieren.

Erwarteter Zwischenzustand: gesendete Anlass-Pitches erscheinen als Karte im bestehenden Smalltalk-Review-Kanal.

Validierung: `/home/nathanael/.cargo/bin/cargo build -p tb-bot`. Live-Nachweis in Deploy.

Stop-Regel: kein Discord-Post für verworfene Pitches (die stehen nur im Log). Kein neuer Discord-Kanal, kein zweiter Sendeweg.

---

## M7 - Regressionstests und Guard-Test

Änderungen (`crates/tb-chat/tests/` oder `#[cfg(test)] mod db_tests` in promos.rs):
- Guard-Test (analog `outreach_shadow_steht_in_der_nur_fireworks_liste` `outreach_shadow.rs:629`): in tb-chat prüfen, dass `tb_llm::FIREWORKS_ONLY_USE_CASES.contains(&promo_pitch::USE_CASE)`.
- Regressionstest Auslöser (Contract, Symphooniee): DB-Test mit `pool_in_schema` und einem gemockten `PitchJudge`, der für die Symphooniee-Nachricht ("...deadlock so unpopulär..." / "...zu tryharded...") eine gültige `PitchResponse` mit `occasion=Some(GameUnpopular)` und sauberem `reply` liefert. Erwartung: Nachricht passiert den 25-Zeichen-Vorfilter, alle Gates offen (Partner, Allowlist, Werbefrei aus), es wird über die MockApi eine Nachricht mit Prefix `@Symphooniee ` gesendet und eine Zeile mit `pfad='anlass'`, `sent_at IS NOT NULL` steht in `twitch_promo_pitch_log`. Dieser Test muss VOR der M5-Implementierung rot sein (Methode `on_message_pitch` existiert noch nicht bzw. sendet nichts); roten Lauf mit Testname und Fehlermeldung in `EVIDENCE.md` festhalten.
- Regressionstest Werbefrei: bei `streamer_plans.promo_disabled=1` sendet `on_message_pitch` für dieselbe Nachricht nichts (MockApi zählt 0 Sends), und die Log-Zeile trägt `reject_reason` (Plan-Block), `sent_at IS NULL`. Prüft INV-01.
- Weitere kleine Tests: Vorfilter blockt Kurznachricht (unter 25 Zeichen) ohne LLM-Aufruf; harter Filter verwirft eine Judge-Antwort mit Link/Strich und sendet nichts.

Erwarteter Zwischenzustand: neuer Pfad ist durch Tests abgedeckt, die alten Preset-Tests sind ersetzt (M4).

Validierung: `TB_TEST_DATABASE_URL=... /home/nathanael/.cargo/bin/cargo test -p tb-chat` (voll grün gegen die rote Baseline, keine neuen roten Tests außer dem bewusst zuerst roten Regressionstest, der nach M5 grün wird).

Stop-Regel: Bestehende Tests nicht abschwächen (INV-06). Der Implementierer ändert den Regressionstest nach dem Rot-Lauf nicht mehr.

---

## M8 - Streamer-FAQ aktualisieren

Änderungen:
- `rust/knowledge/bot/faq-werbung.md` (REQ-08): Verhalten wahrheitsgemäß beschreiben. Der Bot antwortet in Partnerkanälen auf die Situation eines Zuschauers (keine Freunde, Spiel unpopulär, zu tryhard, Solo-Queue-Frust, neu, sucht Hilfe) mit einer echten Antwort plus einem kurzen Community-Satz; ein Discord-Link kommt nur in der periodischen Einladung oder auf `!discord`/`!invite`, nie in der Anlass-Antwort. Fertige Preset-Sprüche gibt es nicht mehr. `last_updated` auf `2026-09-05` setzen. Vorhandene Gedankenstriche in angefassten Zeilen entfernen (Sätze umbauen), keine neuen setzen.

Erwarteter Zwischenzustand: FAQ deckt sich mit dem Code.

Validierung: Sichtprüfung der Datei; keine Gedankenstriche, echte Umlaute.

Stop-Regel: Preispläne und Werbefrei-Aussagen inhaltlich nicht ändern (INV-07), nur das Werbe-Verhalten neu beschreiben.

---

## Deploy

Reihenfolge nach dem Review-FREIGABE (Stufe 4 des Coding-Ablaufs, adversariales Zweitmodell gegen Diff plus Contract):

1. Merge nach main erst nach Freigabe, Release-Build in einem isolierten Worktree, nie im geteilten Checkout `_ttb-main-deploy` (Memory `twitch-release-deploy-weg`): parallele Checkouts verderben das Binary. Vor dem Merge `git rev-list --left-right --count origin/main...main` prüfen (Memory `live-checkout-push-erbt-fremde-commits`), alles außer `0 0` klären.
2. Migration als Rolle `postgres` in `twitch_analytics` anwenden (`TB_DB_MIGRATE=0`, der Bot migriert nicht selbst): `20260905090000_twitch_promo_pitch_log.sql` einspielen, danach `GRANT SELECT, INSERT, UPDATE, DELETE ON public.twitch_promo_pitch_log TO twitchbot, twitchdash;` und `GRANT USAGE, SELECT ON SEQUENCE public.twitch_promo_pitch_log_id_seq TO twitchbot, twitchdash;` setzen, kein CREATE auf `public` an die Dienst-Rollen. Anschließend die Version manuell in `_sqlx_migrations` eintragen (Memory `twitch-release-deploy-weg`).
3. `cargo sqlx prepare` muss vor dem Release-Build gelaufen sein (M5), sonst bricht der Offline-Build.
4. Release bauen und installieren nach dem etablierten Weg (eingefrorener Root-Clone unter `/opt/deadlock/twitch/builds/<sha>`, `install-twitch-release`), dann `sudo systemctl restart deadlock-twitch-bot-rust` (System-Unit, `User=twitchbot`; die `--user`-Dublette ist tot).
5. Live-Prüfung: mit einem Wegwerf-Twitch-Konto (nie ein echtes Streamer-Konto, Memory `live-beweis-nur-wegwerf-konten`) in einem Partnerkanal mit aktivem Deadlock-Stream eine Anlass-Nachricht (z. B. "sind gerade keine freunde da zum zocken, solo queue nervt") schreiben. Erwartung: `@konto`-Antwort im Chat innerhalb von 30 s, eine Karte im Smalltalk-Review-Kanal, eine Zeile mit `pfad='anlass'`, `sent_at IS NOT NULL` in `twitch_promo_pitch_log`. Zusätzlich einen Kanal mit `promo_disabled=1` gegenprüfen: keine Antwort, Log-Zeile mit `reject_reason`, `sent_at IS NULL`.
6. Branch und Worktree nach dem Merge wirklich löschen.

## Risiken

- Reply-Semantik fehlt im Port (`ChatApi::send_message` ohne `reply_parent`, `api.rs:22`). Gegenmaßnahme: Amendment umgesetzt, Anlass-Pitch als `@login`-Nachricht statt echtem Twitch-Reply. Kein Eingriff in tb-transport-twitch, kein Scope-Verstoß.
- Filter-Drift zwischen outreach und pitch (Filter privat in tb-engagement, INV-08 verbietet Änderung). Gegenmaßnahme: Filter in `promo_pitch.rs` als Kopie plus drei Zusatzregeln (harte Strich-Sperre, 400 Zeichen, "komm auf"/"join"/"tritt bei"), eigene Unit-Tests in M2, Strich-Regel deckungsgleich mit `hat_gedankenstrich`.
- sqlx-Offline bricht ohne `prepare`, GRANTs fehlen im Migrations-File. Gegenmaßnahme: `cargo sqlx prepare` als fester Schritt am Ende von M5, GRANTs als expliziter Deploy-Schritt 2 als `postgres`. Log-Insert-Fehler sind fail-open (`warn!`), blockieren nie den Sendepfad.
- LLM-Latenz blockiert die Chat-Pipeline. Gegenmaßnahme: Anlass-Pitch läuft asynchron per `tokio::spawn` (M5), Timeout 20 s im Judge, Vorfilter (25 Zeichen, kein Befehl) hält Kurznachrichten vom Modell fern.
- Kosten und Spam durch zu viele LLM-Aufrufe. Gegenmaßnahme: Vorfilter vor dem Modell, Kanal-Limit 3 pro Stream mit 10 min Abstand, User-Limit 7 Tage, Cooldown-Kopplung an den Promo-Cooldown; alle Limits über `twitch_promo_pitch_log` als einzige Quelle.
- Doppelte Werbung (Anlass-Pitch plus Timer-Promo kurz danach). Gegenmaßnahme: gesendeter Anlass-Pitch belegt den Promo-Cooldown wie `send_timeout_pitch` (`mark_promo_sent`), Doppelsend-Lock greift für beide Pfade.
- Löschen der Presets trifft Sonder-Event-Texte oder andere Nutzer der Kategorie-Helfer. Gegenmaßnahme: vor dem Löschen `graphify affected`/`grep` auf die Kategorie-Funktionen, nur ausschließlich von den fünf Listen genutzten Code entfernen, Sonder-Event-Pfad unberührt lassen (Nicht-Ziel).
