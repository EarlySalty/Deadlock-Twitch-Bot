# Contract: Timer-Targeted-Pitch (targeted_global) abschalten

status: aktiv
datum: 2026-09-07
klasse: niedrig
repo: Deadlock-Twitch-Bot

## Ziel

Der Abschalt-Commit 449d84ea vom 06.09 hat nur den targeted_user-Pfad gekappt;
der periodische Kanal-Pitch (targeted_global) läuft weiter und sendet pro
Partnerkanal alle 15 Minuten ein Announcement mit erfundenem Anschluss
(Pitch-Log 07.09: „auch wer bald hochzeit feiert, ist hier gut aufgehoben").
Der Nutzer will diese zufälligen Engagement-Nachrichten komplett weg.

## Anforderungen (user-sichtbares Verhalten)

- REQ-01 Der Timer-Slot sendet nie wieder ein Announcement über den
  targeted_global-Pfad; `twitch_promo_pitch_log` bekommt keine neuen Zeilen mit
  `pfad = 'targeted_global'` (und `'targeted_user'` bleibt weg).
- REQ-02 Der Promo-Slot fällt auf die aktivitätsbasierte Community-Einladung
  (pfad `periodic`, 90-Minuten-Overall-Cooldown) und den Viewer-Spike zurück;
  beide bleiben unverändert.
- REQ-03 Der tote Code (Funktion, TargetedState, Kanal-Cooldown-Konstante,
  `get_active_chatters` ohne Aufrufer) wird entfernt, nicht nur abgeschaltet.

## Invarianten (darf sich nicht ändern)

- INV-01 Anlass-Pitch (`anlass`), Partner-Pitch (`partner`) und gezielter
  Zuschauer-Pitch (`gezielt`) bleiben unverändert; `TargetedPitchContext` und
  `build_targeted_pitch_text` bleiben (nutzt `gezielt`).
- INV-02 Partner-Gate, Kanal-Allowlist, Werbefrei-Plan, Suppression-Guard,
  Stream-Start-Verzögerung und Cooldowns des Periodik-Pfads bleiben unverändert.
- INV-03 Keine Migration, keine neue Tabelle, keine ENV-Config, kein neues LLM.
- INV-04 Keine Änderung außerhalb `rust/crates/tb-chat/src/promos.rs` und
  `.tasks/2026-09-07-targeted-global-aus/`.

## Nicht-Ziele

- Keine Änderung am Lurker-Tax-Pfad.
- Kein Umbau der Pitch-Limits oder des Zuschauer-Registers.
