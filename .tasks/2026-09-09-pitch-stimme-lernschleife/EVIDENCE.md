# EVIDENCE - Pitch-Stimme und Lernschleife

Bestandsaufnahme im Deploy-Checkout `~/repos/_ttb-main-deploy` (Deadlock-Twitch-Bot, main). Reine Fundstellen, kein Code geaendert. Cross-Repo-Teile (dl-bot Discord-Gateway) aus `~/repos/Deadlock-Bots`.

## 1. Review-Karten nach Discord (Token, Kanal, Message-ID)

- rust/bin/tb-bot/src/chat_wiring.rs:1887 - `const PITCH_REVIEW_CHANNEL_ID: i64 = 1_374_364_800_817_303_632;` (Kanal-ID hartkodiert)
- rust/bin/tb-bot/src/chat_wiring.rs:1888 - `const PITCH_REVIEW_GOLD: i64 = 0x00C8_A86B;` (Akzentfarbe der Karte)
- rust/bin/tb-bot/src/chat_wiring.rs:1890 - `struct DiscordPitchReviewSink { discord: Arc<dyn DiscordBackend> }`
- rust/bin/tb-bot/src/chat_wiring.rs:1955 - `impl PitchReviewSink for DiscordPitchReviewSink`, Methode `send_card(...)`
- rust/bin/tb-bot/src/chat_wiring.rs:1972-1990 - Karte als Components-V2 (`type 17` Container, `type 10` Textzeilen): Titel/Kanal, `An <target>`, `> <trigger>`, `Antwort: <reply>`, optional `candidate_hint`; Feld `channel_id: PITCH_REVIEW_CHANNEL_ID`
- rust/bin/tb-bot/src/chat_wiring.rs:2003 - `self.discord.send_rich_message(payload).await` (Fehler wird nur `tracing::warn!`)
- rust/bin/tb-bot/src/chat_wiring.rs:727-730 - Verdrahtung: `DiscordPitchReviewSink { discord: Arc::new(relay) }`, `relay` ist `review_relay: Option<BrokerRelay>`
- rust/bin/tb-bot/src/chat_wiring.rs:629 - `pub review_relay: Option<BrokerRelay>`
- rust/bin/tb-bot/src/main.rs:1315 - `review_relay: BrokerRelay::new(&settings.broker).ok()`

Transportweg (kein direkter Discord-REST aus tb-bot, sondern ueber den Master-Broker im Discord-Bot):
- rust/crates/tb-transport-discord/src/relay.rs:34-38 - `struct BrokerRelay { client, base_url, token }`
- rust/crates/tb-transport-discord/src/relay.rs:207-214 - `BrokerRelay::new(config: &BrokerConfig)`, base_url+token aus Config
- rust/crates/tb-transport-discord/src/relay.rs:490-508 - `send_rich_message` POSTet an `SEND_PATH` mit Idempotency-Key (sha256), Retry, `EDIT_PATH`/`DELETE_PATH` existieren analog
- rust/crates/tb-transport-discord/src/relay.rs:220-226 - Idempotency-Key `<prefix>-<sha256hex[..48]>`
- rust/crates/tb-transport-discord/src/backend.rs:110-111 - `async fn send_rich_message(&self, payload) -> Result<SendResult, DiscordError>` (Rueckgabe traegt `message_id` in `SendResult`)
- rust/bin/tb-bot/src/streamer_link.rs:327 - `notify_embed(relay: &BrokerRelay, ...)` nutzt denselben Broker-Weg (Beleg fuer Broker im Discord-Bot)
- rust/crates/tb-dashboard-api/src/handlers/discord_link.rs:41 - `const BROKER_BASE_URL: &str = "http://127.0.0.1:8766";` (Master-Broker-Port, lebt im Discord-Bot dl-web/dl-bot)

Message-ID der Karte:
- rust/bin/tb-bot/src/chat_wiring.rs:1955 (Trait) und rust/crates/tb-chat/src/promos.rs:391-400 - `send_card(...)` gibt `()` zurueck, die `SendResult`/`message_id` aus `send_rich_message` wird VERWORFEN.
- rust/migrations/20260905090000_twitch_promo_pitch_log.sql - Tabelle hat KEINE Discord-Message-ID-Spalte.
- Befund: Die Discord-Message-ID der Karte wird nirgends gespeichert; es gibt aktuell keine Verknuepfung Karte -> `twitch_promo_pitch_log`-Zeile.

## 2. Discord-Gateway / Reaktions-Lesepfad

tb-bot selbst hat KEINEN Discord-Gateway und keinen Inbound-Reaktionspfad; es sendet nur ausgehend ueber `BrokerRelay` (Abschnitt 1). Vorhandene Wiederverwendungs-Kandidaten:

Polling-Lesepfad ueber Broker/REST (dl-bot MCP):
- ~/repos/Deadlock-Bots/rust/bin/dl-bot/src/mcp.rs:510-517 - `read_messages` liest pro Nachricht `reactions[]` mit `emoji.name` und `count`
- ~/repos/Deadlock-Bots/rust/bin/dl-bot/src/mcp.rs:546-547 - Reaktionen werden ins Ergebnis gepackt
- Befund: Ein Poll auf Nachrichten des Review-Kanals liefert Reaktionen (emoji+count) bereits als JSON; als Reaktions-Poll wiederverwendbar, wenn tb-bot die Karten-Message-ID kaeme (siehe Abschnitt 1, fehlt).

Live-Gateway-Reaktionen (dl-discord, im Discord-Bot):
- ~/repos/Deadlock-Bots/rust/crates/dl-discord/src/gateway.rs:189-200 - `trait ReactionRoleGatewayPort { reaction_add(...); reaction_remove(...) }`
- ~/repos/Deadlock-Bots/rust/crates/dl-discord/src/gateway.rs:204-212 - `struct ReactionRoleAddEvent { emoji, ... }`
- ~/repos/Deadlock-Bots/rust/crates/dl-discord/src/gateway.rs:315-334 - Handler `reaction_add(ctx, add: Reaction)` reicht Event an den Port
- ~/repos/Deadlock-Bots/rust/crates/dl-discord/src/gateway.rs:681 - Intent `GatewayIntents::GUILD_MESSAGE_REACTIONS` aktiv
- Befund: Der Discord-Bot hat einen kompletten Live-Reaktions-Gateway (fuer Reaction-Roles). Er koennte Reaktionen auf Pitch-Karten an einen anderen Dienst melden; aktuell existiert dafuer kein Port/keine interne API Richtung tb-bot.

## 3. Schema `twitch_promo_pitch_log`

- rust/migrations/20260905090000_twitch_promo_pitch_log.sql - `CREATE TABLE public.twitch_promo_pitch_log`: Spalten
  - `id BIGSERIAL PK`
  - `channel_login TEXT NOT NULL` (Kanal)
  - `target_user_id TEXT` (angesprochener Chatter)
  - `pfad TEXT NOT NULL` (Pfad: anlass / partner / zuschauer)
  - `occasion TEXT` (Judge-Anlass, z. B. no_mates/new_player)
  - `trigger_text TEXT` (Ausloesetext)
  - `generated_text TEXT` (die generierte Antwort)
  - `reject_reason TEXT`
  - `sent_at TIMESTAMPTZ` (NULL = nicht gesendet/pending/verworfen)
  - `created_at TIMESTAMPTZ`
- rust/migrations/20260906150000_twitch_promo_pitch_log_limits.sql - nur Indizes (target/pfad/sent, channel/pfad/sent, pfad/sent, reject-dedupe)
- Log-Helfer: rust/crates/tb-chat/src/promos.rs:2707 `record_pitch_log`, :2733 `insert_pitch_log_pending` (gibt id zurueck), :2764 `mark_pitch_log_sent`, :2777 `mark_pitch_log_dropped`
- Befund: Antworttext (`generated_text`), Ausloesetext (`trigger_text`), Kanal (`channel_login`), Pfad (`pfad`) und Judge-Anlass (`occasion`) liegen dort. NICHT dort: Judge-Confidence (nur `occasion`), Discord-Message-ID, Bewertung/Feedback.

## 4. Prompt-Bau, Budgets, denken_aus, Filter

Systemprompt-Konstanten (rust/crates/tb-chat/src/promo_pitch.rs):
- :11 `PITCH_SYSTEM_PROMPT` - Anlass-Judge; Rueckgabe JSON `{"occasion", "reply", "confidence"}`; sechs Anlaesse; verbietet Link/Druck/Superlative/Gedankenstriche/"komm auf|join|tritt bei"
- :35 `CHANNEL_PROMO_SYSTEM_PROMPT` - periodische Einladung, ein Satz, Link wird angehaengt
- :43 `TARGETED_PITCH_SYSTEM_PROMPT` - gezielter Zuschauer-Pitch (Code noch vorhanden, aus dem Live-Pfad entfernt)
- :51 `PARTNER_PITCH_SYSTEM_PROMPT` - Partner-Pitch an Deadlock-Streamer, "zwei Teile", dritte Person

User-Prompt = JSON der Kontext-Struct, keine Few-Shot-Beispiele:
- rust/crates/tb-chat/src/promo_pitch.rs:345-353 - `PitchJudgeInput { trigger_text, game, title, recent_chat: Vec<String>, target_login }`
- :366-367 - `serde_json::to_string(&input)` als User-Message, `Request::simple(PITCH_SYSTEM_PROMPT, user)`
- :396-408 - `ChannelPromoContext` / `PartnerPitchContext { target_login, target_messages, game, title, recent_chat }`
- Chatverlauf kommt aus rust/crates/tb-chat/src/promos.rs:874 `load_recent_channel_messages(&login, 8)` (letzte 8 Nachrichten)
- Befund: Keine Few-Shot-Beispiele, kein Beispielblock, keine Bot-Identitaets-Vorgabe, keine Spielwissen-Sperre im Prompt.

Budgets / denken_aus / Sampling:
- rust/crates/tb-chat/src/promo_pitch.rs:8 - `PITCH_TIMEOUT = 20s`, :9 `PITCH_MAX_CHARS = 400`
- :367-371 - Judge: `.temperature(0.0).json_object().denken_aus().timeout(PITCH_TIMEOUT)`
- :445-447 - `build_channel_promo_text`: `.temperature(0.7).timeout(...)` (KEIN denken_aus)
- :453-456 - `build_targeted_pitch_text`: `.temperature(0.7).denken_aus().timeout(...)`
- :467-470 - `build_partner_pitch_text`: `.temperature(0.7).timeout(...)` (KEIN denken_aus)
- Kein `max_tokens`/`.tokens(...)` in promo_pitch.rs oder promos.rs gesetzt (grep leer) - es gelten die tb_llm-Defaults.

Harte Filter (rust/crates/tb-chat/src/promo_pitch.rs):
- :165 `pitch_filter_reject` - prueft Link (:191 `contains_link`), Mitgliederzahl (:205), Superlativ (:242), harten Gedankenstrich (:259), Smiley (:267), Join-Phrase (:~305 `contains_join_phrase`: "komm auf"/"join"/"tritt bei")
- :314 `pitch_injection_reject(reply, target_login)` - Sperrwoerter (ignoriere/vergiss/system prompt/systemprompt/als ki/as an ai ...) und Fremd-@-Mentions != target
- :98 `parse_pitch_response`, :107 `extract_json_object`, :414 `clean_model_line`, :418 `finalize_channel_promo`, :432 `finalize_targeted_pitch` (rufen die Filter erneut)

## 5. Adressat der Nachricht (Reply-Parent, Mention, Broadcaster)

- rust/crates/tb-chat/src/types.rs:44-73 - `ChatMessageEvent` hat KEINE `reply_parent_*`-Felder. Vorhanden: `broadcaster_user_*`, `chatter_user_*`, `message_id`, `message`, `badges`, `color`, plus Shared-Chat-Felder `source_broadcaster_*`/`source_message_id`.
- rust/crates/tb-chat/src/types.rs:26-31 - `MessageFragment { fragment_type, text }` - nur Typ und Text; ein `mention`-Fragment traegt den @-Text, aber KEINE User-ID.
- Prompt-Eingabe: `target_login` = `chatter_user_login` (der Schreibende), rust/crates/tb-chat/src/promos.rs:737. Der Prompt weiss also, WER geschrieben hat, aber nicht, an wen sich die Nachricht per Reply/@-Mention richtet.
- Befund: Reply-Parent gibt es im Event-Typ nicht; Mentions nur als Fragment-Text ohne ID. Fuer "an wen ist die Nachricht gerichtet" muesste der Event-Typ erweitert werden (EventSub `channel.chat.message` liefert `reply.parent_*` und `mention`-Fragmente mit `user_id`, die hier nicht deserialisiert werden).

## 6. Bestehendes Muster "Beispiele aus DB in Prompt"

Direkt wiederverwendbar - `tb-engagement/src/style_examples.rs` (Few-Shot "show, don't tell"):
- rust/crates/tb-engagement/src/style_examples.rs:1-9 - Modul-Zweck: echte Channel-Zeilen als Stilvorlage, Stil vom Inhalt getrennt (Fakten nicht uebernehmen)
- :30-47 `SEED_EXAMPLES` (kalter Channel), :~50 `GOLD_EXAMPLES` (handverlesenes EarlySalty-Register)
- :22-24 `MIN_LEARNED_GOLD = 4` - ab 4 brauchbaren gelernten Zeilen ersetzen sie das feste Register
- :233-248 `load_learned_gold` - `SELECT my_message ...` (gelernte eigene Zeilen aus DB)
- :214-231 `load_user_turns` - `SELECT content FROM twitch_engagement_conversation ...`
- :146 `assemble_examples_with_gold`, :179 `build_fragment`, :264 `build_style_fragment` - baut den Beispielblock fuer den Prompt
- Befund: Genau das Muster fuer die Lernschleife (feste Startbeispiele, ab N gelernten Zeilen aus DB ziehen und in den Prompt setzen). Uebertragbar auf Pitch (gute Antworten mit Daumen-hoch als Beispiele).

Verwandt:
- rust/crates/tb-engagement/src/reaction_learning.rs:1-4 - baut Stilprofil aus echten eigenen Reaktionen; :91 `render_samples`, :121 `profile_user_prompt`
- rust/crates/tb-engagement/src/persona.rs, crew_review.rs, soul_store.rs - weitere DB-gestuetzte Prompt-Bausteine im selben Crate

## 7. Tests und Test-DB

Prompt-Filter (reine Unit-Tests, keine DB) - rust/crates/tb-chat/src/promo_pitch.rs (mod tests ab :512):
- :517-520 `parse_pitch_response` + `pitch_filter_reject`/`pitch_injection_reject`
- :758 `targeted_pitch_ohne_link_bleibt`, :764 `targeted_pitch_mit_link_faellt_weg`
- :770-790 mehrere `pitch_injection_reject`-Faelle

Pitch-Pfad (DB-Tests) - rust/crates/tb-chat/src/promos.rs:
- :3569 Kommentar "DB-Tests (gegen TB_TEST_DATABASE_URL)"
- :3701-3712 `RecordingReviewSink` (Mock, faengt Karten als Tupel), :5027 Assertion `cards[0].4 == PitchCardKind::Partner`
- :3797-3801 - Tests SKIPpen ohne `TB_TEST_DATABASE_URL`; `TB_TEST_REQUIRE_DB=1` erzwingt sie
- :3860-3900 - Test-DDL wird inline erzeugt (u. a. `twitch_streamer_identities`, `twitch_zuschauer_register`); die Pitch-Log-DDL wird ebenso im Test angelegt
- viele `#[tokio::test]` ab :3105 (on_message_pitch / Partner-Pitch / Limits)

Review-Karten-Test (tb-bot) - rust/bin/tb-bot/src/chat_wiring.rs:
- :3081-3082 `CapturingDiscordBackend` (Test-Stub fuer `send_rich_message`)
- :3143-3144 `pitch_review_karte_neutralisiert_mentions_in_allen_feldern`
- :3148-3157 `DiscordPitchReviewSink` mit `PitchCardKind::Anlass` im Test

Test-DB-Umgebung: `TB_TEST_DATABASE_URL` (laut Workspace-Faktenlage `postgres:///tb_bb_test?host=/var/run/postgresql`, `SQLX_OFFLINE=1`; tb-chat gehoert zu den "uebrigen Crates", nicht zum Docker-Container von tb-db/tb-raid).

## Offene Fragen

- `SendResult`-Struktur (Feldname der Message-ID, weitere Felder) nicht im Detail gelesen; nur belegt, dass `send_rich_message` `SendResult` liefert und `send_card` es verwirft.
- Konkreter Broker-Pfad `SEND_PATH`/`EDIT_PATH`/`DELETE_PATH` (String-Werte) und das genaue REST-Endpoint-Mapping im Discord-Bot (dl-web/dl-bot Broker) nicht ausgelesen; Port 8766 und "Broker lebt im Discord-Bot" sind belegt.
- Ob der Broker (Port 8766) einen Endpoint zum Abfragen von Reaktionen einer bekannten Message-ID anbietet, ist nicht geprueft (nur der dl-bot-MCP-`read_messages`-Pfad liefert Reaktionen).
- Wie `settings.broker` (base_url/token) konkret aus Config/Infisical befuellt wird, nicht verfolgt (nur `BrokerRelay::new(&settings.broker)` in main.rs:1315).
- Ob EventSub die Felder `reply.parent_*` und `mention.user_id` tatsaechlich sendet, ist Twitch-seitig anzunehmen, aber im Repo nicht deserialisiert und daher nicht am Code belegbar.
- `denken_aus`-Verhalten: dass Judge/Targeted es setzen, Channel-Promo und Partner-Pitch nicht, ist belegt; ob das gewollt ist (Budget vs. Qualitaet), nicht aus dem Code ableitbar.
