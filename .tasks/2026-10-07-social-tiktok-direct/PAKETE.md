# Paketgrenzen

Alle Pakete werden in diesem einen Blatt-Worker bearbeitet. Keine weiteren Threads.

| Paket | Schreibbereich | Vertrag | Stand |
| --- | --- | --- | --- |
| E1 | tb-social-media Uploader, Worker, Recovery | Individuelle Freigabe, Direct Post, Konto- und Video-Bindung, unverwechselbare Übertragung, Status | Implementiert und betroffene Tests bestanden |
| E2 | Neue Migration, Schema-Snapshot, Dashboard-API | Atomarer Job-Snapshot, geschützte Creator-Abfrage, Freigabespeicherung, sichtbarer Providerstatus | Frische Migrationen und betroffene API-Tests bestanden |
| E3 | TikTok-Dialog, API-Client, Clipkarte, TikTok-Rechtsabsätze | Explizite Sichtbarkeit, Interaktionen, Werbung, Zustimmung, Beschreibung und Verarbeitung | Bundle, Verträge und Browserprüfung bestanden |

Aufträge A und D bleiben fremde Zuständigkeiten. Übernahme ausschließlich per Rebase auf origin/main. Die einzige Codeprüfung ist der lokale Merge-Gate. Bei BLOCK übernimmt ein frischer Fixer des Haupt-Orchestrators.
