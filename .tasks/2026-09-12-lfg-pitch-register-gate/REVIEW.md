# Review Runde 1: lfg-pitch-register-gate

Urteil: FREIGABE

Alle Arbeitsschritte und beide Fertig-Kriterien sind erfuellt. Der Pfad ist
fail-closed: ohne Register liefert `decide` `RegisterFehlt`, bei leerer
`chatter_user_id` und bei Broadcaster/Mod ein Kurzschluss vor dem Register, bei
`gate`-Reject `RegisterReject`. Der Judge wird in all diesen Faellen nicht
aufgerufen (keine LLM-Kosten). `gate` selbst blockt bei DB-Fehlern
(session/exclusion/ensure_current) durchgehend mit Reject, also fail-closed.
Wiring baut das `ZuschauerRegister` einmal aus `member_relay` und gibt dieselbe
`Arc` an PromoEngine und LfgPitchResponder; ohne `member_relay` bekommen beide
`None`, der LFG-Pfad bleibt still und loggt `register_fehlt` auf debug. Die
zweite `LfgPitchResponder::new`-Stelle (chat_wiring.rs:3185) liegt in
`chat_notification_tests` und bekommt korrekt `None`.

Erkennung: `classify_lfg("Drifter und noch wer")` ist false (`noch wer` matcht
jetzt nur `search_lfg_re`, ohne Objekt-Treffer greift die UND-Bedingung nicht;
`wer` ist aus `join_lfg_re` entfernt). `noch wer bock auf ranked?` bleibt true
(search plus Objekt `ranked`), `wer zockt noch?` bleibt true ueber
`join_lfg_re`. Kein bares `wer` als neues Signal.

Judge-Kontext: `LfgJudgeInput` traegt `recent_chat`, gefuellt aus
`load_recent_channel_messages_before` mit `LIMIT 8` und Ausschluss der aktuellen
Nachricht per `message_id IS DISTINCT FROM $3`, Format `login: text`. Prompt-Regel
fuer Antworten, Aufzaehlungen und Kommentare ist ergaenzt.

## Maengel (blockierend)

Keine.

## Hinweise (nicht blockierend)

- H1: rust/crates/tb-chat/src/lfg_pitch.rs:190 Der Chatverlauf geht als
  `login: text`-Zeilen unter der Ueberschrift "Chatverlauf vor der Nachricht"
  in den User-Prompt, ohne harte Datengrenze (Fence oder Delimiter). Eine
  fruehere Chat-Nachricht mit Instruktions-Text ist eine schwache
  Injection-Flaeche fuer den naechsten Judge-Aufruf. Wirkung gering (Judge gibt
  nur yes/no/unsure, Folge ist eine einzelne Einladung), daher kein Blocker.
- H2: rust/crates/tb-chat/src/lfg_pitch.rs:665 Der Verlauf kommt aus
  `ORDER BY message_ts DESC`, also neueste Zeile zuerst. Fuer die Judge-Regel
  "Antwort auf eine Frage im Verlauf" waere aelteste-zuerst natuerlicher.
  Funktional unkritisch.
- H3: rust/crates/tb-chat/src/promos.rs Von rund 230 geaenderten Zeilen sind
  nur die zwei `use`-Zeilen, `load_recent_channel_messages_before` (rund 40
  Zeilen) und `impl RecentChatPort` (rund 7 Zeilen) inhaltlich. Der Rest ist
  reines rustfmt-Reflow unveraenderter Funktionen und Tests. Das ist laut
  Auftrag (Formatieren der eigenen Dateien) erlaubt, vergroessert aber die
  Merge-Konfliktflaeche gegen parallele promos.rs-Aenderungen. Kein Blocker,
  aber beim Merge im Auge behalten.
- H4: rust/crates/tb-chat/src/lfg_pitch.rs:618 Die harten Kurzschluesse
  (leere Id, Broadcaster/Mod) melden `SilentReason::RegisterReject` mit `grund`
  `chatter_id_fehlt` bzw. `broadcaster_mod_bot`, obwohl es kein Register-Reject
  ist. Der Log unterscheidet das ueber `grund`, funktional korrekt, nur die
  Benennung des `SilentReason` mischt zwei Ursachen.
- H5: Testzahl. Der Diff bringt vier neue Unit-Tests im Modul plus zwei
  Integrationstests in tests/lfg_pitch.rs, zusammen sechs. Der Worker meldet
  816/4/5 auf 820/4/5, also plus vier. Die Differenz kommt vermutlich aus
  getrennten Test-Binaries; ohne eigenen Testlauf hier nicht verifiziert.

## Zwillinge (ausserhalb des Pakets)

- Z1: rust/crates/tb-chat/src/commands.rs `InviteQuestionResponder` schickt bei
  Invite-Fragen einen Discord-Link an Chatter, ohne das Register zu fragen. Laut
  Auftrag ausdruecklich unveraendert, hier nur als Zwilling gemeldet.
- Z2: Periodische Einladung und `!invite` schicken ebenfalls Einladungen ohne
  Register-Gate. Auftrag schliesst sie aus, kein Mangel dieses Pakets.
