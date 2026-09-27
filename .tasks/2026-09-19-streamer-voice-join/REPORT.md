# Twitch-Prüfbericht: direkter Voice-Beitritt

## Ergebnis

Mitspiel-Anfragen werden vor Spielzugangs-FAQ und Werbe-Pitch behandelt. Der vorhandene LFG-Judge wird einschließlich zentraler Modellwahl und Chatverlauf wiederverwendet. Ein bestätigter Mitspielwunsch löst über die ID-Verknüpfung des aktiven, frisch live erkannten Partners die neue Broker-Aktion im Discord-Bot aus. Erst deren bestätigte Antwort ergibt einen Voice-Link oder den Hinweis auf einen zusätzlichen Platz.

Keine neue Konfiguration, Migration oder Modellauswahl. Bestehende Discord-Mitglieder und Moderatoren werden von dieser angeforderten Hilfe nicht durch Werbe-Zielgruppenregeln ausgeschlossen. Discord-Privaträume werden nicht geöffnet. Doppelte Nachrichten und zeitgleiche Anfragen verursachen keine doppelten Antworten.

## Prüfungen

- `cargo clippy -p tb-bot -p tb-chat -p tb-transport-discord --all-targets --jobs 2`: erfolgreich. Damit sind auch produktive Verdrahtung und Bot-Binary typgeprüft. Warnungen in unveränderten Altbereichen bleiben bestehen.
- `cargo test -p tb-chat -p tb-transport-discord --lib voice --jobs 2`: 8 Chat-/Pipeline-Tests und 2 Broker-Client-Tests bestanden, keine Fehler.
- `cargo test -p tb-transport-discord --lib --jobs 2`: alle 29 Tests bestanden.
- `cargo test -p tb-chat --lib invite_question::tests --jobs 2`: alle 35 Tests bestanden.
- `cargo test -p tb-chat --lib lfg_pitch::tests --jobs 2`: alle 24 Tests bestanden.
- Die Pipeline-Regression sendet tatsächlich über den ChatApi-Testadapter genau eine Voice-Antwort und unterdrückt sowohl die Spielzugangsantwort als auch eine doppelte Zustellung.
- Der Screenshot-Satz und kurze deutsche/englische Anschlusswünsche sind abgedeckt. „Ich suche Mitspieler“ ist kein eigener Anschlusswunsch. Eine nachfolgende Meldung „Ihr seid voll“ wird nur für kürzlich tatsächlich eingeladene Zuschauer behandelt.

## Gesamt-Suite und Baseline

Der erste Lauf der gesamten Chat-Bibliothek ergab 878 bestanden, 8 fehlgeschlagen, 3 ignoriert. Ein eigener Regex-Fehler bei dem Wort „Mitspieler“ wurde korrigiert und mit allen neuen Voice-Tests erneut grün geprüft.

Sieben übrige Fehler liegen außerhalb der Änderung:

- `promos::tests::periodischer_promo_text_traegt_invite_am_ende_ohne_strich`: bestehendes `unwrap(None)` in `promos.rs:3457`. Im separaten, unveränderten Baseline-Worktree auf `0c053e46` exakt reproduziert: derselbe Test schlägt an derselben Stelle fehl.
- Sechs Tests aus `standard_replies` verlangen ausdrücklich `TB_TEST_DATABASE_URL`, das in dieser Testumgebung nicht gesetzt ist. Diese Dateien wurden nicht verändert.

Keine behauptete grüne Gesamtsuite und kein Eingriff in produktive Datenbanken für Tests. Nach der Baseline-Gegenprobe wurden die Feature-Tests aus dem Feature-Worktree erneut gebaut und ausgeführt.

## Veröffentlichung

Feature-Implementierung und lokale Prüfung; unabhängiges Review, Merge und Deployment sind noch nicht abgeschlossen. Zuerst muss der Discord-Broker bereitstehen, danach der Twitch-Client. Es wurde kein produktiver Voice-Kanal verändert und noch kein Dienst für diese Änderung neu gestartet.
