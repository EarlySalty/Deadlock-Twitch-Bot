status: aktiv
datum: 2026-09-05

# Research: Partner-Pitch an Zuschauer, die selbst Deadlock streamen

Stand origin/main 2c0a8bbf. Beobachtung (Code/DB belegt) strikt getrennt von Vermutung. Alle Zeilenangaben gegen `git show origin/main:<pfad>`.

## 1. Datenquellen "dieser Chatter streamt Deadlock"

### Beobachtung: Tabellenstruktur und Füllstand (public-Schema, twitch_analytics)

- `twitch_scout_candidates` Spalten: streamer_login, twitch_user_id, sessions_count, avg_viewers, first_seen, last_seen, language, deadlock_share, status, entscheid_grund, approver, decided_at, dispatched_at, visited_at. Statuswerte im Code: `vorgeschlagen`, `approved`, `uebersprungen`, `pausiert`, `persoenlich`, `bekannt` (tb-scout/src/lib.rs:13-41). Zeilen heute: 0. Die Tabelle ist leer, keine Statusverteilung.
- `streamer_dim` Spalten: twitch_login, twitch_user_id, discord_user_id, discord_display_name, is_partner, is_monitored_only, archived_at, updated_at. Zeilen heute: 0 (leer).
- `twitch_streamers` (BASE TABLE): twitch_login, twitch_user_id, created_at. 72 Zeilen, davon 3 nicht Partner (per user_id). Kein Deadlock-Merkmal in dieser Tabelle.
- `twitch_streamers_partner_state` ist eine VIEW, 61 Zeilen, alle is_partner=1 (53 aktiv, 8 inaktiv), is_monitored_only=0 bei allen. Führt also nur Partner, keine monitored-only-Streamer.
- `twitch_partners`: 69 Zeilen, alle mit twitch_user_id gesetzt. Partner-Erkennung über twitch_user_id ist sauber möglich.
- `twitch_scout_pitch_ledger`: id, streamer_login, trigger_type, judge_input_excerpt, judge_verdict, confidence, action, detail, discord_message_id, created_at, feedback_up/down/synced_at, twitch_user_id. 301 Zeilen, twitch_user_id ist inzwischen in allen 301 gesetzt. action-Werte: posted 7, judge_none 27, judge_error 9, suppressed_cooldown 254, suppressed_per_stream_limit 3, suppressed_sanitizer 1. Identität primär streamer_login (Index auf LOWER(streamer_login), migrations/20260714210000_twitch_scout_pitch.sql:14-15); twitch_user_id kam per Folgemigration dazu.
- `twitch_scout_pitch_blacklist`: streamer_login PRIMARY KEY, reason, created_at, twitch_user_id. 0 Zeilen heute. Identität ist der Login (PK), twitch_user_id nur additiv.
- `twitch_partner_outreach`: streamer_login, streamer_user_id, detected_at, cooldown_until u.a. 57 Zeilen. Kontaktvermerk der Outreach-Kette; Erkennung im Detector läuft über LOWER(streamer_login) mit `cooldown_until > NOW()` (tb-scout/src/detector.rs:73-75).
- `twitch_partner_outreach_conversations`: streamer_login, streamer_user_id, twitch_user_id, state u.a. 1 Zeile (state=listening).
- Signalquelle "streamt Deadlock": `twitch_stats_category` (ts_utc, streamer, viewer_count, is_partner, game_name, stream_title, language, twitch_user_id). twitch_user_id ist bei ALLEN Zeilen NULL (0 von 473799 der letzten 30 Tage nicht-Partner). Das Deadlock-Signal ist login-basiert (streamer + game_name).
- `twitch_stream_sessions`: streamer_login, started_at, stream_title, game_name, twitch_user_id. Trägt das Paar (login, user_id, game_name); 276 distinct twitch_user_id mit Deadlock-Session in 30 Tagen. Der Detector zieht die user_id genau von hier (detector.rs:90-93).
- `twitch_chat_messages`: chatter_login, chatter_id (text, voll gefüllt: 73750/73750 numerisch in 30 Tagen), streamer_login, message_ts. Chat-Log ab 2026-01-31, gesamt 231460 Zeilen. `twitch_session_chatters`: session_id, streamer_login, chatter_login, chatter_id, seen_via_chatters_api.

### Beobachtung: kanonische Kombination "Deadlock-Streamer, nicht Partner, nicht kontaktiert, nicht gesperrt"

Der Scout-Detector definiert genau diese Menge (detector.rs:55-101): aus `twitch_stats_category` mit `is_partner=FALSE` und `game_name='deadlock'` (deadlock_share, L88-89), ausgeschlossen per NOT EXISTS: `twitch_partners` (L61-62), `twitch_raid_blacklist` (L63-64), `twitch_partner_signup_denylist` (L65-66), `twitch_scout_pitch_blacklist` (L67-68), `twitch_outbound_chat_suppressions` source=recruitment (L69-72), `twitch_partner_outreach` cooldown_until>NOW (L73-75), `twitch_scout_candidates` status<>vorgeschlagen (L76-78). Die user_id wird nachträglich aus `twitch_stream_sessions` aufgelöst (L90-93). Alle Joins laufen über LOWER(login), nicht über user_id.

### Beobachtung: Personenzahl heute (COUNT)

- Nicht-Partner-Deadlock-Streamer 30 Tage (Detector-Kern, ohne Blacklist/Outreach-Treffer): 418 (distinct login).
- Davon per Login in einem Partnerkanal chattend (30 Tage): 18.
- Davon INV-08-konform per twitch_user_id (aufgelöst über twitch_stream_sessions, chatten in FREMDEM Partnerkanal): 17.
- Strikter Join über `twitch_stats_category.twitch_user_id`: 0, weil diese Spalte durchgängig NULL ist.

Chat-Häufigkeit solcher Personen: gemessen als "distinct Kandidaten mit mindestens einer Nachricht in Partnerkanälen in 30 Tagen" (siehe 17/18 oben); es ist ein kleiner, aber echter Kreis.

### Vermutung / Bewertung

- Die im Contract zuerst genannte Quelle `twitch_scout_candidates` ist leer, `streamer_dim` ebenfalls; der Scout kuratiert derzeit keine Kandidaten in die Tabelle. Wer sich allein darauf stützt, erkennt heute niemanden.
- Praktikable, INV-08-konforme Quelle: `twitch_stream_sessions` (login, user_id, game_name='deadlock') als Erkennung UND user_id-Resolver in einem, gefiltert gegen `twitch_partners` (user_id), `twitch_scout_pitch_blacklist`, `twitch_partner_outreach`, `twitch_scout_pitch_ledger`. Das entspricht der Detector-Logik ohne Änderung an tb-scout (INV-04). Erkennung: die im Chat-Event mitgelieferte `chatter_user_id` gegen diese Kandidatenmenge prüfen.

## 2. Anlass-Pitch-Pfad und Einhängepunkt für "partner"

### Beobachtung

- `on_message_pitch` (promos.rs:723-882): Guard <25 Zeichen oder `!`-Befehl (L725), Broadcaster-Guard chatter==broadcaster bzw. leere id (L728-729), `pitch_semaphore.try_acquire` (L737, PITCH_MAX_CONCURRENT=8, L111/568), Judge-Drossel `pitch_judge_throttle_reserve` (L742; 15 min je Chatter, 30 je Kanal/Stunde: Konstanten L108-110, Logik L892-914), partner_check (L747), Allowlist `promo_channel_allowed_db` (L756), Werbefrei `promo_blocked_by_plan_or_flag` (L761), Suppression `is_muted` (L766), Startverzögerung `stream_start_delay_ok` (L771), User-Limit `pitch_user_limit_ok` (L777; 7 Tage je user_id, pfad='anlass', L936-964), Kanal-Limit `pitch_channel_limit_ok` (L782/833; höchstens 3 pro Stream, 600s-Abstand, L966-996). Danach Kontext laden, `pitch_judge.decide` (L797), Filter `pitch_filter_reject`/`pitch_injection_reject` (L806/818), Doppelsend-Lock `get_send_lock` (L830), `insert_pitch_log_pending` mit pfad="anlass" (L841-852), Send (L858), `mark_pitch_log_dropped`/`mark_pitch_log_sent` (L865/869), `mark_promo_sent` (L870), Review-Karte `sink.send_card` (L880).
- promo_pitch.rs: `PitchOccasion` (L51-60: NoMates, GameUnpopular, TooTryhard, SoloQueue, NewPlayer, WantsHelp), `as_str` (L62-73), `parse_pitch_response` (L85), `PitchJudgeInput`/`PitchJudge`-Trait (L332-343), `FireworksPitchJudge` (L345), Systemprompt für den Anlass am Dateikopf (L1-49, zweiteilige Antwort). Filter `pitch_filter_reject` (L152: Link, Mitgliederzahl, Superlativ, Emoji, too_long, join_phrase), `pitch_injection_reject` (L301).
- pfad-Werte heute: nur "anlass" wird je in `twitch_promo_pitch_log` geschrieben (einzige Aufrufe promos.rs:845 und 923 in log_anlass_reject). Andere Pfade (channel-promo, targeted) schreiben nicht in diese Tabelle.
- Log-Helfer: `record_pitch_log` (promos.rs:2643), `insert_pitch_log_pending` (2669, RETURNING id), `mark_pitch_log_sent` (2700), `mark_pitch_log_dropped` (2713).

### Bewertung: sauberster Einhängepunkt

- Ein neuer Occasion-Wert reicht nicht. Der Anlass-Judge (promo_pitch.rs:1-49) kennt nur die sechs Anlässe und erzeugt eine zweiteilige Antwort ohne Partnerangebot; er weiß nichts vom Kandidatenstatus. Der Partner-Pitch braucht einen eigenen Systemprompt (Partner-Angebot in dritter Person, konditional) und einen eigenen `tb_llm::complete`-Aufruf, Use-Case `promo_pitch` (INV-02).
- Empfehlung: in `on_message_pitch` unmittelbar nach den Gates (nach L782, vor `load_live_context`) eine Kandidatenprüfung gegen `chatter_user_id` einziehen. Trifft sie zu, in den Partner-Zweig verzweigen (eigener Prompt und eigene build/finalize, dieselben Filter `pitch_filter_reject`/`pitch_injection_reject`), Log mit pfad="partner", eigene Limits (User lifetime, Kanal pro Stream, 5/Tag), danach `return` (REQ-07: Anlass-Zweig nicht mehr betreten). Nur wenn kein Kandidat, läuft der bestehende Anlass-Zweig weiter. So bleibt die Reihenfolge und Gate-Kette des Anlass-Pitch unverändert (INV-03) und die gegenseitige Ausschließlichkeit sitzt an einer Stelle.
- Neuer Systemprompt gehört nach promo_pitch.rs (analog PROMO_/TARGETED_PITCH_SYSTEM_PROMPT L35/L43), plus eine `build_partner_pitch_text`/`finalize_partner_pitch`-Funktion analog `finalize_targeted_pitch` (L396). Kandidaten-Query und Limits in promos.rs.

## 3. Stilvertrag und Partner-Angebot (outreach_shadow.rs)

### Beobachtung

- `OUTREACH_SYSTEM_PROMPT` (tb-engagement/src/outreach_shadow.rs:23-54), abgeleitet aus `docs/superpowers/specs/2026-07-27-selbstvermarktung-stilvertrag.md` (Kommentar L16-22, 4974 echte Chatnachrichten, bewusst kein Gedankenstrich).
- Schritt "Qualifizieren" (L30): wörtlich "Streamst du öfters DL?".
- Schritt "Anbieten, konditional, dritte Person" (L31): wörtlich "Aber wenn du generell mehr DL zockst, auf Discord gibts ne Deutsche Deadlock Community. Die bieten auch so ne Streamer Partnerschaft, hat einige sehr geile vorteile.".
- Ton (L44-46): deutsch, kurz, locker, Kleinschreibung normal, Lachen ausgeschrieben, Emoji nur `:)`.

### Bewertung

- Der Angebotssatz aus L31 taugt wörtlich als Beispiel im neuen Partner-Systemprompt (dritte Person, konditional "wenn du öfter Deadlock streamst", Nennung der Community, Partnerschaft mit Vorteilen). Die Qualifizierungsfrage L30 passt für einen einmaligen Chat-Pitch weniger (dort geht es um Anknüpfen, hier um eine direkte Antwort), sie belegt aber den Ton. Die Mechanik-Ehrlichkeit (REQ-03: gegenseitige Raids, Chat-Schutz) steht so nicht im Outreach-Prompt und muss neu formuliert werden.

## 4. `!invite` (commands.rs, cmd_invite)

### Beobachtung

- `cmd_invite` (commands.rs:1655-1711): exact-match nur "!invite" (L1657), Partner-Gate `is_partner_channel` (L1665), 1h-Cooldown je (channel, chatter) (L1673-1681, INVITE_COOLDOWN_SECS), holt `invite.invite_line(channel, chatter)` (L1683-1687) und sendet per `reply_plain` (L1689), danach `note_invite_reply` an den Notifier (L1694-1696). Sendet also die Community-Discord-Einladungszeile an den fragenden Chatter.

### Bewertung

- `!invite` ist der bestehende Weg auf Nachfrage und der richtige Kanal, um den Anmeldeweg nur bei aktiver Nachfrage zu nennen (REQ-03). Er ist Partner-gegated und cooldown-geschützt. `commands.rs` steht auf der Verbotsliste (INV-07); der Partner-Pitch darf ihn nicht anfassen und stützt sich nur auf das bestehende Verhalten. Der Partner-Pitch selbst nennt weder Link noch Anmeldeweg, verweist implizit auf `!invite` oder Nachfrage.

## 5. Review-Karte (DiscordPitchReviewSink, chat_wiring.rs)

### Beobachtung

- Trait `PitchReviewSink::send_card(channel_login, target_login, trigger, reply)` (promos.rs:393-394); Verdrahtung chat_wiring.rs:723-743 (`set_pitch_review_sink`).
- `DiscordPitchReviewSink` (chat_wiring.rs:1878-1980): baut eine Components-V2-Karte, Text hart "**Anlass-Pitch** in `{channel}`" (L1946), "An **{target}**" (L1947), "> {trigger}" (L1948), "Antwort: {reply}" (L1949), Kanal `PITCH_REVIEW_CHANNEL_ID` (L1875/1952), Farbe `PITCH_REVIEW_GOLD` (L1876/1957), Feld-Neutralisierung gegen Mentions und Links (L1882-1940).

### Bewertung

- Für den Kandidatenstatus muss die Karte einen Partner-Fall unterscheiden. Sauberster Weg: `send_card` um einen Parameter erweitern (Pitch-Art plus kurzer Kandidaten-Hinweis, etwa Login und Deadlock-Anteil oder "Kandidat: streamt Deadlock, kein Partner") und den festen Titel L1946 verzweigen ("Partner-Pitch"). Der Trait liegt in promos.rs (erlaubt), die Impl in chat_wiring.rs (erlaubt). Alternativ eine neue Trait-Methode `send_partner_card`; ein zusätzlicher Parameter ist der kleinere Diff. Der Anlass-Aufruf (promos.rs:880) muss dann mitgezogen werden (INV-03: Verhalten unverändert, nur Signatur).

## 6. Tests

### Beobachtung

- Es gibt keine Testdatei in `rust/crates/tb-chat/tests/`, die `on_message_pitch` abdeckt. Der Anlass-Pitch wird über einen Inline-`#[cfg(test)]`-Block in promos.rs getestet.
- Test-Infrastruktur inline: `MockPitchJudge` (promos.rs:3585-3610), Helfer `pitch_event(...)` (3634), `pitch_response(...)` (3677), DDL-Block als `CREATE TABLE`-Serie ab 3733 mit u. a. `twitch_promo_cooldowns`, `twitch_streamers_partner_state`, `streamer_plans`, `twitch_live_state`, `twitch_stream_sessions`, `twitch_session_chatters`, `twitch_promo_pitch_log` (3845), `twitch_chat_messages` (3857). Pitch-Tests folgen ab 3881.
- Die separaten DB-Tests (chatter_tracking_db.rs u. a.) legen ihr Schema über eine eigene `apply_ddl`-Funktion an (chatter_tracking_db.rs:53).

### Bewertung

- Der Regressions- und Kandidatentest gehört in denselben Inline-Block. `twitch_stream_sessions` (login, user_id, game_name, started_at) ist bereits im DDL-Block. Neu anzulegen für die Kandidatenprüfung: `twitch_partners` (fehlt im Inline-DDL), dazu `twitch_scout_pitch_ledger` und `twitch_scout_pitch_blacklist`, falls die Kandidaten-Query darauf joint. Ein neuer Mock-Textgenerator für den Partner-Zweig (analog MockPitchJudge) und ein roter Regressionstest ("Kandidat bekommt Partner-Pitch mit pfad=partner, Nicht-Kandidat den Anlass-Pitch") sind Pflicht (rote Baseline vor dem Fix).

## 7. sqlx: neue Queries, Offline-Cache, Snapshot

### Beobachtung

- `rust/scripts/sqlx-prepare.sh` existiert (Offline-Cache-Pflege). Schema-Snapshot `rust/crates/tb-db/tests/fresh_schema_snapshot.txt` enthält `twitch_promo_pitch_log` (Zeilen 1191-1200).

### Bewertung / Vermutung

- Neue kompilierzeit-geprüfte Queries: (a) Kandidatenprüfung (SELECT gegen twitch_stream_sessions plus NOT EXISTS twitch_partners/twitch_scout_pitch_blacklist/twitch_partner_outreach/twitch_scout_pitch_ledger, per user_id), (b) Partner-User-Limit (SELECT MAX(sent_at) FILTER ... pfad='partner' lifetime), (c) Partner-Kanal-Limit pro Stream (pfad='partner', seit stream_start), (d) Partner-Tageslimit (COUNT pfad='partner' sent 24h kleiner 5), (e) Ledger-Eintrag (INSERT INTO twitch_scout_pitch_ledger). Jede neue Query braucht `.sqlx`-Regen (rust/scripts/sqlx-prepare.sh) und ist im Contract-Scope (rust/.sqlx/). Der Schema-Snapshot ändert sich nur, wenn eine Migration Tabellen oder Spalten ändert; ohne neue Spalte bleibt er unverändert. Ein Ledger-Insert auf bestehende Spalten braucht keine Migration (INV-05); ein zusätzlicher pfad-Index auf twitch_promo_pitch_log wäre additiv möglich, aber nicht zwingend.

## 8. Risiken

### Beobachtung

- Verwechslung Login gegen ID: Deadlock-Signal (`twitch_stats_category`) und Ledger/Blacklist sind login-basiert; `twitch_stats_category.twitch_user_id` ist komplett NULL. INV-08 verlangt Erkennung nur über user_id, also muss die Auflösung über `twitch_stream_sessions` laufen. Altzeilen in Ledger/Blacklist tragen twitch_user_id evtl. nicht in jeder historischen Zeile (Blacklist heute 0 Zeilen, Ledger inzwischen 301/301 gefüllt).
- Kandidat inzwischen Partner: zwischen Erkennung und Send kann sich der Partnerstatus ändern; die Partner-Prüfung (twitch_partners per user_id) muss im finalen Lock erneut greifen, analog dem doppelten `pitch_channel_limit_ok` (promos.rs:782/833).
- Broadcaster im eigenen Kanal: `on_message_pitch` bricht bereits bei chatter==broadcaster ab (L728). Ein Kandidat, der im FREMDEN Partnerkanal schreibt, ist zulässig; im eigenen Kanal ist er Broadcaster und der Guard greift. Nicht-Ziel: kein Pitch im eigenen Kanal (bleibt Outreach).
- Rate: höchstens 5 Partner-Pitches pro Tag gesamt und einmal je Person für immer begrenzt bei rund 17 aktiven Kandidaten den Effekt stark; die Kandidaten-DB-Prüfung läuft jedoch je qualifizierender Nachricht (mindestens 25 Zeichen, kein Befehl) und ist ein zusätzlicher Join pro Nachricht in Partnerkanälen.

### Die drei wichtigsten Risiken

1. Datenquelle fast leer und Login-User-ID-Bruch. `twitch_scout_candidates` und `streamer_dim` haben 0 Zeilen, `twitch_stats_category.twitch_user_id` ist durchgängig NULL. Ein strikt user-id-basierter Erkennungsweg liefert heute 0 Treffer; die Erkennung muss über `twitch_stream_sessions` (login plus user_id plus game_name) laufen, sonst greift das Feature nie oder verletzt INV-08. Realistischer Kandidatenkreis: rund 17 Personen.
2. Ledger und Blacklist identifizieren primär über streamer_login (PK bzw. Login-Index); twitch_user_id ist nur additiv. Eine Prüfung "schon kontaktiert oder gesperrt" rein per user_id kann Personen durchlassen (Login-Zeile ohne user_id) oder verfehlen; Verwechslung gleicher Login mit anderer ID. Die Prüfung sollte user_id und Login-Fallback konsistent zur Detector-Logik führen, ohne tb-scout zu ändern (INV-04).
3. Statuswechsel zwischen Erkennung und Send (Kandidat wird Partner) und Kosten pro Nachricht: die Partner-Prüfung muss im finalen Doppelsend-Lock erneut gegen twitch_partners laufen, und der zusätzliche DB-Join je qualifizierender Nachricht ist gegen Judge-Drossel und Semaphore abzusichern, damit die Chat-Pipeline nicht belastet wird.
