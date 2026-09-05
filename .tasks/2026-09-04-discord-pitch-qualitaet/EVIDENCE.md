status: erledigt
datum: 2026-09-04

# Evidence: Discord-Pitch-Qualität

Jede Zeile eine selbst nachgelesene Fundstelle. Pfade relativ zu rust/ im Repo Deadlock-Twitch-Bot, sofern nicht anders genannt.

crates/tb-chat/src/promos.rs:720  Struct PromoEngine, Träger aller Promo-Pfade
crates/tb-chat/src/promos.rs:860  on_message, On-Message-Trigger mit Prefix/Partner/Allowlist-Gates
crates/tb-chat/src/promos.rs:934  spawn_periodic_loop, 60s-Loop-Trigger
crates/tb-chat/src/promos.rs:1001  send_promo_if_due, Slot-Verteilung Lurker/Targeted/Promo/Spike
crates/tb-chat/src/promos.rs:1076  Fälliger Slot: overall_ready und activity_ready und stream_start_delay_ok
crates/tb-chat/src/promos.rs:1182  send_promo_message ruft send_announcement Farbe purple
crates/tb-chat/src/promos.rs:1210  send_timeout_pitch, Werbefrei-Pitch über Announcement blue
crates/tb-chat/src/promos.rs:1240  build_promo_text mit Override- dann Pool-Reihenfolge
crates/tb-chat/src/promos.rs:1252  reason-Routing viewer_spike/chat_activity/sonst auf die drei Pools
crates/tb-chat/src/promos.rs:1307  load_global_promo_message über tb_analytics promo_mode
crates/tb-chat/src/promos.rs:1322  load_streamer_promo_message aus streamer_plans.promo_message
crates/tb-chat/src/promos.rs:355  all_promo_messages, zu löschende Sammelliste
crates/tb-chat/src/promos.rs:367  activity_promo_messages, zu löschende Aktivitätsliste
crates/tb-chat/src/promos.rs:339  promo_messages_hype, zu löschende Hype-Liste
crates/tb-chat/src/promos.rs:497  global_presets, zu löschende Global-Presets
crates/tb-chat/src/promos.rs:545  user_presets, zu löschende User-Presets
crates/tb-chat/src/promos.rs:1952  maybe_send_targeted_promo, Targeted-Pfad mit PresetPicker
crates/tb-chat/src/promos.rs:2214  promo_channel_allowed_db, Allowlist plus Partner-State
crates/tb-chat/src/promos.rs:2259  promo_blocked_by_plan_or_flag, Werbefrei-Gate promo_disabled
crates/tb-chat/src/promos.rs:2184  stream_start_delay_ok, 10-Minuten-Startverzögerung
crates/tb-chat/src/promos.rs:2427  save_promo_cooldown in twitch_promo_cooldowns
crates/tb-chat/src/promos.rs:2146  load_user_context_snippets liest twitch_engagement_conversation LIMIT 5
crates/tb-chat/src/promos.rs:2508  hat_gedankenstrich Test-Helfer prüft Strich-Verbot
crates/tb-chat/src/promos.rs:2979  make_engine_no_db, Unit-Test-Engine ohne echte DB
crates/tb-chat/src/promos.rs:3247  pool_or_skip Makro mit TB_TEST_DATABASE_URL für DB-Tests
crates/tb-chat/src/promos.rs:3288  apply_ddl, hier muss twitch_promo_pitch_log-DDL rein
crates/tb-chat/src/pipeline.rs:1146  on_message-Aufruf nur bei class.is_deadlock_live
crates/tb-chat/src/pipeline.rs:1127  record_raw_message-Aufruf immer, Schritt 11
crates/tb-chat/src/types.rs:44  ChatMessageEvent-Felder inkl. message_id und chatter_user_id
crates/tb-chat/src/api.rs:22  ChatApi::send_message ohne reply_parent-Parameter
crates/tb-chat/src/commands.rs:647  reply setzt nur @login-Prefix, kein Twitch-Reply
crates/tb-llm/src/selection.rs:14  FIREWORKS_ONLY_USE_CASES-Liste, hier neuen Use-Case eintragen
crates/tb-llm/src/selection.rs:26  endpoint_for bindet jeden Use-Case an Fireworks/Deepseek
crates/tb-llm/src/hub.rs:279  complete, Einstieg für den LLM-Aufruf
crates/tb-llm/src/hub.rs:70  Request mit json_object, temperature, timeout, max_tokens
crates/tb-engagement/src/outreach_shadow.rs:23  OUTREACH_SYSTEM_PROMPT pub const, Stilvertrag
crates/tb-engagement/src/outreach_shadow.rs:225  decide baut Request und parst JSON-Antwort
crates/tb-engagement/src/outreach_shadow.rs:402  forbidden_opener privat, nicht importierbar
crates/tb-engagement/src/outreach_shadow.rs:410  contains_link privat
crates/tb-engagement/src/outreach_shadow.rs:461  contains_superlative privat
crates/tb-engagement/src/outreach_shadow.rs:496  contains_forbidden_emoji whitelistet lange Striche
crates/tb-engagement/src/outreach_shadow.rs:629  Test der Nur-Fireworks-Liste, Vorbild für neuen Test
crates/tb-chat/Cargo.toml:22  tb-chat hängt an tb-engagement
crates/tb-chat/Cargo.toml:26  tb-chat hängt an tb-llm
crates/tb-engagement/Cargo.toml:20  tb-engagement hängt an tb-llm, nicht an tb-chat, kein Zyklus
bin/tb-bot/src/smalltalk_loop_wiring.rs:37  DEFAULT_REVIEW_CHANNEL_ID des Smalltalk-Reviews
bin/tb-bot/src/smalltalk_loop_wiring.rs:274  BrokerRelay als Arc dyn DiscordBackend
crates/tb-transport-discord/src/backend.rs:110  DiscordBackend::send_rich_message
bin/tb-bot/src/main.rs:583  smalltalk_loop_wiring::start mit settings.broker
bin/tb-bot/src/chat_wiring.rs:739  set_preset_picker mit EngagementMinimaxClient::new(None,None,None,None)
bin/tb-bot/src/chat_wiring.rs:1889  pick_preset fällt bei leeren snippets auf RandomPresetPicker
crates/tb-engagement/src/minimax_chat.rs:729  EngagementMinimaxClient nutzt tb_llm::endpoint_for
rust/migrations/20260903090000_twitch_moderation_settings.sql:1  additive Tabelle ohne GRANT im File
bin/tb-bot/src/main.rs:568  Migrationen gated durch TB_DB_MIGRATE
rust/migrations/20260601000000_baseline_schema.sql:1011  twitch_live_state.last_title für Stream-Titel
rust/knowledge/bot/faq-werbung.md:1  bestehende FAQ, per REQ-08 zu aktualisieren

## Rote Baseline 2026-09-05

- Befehl: `/home/nathanael/.cargo/bin/cargo test -p tb-chat` (ohne TB_TEST_DATABASE_URL), Worktree HEAD ac9ea725e24aac14f6ed080a8f8168e4bd9cdaf4 (Task-Commit auf origin/main ec1ba600).
- Ergebnis: `test result: FAILED. 701 passed; 6 failed; 4 ignored; 0 measured; 0 filtered out; finished in 270.30s`.
- Die 6 Fehler sind alle in `standard_replies::tests` und panicken mit "TB_TEST_DATABASE_URL fehlt": sie verlangen die Test-DB hart (kein pool_or_skip). Mit gesetzter TB_TEST_DATABASE_URL laufen sie durch.
  - standard_replies::tests::gruss_aus_dem_vorigen_stream_blockiert_nicht (standard_replies.rs:823)
  - standard_replies::tests::gruss_bleibt_aus_wenn_der_kanal_ihn_abgeschaltet_hat (standard_replies.rs:740)
  - standard_replies::tests::ohne_plan_zeile_gruesst_der_bot_weiter (standard_replies.rs:759)
  - standard_replies::tests::release_antwort_haengt_nicht_am_gruss_schalter (standard_replies.rs:843)
  - standard_replies::tests::unterdrueckter_doppelgruss_sperrt_den_naechsten_chatter_nicht (standard_replies.rs:795)
  - standard_replies::tests::zweiter_gruss_desselben_chatters_bleibt_aus (standard_replies.rs:773)
- Merke: ab hier gilt jeder neue rote Test, der nicht in dieser Liste steht, als Fehler. Der Endlauf wird mit Test-DB gefahren, dann sind die 6 grün.
