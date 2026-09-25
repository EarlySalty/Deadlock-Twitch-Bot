# Verbindliche Nutzerkorrektur vom 18.09.2026

Der Nutzer hat nach der Unterbrechung ausdrücklich festgelegt:

> Mach mal keine generelle Datenlöschung. Unser Bot darf nicht auf Viewer-Bots oder Scam-Bots reagieren. 90 Tage finde ich nicht gut, ich will die Daten zukünftig auswerten können.

Diese Anweisung ersetzt die widersprechenden Retention-Vorgaben in AUFTRAG, SCOPE-R2, REVIEW-NOTES, OPS-CHECK und API-CONTRACT.

- Keine pauschale zeitgesteuerte Löschung oder Kürzung von Rohchat, Snapshots oder Metadaten. Kein 90-Tage-Maximum, kein versteckter anderer Grenzwert. Rohdaten dauerhaft behalten.
- Rollups sind zusätzliche Auswertungen, kein Ersatz für gelöschte Rohdaten.
- 7/30/90 Tage im Dashboard sind Ansichtsfenster, keine Aufbewahrungsfristen. Anzeige: „Keine automatische Löschung“.
- Datenträger überwachen und bei Engpass sichtbar warnen bzw. neue Erfassung kontrolliert anhalten; Bestandsdaten nicht löschen. Speicherbedarf muss am realen Wachstum gemessen werden.
- Bot-/Scam-Verdacht markieren und für Auswertungen filterbar halten, nicht als Löschgrund verwenden. Keine automatische Einstufung stiller Zuschauer als Bots.
- Gezielte rechtlich erforderliche Löschungen bleiben ein separater Vorgang. Ein allgemeines CLEARCHAT darf nicht den gesamten historischen Datenbestand eines Kanals vernichten.
- Collector bleibt strikt passiv, ohne Bot-Chatkonto, ChatApi, Moderation oder Whispers. API-Zugang darf vorhandene Helix-Credentials verwenden; keine neue ENV-Konfiguration.
- Der antwortende Bot darf bekannte Bots, gesperrte Identitäten und erkannte Spam-/Scam-Auslöser nicht begrüßen, mit ihnen Smalltalk führen oder durch sie Promos/Befehlsantworten auslösen. Das erweitert nicht die autorisierten Zielkanäle.

## Parallele Umsetzung, keine Dateikollision

A (50f38cec) setzt den Collector-/Storage-Anteil im vorhandenen Collector-Worktree um; Nachricht per T3 am 18.09.2026 zugestellt.
B (744d1bee) korrigiert die Retention-Anzeige im vorhandenen Frontend; Nachricht per T3 zugestellt.
Der ergänzende Antwortschutz wird getrennt auf `fix/ignore-bots-reaction-guard-20260918` unter `/home/nathanael/repos/tb-reaction-guard-20260918` implementiert. Dieser Arbeitsgang besitzt `tb-chat`-Antwortschutz und zugehörige Tests. Andere Orchestratoren bitte dort nicht parallel editieren; den geprüften Commit anschließend in den Collector-Integrationsstand übernehmen.

Noch kein Live-Erfolg durch dieses Dokument behauptet. Tests, Push, Deployment und tatsächlich laufende Version werden separat belegt.
