status: aktiv
datum: 2026-09-04

# Research: Discord-Pitches mit Qualität statt Preset-Sprüchen

Alle Zeilenangaben gegen den Stand des Checkouts /home/nathanael/Documents/Deadlock-Twitch-Bot (Deadlock-Twitch-Bot-Repo, HEAD detached auf 4ce0f62d). Beobachtung ist mit Fundstelle belegt, Vermutung ist als solche markiert.

## 1. Promo-Engine (rust/crates/tb-chat/src/promos.rs)

### Trigger-Pfade

Beobachtet:
- Es gibt drei Eintrittspunkte plus den Targeted-Pfad, alle in `PromoEngine` (Definition `promos.rs:720`, Konstruktor `promos.rs:762`).
- On-Message-Pfad: `on_message(&self, event: &ChatMessageEvent)` (`promos.rs:860`). Gates in Reihenfolge: `!`-Prefix-Guard (`promos.rs:865`), Partner-Check (`promos.rs:870`), Kanal-Allowlist `promo_channel_allowed_db` (`promos.rs:880`), Aktivität aufzeichnen (`promos.rs:897`), Doppelsend-Lock `get_send_lock` (`promos.rs:909`), dann `maybe_send_promo_with_stats` (`promos.rs:906`).
- 60s-Loop-Pfad: `spawn_periodic_loop` (`promos.rs:934`) ruft `restore_promo_cooldowns` (`promos.rs:939`) und tickt `send_promo_if_due` (`promos.rs:1001`). Der Loop-Slot entscheidet je Kanal: Lurker-Tax (`promos.rs:1020`), Allowlist (`promos.rs:1053`), bei `overall_ready && activity_ready && stream_start_delay_ok` (`promos.rs:1076`) entweder `maybe_send_targeted_promo` (`promos.rs:1082`) oder `maybe_send_promo_with_stats` (`promos.rs:1090`), sonst `maybe_send_viewer_spike_promo` (`promos.rs:1095`).
- Viewer-Spike-Pfad: `maybe_send_viewer_spike_promo` (`promos.rs:1530`) mit `promo_attempt_allowed_inner` (`promos.rs:1549`), Allowlist (`promos.rs:1559`), Start-Delay (`promos.rs:1564`), Kontext `get_viewer_spike_context` (`promos.rs:1591`).
- Targeted-Pfad: `maybe_send_targeted_promo` (`promos.rs:1952`). Kanal-Cooldown `CHANNEL_TARGETED_COOLDOWN_SEC` (`promos.rs:1962`), Alternierung global/user über `TargetedState.channel_last_type` (`promos.rs:1971`). User-Zweig wählt `pick_user_target` (`promos.rs:1982`), lädt `load_user_context_snippets` (`promos.rs:1985`) und pickt `user_presets()` über den PresetPicker mit 5s-Timeout (`promos.rs:1988`). Global-Zweig nutzt `global_presets()` mit PresetPicker (`promos.rs:2036`).

### Textaufbau, Senden, Presets

Beobachtet:
- Text wird für periodische Promo in `build_promo_text(login, invite, reason)` gebaut (`promos.rs:1240`): erst globaler Override (`promos.rs:1242`), dann Streamer-Override (`promos.rs:1247`), dann Kategorie-Pool je reason: `viewer_spike` gibt `promo_messages_hype()`, `chat_activity` gibt `activity_promo_messages()`, sonst `all_promo_messages()` (`promos.rs:1252`), Anti-Repeat gegen `last_promo_text` (`promos.rs:1259`), Zufallswahl (`promos.rs:1285`).
- Presets: `PromoPreset` (`promos.rs:489`), `global_presets()` (`promos.rs:497`), `user_presets()` (`promos.rs:545`). Text-Pools: `promo_messages_hype()` (`promos.rs:339`), `all_promo_messages()` (`promos.rs:355`), `activity_promo_messages()` (`promos.rs:367`), plus Kategorien competitive/community/growth/coaching/partner (`promos.rs:308` ff). Das sind exakt die fünf Listen aus REQ-04, die gelöscht werden.
- PresetPicker-Trait `pick_preset` (`promos.rs:465`), Random-Fallback `RandomPresetPicker` (`promos.rs:592`).
- Senden: periodische Kanal-Promo über `send_promo_message` ruft `api.send_announcement(channel_id, text, "purple")` (`promos.rs:1182`). Targeted-User über `guarded_api_for("promo",...).send_message` (`promos.rs:2006`, echtes Reply gibt es nicht, siehe unten), Targeted-Global über `send_announcement("purple")` (`promos.rs:2052`). Werbefrei-Pitch `send_timeout_pitch` ruft `send_announcement("blue")` (`promos.rs:1221`). Ein echtes `reply` mit `@login`-Prefix gibt es nur im CommandEngine (`commands.rs:647`, nur `@login`-Prefix plus `send_message`, kein Twitch-Reply-Parent).

### Gates und Reihenfolge

Beobachtet:
- Allowlist und Partner-State: `promo_channel_allowed_db` (`promos.rs:2214`).
- Werbefrei/Plan: `promo_blocked_by_plan_or_flag` (`promos.rs:2259`), harte Spalte `streamer_plans.promo_disabled` (`promos.rs:2266`, `promos.rs:2286`).
- Outbound-Suppression: Trait `OutboundSuppressionCheck::is_muted` (`promos.rs:386`), geprüft in `send_promo_message` (`promos.rs:1166`) und `send_timeout_pitch` (`promos.rs:1212`).
- Startverzögerung: `stream_start_delay_ok` mit `PROMO_STREAM_START_DELAY_MIN = 10` (`promos.rs:2184`, Konstante `promos.rs:296`).
- Doppelsend-Lock: `get_send_lock` (`promos.rs:965`).
- Cooldown-Bereitschaft: `overall_promo_ready_inner` (`promos.rs:1400`), `promo_activity_ready_inner` (`promos.rs:1408`), `promo_attempt_allowed_inner` (`promos.rs:1522`); Persistenz `save_promo_cooldown`/`restore_promo_cooldowns` in `twitch_promo_cooldowns` (`promos.rs:2427`, `promos.rs:2379`).
- Stammgast-Ausnahme im Targeted-Pfad: `pick_user_target` überspringt Stammgäste (`promos.rs:2122` ff, `STAMMGAST_MIN_MESSAGES=10`/`STAMMGAST_DAYS=30`, `promos.rs:276`).

### Konstanten Cooldowns

Beobachtet (`promos.rs:56` bis `promos.rs:296`): `PROMO_INTERVAL_MIN=30`, `PROMO_LOOP_INTERVAL_SEC=60`, `PROMO_COOLDOWN_MIN_MIN=45`, `PROMO_COOLDOWN_MAX_MIN=180`, `PROMO_OVERALL_COOLDOWN_MIN=90`, `PROMO_ATTEMPT_COOLDOWN_MIN=10`, `PROMO_VIEWER_SPIKE_COOLDOWN_MIN=60`, `PROMO_NEW_CHATTERS_MIN=2`, `PROMO_STREAM_START_DELAY_MIN=10`. Targeted: `CHANNEL_TARGETED_COOLDOWN_SEC=900` (`promos.rs:279`), `USER_PITCH_COOLDOWN_SEC=86400` (`promos.rs:281`), `MINIMAX_TIMEOUT_SEC=5` (`promos.rs:294`).

### Dashboard-Override-Texte

Beobachtet:
- `load_global_promo_message(invite)` (`promos.rs:1307`) lädt über `tb_analytics::promo_mode::load_global_promo_mode(&pool)`, wertet mit `evaluate_global_promo_mode` aus (Singleton-Config, Tabelle laut Doc-Kommentar `twitch_global_promo_modes`, `promos.rs:1303`).
- `load_streamer_promo_message(login, invite)` (`promos.rs:1322`) liest `streamer_plans.promo_message` per SQL (`promos.rs:1324`), validiert mit `tb_analytics::promo_mode::validate_streamer_promo_message` (`promos.rs:1338`).
- Beide werden in `build_promo_text` VOR dem Pool geprüft (`promos.rs:1242`, `promos.rs:1247`), das ist der Vorrang aus INV-03.

## 2. Anbindung in der Chat-Pipeline (rust/crates/tb-chat/src/pipeline.rs)

Beobachtet:
- `PromoEngine` hängt als `p.promos: Arc<PromoEngine>` in den Pipeline-Parts (`pipeline.rs:698`, Import `pipeline.rs:65`).
- `record_raw_message` wird immer aufgerufen (Schritt 11, `pipeline.rs:1127` bis `pipeline.rs:1138`).
- `on_message` wird nur aufgerufen, wenn `class.is_deadlock_live` (Schritt 13/14, `pipeline.rs:1146` bis `pipeline.rs:1158`). Das gilt auch für den neuen Anlass-Pitch als natürlicher Aufhängepunkt.
- Doppelsend-Dedup über `message_id` (`pipeline.rs:813`, `mark_message_seen` `pipeline.rs:788`).

Beobachtet zu `ChatMessageEvent` (rust/crates/tb-chat/src/types.rs):
- Felder: `broadcaster_user_id`, `broadcaster_user_login`, `chatter_user_id`, `chatter_user_login`, `message_id`, `message`, `badges`, plus Shared-Chat-Felder (`types.rs:44` ff). `text()` liefert getrimmten Text (`types.rs:90`). Rollen: `is_mod_or_broadcaster` (`types.rs:78`), `is_broadcaster` (`types.rs:84`).
- Kein First-Time-Flag direkt im Event. First-Time wird an anderer Stelle über `twitch_session_chatters.is_first_time_streamer` gehalten (Schema-Beleg `pipeline.rs:2582`).
- Reply-Fähigkeit: `ChatApi::send_message` hat KEINEN reply_parent-Parameter (`api.rs:22`); es gibt keine native Reply-Methode. `reply()` in commands.rs setzt nur `@login`-Prefix (`commands.rs:647`).

Beobachtet zu Kontext-Quellen:
- Letzte Nachrichten einer Zielperson: `load_user_context_snippets` liest `twitch_engagement_conversation` (role='user', ORDER BY ts DESC LIMIT 5, `promos.rs:2146`). Aktive Chatter aus dem In-Memory-Bucket `get_active_chatters` (`promos.rs:2163`).
- Allgemeiner Chatverlauf liegt in `twitch_chat_messages.content` (Schema `pipeline.rs:2583`).
- Spiel/Titel: `twitch_live_state` trägt `last_game` und `last_title` (Baseline `migrations/20260601000000_baseline_schema.sql:1006`, `last_title` auf Zeile 1011). Session-Spiel in `twitch_stream_sessions.game_name` (`pipeline.rs:2580`).

Vermutung: Der Anlass-Pitch braucht Zielperson (chatter_user_id), Auslöser-Text (event.text()), Kanal-Spiel/Titel aus `twitch_live_state` und optional die letzten Chatzeilen aus `twitch_chat_messages`. Alle Bausteine existieren; ein neuer Loader für die letzten N Kanalnachrichten aus `twitch_chat_messages` fehlt noch und ist neu zu schreiben.

## 3. tb-llm: neuen Use-Case anlegen

Beobachtet:
- Modellauswahl ist fest: `endpoint_for(use_case)` gibt immer den Fireworks/Deepseek-Endpunkt (`selection.rs:26`, Modell `FIREWORKS_DEFAULT_MODEL = accounts/fireworks/models/deepseek-v4-flash-0731`, `selection.rs:10`). `use_case` dient nur Ledger und Warnungen (`selection.rs:24`).
- Nur-Fireworks-Liste: `FIREWORKS_ONLY_USE_CASES = ["ricky_crew_review", "outreach_shadow"]` (`selection.rs:14`). Der neue Use-Case gehört per INV-02 hier hinein.
- Test der Liste: `outreach_shadow_steht_in_der_nur_fireworks_liste` (`outreach_shadow.rs:629`) und `crew_review_steht_in_der_nur_fireworks_liste` (`crew_review.rs:583`) prüfen `FIREWORKS_ONLY_USE_CASES.contains(&USE_CASE)`. Für den neuen Use-Case ist ein analoger Test in tb-chat nötig.
- Aufruf: `tb_llm::complete(use_case, request)` (`hub.rs:279`). `Request` hat `system`, `messages`, `max_tokens`, `temperature`, `json_object` (setzt response_format json_object, `hub.rs:78`), `timeout` (`hub.rs:70` ff). Builder: `Request::simple(system, user)` (`hub.rs:107`), `.temperature(0.0)`, `.json_object()`, `.timeout(...)`, `.no_ledger()`, `.endpoint(...)`.
- Bestehendes JSON-Beispiel: `OutreachReviewClient::decide` (`outreach_shadow.rs:225`) baut `Request::simple(OUTREACH_SYSTEM_PROMPT, user_json).temperature(0.0).json_object().timeout(FIREWORKS_TIMEOUT).no_ledger().endpoint(...)` und parst die Antwort (`outreach_shadow.rs:231` bis `outreach_shadow.rs:248`). Timeout-Konvention dort: `FIREWORKS_TIMEOUT = Duration::from_secs(20)` (`outreach_shadow.rs:12`). Der Preset-Picker nutzt 5s (`promos.rs:294`).

## 4. Stilvertrag und Filter (rust/crates/tb-engagement/src/outreach_shadow.rs)

Beobachtet:
- `OUTREACH_SYSTEM_PROMPT` ist `pub const` (`outreach_shadow.rs:23`), enthält den vollständigen Stilvertrag (Deutsch, kurz, Kleinschreibung erlaubt, kein Emoji außer Smiley, keine Superlative, keine Mitgliederzahlen, keine langen Striche, kein Link, dritte Person, Anlass-Bindung). `USE_CASE = "outreach_shadow"` ist `pub const` (`outreach_shadow.rs:194`).
- Die Filter sind PRIVAT (`fn`, nicht `pub fn`): `forbidden_opener` (`outreach_shadow.rs:402`), `contains_link` (`outreach_shadow.rs:410`), `contains_member_count` (`outreach_shadow.rs:424`), `contains_superlative` (`outreach_shadow.rs:461`), `contains_forbidden_emoji` (`outreach_shadow.rs:478`). Sie sind von außen nicht importierbar.
- `contains_forbidden_emoji` behandelt den langen und den kurzen Gedankenstrich ausdrücklich als ERLAUBT (Whitelist am Funktionsende, `outreach_shadow.rs:496`). Ein eigener Strich-Filter ist also nötig; die Testhilfe `hat_gedankenstrich` in promos.rs prüft genau das schon (`promos.rs:2508` ff).
- Abhängigkeitsgraph: tb-chat hängt bereits an tb-engagement UND tb-llm (`crates/tb-chat/Cargo.toml:22`, `:26`). tb-engagement hängt NICHT an tb-chat (nur tb-llm, `crates/tb-engagement/Cargo.toml:20`). Es gäbe also keinen Zyklus, aber die Filter sind privat, und der Contract verbietet Änderungen an tb-engagement (INV-08, Verbotene Änderungen).

Empfehlung: Die Filter (contains_link, contains_member_count, contains_superlative, contains_forbidden_emoji, forbidden_opener) und der Systemprompt-Kern werden in tb-chat neu (kopiert) angelegt, in der neuen `promo_pitch.rs`. Grund: Die Funktionen sind privat, ein `pub`-Machen wäre eine verbotene Änderung an tb-engagement; ein Import scheidet damit aus, obwohl kein Zyklus bestünde. Zusätzlich braucht der Pitch-Filter drei Regeln, die outreach nicht hat: harte Strich-Sperre (outreach whitelistet Striche), 400-Zeichen-Grenze, und die Wendungen "komm auf"/"join"/"tritt bei" (REQ-03). Der Systemprompt sollte den outreach-Stilvertrag übernehmen, aber auf den Anlass-Pitch im eigenen bzw. Partnerkanal zuschneiden (zwei Teile, keine Qualify-Stufe).

## 5. Discord-Review-Kanal (rust/bin/tb-bot/src/smalltalk_loop_wiring.rs)

Beobachtet:
- `DEFAULT_REVIEW_CHANNEL_ID = 1_374_364_800_817_303_632`, `DEFAULT_REVIEW_GUILD_ID = 1_289_721_245_281_292_288` (`smalltalk_loop_wiring.rs:37`, `:36`).
- Der Discord-Zugang kommt über `BrokerRelay::new(broker)` als `Arc<dyn DiscordBackend>` (`smalltalk_loop_wiring.rs:274`), Karten baut `build_discord_card` (`smalltalk_loop_wiring.rs:584`), gesendet über `SendRichMessage` (Payload `tb-transport-discord/src/backend.rs:7`, Trait-Methode `DiscordBackend::send_rich_message` `backend.rs:110`).
- Verdrahtung in main.rs: `smalltalk_loop_wiring::start(&supervisor, pool.clone(), &settings.broker)` (`main.rs:583`), Broker-Config aus `settings.broker`, Relay via `BrokerRelay::new(&settings.broker)` (Muster `main.rs:861`, `main.rs:1891`). Import `use tb_transport_discord::BrokerRelay` (`main.rs:329`).
- tb-chat hängt NICHT an tb-transport-discord (kein Eintrag in `crates/tb-chat/Cargo.toml`).

Empfehlung: Der neue Pitch-Pfad in tb-chat darf tb-transport-discord nicht selbst kennen (das wäre eine neue Crate-Abhängigkeit gegen den schlanken Zuschnitt). Stattdessen einen schmalen Trait (z. B. `PitchReviewSink` mit einer `send_card`-Methode) in tb-chat definieren und in chat_wiring.rs bzw. main.rs mit einer Implementierung hinterlegen, die einen `Arc<dyn DiscordBackend>` (BrokerRelay auf `DEFAULT_REVIEW_CHANNEL_ID`) umwickelt. Das folgt dem bestehenden Muster (ChatApi, PresetPicker, OutboundSuppressionCheck sind alle so injizierte Traits).

## 6. Migrationen

Beobachtet:
- Verzeichnis `rust/migrations/`, Konvention `YYYYMMDDHHMMSS_slug.sql` (Liste bis `20260903090000_twitch_moderation_settings.sql`).
- Additives Beispiel: `20260903090000_twitch_moderation_settings.sql` ist ein reines `CREATE TABLE IF NOT EXISTS public.twitch_moderation_settings (...)`, ohne GRANT im File. Größeres Beispiel `20260901100000_twitch_ad_manager.sql` (mehrere additive Tabellen, IF NOT EXISTS).
- GRANT steht NICHT im Migrations-File. Anwendung läuft über `tb_db::run_migrations` beim Start, gated durch `TB_DB_MIGRATE` (`main.rs:568`); in Prod ist der Bot laut Memory `TB_DB_MIGRATE=0` und Grants werden manuell als `postgres` gesetzt (INV-09: Anwendung als postgres, Grants an twitchbot und twitchdash).
- sqlx-Offline: `rust/.sqlx/` mit 867 query-*.json-Dateien. Neue `sqlx::query!`/`query_scalar!`-Aufrufe brauchen einen `cargo sqlx prepare`-Lauf gegen eine DB mit der neuen Tabelle, sonst bricht der Offline-Build.

Vermutung: Neue Datei etwa `rust/migrations/20260904<hh>0000_twitch_promo_pitch_log.sql` mit `CREATE TABLE IF NOT EXISTS public.twitch_promo_pitch_log (...)` gemäß REQ-07 (Kanal, Twitch-User-ID nullbar, Pfad, Anlass, Auslöser-Text, erzeugter Text, Verwerfungsgrund, gesendet-Zeitpunkt). Grants manuell nachziehen.

## 7. Tests in promos.rs

Beobachtet:
- Preset-/Pool-Tests, die auf die zu löschenden Funktionen zeigen und ERSETZT werden müssen: `alle_sichtbaren_promo_texte` (Helfer, `promos.rs:2517`), `jeder_promo_text_traegt_einen_link_und_ist_einzigartig` (`promos.rs:2527`), `promo_texte_haben_keine_gedankenstriche` (`promos.rs:2538`), `partner_texte_liegen_im_chat_activity_pool` (`promos.rs:2545`), `coaching_texte_liegen_im_chat_activity_pool` (`promos.rs:2553`), `promo_pools_sind_nicht_leer` (`promos.rs:2561`), `community_texte_liegen_im_chat_activity_pool` (`promos.rs:2571`). Diese hängen an `all_promo_messages`, `activity_promo_messages`, `promo_messages_hype`, `global_presets`, `user_presets`.
- Unit-Tests ohne DB nutzen `make_engine_no_db()` (Definition `promos.rs:2979`, `connect_lazy` auf nonexistent-DSN, `MockApi` `promos.rs:2669`).
- DB-Tests in `mod db_tests` (`promos.rs:3233`) nutzen KEIN `sqlx::test`, sondern `pool_or_skip!` mit `TB_TEST_DATABASE_URL` (`promos.rs:3247`), `pool_in_schema` legt ein eigenes Schema an (`promos.rs:3260`), DDL in `apply_ddl` (`promos.rs:3288`), Engine-Helfer `make_engine(pool)` (`promos.rs:3409`). Für die neue Tabelle muss das DDL in `apply_ddl` erweitert werden.
- Es gibt einen `hat_gedankenstrich`-Test-Helfer (`promos.rs:2508`), der das Strich-Verbot bereits kodiert und für den neuen Filter-Test wiederverwendbar ist.

## 8. chat_wiring.rs: MinimaxPresetPicker und EngagementMinimaxClient

Beobachtet:
- `MinimaxPresetPicker` (`chat_wiring.rs:1871`) hält einen `EngagementMinimaxClient` (`chat_wiring.rs:1872`), gesetzt in `set_preset_picker(...)` mit `EngagementMinimaxClient::new(None, None, None, None)` (`chat_wiring.rs:739`).
- `EngagementMinimaxClient::new` baut den Endpunkt über `tb_llm::endpoint_for(USE_CASE)` (`minimax_chat.rs:729`); mit allen None bleibt es beim gemeinsamen Fireworks/Deepseek-Endpunkt. Der Name "Minimax" ist Alt-Name; der Client läuft heute über tb_llm, NICHT über einen toten oder direkten MiniMax-Client (`minimax_chat.rs:706` bis `minimax_chat.rs:745`, `call` über `tb_llm::Request` `minimax_chat.rs:926`).
- `pick_preset` fällt bei `presets.len() <= 1 || snippets.is_empty()` sofort auf `RandomPresetPicker` (`chat_wiring.rs:1889`), sonst fragt es Deepseek nach EINER Preset-ID mit 5s-Timeout und fällt bei Fehler/Timeout wieder auf Random (`chat_wiring.rs:1895` bis `chat_wiring.rs:1925`).

Live-Betrieb beim Targeted-Pitch: Der Picker wählt nur eine ID aus der festen Liste (global_presets/user_presets), der Text bleibt ein Preset. Ohne Fireworks-Key oder bei Timeout ist es Random-Fallback. Es wird also KEIN freier Text erzeugt; genau das soll REQ-04 ersetzen.

Vermutung: Nach der Umstellung wird `MinimaxPresetPicker`/`EngagementMinimaxClient` für den Promo-Pfad überflüssig; die Preset-Auswahl entfällt komplett zugunsten eines direkten `tb_llm::complete`-Aufrufs. Der EngagementMinimaxClient wird an anderen Stellen (Scam/LFG-Judges, `chat_wiring.rs:707`, `:837`) weiter genutzt und bleibt dort.

## 9. Lurker-Pitch-Task (.tasks/2026-08-30-lurker-pitch/)

Beobachtet:
- Artefakte vorhanden (CONTRACT.md, EVIDENCE.md, PLAN.md, RESEARCH.md), Status "aktiv".
- NICHT gemergt als eigenes Feature: kein `lurker_pitch`/`twitch_lurker_pitch_log`/`maybe_send_lurker_pitch`/`build_lurker_pitch` im Code (grep leer), keine Migration mit lurker_pitch. Der LFG-Pitch-Teil (Feature 1) IST gemergt (`git log` zeigt 885324a9 "merge: LFG-Mitspieler-Pitch Feature 1", Datei `lfg_pitch.rs` existiert). Der Lurker-Tax-basierte Pitch (Feature 2 des Tasks) fehlt im Code.
- Der Task fasst laut EVIDENCE.md dieselben Stellen an, die dieser Task berührt: `maybe_send_lurker_tax_reminder` (`promos.rs:1679`), `get_lurker_tax_candidates` (`promos.rs:1852`), `build_lurker_tax_text` (`promos.rs:1937`).

Vermutung: Da der Lurker-Pitch nie gemergt wurde, gibt es keinen Merge-Konflikt-Zwang, aber die Contracts überlappen konzeptionell (beide wollen Preset-Sprüche durch gezielte Ansprache ersetzen). Der neue Task soll den Lurker-Tax-Pfad (`maybe_send_lurker_tax_reminder`, `build_lurker_tax_text`) NICHT umbauen (Nicht-Ziel im aktuellen Contract) und diese Funktionen unangetastet lassen.

## 10. Build-Toolchain und Test-Baseline

Beobachtet:
- `/home/nathanael/.cargo/bin/cargo` ist 1.97.1, `/usr/bin/cargo` ist 1.75.0. Toolchain `1.97.1-x86_64-unknown-linux-gnu` und `stable` sind installiert (`~/.rustup/toolchains`). `rust/rust-toolchain.toml` = channel "stable".
- `cargo test -p tb-chat` wurde bewusst NICHT gelaufen (Laufzeitrisiko und geteilte Ressourcen). Bekannte rote Baseline laut Memory (tb-bot-Build-Toolchain): vorbestehende rote Tests im Repo, außerdem brauchen die DB-Tests `TB_TEST_DATABASE_URL`, sonst SKIP. Vor dem Bauen ist die rote Baseline mit `/home/nathanael/.cargo/bin/cargo test -p tb-chat` einmal zu messen und festzuhalten.

## Zusammenfassung: die drei wichtigsten Risiken

1. Reply-Semantik fehlt im Port. `ChatApi::send_message` hat keinen reply_parent-Parameter (`api.rs:22`); "als Reply auf diese Nachricht" (REQ-01) ist mit dem heutigen Port nur als `@login`-Prefix machbar, nicht als echter Twitch-Reply-Thread. Entweder Port um `reply_parent_message_id` erweitern (berührt tb-transport-twitch, nicht im Contract-Scope) oder REQ-01 als `@login`-Reply auslegen. Das ist vor dem Plan zu entscheiden.
2. Filter-Duplizierung statt Wiederverwendung. Die Stil-Filter liegen privat in tb-engagement (INV-08/verboten), müssen also in tb-chat neu entstehen, plus drei zusätzliche Regeln (harte Strich-Sperre, 400-Zeichen, "komm auf"/"join"/"tritt bei"). Risiko: Drift zwischen outreach- und pitch-Filter; die Kopie braucht eigene Tests (der `hat_gedankenstrich`-Helfer `promos.rs:2508` ist wiederverwendbar).
3. sqlx-Offline und Grants. Neue `query!`-Aufrufe gegen `twitch_promo_pitch_log` brechen den Offline-Build ohne `cargo sqlx prepare` gegen eine DB mit der Tabelle; die Migration trägt keine GRANTs, die müssen manuell als postgres an twitchbot/twitchdash gesetzt werden (INV-09), sonst schreibt der Bot in Prod nicht ins Log. Beides ist leicht zu übersehen und erst live sichtbar.
