# Bericht: Admin-Grenzen fuer Community-Werbung

## TLDR

Der Community-Kanal umging die eingestellten Chat-Grenzen: der Overall-Timer
sendete auch bei ruhigem Chat. Behoben. Community, erster Versand und
Viewer-Spike laufen jetzt durch dieselbe Aktivitaets-Pruefung wie alle Kanaele.
Zusaetzlich der vom unabhaengigen Review gemeldete P2-Reset-Fehler behoben.
Checks gruen, Self-Gate ALLOW. Kein Merge, kein Deploy durch mich.

Basis: b440e84a (fix). Branch: fix/promo-admin-grenzen. Finaler Code-HEAD: ce9e7450.

## Was war der Bug

- `send_promo_if_due`-Filter liess Community ueber `community_channel || activity_ready`
  durch, d.h. der 20-min-Overall-Timer reichte, Chat-Aktivitaet egal.
- `process_due_channel` schickte Community ueber einen eigenen Sendepfad
  (`community_timer`) ganz ohne Aktivitaets-Gate, andere Kanaele ueber
  `maybe_send_promo_with_stats`.
- Ergebnis: drei Werbungen 20:08 / 20:29 / 20:49 bei fast ruhigem Chat, exakt im
  20-min-Takt.

## Geaenderte Regeln (tatsaechlich geprueft)

Alle Werte kommen unveraendert aus den gespeicherten Timern (`state.timers`), keine
neue harte Schwelle erfunden.

1. **Faelligkeits-Filter** (`send_promo_if_due`): verlangt jetzt fuer ALLE Kanaele
   `overall_ready && activity_ready && stream_start_delay_ok`. Community-Ausnahme
   entfernt.
2. **Sendepfad vereinheitlicht** (`process_due_channel`): Community laeuft wie alle
   anderen durch `maybe_send_promo_with_stats` (Allowlist, Stream-Start, Overall,
   Aktivitaet, Attempt-Reservierung). Nur die geladenen Timer-Werte unterscheiden
   sich, keine Sonderlogik mehr.
3. **Erster Versand** (`promo_activity_ready_inner`): die new_chatters-Grenze greift
   jetzt auch beim ersten Versand (frueher nur wenn `last_promo_sent` gesetzt war).
   Ohne gesehen-Basis zaehlen alle aktiven Chatter als neu, die eingestellte
   Schwelle bleibt damit wirksam. Bei `new_chatters == 0` (Migrations-Seed) bleibt
   der Schritt uebersprungen.
4. **Viewer-Spike** (`maybe_send_viewer_spike_promo`): prueft jetzt `activity_ready`
   (min_messages, Aktivitaetsfenster, neue Chatter) statt nur einer einzelnen
   Roh-Nachricht. Keine alternative Werbeschleife umgeht die Grenzen mehr.
5. **P2-Reset-Fehler** (`mark_promo_sent` / `update_seen_chatters_inner`): der
   Versand markiert jetzt dieselbe Union (Aktivitaets-Bucket UNION Session-Viewer),
   die `get_new_chatters_in_window_inner` als potenziell neu zaehlt. Vorher wurde
   nur der Bucket markiert, API-only-Viewer zaehlten so bei jeder Folgewerbung
   erneut als neu. API-Viewer-Definition unveraendert (lowercase auf beiden Seiten).
6. **Dashboard-Text** (`PromoTimers.tsx`): die Zeile, die die verworfene Ausnahme
   erklaerte, sagt jetzt wahrheitsgemaess, dass auch der Timer erst bei erreichten
   Chat-Schwellen sendet.

## Bewusst NICHT geaendert

- `build_promo_text` (Community-Textrotation / redaktionelle Ankuendigungstexte):
  waehlt nur den Inhalt, kein Timer-Gate. Bleibt.
- `pitch_channel_limit_ok` / `partner_channel_limit_ok`: die community-spezifischen
  Werte (`pitch_max_per_stream`, persoenliche Einladungen) sind gespeicherte
  Einstellungswerte, keine Timer-Ausnahme. Der community-Zweig in
  `pitch_channel_limit_ok` ist sogar strenger (zusaetzlicher Overall-Check). Bleibt.
- Eigene Kanalwerte (Overall 20 min, Aktivitaet 20-30 min, minMessages 8,
  newChatters 2, ...) unveraendert.

## Checks (alle mit rustup-cargo 1.97.1, Debug, im Worktree; kein Release-Build)

- Finaler `cargo check -p tb-chat` eindeutig auf HEAD
  ce9e7450b795de17f5e21ed0e5aaf4361cdb198a: exit 0. Log: /tmp/final_check.log
- Finaler `cargo clippy -p tb-chat --all-targets` auf demselben HEAD: exit 0.
  Log: /tmp/final_clippy.log. Verbleibende 2 Warnungen (`dead_code`,
  `type_complexity` in Test-Mocks) sind preexisting und nicht von dieser Aenderung.
- Test-Binaries (`cargo test -p tb-chat --no-run`) auf ce9e7450: exit 0.
- Gezielte Tests (exit 0 je Lauf, 0 FAILED, per Namen verifiziert):
  - `promos::db_tests::folgewerbung_ohne_neuzugang_bleibt_gesperrt` ... ok  (neuer Regressionstest, P2)
  - `promos::db_tests::community_timer_persistenz_farbauswahl_und_sendpfad` ... ok  (angepasst: ohne Chat 0 Sends, mit erreichten Schwellen 1 Send)
  - `promos::tests::community_timer_und_admin_standard_sind_unabhaengig` ... ok
  - `promos::tests::activity_ready_true_bei_allen_schwellen` ... ok
  - `promos::tests::activity_ready_fehlschlag_bei_zu_wenig_msgs` ... ok
  - `promos::tests::activity_ready_fehlschlag_bei_zu_wenig_raw_msgs` ... ok
  - Summe: 6 passed, 0 failed, 0 ignored. DB-Tests ueber isolierte lokale
    PostgreSQL-16-Instanz (TestPostgres, kein Docker/geteilter Port).

### fmt

`cargo fmt -p tb-chat -- --check` meldet Abweichungen ueber die GANZE Crate, auch
in von mir unberuehrten Dateien (z.B. `api.rs`). Ursache: das committete Repo ist
unter stable-1.97 nicht fmt-sauber (aeltere rustfmt-Version beim Commit). Ein
`cargo fmt` haette 18 fremde Dateien umformatiert = verbotenes kosmetisches
Nebenthema. Daher KEINE repo-weite fmt-Aufraeumung; meine Aenderungen folgen dem
Umgebungsstil. Die Altformatierung ist die Basis, nicht ein durch diese Aenderung
verursachter Zustand.

## Self-Gate (review_gate ueber gate_hook.py, Basis b440e84a, HEAD ce9e7450)

Urteil: **ALLOW, kein Merge-Blocker.** Drei NIT-Funde, alle bewertet und bewusst
nicht geaendert (kein weiterer Code-Churn, unabhaengiger intent_review bestaetigt
P2 als behoben):

1. Spike-Pfad feuert jetzt nur im Band 2-8 min alter Aktivitaet (braucht
   `chat_silent` UND `activity_ready`). So gewollt: kein Promo in wirklich toten
   Chat. Konstanten machen ihn enger, nicht unerreichbar.
2. `get_current_session_viewers` liefert bei DB-Fehler leere Menge ohne Log.
   Preexisting; bei Fehler koennte der Doppelzaehl-Bug still wiederkehren. Als NIT
   dokumentiert, kein Eingriff in Bestandscode innerhalb dieses Pakets.
3. `update_seen_chatters_inner` markiert den ganzen Aktivitaets-Deque, die
   Zaehlseite nur den fenstergefilterten Teil. Uebermarkierung ist ungefaehrlich
   (aeltere Chatter zaehlen ohnehin nie als neu).

## Pflichtzeilen

MERGEPROTOKOLL[MS-1]: 4 Git-Schritte einzeln (fetch, merge-base-Pruefung, rebase --onto origin/main, Commits) | Anläufe: 0 (kein Merge nach main; Merge/Deploy macht die Hauptsession) | Gate: ALLOW, kein Merge-Blocker
TESTNACHWEIS[TW-1]: 6 passed, 0 ignored | Baseline: keine Altfehler behauptet (gezielte Laeufe, volle Suite ressourcenschonend nicht gefahren)

## Offen fuer die Hauptsession

Abnahme (unabhaengige Rust-/Intent-Pruefung laeuft parallel), dann Merge nach main,
Deploy auf Basis b440e84a inkl. Overlay, Service-Restart, Live-Pruefung im
Dashboard (PromoTimers) und im Chat-Verhalten von dach_lock.
