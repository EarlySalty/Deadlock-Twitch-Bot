# Plan und Paketgrenzen

1. Bestehende TikTok-Übertragung auf individuelle Direct-Post-Freigaben umstellen. Creator-Abfrage, Caption-Grenze, Chunktransfer, Checkpoint und Status-Nachprüfung bleiben die wiederverwendeten Bausteine.
2. Neue Migration und Schema-Snapshot ergänzen. Dashboard-API und Freigabe-Dialog verbinden, Konto und Video eindeutig binden, Veröffentlichung erst nach TikToks Bestätigung anzeigen. Nur TikTok-spezifische Rechtstexte bearbeiten.
3. Betroffene Rust- und Dashboard-Prüfungen ausführen. Bestehende Tests, die Uploads ohne TikTok-Freigabe als erfolgreich festschreiben, an den neuen Vertrag anpassen und dies melden. Fremde Fehler anhand eines unveränderten Ausgangsstands messen, nicht pauschal als alt bezeichnen.
4. Aktuelles origin/main übernehmen und den lokalen Merge-Gate ausführen. Bei BLOCK die Befunde dem Haupt-Orchestrator übergeben; als Blatt-Worker keinen eigenen Fixer starten.
5. Nach ALLOW mergen, pushen, Migration vor dem neuen Dienstcode anwenden und den aktuellen origin/main-Stand über den Deploy-Wrapper aktivieren. Getrennt erlaubten privaten Live-Test durchführen, Beweise dokumentieren und eigene Arbeitsartefakte nach dem Abschluss aufräumen.
