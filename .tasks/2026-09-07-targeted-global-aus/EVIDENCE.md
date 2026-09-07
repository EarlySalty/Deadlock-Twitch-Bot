# Evidence: Timer-Targeted-Pitch (targeted_global) abschalten

## Befund (2026-09-07)

Der Abschalt-Commit 449d84ea (Release 83f069b4, 06.09 13:56) kapt nur den
targeted_user-Pfad. Die Commit-Beschreibung sagt selbst: „Der periodische
Kanal-Pitch (targeted_global) und der Anlass-Pitch bleiben aktiv."

`twitch_promo_pitch_log`, gesendet, 06.09 12:00 bis 07.09 15:00:
- `targeted_global`: 8 Sends, u. a. 07.09 13:50 marcymcwhy „wer mehr deadlock
  will, findet die community", 06.09 22:14 einsbezi „auch wer bald hochzeit
  feiert, ist hier in der community gut aufgehoben" — erfundener Bezug, alle
  15 Minuten Kanal-Cooldown, das bemängelte Verhalten.
- `targeted_user`: letzter Send 06.09 11:19 (vor dem Fix), danach keiner —
  der alte Fix wirkt.
- `periodic` (Einladung, 90-min-Cooldown), `anlass`, `partner`, `gezielt`
  (Antworten auf echte Nachrichten): unverändert, laut Auftrag nicht Teil
  dieser Abschaltung.

## Änderung

`rust/crates/tb-chat/src/promos.rs`:
- `process_due_channel` ruft nur noch `maybe_send_promo_with_stats`
  (Periodik) und bei Leerlauf `maybe_send_viewer_spike_promo`; der
  targeted_global-Zweig ist raus, der Slot fällt auf die Einladung zurück.
- Gelöscht: `maybe_send_targeted_promo`, `TargetedState` samt Feld und Init,
  `CHANNEL_TARGETED_COOLDOWN_SEC`, `get_active_chatters` (aufruferlos).
- Tests: Gate-Test umbenannt (Verhalten gleich); drei Direktaufruf-Tests des
  gelöschten Pfads entfernt; neuer Regressionstest
  `send_promo_if_due_laeuft_ohne_targeted_global` (Sende-Slot läuft, genau
  ein periodic-Announcement, kein @-Pitch, keine targeted-Zeilen im Log).

## Testnachweis

Siehe REVIEW.md, wird nach dem Lauf ergänzt.

## Deploy

Merge nach main, Release-Bau nach Memory twitch-release-deploy-weg
(isolierter Build, eingefrorener Clone unter /opt/deadlock/twitch/builds/<sha>,
install-twitch-release, Neustart deadlock-twitch-bot-rust und
deadlock-twitch-dashboard-rust), Live-Prüfung über Pitch-Log.
