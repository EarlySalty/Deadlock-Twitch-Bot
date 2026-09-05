status: aktiv
datum: 2026-09-05

# Evidence: Partner-Pitch an streamende Zuschauer

Je Zeile eine Fundstelle (pfad:zeile oder Tabelle:Spalte) plus Aussage. Stand origin/main 2c0a8bbf.

rust/crates/tb-chat/src/promos.rs:723  on_message_pitch ist der Anlass-Pitch-Einstieg, hier hängt der Partner-Zweig ein.
rust/crates/tb-chat/src/promos.rs:725  Guard: unter 25 Zeichen oder Befehl mit ! bricht ab (REQ-02).
rust/crates/tb-chat/src/promos.rs:728  Broadcaster-Guard: chatter==broadcaster oder leere chatter_user_id bricht ab (kein Pitch im eigenen Kanal).
rust/crates/tb-chat/src/promos.rs:737  pitch_semaphore.try_acquire begrenzt gleichzeitige Pitches (PITCH_MAX_CONCURRENT=8).
rust/crates/tb-chat/src/promos.rs:742  pitch_judge_throttle_reserve ist die Judge-Drossel vor allen weiteren Gates.
rust/crates/tb-chat/src/promos.rs:108  PITCH_JUDGE_CHATTER_COOLDOWN=15min, CHANNEL_WINDOW=60min, CHANNEL_MAX_PER_WINDOW=30, PITCH_MAX_CONCURRENT=8.
rust/crates/tb-chat/src/promos.rs:747  partner_check.is_partner_channel_for_chat_tracking gateet auf Partnerkanal (REQ-05).
rust/crates/tb-chat/src/promos.rs:756  promo_channel_allowed_db ist das Allowlist-Gate.
rust/crates/tb-chat/src/promos.rs:761  promo_blocked_by_plan_or_flag ist das Werbefrei-Gate (INV-01).
rust/crates/tb-chat/src/promos.rs:766  suppression.is_muted ist das Outbound-Suppression-Gate.
rust/crates/tb-chat/src/promos.rs:771  stream_start_delay_ok ist die 10-Minuten-Startverzögerung.
rust/crates/tb-chat/src/promos.rs:841  insert_pitch_log_pending schreibt pfad="anlass"; der Partner-Zweig braucht pfad="partner" (REQ-06).
rust/crates/tb-chat/src/promos.rs:936  pitch_user_limit_ok: 7 Tage je user_id, pfad='anlass'; Vorlage für Partner-Lifetime-Limit.
rust/crates/tb-chat/src/promos.rs:966  pitch_channel_limit_ok: höchstens 3 pro Stream und 600s Abstand, pfad='anlass'.
rust/crates/tb-chat/src/promos.rs:2669  insert_pitch_log_pending (RETURNING id) ist der Send-Log-Weg.
rust/crates/tb-chat/src/promos.rs:2700  mark_pitch_log_sent setzt sent_at nach erfolgreichem Send.
rust/crates/tb-chat/src/promos.rs:393  Trait PitchReviewSink::send_card(channel_login, target_login, trigger, reply); für Kandidatenstatus zu erweitern.
rust/crates/tb-chat/src/promos.rs:3585  MockPitchJudge und ab 3733 der Inline-Test-DDL-Block; Pitch-Tests leben inline, nicht in tests/.
rust/crates/tb-chat/src/promos.rs:3845  Inline-DDL legt twitch_promo_pitch_log an; twitch_partners fehlt und ist für den Kandidatentest zu ergänzen.
rust/crates/tb-chat/src/promo_pitch.rs:51  PitchOccasion-Enum mit sechs Anlässen; ein neuer Wert reicht nicht ohne eigenen Prompt.
rust/crates/tb-chat/src/promo_pitch.rs:35  PROMO_SYSTEM_PROMPT und L43 TARGETED_PITCH_SYSTEM_PROMPT sind die Vorlage für einen PARTNER_PITCH-Systemprompt.
rust/crates/tb-chat/src/promo_pitch.rs:152  pitch_filter_reject deckt Link, Mitgliederzahl, Superlativ, Emoji, too_long, join_phrase (REQ-03).
rust/crates/tb-chat/src/promo_pitch.rs:396  finalize_targeted_pitch ist die Vorlage für finalize_partner_pitch.
rust/crates/tb-chat/src/pipeline.rs:1145  on_message_pitch wird nur bei is_deadlock_live gespawnt (pipeline.rs:1158-1162).
rust/crates/tb-chat/src/commands.rs:1655  cmd_invite ist Partner-gegated, 1h-Cooldown, sendet die Discord-Einladung auf Nachfrage (INV-07: nicht ändern).
rust/crates/tb-engagement/src/outreach_shadow.rs:31  Wörtlicher Partner-Angebotssatz (dritte Person, konditional) als Prompt-Beispiel.
rust/crates/tb-engagement/src/outreach_shadow.rs:23  OUTREACH_SYSTEM_PROMPT (Stilvertrag), Basis für Ton und kein-Gedankenstrich-Regel.
rust/crates/tb-scout/src/detector.rs:55  Kandidaten-SQL: twitch_stats_category is_partner=FALSE game_name='deadlock' mit allen Ausschlüssen (Detector-Logik).
rust/crates/tb-scout/src/detector.rs:90  twitch_user_id wird aus twitch_stream_sessions aufgelöst, weil twitch_stats_category keine user_id trägt.
rust/bin/tb-bot/src/chat_wiring.rs:1946  Review-Karten-Titel ist hart "Anlass-Pitch"; für Partner-Pitch zu verzweigen.
rust/bin/tb-bot/src/chat_wiring.rs:723  set_pitch_review_sink verdrahtet DiscordPitchReviewSink; PITCH_REVIEW_CHANNEL_ID L1875.
rust/migrations/20260905090000_twitch_promo_pitch_log.sql:1  twitch_promo_pitch_log: id, channel_login, target_user_id, pfad, occasion, ..., sent_at (INV-05: reicht für Partner-Pfad).
rust/migrations/20260714210000_twitch_scout_pitch.sql:14  Ledger-Index und Blacklist-PK laufen über streamer_login, nicht user_id.
twitch_scout_candidates:*  0 Zeilen heute; leer, die zuerst genannte Quelle liefert derzeit niemanden.
streamer_dim:*  0 Zeilen heute; leer.
twitch_stats_category:twitch_user_id  0 von 473799 nicht-Partner-Zeilen (30 Tage) gefüllt; Spalte durchgängig NULL.
twitch_stream_sessions:twitch_user_id  276 distinct user_id mit Deadlock-Session (30 Tage); INV-08-konformer Resolver.
twitch_chat_messages:chatter_id  73750/73750 gefüllt und numerisch (30 Tage); Chat-Identität per user_id verfügbar.
twitch_partners:twitch_user_id  69/69 gefüllt; Partner-Ausschluss per user_id sauber möglich.
twitch_scout_pitch_ledger:action  301 Zeilen, action-Werte posted/judge_none/judge_error/suppressed_cooldown; alle mit twitch_user_id.
Kandidatenzahl:count  418 Nicht-Partner-Deadlock-Streamer (login, 30 Tage); 18 chatten per Login in Partnerkanälen; 17 INV-08-konform per user_id; 0 bei striktem stats_category-user_id-Join.

## Drei wichtigste Risiken

1. Datenquelle fast leer und Login-User-ID-Bruch: twitch_scout_candidates und streamer_dim sind 0 Zeilen, twitch_stats_category.twitch_user_id ist NULL. Ohne Auflösung über twitch_stream_sessions liefert ein strikter user-id-Weg 0 Kandidaten; realistischer Kreis rund 17 Personen.
2. Ledger und Blacklist identifizieren primär über streamer_login (PK bzw. Login-Index), twitch_user_id nur additiv: eine reine user-id-Prüfung "schon kontaktiert oder gesperrt" kann Personen durchlassen oder verfehlen, Verwechslung gleicher Login mit anderer ID.
3. Statuswechsel zwischen Erkennung und Send und Kosten pro Nachricht: Partnerstatus kann sich vor dem Send ändern (erneute twitch_partners-Prüfung im Doppelsend-Lock nötig), und der zusätzliche DB-Join je qualifizierender Nachricht muss hinter Judge-Drossel und Semaphore bleiben.

## Rote Baseline M1

Test: promos::db_tests::partner_kandidat_bekommt_partner_pitch
Lauf vor dem Partner-Zweig (nur Gerüst): FAILED.
Meldung: assertion `left == right` failed: genau ein Partner-Pitch erwartet, left: 0, right: 1 (crates/tb-chat/src/promos.rs, msgs.len()).
Grund: der Partner-Zweig fehlt, der Anlass-Judge liefert kein Anlass, also 0 Sends.
