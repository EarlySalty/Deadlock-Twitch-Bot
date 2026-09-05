status: aktiv
datum: 2026-09-05

# Review Partner-Pitch an streamende Zuschauer (Stufe 4, adversarial)

## Ergebnis

FREIGABE. Kein blockierender Mangel. Drei nicht blockierende Punkte (zwei "sollte", einer Hinweis) unten.

## Vorbedingungen geprüft

- `git fetch origin` gelaufen, origin/main auf b00ba18d.
- Rebase konfliktfrei: `git merge-tree --write-tree origin/main HEAD` liefert einen sauberen Tree (88297fd2, keine Konfliktmarker).
- Keine neuen "minimax"-Bezeichner im Diff unter `rust/` (`git diff origin/main...HEAD -- rust/ | grep -i minimax` leer). Die Umbenennung auf llm_usage in origin/main wird nicht berührt.
- Graphify-Graph fehlt in diesem Worktree (`graphify-out/graph.json` nicht vorhanden), Pflichtversuch erfolgt, danach gezielt im Code nachgelesen.
- Scope: Diff berührt nur promos.rs, promo_pitch.rs, chat_wiring.rs, faq-werbung.md, `.sqlx/`, `.tasks/`. Alles im erlaubten Bereich. Keine Migration, kein Schema-Snapshot-Diff (INV-05 gehalten). commands.rs, tb-scout, lib.rs, pipeline.rs unberührt.

## Testlauf

`cd rust && SQLX_OFFLINE=true /home/nathanael/.cargo/bin/cargo test -p tb-chat partner`
Eine DB war erreichbar, die db_tests liefen echt (kein Skip). Ergebnis: lib 21 passed, 0 failed. Alle sieben neuen Partner-Tests grün: partner_kandidat_bekommt_partner_pitch, partner_pitch_schreibt_ledger_und_review_karte, nicht_kandidat_geht_in_anlass_pfad, partner_wird_nicht_gepitcht, zweiter_partner_pitch_derselben_user_id_geblockt, partner_tageslimit_fuenf, promo_disabled_sendet_keinen_partner_pitch, partner_filter_verwirft_link. Keine neue Rotfärbung, keine neuen tb-chat-Warnungen (nur vorbestehende aes_gcm-Deprecation aus tb-crypto).

## REQ einzeln

- REQ-01 Erkennung nur über Twitch-User-ID: erfüllt. `partner_candidate` (promos.rs:1205) matcht ausschließlich `s.twitch_user_id = $1`, kein Login-Vergleich als Erkennung. Ausschlüsse als NOT EXISTS: twitch_partners (Zeile 1212), twitch_scout_pitch_blacklist (1213), twitch_partner_outreach (1216), twitch_scout_pitch_ledger mit `action='posted'` (1219). Broadcaster-Ausschluss in on_message_pitch (promos.rs:755). Die Ausschluss-Subqueries matchen zusätzlich per LOWER(streamer_login), rein additiv zur user-id-Erkennung, INV-08 nicht verletzt.
- REQ-02 Auslöser: erfüllt. promos.rs:752 (`!`-Präfix oder <25 Zeichen -> return), promos.rs:755 (Broadcaster oder leere chatter_id -> return). Kein Timerpfad; Einhängung sitzt in on_message_pitch, das nur auf Nachricht läuft.
- REQ-03 Text und Prompt: erfüllt. Eigener `PARTNER_PITCH_SYSTEM_PROMPT` (promo_pitch.rs:51), Weg über `build_partner_pitch_text` -> `tb_llm::complete(USE_CASE, ...)` (promo_pitch.rs:451), also derselbe Use-Case promo_pitch/Deepseek (INV-02). Prompt fordert: erst auf das Gesagte eingehen, dann dritte Person, konditional "wenn du öfter Deadlock streamst", ehrliche Mechanik (Raids, Chat-Schutz), kein Link, keine Superlative, keine Mitgliederzahlen, kein Gedankenstrich, kein "komm auf"/"join"/"tritt bei", keine Emojis außer :). Harte Filter greifen: `pitch_filter_reject` (promos.rs:1076) und `pitch_injection_reject` (promos.rs:1087) auf die Antwort, identisch zum Anlass-Pfad.
- REQ-04 Limits: erfüllt. Lifetime je User: `partner_user_limit_ok` (promos.rs:1243) blockt, sobald `MAX(sent_at)` über pfad='partner' gesetzt ist, ohne Zeitfenster, plus pending-Schutz. Je Kanal je Stream: `partner_channel_limit_ok` (promos.rs:1270) blockt bei `partner_count >= 1` seit stream_start. Fünf pro Tag: `partner_daily_limit_ok` (promos.rs:1305) `count < 5` über rollende 24h. Cooldown-Kopplung: `mark_promo_sent(..., "partner_pitch", ...)` (promos.rs:1043) belegt den Promo-Cooldown wie der Anlass-Pitch; zusätzlich koppelt die 600s-Sperre in partner_channel_limit_ok an anlass+partner.
- REQ-05 Gates: erfüllt. Partner-Zweig sitzt nach allen REQ-05-Gates und vor `pitch_user_limit_ok`: Semaphore (promos.rs:764), Judge-Drossel-Reservierung (769), Partnerkanal (774), Allowlist (783), Werbefrei promo_disabled (788), Outbound-Suppression (793), Startverzögerung (798), dann Kandidatenprüfung (804). Der Semaphore-Permit bleibt über run_partner_pitch gehalten (lokale Var in on_message_pitch). Test promo_disabled_sendet_keinen_partner_pitch belegt INV-01/Werbefrei.
- REQ-06 Sichtbarkeit: erfüllt. Gesendet: insert_pitch_log_pending mit pfad="partner" (promos.rs:1015) plus mark_pitch_log_sent. Generierungsstufe verworfen (no_text, link/filter, injection, und die Re-Checks unter dem Lock): `log_partner_reject` mit pfad="partner". Review-Karte mit Kandidatenhinweis (Login, Datum letzte Deadlock-Session) und PitchCardKind::Partner (promos.rs:1055), Felder in chat_wiring.rs:1946 über neutralize_pitch_field neutralisiert. Ledger-Eintrag `record_partner_ledger` schreibt action='posted', streamer_login=Kandidat-Login, twitch_user_id=chatter (promos.rs:1322); passt genau auf die Ausschlussbedingung, sperrt die zweite Ansprache derselben Person.
- REQ-07 Ausschluss zum Anlass-Pitch: erfüllt. Bei Kandidaten-Treffer läuft run_partner_pitch und die Funktion endet mit `return` (promos.rs:815), der Anlass-Code ab pitch_user_limit_ok wird für Kandidaten nie erreicht. Test nicht_kandidat_geht_in_anlass_pfad belegt den Gegenfall (Generator 0 Aufrufe, Anlass sendet).
- REQ-08 FAQ: erfüllt und wahrheitsgemäß. faq-werbung.md:24 nennt: nur streamende Nicht-Partner, nur als Antwort auf eigene Nachricht, kein Link/kein Aufruf, Deckel (einmal je Person, einer je Stream, wenige pro Tag), Werbefrei schaltet ab. Deckt sich mit dem Code.

## INV einzeln

- INV-01 erfüllt (promo_disabled-Gate vor dem Partner-Zweig, Test grün).
- INV-02 erfüllt (build_partner_pitch_text über tb_llm::complete, Use-Case promo_pitch).
- INV-03: erfüllt mit Anmerkung. Der Anlass-Zweig ab promos.rs:817 ist für Nicht-Kandidaten byte-gleich. Geändert wurden zwei Anlass-Queries auf `pfad IN ('anlass','partner')`: pitch_channel_limit_ok (vertraglich durch REQ-04/REQ-05 gefordert) und pitch_user_limit_ok (nicht gefordert, siehe Punkt 1). Bestehende Anlass-Tests unverändert grün, da ohne Partner-Zeilen identisch. Targeted-Pitch und periodische Promo im Diff nicht angefasst.
- INV-04 erfüllt: tb-scout, twitch_partner_outreach, outreach_shadow.rs unverändert; nur lesend plus der eine erlaubte Ledger-Eintrag.
- INV-05 erfüllt: keine Migration, keine neue Tabelle, Schema-Snapshot unverändert.
- INV-06 erfüllt: kein Test gelöscht oder abgeschwächt; RecordingReviewSink/send_card nur mechanisch um kind+hint erweitert.
- INV-07 erfüllt: commands.rs unberührt, keine ENV-Config, kein neues Secret.
- INV-08 erfüllt: Erkennung rein über twitch_user_id.

## Korrektheit

- Kandidaten-SQL: Zeitfenster 60 Tage korrekt, `LOWER(s.game_name)='deadlock'` fängt Groß-/Kleinschreibung, keine JOINs (eine Tabelle), NULL-Fälle sicher: Zeilen mit NULL twitch_user_id matchen nicht (gewollt), `MAX(started_at) AS "last_session!"` ist durch das WHERE started_at >= NOW()-60d immer non-null. Klammerung der Ledger-Bedingung `(login OR user_id) AND action='posted'` korrekt (promos.rs:1219).
- Limit-Queries: pitch IN ('anlass','partner') an den gekoppelten Stellen korrekt. Fail-closed durchgängig: alle drei Partner-Limit-Checks und partner_candidate liefern bei DB-Fehler block bzw. None (kein Pitch).
- Reihenfolge/Lock: LLM-Aufruf (partner_pitch_gen) liegt vor der Sendelock-Akquise (promos.rs:1071 vor 1096), also kein LLM unter dem Lock. Unter dem Lock werden partner_candidate, user- und channel-Limit erneut geprüft. Semaphore und Judge-Drossel liegen davor und gelten unverändert; auf allen frühen Abbruchpfaden wird die Drossel via pitch_judge_throttle_release zurückgerollt.
- Prompt-Injection über den Kandidaten-Login: reply-Prüfung via pitch_injection_reject(&reply, target_login); Kontext als reine JSON-Daten, Systemprompt behandelt Text als Zitat. Kandidat-Login stammt aus der DB (streamer_login), in der Karte neutralisiert.
- Tageslimit-Zeitzone: rollende 24h statt Kalendertag. Vertragswortlaut "fünf pro Tag" damit eher strenger erfüllt, kein Fehler.

## Prompt-Qualität als Leser

Als kleiner Streamer liest sich die Vorgabe ehrlich und nicht werblich: erst echte Reaktion auf das Gesagte, dann bedingter Hinweis in dritter Person, ehrliche Mechanik ohne Superlative, ohne Mitgliederzahlen, ohne Link, ohne Beitrittsdruck. Kleinschreibung und Ton passen zum Chat. Keine Gedankenstriche, echte Umlaute in den Testfixtures. Emojis auf :) begrenzt.

## Nicht blockierende Punkte

1. sollte: pitch_user_limit_ok (Anlass-User-Limit, promos.rs, geänderte Query auf `pfad IN ('anlass','partner')`) koppelt den Anlass-User-Deckel an gesendete Partner-Pitches, ohne dass ein REQ das verlangt. Fehlbild: milde INV-03-Spannung, ein Nutzer, der einen Partner-Pitch bekam und danach nicht mehr Kandidat ist, wird im Anlass-Fenster zusätzlich gesperrt. Praktisch harmlos (kein Test rot, im selben Message-Fall nie beides). Vorschlag: entweder diese Query auf `pfad='anlass'` zurücksetzen, damit der Anlass-User-Deckel strikt unverändert bleibt, oder die gewollte Anti-Doppelansprache im Contract als Amendment festhalten. pitch_channel_limit_ok bleibt gekoppelt, das ist durch REQ-04 gedeckt.
2. sollte: partner_daily_limit_ok (promos.rs:1305) wird nicht unter dem Sendelock erneut geprüft, und der Sendelock ist je Kanal. Fehlbild: zwei zeitgleiche Partner-Pitches in verschiedenen Kanälen können beide count=4 sehen und zusammen die Fünf überschreiten. Bei fünf pro Tag und Judge-Drossel real sehr selten. Vorschlag: entweder als bewusst akzeptiert notieren oder die Tageszählung ebenfalls im finalen Re-Check-Block mitführen.
3. Hinweis: Gate-Blocks vor der Generierung (Werbefrei, Limits an promos.rs:1108 bis 1122) werden nicht in twitch_promo_pitch_log geschrieben, nur Generierungsstufen-Verwürfe. Der Test promo_disabled_sendet_keinen_partner_pitch erwartet das ausdrücklich (count=0). REQ-06 sagt wörtlich "gesendet oder verworfen"; die Auslegung deckt sich exakt mit dem Anlass-Pfad (dort auch keine Gate-Block-Logs), daher konsistent und vertretbar. Nur zur Kenntnis für den Merge.

## Fazit

Alle REQ und INV belegbar erfüllt, Tests grün, Rebase konfliktfrei, Scope sauber, keine minimax-Bezeichner. FREIGABE. Punkt 1 und 2 sind vor dem Merge zu entscheiden (Zurückbau oder bewusste Annahme), blockieren aber nicht.
