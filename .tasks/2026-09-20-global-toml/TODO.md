# Zentrale TOML-Konfiguration des Twitch-Bots

Stand: 20.09.2026. Status: in Bearbeitung, nicht produktiv umgestellt.

## Verbindliche Grenzen

Der Auftrag betrifft Deadlock-Twitch-Bot und seine zugehörigen Dienste. Globale Betriebswerte gehören in eine gemeinsame `config/bot.toml`; Zugangsdaten bleiben im geschützten Infisical-Pfad. Streamer-Einstellungen, Kalender, OAuth-Zustand, Sessions und Modell-Cache bleiben Laufzeitdaten. Keine TOML-zu-ENV-Brücke, kein automatischer Anbieterwechsel, keine öffentlichen Testnachrichten. Die automatische Auswahl darf die freigegebene DeepSeek-Flash-Familie nicht verlassen. Die bestehende ZAI-Ausnahme der Titel-KI und die Zustimmungsschranken für externe Transkriptverarbeitung bleiben erhalten.

## Ausgangsstand

- Kanon: `/home/nathanael/repos/Deadlock-Twitch-Bot`, lokales main `e63ce0043c7ba3d15ae482aa31d5d60e94380bc7`. Fremde unversionierte Dateien wurden nicht angefasst.
- Remote: `git@github.com:EarlySalty/Deadlock-Twitch-Bot.git`.
- Frisch geholtes origin/main: `c8c8b258977a96bbc4639f242f96a6394cf5e6e6`.
- Eigener Branch: `feat/twitch-global-toml-20260920`.
- Eigener Worktree: `/home/nathanael/.worktrees/Deadlock-Twitch-Bot-global-toml-20260920`.
- Live-Link: `/opt/deadlock/twitch/current` zeigt auf Release `4bfc09af005d407061d523a9f7563466b5890de3`. Nicht mit origin/main gleichsetzen.
- Bot und Dashboard sind Systemdienste mit getrennten Konten `twitchbot` und `twitchdash`. Beobachtete PIDs: 6333 und 6384; beide NRestarts=0.
- Coaching hat einen eigenen Audit-Release-Pfad mit SHA `dd27dd95e8075db8e2879de6f1f78ebd2e0162a5`; zuletzt MainPID=0, SubState=dead, ExecMainStatus=0. Das ist noch kein Nachweis einer beabsichtigten Abschaltung.
- STT ist ein eigener User-Dienst, PID 1240, `/usr/bin/python3.12`, NRestarts=0. faster-whisper bleibt die bestehende Python-Ausnahme.
- Keine geeignete zentrale TOML vorhanden. Die beiden TOMLs des Highlight-Detektors enthalten fachliche Gewichte beziehungsweise Bildregionen und sind kein globaler Betriebslader.
- `tb-config` lädt bislang ENV. Die Startskripte überschreiben Nicht-Secret-Werte vor und nach dem Infisical-Bulk-Load. Der produktive DSN-Vorrang der getrennten Konten muss erhalten bleiben.
- `tb-llm/src/model_resolver.rs` wurde in `f17d5719` entfernt. Vorhandene Resolver-Linie: `a908f2c7`, `00adacd1`, `988d7b56`. Diese Linie kontrolliert weiterentwickeln, keinen parallelen Resolver einführen.
- Aktuelle feste Fireworks-ID ist kein automatischer Resolver. Der alte Resolver hat weder belastbare Zahlenversionen noch Proben, Paginierung, Policy-Bindung oder Ablaufprüfung.
- Der Schutz `ENGAGEMENT_LEARN_IDLE_CHANNELS=0` darf nicht verloren gehen.

## Arbeitsfolge und Abnahme

1. Bestand und Prioritäten vollständig erfassen, einschließlich dynamischer Schlüssel, geschützter Uplink-Konfiguration und STT-Eigentümer.
2. Typisierten, streng prüfenden Lader in `tb-config` ergänzen; Fehler und Debug-Ausgaben dürfen Eingabewerte nicht wiedergeben. Pfade relativ zur absoluten Config-Datei auflösen.
3. Bestehende Betriebswerte kontrolliert übernehmen. Produktive Datei nicht mit Defaults überschreiben. Sichere Vorher-Nachher-Sicht vergleichen.
4. Verbraucher schrittweise auf dieselbe validierte Momentaufnahme umstellen. Keine Nicht-Secret-ENV-Overrides. Betriebsänderungen zunächst ausdrücklich neustartpflichtig; kein behauptetes Hot-Reload.
5. Bestehenden `tb-llm`-Resolver mit offiziellen Katalogdaten, Zahlenversionen, freigegebener Familie, Proben und an die Policy gebundenem Postgres-Zustand vervollständigen. Titel-Ausnahme und Datenschutzgrenzen nicht erweitern.
6. Konfigurations-, Resolver- und Fachtests einschließlich Negativfällen ausführen; neue direkte beziehungsweise dynamische ENV-Leser durch Regressionstest verhindern.
7. Verifizierte Teilstände committen und sofort auf den Feature-Branch pushen. Vor Main-Integration Test-Gate und unabhängigen Merge-Kritiker ausführen.
8. Nach erfolgreicher Integration den bestätigten Produktionsstand mit `-j 2` bauen und über den vorhandenen Deploy-Weg ausrollen. PID, Binary, Fehlerjournal, Anker, Funktion, Ort und Heartbeats prüfen.
9. Dev-/Support-Doku nach Deadlock-Docs `internal/Deadlock-Twitch-Bot/`, operative Erkenntnisse nach dem 2nd-Brain-Schema. Keine Community-Ankündigung für diese Admin-Änderung.
10. Erst nach Merge und Live-Beweis den eigenen gemergten Branch und Worktree bereinigen. Fremde oder ungemergte Arbeit bleibt bestehen.

## Bisherige Testnachweise

`cargo test -p tb-config -p tb-llm -j 2` auf unverändertem Ausgangsstand: Exit 0. Job `j-1789863572-15`; exakte Einzelzahlen werden mit dem vollständigen Testprotokoll in EVIDENCE.md nachgetragen.

## Offene Nachweise

Noch keine Laufzeitmigration, kein Merge, kein Deploy und kein Live-Funktionsnachweis. Diese Akte ist kein Abschlussbericht.
