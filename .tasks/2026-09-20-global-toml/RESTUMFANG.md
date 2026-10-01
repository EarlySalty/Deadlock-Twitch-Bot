# Restumfang nach Paket 2a

Quellscan des Integrationsstands 20.09.2026: **217 Leserstellen insgesamt**, davon **82 direkte operative Leser**, 50 direkte Credentialleser, 76 dynamische Hilfsleser/Makrostellen, 6 Test-DSNs, 2 OS-HOME-Zugriffe und ein historischer Secret-Dateipfad. Keine gelesenen Betriebs-/ENV-Werte. Die 508 Stringkandidaten sind keine Einstellungsanzahl.

| Weiteres Bündel | Noch direkte operative Leser | Dynamische Leser/Makrostellen (gemischt) |
|---|---:|---:|
| Dashboard-Binary/API, Auth, öffentliche URLs, Billing-Katalog | 31 | 25 |
| Bot-Binary, Chat, Raid, Internal-API, Monitoring, DB-Retry | 38 | 23 |
| Engagement, STT, Social/VOD, Stream-Audit, Last, zentraler LLM-Anschluss | 13 | 28 |

Dynamische Stellen sind keine 76 zusätzlichen operativen Felder: sieben sind bereits eindeutig reine bestehende Credential-Einstiege (Bot-/Dashboard-runtime_settings, Community-Broker-token, Self-Explainer-Broker-token, tb-llm-key/ledger, Stream-Audit-Broker-token). Die übrigen 69 enthalten echte Betriebsparameter, weitere Secrets und/oder obsolete Override-Warnlogik. Ein Helfer kann viele Felder lesen. Die genaue Anzahl **einzelner** offener Einstellungen folgt erst aus der Aufrufmatrix; keine pauschale Hochrechnung aus Stringkonstanten.

Arbeitsbündel: (A) Dashboard/Auth inklusive getrenntem internem Clientziel, (B) Bot-/Chat-/Raid-/Monitoringregeln mit unterschiedlichen bisherigen Fallbacks, (C) Medien/Last/Nebenprozesse. Danach Startskripte/Installer und Gesamtprüfung. Kleine baubare Commits innerhalb dieser Bündel; gemeinsame Abnahme/Gate je zusammenhängendem Lese-/Schreibpaket statt je Einzelwert. Produktionswerte und Browserverbindung bleiben gesonderte Liveblocker.

Quellfallen: `admin_affiliate::env_secret` liest trotz seines Namens öffentliche URLs, und `Settings::from_env` wird noch vom Zuschauer-Register-Backfill benutzt. Beide bleiben ausdrücklich im gemischten/operativen Rest, nicht als reine Secrets abgehakt.
