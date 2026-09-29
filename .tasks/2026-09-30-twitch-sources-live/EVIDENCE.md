# Quellenpakete für Challenges: Integrationsübergabe

Arbeitsbasis: `1a494222`, integriert durch den separaten Integrator. Kein Merge oder Deploy in dieser Übergabe.

## Behobene Reviewpunkte aus #998 und #999

- Discord-Broker erhält absolute, im echten dl-dashboard-Vertrag erlaubte HTTPS-Callbacks. Beide Twitch-Aufrufer teilen den bestehenden Brokerclient mit Statusprotokollierung.
- Mitgliedschaft ohne frischen Vollabgleich bleibt gesperrt. Der Wettbewerb nutzt den gemeinsamen zentralen Lesepool als `ContestCentralPool`-Extension.
- Einreichung und Stimme prüfen ihre Frist direkt im INSERT. Neustarts schließen auch vollständig verpasste leere Monate seit dem ersten bekannten Wettbewerbsmonat ab.
- Moderation verlangt einen Grund und speichert die echte Discord- oder Twitch-Sessionidentität. Anonyme interne Admin-Kontexte dürfen nicht moderieren.
- Die Seitenaktualisierung lädt sämtliche zuvor geladenen Clip- und Archivseiten neu und erhält die ausgewählte Archivseite. Der Browsernachweis prüft auch die begründete Moderation.
- Persönliche Einladungen fragen zuerst den Broker ab. Erst beim Ausfall wird der Kanallink gelesen; der Fehlerstatus wird protokolliert.
- Neue Claims bekommen ihre geprüfte Twitch-ID über den bestehenden Helixclient. Die Gutschrift liest über die eng begrenzte Funktion des Integrators nur Claims zur gleichen Ziel-ID. Unaufgelöste Altclaims zählen nicht.
- Interne Verifikation verändert keine fremde Twitch-ID bei wiedervergebenem Login. Identität und Partneraktivierung stehen in derselben Transaktion.

## Selbstprüfungen

- `rustfmt --edition 2021 --config skip_children=true` für alle geänderten Rust-Dateien: erfolgreich.
- Website: `npm ci`, 52/52 bestehende Tests, Production-Build und gezieltes ESLint: erfolgreich.
- Echter Chromium-Lauf des bestehenden `website/tests/clips-browser.mjs`: erfolgreich, einschließlich Abstimmung, Einreichen, Verbergen/Wiederanzeigen mit Grund, Logout, Fehler/Retry sowie 390/320-Pixel-Ansicht. Screenshot des Desktopzustands persönlich geprüft. Daten sind ausdrücklich synthetisch.
- Caddy-Partnerpaket: 15/15 Routentests und vollständiges `caddy adapt`: erfolgreich.
- Neue Datenbankregressionen: fehlender Mitgliedschaftsabgleich, Frist beim INSERT, komplett verpasste Monate, unveränderte Twitch-ID bei Login-Kollision, Identity-Rollback. Ausführung erfolgt zusammen mit den Rust-Suites im einzigen Integrations-Build, damit keine parallelen Targets den Host füllen.

## Verbindliche Integration vor Abschluss

Der Integrator besitzt `20260930010000_partner_challenge_runtime_roles.sql`, SQLx-Cache, Schema-Snapshot, normale Test-DB-Konfiguration, die dedizierte Contest-Schreibidentität, die beiden SECURITY-DEFINER-Lesefunktionen sowie `main.rs`/Router-Verdrahtung. Die Fixtures referenzieren diese echte Migration und führen deren Referral-Query im isolierten Testschema aus. Der vollständige Rollentest muss dagegen die originale Migration prüfen.

`ContestCentralPool` muss mit der vorhandenen gemeinsamen zentralen Poolinstanz eingefügt werden. Contest-Logout verwendet ausschließlich den vorhandenen Sessionpool. Keine Schreibrechte an `twitchdash`, keine Affiliate-Tabellenrechte an `twitchbot`.

Rust-Compiler/Suites, unabhängige Abnahme, gemeinsamer Merge-Gate, Migrationen, Deploy, Live-Nachweis und Cleanup stehen bei Übergabe noch aus. Diese Datei ist keine Fertigmeldung.
