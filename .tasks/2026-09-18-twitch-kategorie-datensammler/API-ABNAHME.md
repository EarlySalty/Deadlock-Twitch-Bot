# Admin-API des globalen Kategoriesammlers

## Umfang dieses Integrationspakets

Eigenständiger Arbeitsbaum `/home/nathanael/repos/tb-category-admin-api-20260918`, Branch `feat/category-admin-api-20260918`, Basis `1c8dcb85`. Der bestehende Collector-Arbeitsbaum wird parallel beschrieben und wurde deshalb nicht überschrieben.

Implementiert: GET `/twitch/api/v2/admin/category-collector?days=7`, innerhalb des bestehenden authentifizierten Routers. Ohne Authentifizierung 401, Partner 403; nur Admins sehen Aggregate. Exakt 7, 30 oder 90 Tage, Standard 7. Kein Rohtext, keine Chatter-IDs oder Chatter-Logins im Antwortvertrag. Keine Twitch-Anfragen und keinerlei Datenbank-Schreibaktionen durch diese API.

Auswertung: getrennte Stream- und Nachrichtensprachen, zeitgewichtete Zuschauer- und Sendestunden, stündliche Kategorie-Trends, Top 20 Kanäle je Sprache nach Zuschauerstunden und Chat-Tageszeiten in UTC. Geografie wird nicht behauptet. Stunden-Rollup-Unikate werden nicht als eindeutige Personen über längere Zeiträume addiert.

Nur aufeinanderfolgende erfolgreiche Polls mit Abstand von höchstens zwei konfigurierten Poll-Intervallen tragen Sendezeit bei. Ein Fehler trennt die Messkette. Nach einem isolierten letzten Snapshot wird keine Zeit extrapoliert; sein zeitgewichteter Schnitt ist null, nicht 0. Der Zeitraumanfang wird passend zu den historischen Chat-Stundenaggregaten auf eine volle UTC-Stunde gesetzt und exakt im JSON genannt. Trend-Buckets ohne erfolgreiche Polls bleiben aus, statt erfundene Nullwerte zu erhalten. Bestätigte leere Kategorie-Polls bleiben dagegen gültige Null-Beobachtungen.

Der 60-Sekunden-Cache gilt nur pro Router/Datenbank und Zeitraum, ausschließlich für erfolgreiche Aggregate. Jede Anfrage durchläuft vorher die Admin-Prüfung. Ein gemeinsamer Lock begrenzt parallele teure Abfragen. Der Browser erhält `Cache-Control: private, no-store`. Read-only-Transaktion mit konsistentem Datenstand, UTC, 20 Sekunden Statement- und 2 Sekunden Lock-Limit; gesamte Abfrage maximal 25 Sekunden. Fehler liefern 503, kein Null-Dashboard.

`retention_days` kommt unverändert aus der Datenbank und unterstützt null. Das Backend setzt weder 90 Tage noch eine andere Löschfrist durch und enthält keine Löschroutine. 7/30/90 sind reine Analysefenster. `dropped_messages_scope=current_process` unterscheidet die bekannten Prozesszähler von ausgewählten Zeiträumen. Verlorene Roster-Kommandos werden separat als `dropped_roster_commands` ausgegeben, nicht als verlorene Chatnachrichten umgedeutet.

## TDD-Nachweis

Echter roter Ausgangstest vor der Handler-Implementierung:

`cargo test -p tb-dashboard-api handlers::category_collector::tests::seeded_empty_collector_is_an_honest_empty_state --lib -j 2 --target-dir /home/nathanael/repos/tb-category-collector-20260918/rust/target`

Job `j-1789727700-338`: 0 bestanden, 1 fehlgeschlagen, 1255 ausgefiltert. Fehler ausdrücklich `left: 503`, `right: 200`; kein als Rotprobe umgedeuteter Compilerfehler. Zuvor wurde ein Importfehler behoben (Job `j-1789727482-316`), dieser zählt nicht als Rotprobe.

Grüner Endstand: `cargo test -p tb-dashboard-api handlers::category_collector::tests --lib -j 2 --target-dir /home/nathanael/repos/tb-category-collector-20260918/rust/target`, Job `j-1789728442-389`: **13 bestanden, 0 fehlgeschlagen, 1247 ausgefiltert**, Testlauf 15,00 Sekunden. Darunter zehn Tests mit isoliertem PostgreSQL16 ohne Live-DB-Zugriff. Geprüft sind unter anderem echte Router-Verdrahtung, 401 auch nach erwärmtem Admin-Cache, 403 für Partner, vollständige Datensätze, leeres Schema, Null-Schnitte, dauerhafte Speicherung als nullable Konfigurationswert, Cache-Fehlerbehandlung und keine Rohtext-Ausgabe.

`cargo check -p tb-dashboard-api --tests -j 2 --target-dir /home/nathanael/repos/tb-category-collector-20260918/rust/target`, Job `j-1789728289-382`: Exit 0. Bestehende Warnung in `uplink_config.rs:781` zur veralteten nix-Typalias-Bezeichnung, nicht aus diesem Paket. Die beiden neuen Rust-Dateien wurden gezielt mit rustfmt formatiert. `git diff --check` ohne Fehler. `cargo clippy -p tb-dashboard-api --lib --no-deps -j 2 --target-dir /home/nathanael/repos/tb-category-collector-20260918/rust/target`, Job `j-1789728860-424`: Exit 0. Zwei bestehende `needless_borrows_for_generic_args`-Warnungen in `uplink_config.rs:147/150`, keine Warnung aus den neuen Sammler-Dateien.

Die 15 bestehenden Frontend-Tests im parallelen Collector-Arbeitsbaum liefen ebenfalls grün. Das ist jedoch noch keine vollständige UI-Abnahme: Eine zusätzliche echte Node/TSX-Probe zeigte am 18.09.2026 weiterhin `buildTrendreihe([0,0,100])` für drei aufeinanderfolgende Stunden als nur einen Tagespunkt mit Wert 50. Der API-Vertrag verlangt drei Stundenpunkte, eine bewusste Tagesaggregation müsste mindestens korrekt gewichtet 33,33 ergeben. Der Befund ist im Frontend-Paket zu beheben. Außerdem sind nullable Typen für Schnitte/Aufbewahrung und die generische 503-Fehlermeldung mit dem API-Vertrag abzugleichen.

Zusätzlich unabhängig im ursprünglichen Collector-Worktree ausgeführt: `cargo test -p tb-monitoring anon_chat --lib -j 2`, Job `j-1789726981-269`: **7 bestanden, 0 fehlgeschlagen, 90 ausgefiltert**. Darunter Allowlist gegen Chat-Senden/Authentifizierung, harte Shard-Kapazität, fragmentierte IRC-Zeilen und PING trotz erschöpftem JOIN-Budget, schweigende Verbindungen trotz Roster-Updates und begrenzte Eingabepuffer. Das belegt den Transport, nicht die gesamte Collector-Pipeline.

Read-only-Produktionsprüfung am 18.09.2026: Hauptbot und Dashboard aktiv, Live-Release weiterhin `44cc2aa90e1d1b59269533b5d50b643e56bfbba8`. Keine Collector-Systemunit und keine `public.category_collector_config`, `category_stream_snapshots` oder `category_chat_messages` vorhanden. Daher ausdrücklich noch kein Live-Nachweis und keine kategorieweite 24–48-Stunden-Messung.

## Veröffentlichung und zusätzlicher Sendeschutz-Nachweis

Implementierung `ed6c78d96fa07e5662318d933997bad7d11f2878` wurde auf `origin/feat/category-admin-api-20260918` gepusht und die Remote-Referenz per `git ls-remote` verifiziert. Das Push-Gate hat den Push zugelassen. Bestehende OSV-Hinweise betrafen das unveränderte Legacy-Python-Manifest `ops/highlight-detector/requirements.txt`; daraus folgt kein pauschaler Sicherheitsnachweis für das gesamte Repository.

Im parallelen Collector-Arbeitsbaum wurde inzwischen `PolicyContext::Raid` an denselben aktuellen Partner-Roster gebunden wie Standardaktionen. Unabhängig ausgeführt: `cargo test -p tb-chat --test channel_policy -j 2`, Job `j-1789729238-462`: **11 bestanden, 0 fehlgeschlagen**. Darunter fremde Raid-Ziele blockieren, Freigabe nach Widerruf erneut prüfen, fremde Nachrichten/Whispers/Moderation blockieren und autorisierte Raid-Nachrichten erlauben, jedoch keine Raid-Moderation. Dieser Code gehört zum parallelen Collector-Paket, nicht zu Commit ed6c78d9, und ist damit nicht automatisch live.

## Integrationsvoraussetzungen

Der zuletzt erneut gelesene Collector-Startcode `rust/bin/tb-category-collector/src/main.rs` liest weiterhin `TWITCH_ANALYTICS_DSN`, `TWITCH_CLIENT_ID` und `TWITCH_CLIENT_SECRET` über die Prozessumgebung. Das widerspricht dem Auftrag ohne ENV-Konfiguration und muss vor der Live-Freigabe auf einen expliziten geschützten Bootstrap-/Credentialpfad umgestellt werden.

Das neue Schema muss vom Collector-Paket als postgres migriert und für die Dashboard-Rolle lesbar sein. Ohne Schema antwortet die API absichtlich mit 503. Die Collector-Migration braucht für die Minuten-Snapshot-Abfrage einen Index bzw. UNIQUE auf `(poll_id, stream_id)`; das ursprüngliche Scaffold hatte nur Zeit-/Stream-/User-Indizes. Im Dashboard müssen `avg_viewers` und `retention_days` als nullable behandelt werden. Aktuelle und abgeschlossene Stunden dürfen nicht doppelt gezählt werden; der Writer muss vollständige Rollups und seinen Datenstand zuverlässig pflegen.

Der ursprüngliche Collector-Arbeitsstand enthält noch parallele Änderungen an Konfiguration, Migration, Transport und Sendeschutz. Dieses Paket ersetzt sie nicht. Caddy deckt den neuen API-Pfad bereits über `/twitch/api/v2/*` ab; kein neuer öffentlicher Wildcard-Pfad und keine gelockerte Authentifizierung nötig.
