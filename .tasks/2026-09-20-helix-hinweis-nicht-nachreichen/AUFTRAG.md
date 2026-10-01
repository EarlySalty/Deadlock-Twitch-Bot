# Auftrag: Coaching-Audit Helix-Hinweis nicht nachreichen

status: aktiv
datum: 2026-09-20
stufe: klein
repo: Deadlock-Twitch-Bot

## Ziel

Die Discord-Meldung „Coaching-Audit: Twitch-Abfrage scheitert dauerhaft“ kommt
nicht mehr Stunden später, wenn Helix längst wieder antwortet. Nach einem
Reboot startet der Audit-Dienst von allein, so wie Bot und Dashboard.

## Was der Nutzer gesehen hat

18:26 CEST, Deutsche Deadlock Community Bot: „Twitch-Abfrage scheitert
dauerhaft (seit mindestens 5 Anläufen). Laufende Aufnahmen laufen weiter,
neue Sendungen werden nicht erkannt.“

## Vorcheck (Intent-Agent, kein eigener Vorcheck-Thread)

- 02:07–02:12 CEST: fünf Helix-Timeouts gegen
  `https://api.twitch.tv/helix/streams?user_login=helmbombenricky&user_login=skifahrertv&user_login=deadlockgermany`
  (`Connection timed out` / `TimedOut`), `helix_fehler=1` bis `5`.
- 02:12:25: DM an den Broker
  `http://127.0.0.1:8770/internal/master/v1/discord/send-dm` scheitert, Hinweis
  landet in `offene-hinweise/` als
  `20260920T001225Z-225879-helix-ausfall-1`.
- 02:12:27 SIGTERM, 02:12:37 Unit sauber gestoppt. `Restart=on-failure`
  greift bei SIGTERM nicht.
- 02:13:39 neue User-Session (Reboot/Login). Bot- und Dashboard-Units sind
  `enabled`, Audit-Unit war `disabled` → Dienst tot bis 18:26.
- 18:26:11 Deploy `818e21523b13648a26e76f150aba89ae1ad19a89` startet den
  Dienst neu. 18:26:30: `aufgehobenen Hinweis nachgereicht` genau dieser
  Schlüssel. Helix-Fehler danach: keine. Aufnahmeordner
  `helmbombenricky` wurde 18:26 angefasst: Live-Erkennung geht.

## Fundstellen

- Alarmtext und Zähler: `rust/bin/tb-stream-audit/src/main.rs:1133-1209`
  (`get_streams_by_logins`, `helix_fehler >= MAX_STILLE_VERSUCHE`, Text
  „Twitch-Abfrage scheitert dauerhaft“, `hinweis_aufheben(..., "helix-ausfall")`).
- Nachreichen ohne Helix-Check: `offene_hinweise_senden`
  `rust/bin/tb-stream-audit/src/main.rs:3248-3307`. `start-*` wird verworfen
  (`3267-3278`, Test `eine_alte_startmeldung_wird_nicht_verspaetet_nachgereicht`
  bei `5636`). `helix-ausfall.json` wird zugestellt, obwohl der Live-Loop
  denselben Hinweis nach erfolgreicher Abfrage löschen würde (`1203-1208`).
- Bestehendes Muster „erledigte Störung nicht nachreichen“: Kommentar
  `1304-1307` und Test `hinweis_behaelt_seinen_schluessel` (`5792-5795`).
- Unit: `ops/systemd/deadlock-twitch-stream-coaching-watch.service`
  (`Restart=on-failure`, `WantedBy=multi-user.target`). Live am 2026-09-20
  18:45 auf `enabled` gesetzt (Symlink
  `multi-user.target.wants/...`). Das gehört ins Repo bzw. in die
  Install-Doku, damit der nächste Host dasselbe hat.
- Architektur: `docs/architecture/stream-coaching-audit.md:84`
  („Helix antwortet nicht | Nach fuenf Anlaeufen eine DM“).

## Arbeitsschritte

1. In `offene_hinweise_senden`: Hinweise der Ablage `helix-ausfall` nicht
   nachreichen. Datei entfernen wie bei `start-*`. Begründung: der Live-Loop
   ist die Quelle der Wahrheit. Läuft Helix noch nicht, feuert er nach fünf
   stillen Versuchen selbst erneut und liest den offenen Schlüssel über
   `offener_hinweis`. Läuft Helix, darf die alte Störung nicht als aktuell
   ankommen.
2. Regressionstest analog zu `eine_alte_startmeldung_wird_nicht_verspaetet_nachgereicht`:
   `hinweis_aufheben(..., "helix-ausfall", ...)`, `offene_hinweise_senden`,
   Datei weg, kein `dm_rohtext`. Kein Test am Wortlaut der Admin-DM.
3. Admin-DM in `main.rs:1149-1152`: echte Umlaute (`Anläufen`). Konstante
   mit dem Produktionspfad teilen, Test hängt nicht am Satz.
4. `docs/architecture/stream-coaching-audit.md` Abschnitt Fehlerverhalten:
   nachgereichte Helix-Ausfälle gibt es nicht mehr; nach Reboot startet die
   System-Unit, weil sie enabled ist. Keine User-Unit beschreiben.
5. Commit und `git push origin HEAD:fix/helix-hinweis-nicht-nachreichen`.
   Nicht nach main mergen.

## Nicht anfassen

- Helix-Client, Timeouts, Token-Invalidierung.
- Aufnahme, Mitschnitt, Drive-Archiv, Broker-Protokoll.
- Andere Hinweisarten außer dem Skip für `helix-ausfall` in
  `offene_hinweise_senden` (kein Refactor der ganzen Hinweiswarteschlange).
- `Restart=on-failure` nicht auf `always` drehen.
- Geteilter Checkout `/home/nathanael/repos/Deadlock-Twitch-Bot`.

## Fertig-Kriterium

- Test: abgelegter `helix-ausfall` wird beim Nachreichen gelöscht, nicht
  geschickt.
- `cargo test -p tb-stream-audit-bin` im Worktree, Zahlen im Bericht
  (passed/failed/ignored), Baseline falls etwas vorher rot war.
- rustfmt nur die angefassten Dateien.
- Branch gepusht.

## Deploy-Weg

Nach Review und Merge: `deploy-twitch-release <sha>`. Die System-Unit ist
live bereits `enabled`. Funktionsbeweis: nach Start kein Log
`aufgehobenen Hinweis nachgereicht` mit `helix-ausfall`, während Helix
antwortet; laufende Aufnahme eines Live-Kanals (aktuell helmbombenricky).
