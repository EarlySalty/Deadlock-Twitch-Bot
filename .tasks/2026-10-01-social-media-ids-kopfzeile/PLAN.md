# Plan

1. Graphify und Quellprüfung der Freigabe-, Scope-, Besitz- und Credential-Pfade.
2. Einmaliger Backfill; Twitch-ID als Pflichtfeld und Primärschlüssel. Nicht auflösbare Altfreigaben vollständig in einer Berichtstabelle erhalten und sichtbar melden, ohne Zugriff zu gewähren.
3. Verbliebene Social-Media-Zugriffs- und Besitzprüfungen auf IDs umstellen; gemeinsame Kopfzeilen-Komponente für Analyse und Social Media.
4. Isolierte PostgreSQL-Tests, Schema-Snapshot, SQLx-Cache, Rust fmt/clippy/tests und Frontend-Build/Tests; Selbstprüfung und Gate.
5. Main-Push und Produktivmutationen bis zum expliziten Root-Handoff nach vollständig geprüfter Challenges63-Aktivierung halten. Danach aktuellen Main integrieren, den tatsächlichen Integrationsstand erneut regulär prüfen und bei ALLOW pushen. Release vorbereiten, gekoppelte Prod-Migration mit sha384 protokollieren, deployen, Dashboard und Bot laden, Partnerzugriff und Kopfzeile live belegen, Bericht/Register und Cleanup.
