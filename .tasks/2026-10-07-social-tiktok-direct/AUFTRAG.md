# Auftrag E: TikTok Direct Post

Auftraggeber: Haupt-Orchestrator, Thread d3a1741e-82bc-4a48-865b-2845c663dca7.
Rolle: Blatt-Worker, keine Delegation.

## Ziel und Grenzen

TikTok direkt veröffentlichen, Caption übernehmen, vorgeschriebene individuelle Postingwahl im Dashboard erfassen und am Job speichern. Creator-Info beim Öffnen und vor dem Upload lesen. Unvollständige oder nicht mehr passende Jobs anhalten. Veröffentlichung bis zum Endstatus verfolgen. TikTok-Rechtstexte aktualisieren.

Auftrag A besitzt den Zugangserneuerungspfad, Auftrag D die übrigen Upload-Zustände, Plattformfähigkeiten und Rechtstexte. Vor dem Gate auf origin/main rebasen. Keine bestehenden Migrationen ändern, keine neuen Kommentare, kein Python-Anwendungscode, keine Secrets in Nachweisen.

Echter Testpost erst nach ausdrücklicher Freigabe durch den Haupt-Orchestrator, ausschließlich SELF_ONLY auf earlysalty. Bis dahin nur lesende Live-Prüfung.

## Bestand

BESTAND[BS-1]: teilweise | Fundort: rust/crates/tb-social-media/src/uploaders/tiktok.rs:150 | Anknüpfung: Creator-Abfrage, Caption-Bau, Chunktransfer, Upload-Checkpoint und Status-Recovery wiederverwenden; Inbox-Fallback entfernen.

## Umsetzung

1. Typisierte Postingwahl samt Creator-Identität und Dauerprüfung. Neue Migration für Jobwahl und Publish-Status.
2. Bestehende Einplan- und Freigaberouten um aktuelle Creator-Abfrage und validierte Wahl ergänzen. Auswahl atomar vor Freigabe speichern.
3. Pflichtformular an der Clipkarte, mit Vorschau der tatsächlich gesendeten Caption, ohne Privatsphäre- oder Interaktionsvorauswahl.
4. Produktionsfabrik mit validierter Jobwahl verdrahten. Wiederanlauf bleibt checkpointbasiert, kein zweiter Transfer nach unklarem Ergebnis.
5. Formatter, Clippy, betroffene Tests, Dashboard-Build und Sichtprüfung. Gate, Merge, Migration, Deploy und freigegebener Live-Test.
