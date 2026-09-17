# Deadlock Brain im Twitch-Admin

Stand 17.09.2026. Route: `/twitch/admin/content/build-lab`, Menü
**Content & Comms → Build-Labor** auf dem Admin-Host.

## Architektur und Schutz

Die Dashboard-API bindet den bestehenden Rust-Reasoner aus dem öffentlichen
Repository `EarlySalty/Deadlock-Brain` auf Commit
`d8c34270868e129098e12243f53f5b52ee507b8b` ein. Cargo.lock fixiert auch die
transitiven Abhängigkeiten. HTTPS vermeidet neue SSH-Secrets auf CI-Runnern.
Die ältere Brain-main-Laufzeit und ihr Publisher werden nicht ersetzt.

- `GET /twitch/api/admin/brain/catalog`: Helden, Datenbank-Patch und Importzeiten.
- `POST /twitch/api/admin/brain/build`: frischer Read-only-Rechenlauf; ausschließlich
  Held und validierte Szenarioparameter. Keine KI, Persistenz oder Steam-Queue.

Beide Routen liegen im bestehenden Admin-/CSRF-Router und prüfen zusätzlich
Admin-Rechte im Handler. Der JSON-Client sendet den CSRF-Token nur im Header;
der Brain-Request lehnt unbekannte Felder ab. Antworten sind `no-store`.
Es gibt keinen frei wählbaren Backend-Host, Shellaufruf oder generischen SQL-Zugang.

Die Verbindung nutzt bevorzugt `DEADLOCK_BRAIN_READONLY_DSN`, sonst den über den
vorhandenen Service-Bootstrap bereits autorisierten `DEADLOCK_CENTRAL_DSN`.
Alle Pool-Verbindungen erzwingen `default_transaction_read_only=on`, haben
20 Sekunden SQL-Timeout und höchstens drei Verbindungen. Zugangsdaten und
interne DB-Fehler gelangen nicht ins Frontend. Es werden keine Rollen oder
Grants automatisch erweitert. Fehlende Konfiguration/Rechte ergeben HTTP 503.
Die Library begrenzt parallele Berechnungen auf zwei; Überlast liefert 429.
Das HTTP-Antwortlimit beträgt 150 Sekunden. Noch laufende CPU-Arbeit behält
bei Abbruch ihren Rechenplatz; `spawn_blocking` ist nicht zwangsabbrechbar.

## Bedienung und Aussagegrenzen

Held und Kampffenster, Kanal-Uptime, eingehender Waffenanteil sowie optionaler
DPS-Druck wählen, dann **Builds neu berechnen**. Alle belegten Varianten werden
berechnet und separat auswählbar. Pro Variante bleiben Kaufplan, Skillorder,
Imbues, Situationen und Item-Scores zusammen. Ein fehlender Familienscore wird
nicht durch einen globalen oder fremden Score ersetzt. Änderungen am Formular
entfernen den vorigen Bericht, damit kein alter Build unter neuen Parametern steht.

Item-Details zeigen Mechanikbelege, bedingte Aktiv-/Passivwerte, Meta-Anteil,
Wert je Seele/Slot und geladene Eigenschaften. Diese Einzelitem-Scores sind
keine Kaufreihenfolge, keine Prozent-Winrate und keine vollständige In-Game-
Simulation. Der vorhandene Planner bewertet Kombinationen und Käufe separat.

Quellzeiten, fehlende Zeitstempel, historische Modellwerte, Nach-Patch-Matches
und nicht zusätzlich angewandte Patch-Deltas sind sichtbar. Daten werden bei
jeder Berechnung neu gelesen, aber die Seite löst **keinen Datenimport** aus.
Ein neuer Patch-Tag allein macht Daten nicht aktuell. Der JSON-Prüfbericht
enthält alle Varianten und den Modell-Fingerprint zur Diagnose.

Dies ist ein experimenteller Test-Deploy, keine Gesamtfreigabe des Reasoners.
Die bekannten Holdout-/Mechanikgrenzen des Brain-Integrationsbranches bleiben
bestehen. Ein erfolgreicher Endpoint-Aufruf ist kein Nachweis sinnvoller
Builds für sämtliche Helden.

## Ausgeführte lokale Prüfungen

- Brain: 264 Reasoner-Lib-Tests bestanden, 16 ignoriert; Retrieval 22 bestanden,
  12 ignoriert. Clippy lib/examples für beide Crates mit `-D warnings` erfolgreich.
- Dashboard: fünf neue Backend-Tests bestanden (Auth, Partnerausschluss,
  Eingabevalidierung, Überlast und Fehlerredaktion).
- Admin-Frontend: zehn Tests bestanden, einschließlich drei Familienzuordnungs-
  Regressionstests; TypeScript/Vite-Produktionsbuild erfolgreich.
- Dashboard-Clippy `--no-deps` erfolgreich mit bestehenden Warnungen in
  `uplink_config.rs`. Strenges workspaceübergreifendes Clippy ist nicht grün:
  bestehende Lints in `tb-analytics/partner_signup_tag_block.rs` verhindern es.

Ein authentifizierter Live-Rechenlauf und ein frischer Asset-/Populationsimport
sind gesondert nachzuweisen. Der direkte Infisical-Loader ist im verwendeten
MCP-Zugang nicht freigegeben; es wurden weder Credentials ausgelesen noch
Sicherheitsgrenzen umgangen.
