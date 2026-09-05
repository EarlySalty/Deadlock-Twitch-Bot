status: aktiv
datum: 2026-09-05

# Plan: Partner-Pitch an Zuschauer, die selbst Deadlock streamen

Maßgeblich ist `CONTRACT.md` (unveränderlich) im selben Ordner; Bestand in `RESEARCH.md` und `EVIDENCE.md`. Stand origin/main 2c0a8bbf. Der Plan setzt die festen Orchestrator-Entscheidungen um und begründet die offenen Punkte (Ledger-Kontaktbegriff, Trait-Schnitt, Session-Fenster).

## Feste Entscheidungen (Orchestrator, keine Contract-Änderung)

- D1 Kandidatenquelle: `twitch_stream_sessions` (Paar login plus twitch_user_id plus game_name in einer Zeile). Erkennung geht über die im Chat-Event gelieferte `chatter_user_id` als `$1` gegen diese Menge (INV-08). Fenster: mindestens eine Deadlock-Session in den letzten 60 Tagen (`started_at >= NOW() - INTERVAL '60 days'`, `LOWER(game_name) = 'deadlock'`). `twitch_scout_candidates` und `streamer_dim` sind leer und werden nicht benutzt (Research).
- D2 Ausschlüsse: `twitch_partners` per `twitch_user_id` (jeder Status, departnerte Kanäle gelten als kontaktiert), plus die login-basierten Tabellen aus derselben Session-Zeile: `twitch_scout_pitch_blacklist` (LOWER(streamer_login)), `twitch_partner_outreach` (LOWER(streamer_login) oder twitch_user_id), `twitch_scout_pitch_ledger` (LOWER(streamer_login) oder twitch_user_id, nur Zeilen mit Kontakt, siehe D3). Der Broadcaster des Kanals ist über den bestehenden Guard `chatter_user_id == broadcaster_user_id` (promos.rs:728) ausgeschlossen.
- D3 Ledger-Kontaktbegriff: als "schon kontaktiert" zählt eine Ledger-Zeile mit `action = 'posted'`. Grund: `posted` ist der einzige action-Wert, bei dem tatsächlich eine Nachricht rausging (Research: posted 7, sonst judge_none, judge_error, suppressed_*). REQ-01 sagt wörtlich "mit Kontakt". Das hält den Kandidatenkreis (rund 17 Personen) offen und schließt trotzdem jeden aus, der real schon angeschrieben wurde. Der eigene Partner-Pitch schreibt nach dem Send selbst `action = 'posted'` (REQ-06), sodass ein zweiter Versuch derselben Person auch über den Ledger gesperrt ist. Diese Auslegung dem Merge-Kritiker gegenüber offenlegen.
- D4 Einhängung: in `on_message_pitch` nach allen REQ-05-Gates (nach `stream_start_delay_ok`, promos.rs:771) und vor den Anlass-Limits (`pitch_user_limit_ok`, promos.rs:777). Judge-Drossel und Semaphore liegen davor und gelten unverändert. Bei Kandidaten-Treffer läuft der Partner-Zweig und endet mit `return` (REQ-07); ohne Treffer läuft der bisherige Anlass-Pfad ab promos.rs:777 byte-gleich weiter (INV-03).
- D5 Limits (pfad='partner' in `twitch_promo_pitch_log`): je twitch_user_id genau einer für immer (Lifetime), je Kanal einer pro Stream (seit `load_stream_start`), fünf pro Tag global. Erneute Partner-Statusprüfung unter dem Doppelsend-Lock vor dem Send. Ein gesendeter Partner-Pitch belegt den Promo-Cooldown des Kanals per `mark_promo_sent(login, .., "partner_pitch", ..)`.
- D6 Text: eigener Systemprompt `PARTNER_PITCH_SYSTEM_PROMPT` in promo_pitch.rs, Stil aus `outreach_shadow.rs:23ff`, konditionaler Satz aus L31 wörtlich als Beispiel, Mechanik ehrlich (gegenseitige Raids beim Offline-Gehen, Chat-Schutz), kein Link, keine Superlative, keine Mitgliederzahlen, kein Gedankenstrich, kein "komm auf Discord", Emoji nur `:)`. Anmeldeweg nur auf Nachfrage oder `!invite` (bestehendes Verhalten, commands.rs nicht anfassen, INV-07). Ein eigener `tb_llm::complete`-Aufruf, Use-Case `promo_pitch`, Deepseek V4 Flash (INV-02).
- D7 Review-Karte: `send_card` bekommt Pitch-Art und Kandidaten-Hinweis (Login, Datum der letzten Deadlock-Session) als Parameter, Titel "Partner-Pitch" statt "Anlass-Pitch".
- D8 sqlx-Offline-Cache (`rust/.sqlx/`) und Schema-Snapshot sind im Contract-Scope; `rust/scripts/sqlx-prepare.sh` nach neuen Queries. Keine Migration nötig (INV-05): alle Ziel-Spalten existieren in prod.

## Plan-Entscheidung zum Trait-Schnitt

Der bestehende Trait `PitchTextGen` (channel_promo, targeted_pitch) wird NICHT erweitert. Stattdessen ein eigener Trait `PartnerPitchGen` mit einer Methode `partner_pitch(&self, ctx) -> Option<String>`, eigenes Feld `partner_pitch_gen: Arc<dyn PartnerPitchGen>` in `PromoEngine`, Default `FireworksPartnerPitchGen`, Setter `set_partner_pitch_gen`. So bleiben `FixedTextGen`, `SlowTextGen`, `CountingTextGen` und alle Targeted/Channel-Tests unverändert (INV-03, INV-06); der Partner-Zweig wird über eine eigene Test-Stub-Impl getrieben.

`partner_pitch` liefert die rohe, um Anführungszeichen bereinigte Modellzeile (analog `clean_model_line`), ohne selbst zu filtern. Die harten Filter (`pitch_filter_reject`, `pitch_injection_reject`) laufen danach in promos.rs, damit verworfene Partner-Pitches mit pfad='partner' und Grund geloggt werden (REQ-06), genau wie der Anlass-Zweig (promos.rs:806/818).

## Milestones

### M1 Gerüst plus roter Regressionstest (rote Baseline zuerst)

Änderungen:
- promo_pitch.rs: `PARTNER_PITCH_SYSTEM_PROMPT` (Konstante), `PartnerPitchContext { target_login, target_messages, game, title, recent_chat }`, `build_partner_pitch_text(ctx) -> Option<String>` (baut `tb_llm::Request::simple(PARTNER_PITCH_SYSTEM_PROMPT, json).temperature(0.7).timeout(PITCH_TIMEOUT)`, `tb_llm::complete(USE_CASE, ..)`, gibt `clean_model_line` zurück, kein Filter), Trait `PartnerPitchGen` plus `FireworksPartnerPitchGen`.
- promos.rs: Feld `partner_pitch_gen` in `PromoEngine`, Default in `PromoEngine::new`, Setter `set_partner_pitch_gen`. Noch KEIN Zweig in `on_message_pitch`.
- promos.rs Testmodul: Test-DDL ergänzen (siehe M2-DDL-Liste, damit die Tests kompilieren und laufen), `MockPartnerPitchGen` (liefert konfigurierbaren Text), Regressionstest `partner_kandidat_bekommt_partner_pitch`: legt Kandidat in `twitch_stream_sessions` (game_name deadlock, twitch_user_id des Chatters, started_at jetzt), Kanal als Partnerkanal, `set_partner_pitch_gen(MockPartnerPitchGen)`, `MockPitchJudge` liefert kein Anlass, ruft `on_message_pitch`, erwartet genau einen Send `@login ...` und genau eine `twitch_promo_pitch_log`-Zeile mit pfad='partner', sent_at gesetzt.

Erwarteter Zwischenzustand: Test kompiliert, ist ROT (0 Sends, weil der Partner-Zweig fehlt und der Anlass-Judge kein Anlass liefert). Roten Lauf mit Testname und Meldung in EVIDENCE festhalten (Amendment oder eigener Abschnitt).

Validierung:
- `SQLX_OFFLINE=true /home/nathanael/.cargo/bin/cargo check -p tb-chat`
- Container hoch: `sudo bash rust/scripts/test_db.sh up`, `eval "$(sudo bash rust/scripts/test_db.sh env)"`
- `/home/nathanael/.cargo/bin/cargo test -p tb-chat partner_kandidat_bekommt_partner_pitch -- --nocapture` (muss FEHLSCHLAGEN)

Stop-Regel: Test ist grün statt rot oder kompiliert nicht. Dann Setup korrigieren, nicht die Assertion abschwächen.

### M2 Kandidaten- und Limit-Queries plus Ledger-Insert

Änderungen in promos.rs (neue compile-geprüfte `sqlx::query!`):
- `partner_candidate(&self, chatter_user_id) -> Option<PartnerCandidate { login, last_session: DateTime<Utc> }>`:
  ```sql
  SELECT s.streamer_login AS login, MAX(s.started_at) AS "last_session!"
    FROM twitch_stream_sessions s
   WHERE s.twitch_user_id = $1
     AND LOWER(s.game_name) = 'deadlock'
     AND s.started_at >= NOW() - INTERVAL '60 days'
     AND NOT EXISTS (SELECT 1 FROM twitch_partners p WHERE p.twitch_user_id = $1)
     AND NOT EXISTS (SELECT 1 FROM twitch_scout_pitch_blacklist b
                      WHERE LOWER(b.streamer_login) = LOWER(s.streamer_login) OR b.twitch_user_id = $1)
     AND NOT EXISTS (SELECT 1 FROM twitch_partner_outreach o
                      WHERE LOWER(o.streamer_login) = LOWER(s.streamer_login) OR o.twitch_user_id = $1)
     AND NOT EXISTS (SELECT 1 FROM twitch_scout_pitch_ledger l
                      WHERE (LOWER(l.streamer_login) = LOWER(s.streamer_login) OR l.twitch_user_id = $1)
                        AND l.action = 'posted')
   GROUP BY s.streamer_login
   ORDER BY "last_session!" DESC
   LIMIT 1
  ```
- `partner_user_limit_ok(&self, chatter_user_id) -> bool`: MAX(sent_at) und pending-Count (sent_at IS NULL, reject_reason IS NULL, created_at >= NOW() - INTERVAL '10 minutes') aus `twitch_promo_pitch_log` WHERE target_user_id=$1 AND pfad='partner'. Lifetime: bei einem sent_at oder pending>0 blockieren. Bei DB-Fehler blockieren (fail-closed wie der Anlass-Pfad).
- `partner_channel_limit_ok(&self, login) -> bool`: COUNT sent seit `load_stream_start` WHERE channel_login=$1 AND pfad='partner' AND sent_at IS NOT NULL AND sent_at >= $2; blockiert ab 1.
- `partner_daily_limit_ok(&self) -> bool`: COUNT(*) WHERE pfad='partner' AND sent_at >= NOW() - INTERVAL '24 hours'; blockiert ab 5.
- `record_partner_ledger(&self, login, chatter_user_id, channel_login)`: INSERT INTO twitch_scout_pitch_ledger (streamer_login, trigger_type, judge_verdict, action, detail, twitch_user_id) VALUES ($login, 'chat_partner_pitch', 'partner_pitch', 'posted', $channel_login, $chatter_user_id). Fehler nur `warn!`.

Erwarteter Zwischenzustand: kompiliert mit SQLX_OFFLINE gegen frisch erzeugten `.sqlx`-Cache; noch kein Verhalten geändert (Funktionen ungenutzt, deshalb temporär `#[allow(dead_code)]` NICHT setzen, sondern in M3 sofort verdrahten, damit kein toter Code stehen bleibt).

Validierung:
- `DATABASE_URL=$TB_TEST_DATABASE_URL bash rust/scripts/sqlx-prepare.sh` (oder projektüblicher Aufruf) fuer `rust/.sqlx/`
- `SQLX_OFFLINE=true /home/nathanael/.cargo/bin/cargo check -p tb-chat`

Stop-Regel: `sqlx::query!` verlangt eine Spalte, die im Snapshot fehlt. Dann Query gegen `fresh_schema_snapshot.txt` korrigieren, keine Migration erfinden.

### M3 Partner-Zweig in on_message_pitch verdrahten

Änderungen in promos.rs `on_message_pitch`, unmittelbar nach `stream_start_delay_ok` (L771) und vor `pitch_user_limit_ok` (L777):
1. `if let Some(cand) = self.partner_candidate(&target_user_id).await { ... partner-zweig ...; return; }` sonst weiter im Anlass-Pfad.
2. Partner-Limits (Lifetime, Kanal-pro-Stream, Tageslimit). Bei Verstoß: `log_partner_reject(reason)`, `pitch_judge_throttle_release(&login, &target_user_id)`, `return`.
3. Kontext laden (`load_live_context`, `load_recent_channel_messages`), `build`-Aufruf ueber `self.partner_pitch_gen.partner_pitch(&ctx)`. Bei None: `log_partner_reject("no_text")`, throttle release, return.
4. `pitch_filter_reject` und `pitch_injection_reject` auf die Antwort; bei Treffer `log_partner_reject(reason/"injection")`, return.
5. Doppelsend-Lock (`get_send_lock`), unter dem Lock erneut prüfen: Partner-Status (Kandidat weiterhin kein Partner) und `partner_channel_limit_ok`. Bei Verstoß `log_partner_reject`, return.
6. `out_text = format!("@{target_login} {reply}")`, `insert_pitch_log_pending` mit pfad="partner", occasion=None, trigger/generated gesetzt.
7. Send über `guarded_api_for("promo", &login)`, `record_suppression_on_drop`, bei Drop `mark_pitch_log_dropped`, sonst `mark_pitch_log_sent`, `mark_promo_sent(.., "partner_pitch", ..)`.
8. `record_partner_ledger(&cand.login, &target_user_id, &login)`.
9. `sink.send_card(&login, &target_login, text, &reply, PitchCardKind::Partner, Some(hint))` mit hint = Login plus Datum `cand.last_session`.

Weitere Änderungen:
- promos.rs Trait `PitchReviewSink::send_card` Signatur um `kind` und `candidate_hint: Option<&str>` erweitern; Anlass-Aufruf (L880) auf `PitchCardKind::Anlass, None` ziehen. Enum `PitchCardKind { Anlass, Partner }` in promos.rs.
- promos.rs `log_partner_reject` analog `log_anlass_reject`, aber pfad="partner".
- chat_wiring.rs `DiscordPitchReviewSink::send_card` an die neue Signatur; Titelzeile "**Anlass-Pitch**"/"**Partner-Pitch**" nach `kind` verzweigen; bei Partner eine zusätzliche neutralisierte Zeile mit dem Kandidaten-Hinweis anhängen. Verhalten des Anlass-Falls unverändert (INV-03).

Erwarteter Zwischenzustand: der M1-Regressionstest wird GRÜN.

Validierung:
- `SQLX_OFFLINE=true /home/nathanael/.cargo/bin/cargo check -p tb-chat -p tb-bot`
- `/home/nathanael/.cargo/bin/cargo test -p tb-chat partner_kandidat_bekommt_partner_pitch` (jetzt GRÜN)

Stop-Regel: der Anlass-Pfad ab L777 ist nicht mehr byte-gleich fuer Nicht-Kandidaten. Dann Einhängung sauber vor L777 kapseln.

### M4 Restliche Tests plus FAQ

Neue Inline-Tests (im selben `#[cfg(test)]`-Block wie die Anlass-Tests):
- `nicht_kandidat_geht_in_anlass_pfad`: Chatter ohne Deadlock-Session, MockPitchJudge liefert Anlass, erwartet Anlass-Send mit pfad='anlass' (INV-03).
- `partner_wird_nicht_gepitcht`: Chatter mit Deadlock-Session, aber Eintrag in `twitch_partners` (per user_id); erwartet keinen Partner-Send (fällt in Anlass-Pfad, dort ohne Anlass kein Send).
- `zweiter_partner_pitch_derselben_user_id_geblockt`: nach einem gesendeten Partner-Pitch (pfad='partner', sent_at) blockt der Lifetime-Check den zweiten.
- `partner_tageslimit_fuenf`: fünf vorbelegte sent partner-Zeilen (24h) blocken den sechsten.
- `promo_disabled_sendet_keinen_partner_pitch`: `streamer_plans.promo_disabled=1`, kein Send (INV-01).
- `partner_filter_verwirft_link`: MockPartnerPitchGen liefert Text mit Link, erwartet reject-Log pfad='partner' reason='link', kein Send.

Test-DDL im `apply_ddl`-Block ergänzen (prod-treu, Spaltentypen aus `fresh_schema_snapshot.txt`):
- `twitch_stream_sessions` um `game_name TEXT`, `twitch_user_id TEXT`, `stream_title TEXT` erweitern (die bestehende Definition hat sie nicht).
- `twitch_partners (twitch_login TEXT NOT NULL, twitch_user_id TEXT NOT NULL, status TEXT NOT NULL DEFAULT 'active', ...)` minimal fuer die NOT EXISTS.
- `twitch_scout_pitch_blacklist (streamer_login TEXT PRIMARY KEY, twitch_user_id TEXT, reason TEXT, created_at TIMESTAMPTZ DEFAULT NOW())`.
- `twitch_partner_outreach (streamer_login TEXT NOT NULL, streamer_user_id TEXT, twitch_user_id TEXT, detected_at TEXT NOT NULL DEFAULT '', cooldown_until TEXT, status TEXT)`.
- `twitch_scout_pitch_ledger (id BIGSERIAL PRIMARY KEY, streamer_login TEXT NOT NULL, trigger_type TEXT NOT NULL, judge_verdict TEXT NOT NULL, action TEXT NOT NULL, detail TEXT, twitch_user_id TEXT, created_at TIMESTAMPTZ NOT NULL DEFAULT NOW())`.

FAQ (REQ-08): in `rust/knowledge/bot/faq-werbung.md` einen wahrheitsgemäßen Absatz ergänzen, dass ein Zuschauer, der selbst Deadlock streamt und noch kein Partner ist, einmalig einen Hinweis in dritter Person auf das Partner-Netzwerk bekommen kann; nur als Antwort auf eine eigene Nachricht, nie per Timer, kein Link, Anmeldung nur auf Nachfrage oder `!invite`, streng gedeckelt (einmal je Person, einer pro Stream, wenige pro Tag), und Werbefrei schaltet auch das ab. Keine Gedankenstriche, echte Umlaute.

Erwarteter Zwischenzustand: alle neuen Tests grün, keine bestehende Anlass-/Targeted-/Channel-Testdatei geändert.

Validierung:
- `/home/nathanael/.cargo/bin/cargo test -p tb-chat` (voller Crate-Lauf gegen Test-DB)

Stop-Regel: ein bestehender Test wird rot. Ursache klären, nicht den Test anpassen (INV-06).

### M5 Abschluss-Verifikation und Selbst-Review

- `rust/scripts/sqlx-prepare.sh` final; prüfen, dass `rust/.sqlx/` alle neuen Queries enthält und `git status` nur erwartete Pfade zeigt.
- Schema-Snapshot `fresh_schema_snapshot.txt`: bleibt unverändert, weil keine Migration Tabellen oder Spalten ändert. Falls das Snapshot-Diff etwas anderes zeigt, ist versehentlich eine Migration entstanden; zurückbauen.
- Vollständiger Lauf: `SQLX_OFFLINE=true /home/nathanael/.cargo/bin/cargo test -p tb-chat -p tb-bot` gegen die bekannte rote Baseline (siehe Memory tb-bot-build-toolchain: vorbestehende rote Tests separat ausweisen, keine neue Rotfärbung).
- Selbst-Review des Fixers gegen den Contract vor der Fertigmeldung (`gate_hook.py --review` bzw. rolle-wirkungs-pruefer): REQ-01 bis REQ-08, INV-01 bis INV-08, Scope. Danach frischer Merge-Kritiker.

Validierung: grüner Crate-Lauf, Selbst-Review ohne offene Mangelpunkte.

Stop-Regel: neue Rotfärbung gegenüber der Baseline oder ein REQ/INV nicht belegbar erfüllt.

## Deploy

- Kein Migrationsbedarf (INV-05): alle Ziel-Spalten existieren in prod (`twitch_stream_sessions.game_name/twitch_user_id`, `twitch_scout_pitch_ledger.*`, `twitch_promo_pitch_log.pfad`). Kein `_sqlx_migrations`-Eintrag, keine Grants nötig, weil keine neue Tabelle.
- Release-Weg nach Memory `twitch-release-deploy-weg`: Build in isoliertem Worktree, nie im geteilten Checkout `_ttb-main-deploy`; System-Units, eingefrorener Root-Clone unter `/opt/deadlock/twitch/builds/<sha>`, `install-twitch-release`, danach `sudo systemctl restart deadlock-twitch-bot-rust`.
- Vor Merge auf main: `git rev-list --left-right --count origin/main...main` muss `0 0` sein (Memory live-checkout-push-erbt-fremde-commits); vor Deploy `ListAgents`, genau ein Deployer je Ressource.
- Live-Prüfung: im Bot-Log auf eine Zeile mit `pfad=partner` warten (gesendet oder verworfen); zusätzlich in `twitch_promo_pitch_log` eine Zeile mit pfad='partner' und in `twitch_scout_pitch_ledger` eine Zeile `trigger_type='chat_partner_pitch'` nach einem echten Send, plus die Partner-Review-Karte im Discord-Review-Kanal. Da der Kandidatenkreis klein ist (rund 17 Personen), Feature als scharf gelten lassen, sobald der erste echte Kandidatentreffer im Log steht; bis dahin genügt der grüne Testbeweis.

## Risiken und Gegenmaßnahmen

1. Datenquelle fast leer, Login-User-ID-Bruch: `twitch_stats_category.twitch_user_id` durchgängig NULL, `twitch_scout_candidates`/`streamer_dim` leer. Gegenmaßnahme: Erkennung ausschließlich über `twitch_stream_sessions` (login plus user_id in einer Zeile), `$1 = chatter_user_id` (INV-08); kein Weg über `twitch_stats_category`. Alternativ-Signal `had_deadlock_in_session` bewusst nicht genutzt, um der Orchestrator-Vorgabe game_name zu folgen; falls prod-game_name anders geschrieben ist als 'deadlock', greift LOWER(); im Live-Check kurz gegen die 276 bekannten Session-user_ids gegenprüfen.
2. Ledger und Blacklist primär login-basiert, twitch_user_id nur additiv: reine user-id-Prüfung kann Personen durchlassen oder verfehlen. Gegenmaßnahme: die Ausschluss-NOT-EXISTS matchen sowohl LOWER(streamer_login) aus der Session-Zeile als auch twitch_user_id, konsistent zur Detector-Logik, ohne tb-scout zu ändern (INV-04). Ledger-Kontakt eng auf action='posted' (D3), Auslegung dem Kritiker offengelegt.
3. Statuswechsel zwischen Erkennung und Send (Kandidat wird Partner) und Kosten pro Nachricht: Partner-Status wird unter dem Doppelsend-Lock erneut geprüft (analog dem doppelten Kanal-Limit im Anlass-Pfad). Der zusätzliche DB-Join läuft erst nach Judge-Drossel und Semaphore und nur für Nachrichten ab 25 Zeichen ohne Befehl, in Partnerkanälen; damit bleibt die Chat-Pipeline unbelastet.
4. INV-03-Regression im Anlass-Pfad durch die Einhängung: Gegenmaßnahme: Partner-Zweig strikt vor `pitch_user_limit_ok` (L777) einsetzen und mit `return` beenden; der Anlass-Code ab L777 bleibt für Nicht-Kandidaten unverändert; die bestehenden Anlass-Tests laufen ungeändert weiter (INV-06).
5. Doppelter Kontakt Partner-Pitch gegen Outreach-Kette: der eigene Ledger-Insert (action='posted') plus das Lifetime-Limit (pfad='partner') sperren jede zweite Ansprache derselben user_id; die Outreach-Kette schließt Personen mit Ledger-'posted' bereits über ihren Detector aus (Research), sodass beide Ketten dieselbe Person nur einmal treffen.

## Umsetzungsstatus

- M1 fertig: Gerüst (PARTNER_PITCH_SYSTEM_PROMPT, PartnerPitchContext, build_partner_pitch_text, Trait PartnerPitchGen plus FireworksPartnerPitchGen in promo_pitch.rs; Feld partner_pitch_gen, Default, Setter in promos.rs), Test-DDL erweitert, MockPartnerPitchGen und Regressionstest partner_kandidat_bekommt_partner_pitch angelegt. Roter Lauf bewiesen (siehe EVIDENCE, Abschnitt "Rote Baseline M1").
- M2 fertig: partner_candidate, partner_user_limit_ok, partner_channel_limit_ok, partner_daily_limit_ok, record_partner_ledger; sqlx-Cache mit fünf neuen Query-Dateien.
- M3 fertig: Partner-Zweig run_partner_pitch nach der Startverzögerung, vor dem Anlass-User-Limit, mit return (REQ-07). log_partner_reject (pfad=partner), Enum PitchCardKind, send_card um kind und candidate_hint erweitert, chat_wiring.rs DiscordPitchReviewSink mit Titel-Verzweigung und Kandidaten-Hinweiszeile. Regressionstest jetzt grün. Abweichung vom Plan-Wortlaut: bei kein_text/Filter/Injection wird die Judge-Drossel NICHT freigegeben (LLM war schon aufgerufen), nur bei den Partner-Limit-Gates vor dem LLM, konsistent zum Anlass-Pfad und zu REQ-05 (Drossel unverändert).
- M4 fertig: sechs weitere Inline-Tests (nicht_kandidat_geht_in_anlass_pfad, partner_wird_nicht_gepitcht, zweiter_partner_pitch_derselben_user_id_geblockt, partner_tageslimit_fuenf, promo_disabled_sendet_keinen_partner_pitch, partner_filter_verwirft_link) plus FAQ-Abschnitt in faq-werbung.md. Voller Lauf tb-chat 755 passed, tb-bot 292 passed, 0 rot.
