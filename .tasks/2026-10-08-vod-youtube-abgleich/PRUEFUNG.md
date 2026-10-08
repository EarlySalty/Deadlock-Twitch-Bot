# Prüfnachweise

## Bereits ausgeführte Läufe

1. Archiv: `TB_TEST_DATABASE_URL=postgresql://nathanael@127.0.0.1:55683/token_db_youtube cargo-slot test --jobs 3 --manifest-path /home/nathanael/.worktrees/tb-vod-youtube-abgleich-20261008/rust/Cargo.toml -p tb-vod-archive -- --include-ignored --nocapture`. Exit 0, 55 passed, 0 failed, 0 ignored, 0 filtered. Log: `archive-tests3.log`.
2. Oberfläche: `npm run test:vod-archive --prefix /home/nathanael/.worktrees/tb-vod-youtube-abgleich-20261008/bot/dashboard_v2`. Exit 0, 6 passed, 0 failed, 0 skipped. Log: `frontend-tests.log`.
3. Oberfläche gebaut: `npm run build --prefix /home/nathanael/.worktrees/tb-vod-youtube-abgleich-20261008/bot/dashboard_v2`. Exit 0. Log: `frontend-build.log`. Die Ausgabe liegt laut Buildlog unter `bot/analytics/dashboard_v2/dist`, nicht unter dem Quellordner. Die Sichtprüfung und das endgültige Release stehen noch aus.
4. API: `VOD_ARCHIVE_PROOF_PATH=/home/nathanael/.worktrees/tb-vod-youtube-abgleich-20261008/.tasks/2026-10-08-vod-youtube-abgleich/pruefung/api-fixture.json TB_TEST_DATABASE_URL=postgresql://nathanael@127.0.0.1:55683/token_db_youtube cargo-slot test --jobs 3 --manifest-path /home/nathanael/.worktrees/tb-vod-youtube-abgleich-20261008/rust/Cargo.toml -p tb-dashboard-api vod_archive_management -- --include-ignored --nocapture`. Exit 0, 6 passed, 0 failed, 0 ignored, 1336 filtered im Bibliothekslauf. Log: `api-tests2.log`. Der vorherige Filter `social_media_vod_archive` führte keinen Test aus und zählt ausdrücklich nicht als Nachweis.
5. YouTube-Client: `TB_TEST_DATABASE_URL=postgresql://nathanael@127.0.0.1:55683/token_db_youtube cargo-slot test --jobs 3 --manifest-path /home/nathanael/.worktrees/tb-vod-youtube-abgleich-20261008/rust/Cargo.toml -p tb-social-media archive_read -- --include-ignored --nocapture`. Exit 0, 2 passed, 0 failed, 0 ignored, 339 filtered im Bibliothekslauf. Log: `youtube-client-tests.log`.

Diese ersten Läufe wurden nach der Gate-Fixrunde für die betroffenen Rust-Pfade wiederholt. Die beiden Gate-Funde zeigten konkrete nicht abgedeckte Fortschrittsfälle trotz zuvor grüner Tests.

## Aktueller Stand 76332e3d

- Archiv: derselbe Archivbefehl mit `--include-ignored --nocapture`, Exit 0, 57 passed, 0 failed, 0 ignored, 0 filtered. `archive-gatefix2.log`.
- API: derselbe Filter `vod_archive_management`, Exit 0, 7 passed, 0 failed, 0 ignored, 1336 filtered im Bibliothekslauf. `api-gatefix.log`.
- Direkte YouTube-Clientproben: 2 passed, 0 failed, 0 ignored. Oberfläche: 6 passed, 0 failed, 0 skipped. Diese unveränderten Quellen wurden nicht erneut geändert.
- Gesamtumfang dieser unterschiedlichen Proben: 72 passed, 0 ignored. Keine vollständige Workspace- oder Dashboard-Gesamtsuite behauptet.
- Dashboard erneut gebaut nach dem Fixercommit: `frontend-build2.log`, Exit 0. Admin und Website gebaut: `admin-build.log` und `website-build.log`, beide Exit 0.

TESTNACHWEIS[TW-1]: 76 passed, 0 ignored | Baseline: 0 rot

Aktueller zusammengezählter Umfang: 59 Archivtests und acht Frontendtests auf 89bfd5fa, sieben API- und zwei direkte Clienttests auf den seit deren Lauf unveränderten relevanten Quellen. Kein neuer API-/Client-Abschluss im zweiten Fixerauftrag behauptet: zusätzliche Nachläufe wurden während der Neukompilierung gestoppt und zählen nicht.

Die Testbaseline wurde jetzt tatsächlich erneut auf 0ecae137 gemessen: 50 Archivtests, 0 failed, 0 ignored, Exit 0 in eigener separater token_db_youtube_baseline_c5d0. Der erste Versuch ohne die vom alten Resume-Migrationstest zusätzlich verlangte token-db-tests.conf ergab 49 passed und einen Konfigurationsfehler. Nach Bereitstellung der eigenen temporären Testkonfiguration derselbe Befehl erfolgreich, keine Skipmarker. Dieser erste Einrichtungsfehler ist keine rote Codebaseline. Die fokussierte API-Baseline wurde vor einem Testabschluss während der langen Neukompilierung gestoppt; sie wird nicht mit null Fehlern gezählt. Baseline 0 bezieht sich deshalb ausdrücklich auf den gemessenen Archivumfang, nicht auf eine vollständige Workspace-Suite.

## Clippy

Strikter Dependencylauf: `cargo-slot clippy --jobs 3 --manifest-path /home/nathanael/.worktrees/tb-vod-youtube-abgleich-20261008/rust/Cargo.toml -p tb-config -p tb-social-media -p tb-vod-archive --all-targets -- -D warnings`. Aktuell Exit 101 mit einer eindeutigen Fundstelle `tb-raid/src/signup_denylist.rs:71`, `result_unit_err`. Derselbe Befehl im eigenen detached Ausgangsworktree auf 0ecae137, mit eigenem Hauptworktree als `--target-dir`, ebenfalls Exit 101 und dieselbe eine Fundstelle.

Strikt mit `--no-deps`: aktueller Stand und tatsächlich gemessener Ausgangsstand jeweils Exit 101, vier identische eindeutige Fundstellen in tb-social-media: analytics.rs:321 (too_many_arguments), credentials.rs:214 (manual_map), upload_worker.rs:822 (type_complexity), vocab.rs:164 (needless_borrows_for_generic_args). Doppelte lib-/lib-test-Ausgaben zählen nicht als weitere Fehler.

Derselbe ausgewählte Zielumfang mit `--all-targets --no-deps`, ohne `-D warnings`, lief vollständig durch: Exit 0, dieselben vier bestehenden Warnungen, keine zusätzliche Warnung in den geänderten Zielpfaden. Keine Warnung abgeschaltet, keine fremde Datei refaktoriert. Der eigene Baselineworktree wurde nach Ende seiner Läufe sauber entfernt.

## Formatvergleich

Die gezielte Formatprüfung liefert Exit 1 mit vier hunks in bestehenden `store.rs`-Assertions. `pruefung/fmt-vergleich.py` misst denselben Formatter gegen den Ausgangsstand `0ecae1370f1a80d1a101249b5c932663d69be8af` und den aktuellen Quellstand. Beide haben vier hunks und 36 geänderte Zeilen; die vorgeschlagenen Zeilenänderungen sind identisch. Beleg: `pruefung/fmt-vergleich.json`, Exit 0 des Vergleichs. Das bedeutet unveränderte Formatabweichungen, nicht eine grüne Formatprüfung. Der erste Baselineversuch über `rustfmt --check` auf Standardeingabe war dafür nicht aussagekräftig und zählt nicht als Vergleichsnachweis.

## Aussage der Datenbankproben

Der Archivlauf schreibt tatsächlich in eine eigene PostgreSQL-16-Instanz auf Loopback-Port 55683. Die Datenbank `token_db_youtube` enthält synthetische Daten. Fehlende Testkonfiguration lässt die neuen Nachweisproben ausdrücklich fehlschlagen.

Die Fortsetzungsprobe lädt synthetische verschlüsselte Zugangsdaten über den bestehenden CredentialManager. Sie ruft die regulären Rust-Leseendpunkte an einem lokalen HTTP-Server auf. Sechs GET-Aufrufe verteilen sich auf zwei Läufe mit jeweils drei logischen Leseoperationen. Ein gespeicherter Cursor setzt die zweite Playlist-Seite fort. Vor Suchabschluss entstehen weder eine endgültige Fehlzuordnung noch eine Bestätigung. Gleiche Titel mit verschiedenen Twitch-Quellen bleiben getrennt. Der nicht gefundene dritte Fall wird erst nach vollständiger Suche als ungeklärt gespeichert. Ursprüngliche VOD-Zeilen bleiben unverändert, es entstehen keine künstlichen Uploadteile.

Weitere echte SQL-Proben zeigen: Fehlversuche erhalten den vorigen erfolgreichen Nachweis; ein geändertes Zielkonto oder ein parallel geänderter Upload verwirft einen überholten Schreiber. Lokale Bereinigung braucht aktuelle, vollständige, verarbeitete Nachweise zum unveränderten Ziel und Uploadstand.

## Angepasste bestehende Erwartungen

Die bisherige Produktionsprüfung setzte fehlende oder abgelehnte YouTube-Videos zurück und konnte erneut hochladen. Dieser Pfad wurde zugunsten des unabhängigen Leseabgleichs entfernt. Die zugehörige Worker-Erwartung prüft jetzt, dass eine verschwundene Zielantwort keinen erneuten Upload erzeugt und die lokale Kopie erhalten bleibt. Die bestehende Bereinigungsprobe stellt den notwendigen aktuellen Nachweis über den tatsächlichen SQL-Speicherpfad her.

Die Metadatenprobe prüft jetzt, dass Originalquelle und Teilnummer trotz langer Titel und Beschreibungen erhalten bleiben. Bestandsvideos werden dadurch nicht geändert.

## Gebündelte Sichtprüfung

Moli 1.1.14 auf eigenem Loopback-Port 9338, synthetischer lesender Fixture-Server auf 4198. Keine Produktionsanmeldung und keine Provider-Schreibaktionen. Reale synthetische PostgreSQL-Handlerantworten, dieselben gebauten Dashboardassets wie im Hashnachweis. Der erste Aufruf scheiterte vor der Sichtprüfung am falschen Ausgabeordner und zählt nicht als visueller Nachweis.

Eine erfolgreiche gebündelte Runde: current, partial und connection jeweils 1440×1100 und 390×844. Je neun Karten, Dokumentbreite entspricht Viewportbreite, Statusicons und Zeiten sichtbar, Prüfbutton entprellt, Verbindungsweg erhalten. Manrope und Sora geladen, keine Bilder auf dieser Oberfläche, keine pageerrors. Sechs Detailbilder tatsächlich betrachtet. 18 Viewportbilder und sieben Assethashes in `pruefung/moli-layout.json` gesichert. Quell-SHA 76332e3d; dirty=true stammt von noch uncommitteten Auftragsartefakten und Testkonfiguration, nicht von später geändertem UI-Code.

## Gate-Runde 3

Der Gate auf dem gesicherten Stand fb89fe69 hat einen weiteren echten Aktualitätsfehler erkannt: alte Inventarbeobachtungen konnten eine neue Erfolgszeit erhalten. Das ALLOW der Runde 2 war dadurch überholt. Der frische native Fixer hat den Fund und die beiden eng begrenzten UI-Hinweise korrigiert, Commit 89bfd5fa. Runde 4 mit demselben Kritiker gpt-6.1-sol: ALLOW, Inventarzuordnungen werden vor neuer Erfolgsmeldung frisch abgefragt. 59 Archivtests und acht Frontendtests bestanden. Striktes Clippy für den tatsächlich neu geänderten Rust-Zielumfang `cargo-slot clippy --jobs 3 --manifest-path /home/nathanael/.worktrees/tb-vod-youtube-abgleich-20261008/rust/Cargo.toml -p tb-vod-archive --all-targets --no-deps -- -D warnings`: Exit 0.

Der produktive SQL-Nachweis wurde bereits an dieselben Authrevision- und Teil-Snapshot-Guards wie die API angepasst. Veraltete Kontonachweise zählen nicht als aktuell. Zusätzlich werden älteste und neueste Beobachtungszeiten ausgegeben, ohne Video-IDs oder private Inhalte.

## Einzige Bestätigungsrunde nach Runde 3

Nach neuem Frontendbuild auf 89bfd5fa wurden dieselben sechs Desktop-/Mobilkombinationen einmal gebündelt bestätigt. Ausgangsbilder bleiben erhalten. Neue Bilder und Messwerte in pruefung/bestaetigung, sieben neue Assethashes, wieder neun Karten pro Lauf, keine Überbreite und keine pageerrors. Die sechs Detailbilder wurden tatsächlich betrachtet. Keine weitere UI-Korrektur oder Sichtpolitur. Eigene Moli- und Fixture-Prozesse anschließend beendet.

## Noch offen

Main-Integration, sauberer Releasebuild und produktive API-Abfragen. Bisher wurde kein neuer Nachweis in der Produktionsdatenbank geschrieben und keine Produktionsmigration angewendet. Die fünf historischen Fälle sind noch nicht entschieden. Testdatenbank, Moli und Fixture-Server sind eigene temporäre Ressourcen und werden nach Sicherung der Belege beendet.
