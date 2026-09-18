# Smalltalk Live: ein Kanal pro Prozess

## Auswahl und Nachweise

`SMALLTALK_LOOP_ENABLED=1` und `SMALLTALK_LOOP_LIVE_SEND=1` aktivieren den
bereits vorhandenen One-Shot. Shadow/Testbetrieb bleibt unveraendert.

Live-Auswahl verwendet `twitch_live_state.twitch_user_id`, nicht den
Partner-Outreach-Roster. Ein Live-State-Eintrag allein gibt keinen Send frei.
Der Loop prueft pro Tick hoechstens eine Identitaet ueber die vorhandene
Helix-Anbindung. `/streams` muss genau die angefragte ID, den aktuellen Login,
eine Stream-ID und Deadlock liefern. Parallel wird `/channels/followers`
mit dem bestehenden OAuth-User-Token-Manager aufgerufen. Die gemeinsame
Vorpruefung hat eine Frist von fuenf Sekunden; es gibt keinen Scraper oder
seriellen Token-/Provider-Fallback.

Twitch dokumentiert fuer Get Channel Followers: Ohne Moderatorrolle/Scope
werden keine einzelnen Follower ausgegeben, die Gesamtzahl `total` kann mit
User-OAuth dennoch geliefert werden. Verwendet wird ausschliesslich ein
erfolgreich gelesenes, nichtnegatives Integer-`total`; 401/403/429, Fehler,
fehlende Zahlen und Timeouts sind kein Null-Follower-Nachweis.
Quelle, geprueft am 18.09.2026:
https://dev.twitch.tv/docs/api/reference/#get-channel-followers

Vorhandene Monitoring-Snapshots (`twitch_stream_sessions.followers_start` /
`followers_end`) werden fuer diesen neuen Live-Pfad nicht als Identitaets-
oder Frischenachweis uebernommen: der Sessionstart ist kein eigenstaendiger
Messzeitpunkt der Followerzahl. Stattdessen speichert
`twitch_smalltalk_candidate_state` ID, Login, Followerzahl, Messzeitpunkt,
Quelle, Live-Pruefung, naechsten Refresh und ID-basierten Cooldown.
Es werden keine Outreach-Zeilen angelegt oder veraendert.

## Fail-closed-Gates

Auswahl und letzter Sendercheck verwenden denselben SQL-Kandidatenvertrag.
Erforderlich sind 0 bis 49 Follower, ein maximal zwoelf Stunden alter
Helix-Nachweis, eine maximal 90 Sekunden alte Live-Deadlock-Bestaetigung
sowie identische ID/Login in Live-State, Nachweis und offener Live-Testsession.
Zeitstempel aus der Zukunft, Partner in einer der beiden Partnertabellen,
Blacklists nach ID oder Login und aktive Cooldowns sperren den Kanal.
Fehlerhafte Cooldown-Daten oder Datenbankfehler geben niemals einen Send frei.
Ein fehlgeschlagener Refresh entfernt den bisherigen positiven Nachweis.

Unmittelbar nach einem eventuell erforderlichen Sender-Token-Refresh und
vor Chat-HTTP werden Session, Settings (`enabled`, `irc_read`,
`smalltalk_live`), Identitaet und alle Kandidatengates erneut gelesen.
Auch ein direkter Senderaufruf durchlaeuft den bestehenden Werbe-/Linkfilter.
Nach Sessionende bleibt der ID-basierte 24-Stunden-Cooldown bestehen,
auch bei Umbenennung. Der gemeinsame Prozess-Latch verhindert eine zweite
Session; auch ein unklar bestaetigter Commit verbraucht den Live-Slot.

## Verhalten und Provider

Keine Aenderung am globalen LLM-Routing: der vorhandene Smalltalk-Testprompt,
Schweigen, Rhythmus-/Anti-Flood-Gates, Kontexte und die harte
Sieben-Sekunden-Modellfrist bleiben bestehen. Es wird kein OpenAI-Fallback
zugeschaltet. Der bestehende lokale Audio-/Transkriptpfad bleibt unveraendert.

## Verifikation

Die relevanten DB-Tests starten private temporaere PostgreSQL-Cluster;
sie benoetigen keine Live-Zugangsdaten und ueberspringen die Sicherheitsfaelle
nicht stillschweigend bei fehlender Test-DSN.

```text
cargo test -p tb-engagement
cargo test -p tb-engagement --test smalltalk_loop_store
cargo test -p tb-bot smalltalk_loop_wiring
cargo test -p tb-transport-twitch
cargo check -p tb-bot
```

Abgedeckt: leerer Outreach-Roster, One-Shot ueber geklonte Stores,
49/50-Grenze, unbekannte/veraltete/kuenftige Nachweise, fehlgeschlagener Refresh,
Umbenennung, Partner-/Blacklist-/Cooldown-Aenderungen vor HTTP,
Werbung beim direkten Senderaufruf und Followeranstieg waehrend Token-Refresh.
Die Migration und die Runtime-Rechte werden in echtem temporaerem PostgreSQL
gegen den neuen Schema-Snapshot geprueft; nur `twitchbot` darf die
Kandidatennachweise schreiben, Dashboard/Legacy duerfen lediglich lesen.

## Live-Nachweis nach Release

Keinen Zielkanal und keine Followerzahl manuell erfinden. Folgende
strukturierten Ereignisse und Tabellen unterscheiden Vorbereitung von Send:

- `smalltalk_loop.preflight`: ID/Login, Quelle, Followerzahl, Live-Status,
  klassifizierter Fehler; keine Tokens und kein Stream-Audio.
- `smalltalk_loop.session_started` / `twitch_smalltalk_sessions` mit
  `live_test=true`: tatsaechlich geoeffnete One-Shot-Session.
- `smalltalk_loop.send_blocked`: erneute Pruefung hat den Chat-Send verhindert.
- Conversation-Memory und Twitch-Sendeergebnis: erst diese belegen einen
  tatsaechlichen Chat-Send; generierter oder gespeicherter Text allein nicht.

Release immer aus einem eigenständigen Clone mit internem `.git` und allen
drei Rust-Binaries sowie allen drei Frontend-Dists über
`/usr/local/bin/deploy-twitch-release <sha> <source-clone>` installieren.
Keine direkten Service-/Sudo-Aufrufe und kein manuelles Schalten der Gates.

**Auf diesem Host führt der installierte Deploy-Wrapper Migrationen und
Runtime-Grants nicht automatisch aus.** Ein grüner Deploy und MCP-Healthcheck
beweisen deshalb noch keine funktionierende Kandidatenauswahl. Der Bot läuft
mit abgeschalteten Startmigrationen und darf selbst kein DDL ausführen.

Vor dem Releasewechsel den sauberen, geprüften Quell-Clone gegen den
Datenbankstand prüfen. Ausstehende Migrationen mit dem vorhandenen
`/usr/local/libexec/cargo-sqlx sqlx migrate run --no-dotenv` und explizitem
`--source <source-clone>/rust/migrations` über den berechtigten lokalen
DB-Zugang anwenden. Danach die versionierte Datei
`<source-clone>/ops/systemd/twitch-runtime-roles.sql` mit
`psql --no-psqlrc --single-transaction --dbname=twitch_analytics --file=...`
anwenden. Keine eigenen Tabellenkopien, keine manuell gesetzten
Migrationsmarker und keine erfundenen Kandidatennachweise erzeugen.

Nach dem Deploy zusätzlich prüfen: Migration `20260918024500` erfolgreich,
`twitch_smalltalk_candidate_state` vorhanden, `twitchbot` darf schreiben,
`twitchdash` und `twitchlegacy` dürfen nicht schreiben. Eine vom laufenden
Bot erzeugte Helix-Messung und der tatsächliche Sessionzustand sind der
funktionale Nachweis. Der überprüfte Live-Stand vom 18.09.2026 steht in
`.tasks/2026-09-18-smalltalk-live-resume/REPORT.md`.
