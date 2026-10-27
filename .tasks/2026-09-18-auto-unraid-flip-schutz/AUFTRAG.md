# Auto-Unraid bei Flip + Flip-Schutz

## Problem (vom Nutzer gemeldet)
Der Auto-Raid feuert bei `stream.offline`. Kommt der Stream innerhalb der
Raid-Countdown-Zeit (~90 s) wieder online (Flip), bricht der Bot den eigenen
Raid NICHT ab. `/unraid` (Helix `DELETE /raids`) existiert im Code, ist aber nur
an den Blacklist-Guard verdrahtet, nicht an das Go-Live-Event.

## Datenlage (twitch_analytics DB, ~18.01.-17.09.2026)
- 1669 erfolgreiche Auto-Raids (`reason=auto_raid_on_offline`).
- **120 davon (~7,2 %)** wurden vom selben Quellkanal innerhalb von 90 s durch
  einen Stream-Neustart gefolgt = genau die Faelle, die ein Auto-Unraid
  abgefangen haette. 147 innerhalb 5 min, 157 innerhalb 10 min.
- Wiederholungs-Flips haeufen sich bei wenigen Kanaelen: zro_dl 25, earlysalty 23,
  trippymelo 10, dragskope 7, timosius 7. Der 10-min-Wiederholungsschutz ist
  dadurch gerechtfertigt.

## Soll (zwei Teile)

### Teil 1: Auto-Unraid bei Flip
Bei `stream.online` des Quellkanals (`on_stream_went_live` in
`rust/bin/tb-bot/src/eventsub_hooks.rs`):
1. Im geteilten `Arc<Mutex<PendingRaidStore>>` per `cancel_from_source(login)`
   die offenen ausgehenden Raids dieser Quelle holen (die Pipeline registriert
   nach Erfolg ein `PendingRaid` mit `registered_ts`).
2. Nur wenn ein Pending juenger als das Countdown-Fenster ist
   (`TB_AUTO_UNRAID_WINDOW_SECS`, Default 90), den Raid abbrechen:
   Quell-Token via `TokenProvider::get_valid_token`, dann
   `HelixClient::cancel_raid(source_id, token)` (identisches Muster wie
   `BlacklistRaidGuard::cancel_raid`). Erfolg/Fehler loggen.
3. Aeltere Pendings (Raid schon ausgefuehrt) nur aufraeumen, nicht canceln.

### Teil 2: Wiederholungs-Flip-Schutz (pro Quelle)
- Letzten Flip-Zeitpunkt je Quelle in-memory halten.
- Passiert ein zweiter Flip innerhalb `TB_FLIP_REPEAT_WINDOW_SECS` (Default 600),
  dann Auto-Raids fuer diese Quelle pausieren: die bereits im Offline-Pfad
  konsultierte `Arc<Mutex<ManualRaidSuppression>>` mit einer eigenen langen TTL
  markieren (`TB_FLIP_PAUSE_SECS`, Default 43200 = 12 h). KEINEN zweiten
  parallelen Pausenpfad bauen; wenn ein eigener Skip-Grund im Log noetig ist,
  bestehende Struktur erweitern statt duplizieren.
- Beim Ausloesen der Pause EINE Chat-Nachricht in den Quellkanal senden
  (bestehenden Chat-Send-Pfad wiederverwenden, siehe `raid_greeting.rs`
  `chat.send_message(&from_id, ...)` bzw. `chat_wiring.rs` `send_message`).
  Entprellt: nur beim Zustandswechsel in die Pause, nicht pro Poll.

## Chat-Text (Pflicht: Skills `community-ankuendigung` + `no-em-dashes`)
Kurz, Nani-Ton, echte Umlaute, KEINE Em-Dashes, mehrere rotierende Varianten,
kein internes Vokabular (kein "pausiert/Suppression/Auto-Raid" technisch).
Inhalt sinngemaess: dein Stream war eben kurz weg und gleich wieder da, ich
halte die automatischen Raids hier erstmal an. Wenn du raiden willst, schick
deine Leute mit `!raid` los. Kein Fuelltext, keine Bedienungsanleitung im
ersten Satz.

## Wiring-Hinweise
- `RaidEventSubHooks` (eventsub_hooks.rs) haelt aktuell WEDER Pending-Store noch
  TokenProvider/Helix noch Chat-Sender. Diese muessen aus der Composition-Root
  (`wiring.rs` / `chat_wiring.rs`, dort wird `Arc<Mutex<PendingRaidStore>>` fuer
  `AutoRaidPipeline::new` gebaut) als geteilte Handles hineingereicht werden.
- Neue Komponente sauber kapseln (z. B. `FlipUnraidHandler` neben `auto_raid.rs`),
  logikreiche Teile testbar in `tb-raid` (Fenster-/Wiederholungs-Entscheidung als
  reine Funktion mit Unit-Tests, analog `remaining_grace`).
- Bestandssuche zuerst per Graphify (Skill `code-suche`), nichts doppelt bauen.

## Fertig-Kriterien
- Flip innerhalb Fenster -> Raid wird per Helix abgebrochen, Pending entfernt.
- Zweiter Flip < 10 min -> Quelle pausiert, genau eine Chat-Nachricht.
- Erster Flip allein loest KEINE Pause und KEINE Nachricht aus.
- Reine Entscheidungslogik unit-getestet (Fenster, Wiederholung, Entprellung).
- `cargo fmt`, `cargo clippy`, `cargo test` (nur eigene Aenderungen formatieren).
- Keine Code-Kommentare (Repo-Regel). Keine neuen Modelle/Secrets.
