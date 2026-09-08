status: aktiv
datum: 2026-09-08
klasse: mittel

# Vertrag: Lokaler Twitch-Modellvergleich

## Ziel und Freigabe

Der Nutzer hat nach der Recherche ausdrücklich „okay try it daten hast du ja genug“ gesagt. Genehmigt ist der lokale Vergleich von Qwen3.5 4B und 9B mit echten zurückgehaltenen Twitch-Situationen. Nanis Schreibstil gilt ausschließlich für Twitch. Der Versuch verändert weder den Concierge noch das produktive Modell.

## Anforderungen

- REQ-01: Ein reproduzierbarer Rust-Aufruf liest einen eingefrorenen privaten Datensatz und eine normale Config-Datei, erzeugt lokale Antworten und einen privaten Vergleichsbericht.
- REQ-02: Beide Kandidaten werden möglichst an denselben 50 Fällen verglichen. Mindestens zehn identische Fälle erhalten zusätzlich eine Baseline ohne individuelle Stilbeispiele. Unvollständige Läufe bleiben ausdrücklich unvollständig.
- REQ-03: Der verbesserte Prompt verbindet eine Twitch-Systemkarte, zwei bis vier passende ältere Betreiberbeispiele, ausschließlich vorherigen Kontext und bei Bedarf belegtes öffentliches Wissen.
- REQ-04: Referenzantworten und zukünftige Chat-/Audioabschnitte sind niemals Eingabe des Generators. Stilbeispiele stammen aus anderen Kanälen und liegen zeitlich vor dem Fall.
- REQ-05: Bericht und Rohantworten liegen ausschließlich lokal mit privaten Dateirechten. Terminalausgaben enthalten nur Aggregate und keine Rohchats.
- REQ-06: Der Bericht nennt Latenz Median/P95, Tokens soweit geliefert, Kontextumfang und Fehler. Sprachurteile bleiben von Messwerten getrennt; historische Reaktionen beweisen keine Konversion der generierten Alternative.
- REQ-07: Der Nutzer kann echte Referenzen und Modellantworten in einer lokalen, HTML-escaped Vergleichsansicht ohne externe Assets prüfen.

## Invarianten

- INV-01: Jeder Modellaufruf läuft durch den bestehenden zentralen tb-llm-Connector. Lokale Evaluation ist ausdrücklich typisiert und standardmäßig nicht gebaut.
- INV-02: Produktive Anbieter-/Modellfreigaben bleiben unverändert. Kein lokaler Ausfall darf auf einen Cloudanbieter zurückfallen.
- INV-03: Der lokale Pfad erlaubt ausschließlich numerisches Loopback-HTTP ohne Benutzerinformation, Query oder Fragment, ohne Proxy oder Redirect und ohne Secrets.
- INV-04: Keine Nachrichten, Sender, Produktions-DB-Schreibzugriffe, Training, Diensteänderungen oder Deploys im Versuch.
- INV-05: Kein Concierge-Code und keine Übertragung von Nanis Stil außerhalb Twitch.
- INV-06: Keine Rohdaten, Referenzen oder Nutzerbeispiele an externe Coding-/Review-Modelle; dort ausschließlich Schema und synthetische Fixtures.
- INV-07: Keine neuen Environment-Variablen oder ENV-Dateien zur Konfiguration, keine Secrets im Klartext lesen, ausgeben oder schreiben.

## Erlaubter Änderungsbereich

- rust/crates/tb-llm/
- rust/Cargo.lock, ausschließlich notwendige Dependency-Metadaten
- .tasks/2026-09-08-lokaler-twitch-replay/
- Private Versuchsartefakte unter /home/nathanael/.local/share/twitch-local-eval/, niemals in Git

## Nicht-Ziele und verbotene Änderungen

- Produktiver Modellwechsel, Live-Outreach, autonome Einladungen, Änderungen an Concierge, Sendern, Twitch-/Discord-APIs, Migrationen und anderen Crates.
- Ein vollständig autonomes Onboarding anhand eines Offline-Sprachvergleichs als fertig erklären.
- Behauptete menschliche Freigabe oder Konversionswirkung aus einem automatischen Urteil ableiten.

## Offene Produktfragen

Keine für den genehmigten lokalen Versuch.

## Amendments

- 08.09.2026, entschieden von Orchestrator: INV-04 „keine Diensteänderungen“ betrifft bestehende Produktionsdienste. Die eigene isolierte Test-Unit für llama.cpp ist vom genehmigten lokalen Versuch umfasst. Root hat die tatsächlich durchgesetzten CPU-/RAM-Grenzen bestätigt. Keine Änderung an Bot- oder Dashboard-Units.
- 08.09.2026, entschieden vom Nutzer: „ja okay ne dann lass uns das erstmal um deep seek rum bauen doer ein Lokales modell testen“. Der begrenzte Vergleich darf das bereits freigegebene DeepSeek V4 Flash bei Fireworks als zusätzliche Vergleichsstrecke nutzen. Nur ausgewählter vorheriger Twitch-Kontext, passende ältere Stilbeispiele und belegtes Wissen werden übertragen; Referenzantworten, Metadaten und der gesamte Datensatz bleiben lokal. Keine weiteren Cloudanbieter und kein Produktionsmodellwechsel. REQ-01 umfasst damit auch diese ausdrücklich gewählte vorhandene Cloudstrecke.
- 08.09.2026, entschieden vom Orchestrator nach der vorausgegangenen ausdrücklichen Rückfrage zur Coding-Ausnahme: Das erneute aufgabenbezogene Go erlaubt Codex für diese begrenzte Implementierung und Prüfung. Dies ersetzt den dokumentierten offenen Coding-Blocker, keine pauschale Modellfreigabe.
- 08.09.2026, entschieden vom Orchestrator: Die explizite DeepSeek-Replaystrecke nutzt ebenfalls ausschließlich tb-llm, eine fest verdrahtete Fireworks-Adresse und Modellkonstante sowie den vorhandenen direkten Infisical-Zugriff ohne ENV-Brücke. Lokale Ausfälle bleiben lokale Fehler und lösen niemals einen Cloudaufruf aus. Der Versuch schreibt kein produktives Usage-Ledger; tatsächliche Tokens werden privat protokolliert.
- 08.09.2026, entschieden vom Orchestrator: Alle drei Kandidaten bekommen dasselbe Ausgabehöchstbudget von 160 Tokens, Temperatur 0,4, deaktiviertes Denken und 600 Sekunden maximale Laufzeit pro Aufruf. Pro Kandidat werden die 50 Fälle vollständig seriell und zehn identische Baselines ausgeführt. Einzelne Fehler bleiben als Ergebnis erhalten. Ein vom Root angeordneter Abbruch wegen Produktionslast wird ausdrücklich als unvollständig dokumentiert.
- 08.09.2026, endgültige Ausführungsentscheidung des Orchestrators im Rahmen von „oder ein lokales Modell testen“: Diese Runde führt ausschließlich die zwei lokalen Modelle Qwen3.5 4B und 9B aus, jeweils 50 Fälle und zehn Baselines. Der Bestand hat keinen sicheren direkt wiederverwendbaren ENV-freien Cloud-Bootstrap. Es wird kein neuer Secret-Leseweg gebaut. Die zuvor erlaubte Cloudstrecke wird in dieser Runde weder implementiert noch aufgerufen, ihre Ergebnisse werden nicht behauptet. Sämtliche echten Generatorinputs bleiben lokal; DeepSeek bleibt unverändert produktiv. Die gemeinsamen Parameter 160 Tokens, 0,4 und 600 Sekunden gelten für beide lokalen Kandidaten.
