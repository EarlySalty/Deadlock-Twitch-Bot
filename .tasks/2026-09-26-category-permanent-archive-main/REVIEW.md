# Kategoriearchiv auf aktuellem Main schützen

## Anlass und Umfang

Der bereits in `main` integrierte Kategoriesammler enthält noch `category_prune_partitions` mit `DROP TABLE` und `trim_expired_rows` mit `DELETE`. Der ältere Retention-Branch ist ein inkompatibler Prototyp; die ursprüngliche Migration `20260918123000_category_collector.sql` bleibt unverändert. Dieser Branch übernimmt nur den Archivschutz aus dem finalen Kategorie-Entwurf auf aktuellem Main: neue additive Migration, reine Messfunktion unter der alten Signatur, eng begrenzte gezielte Twitch-Entfernung, DB-gesteuerte Speicherpausen, Rollen, Dashboard-Wahrheit und Tests. Zusätzlich schließen gemeinsame transaktionsgebundene Raumsperren eine Race zwischen Rohchat-Einfügung und gezielter Entfernung. Der separate Antwortschutz des Hauptbots ist nicht Teil dieses PR.

## Vorherige Live-Prüfung am 26.09.2026

Read-only geprüft: `tb-category-collector.service` ist auf dem Host nicht installiert. Die bestehenden Tabellen `category_chat_messages` und `category_collection_runs` enthalten jeweils null Zeilen. Drei leere Chat-Partitionen für den 17. bis 19.09. sind vorhanden; `category_collector_config` fehlt noch. Es war kein aktiver Prune-Lauf zu beobachten. Diese Feststellung ersetzt weder Deploy noch längere Abdeckungsmessung.

## Integrationsfolge

Die additive Migration muss vor dem Collector-Start angewandt werden; danach sind die eingeschränkten Rollen mit `ops/systemd/category-runtime-roles.sql` einzurichten. Die alte 123000-Migration darf wegen angewandter Prüfsummen nicht überschrieben werden. Ein Rollback auf einen alten Collector-Stand mit generischer Löschung ist nur bei gestopptem Collector sinnvoll. Der Admin-Report und seine Auth-Routen bleiben beim bereits integrierten Main-Vertrag.

## Nachweise

Der fokussierte Rust-Check für Collector und Analytics ist grün. Zehn isolierte PostgreSQL-Tests für Archiv, Rollen, parallele Entfernung und Zustellung, Shared Chat und Reporting sowie vier Collector-Unit-Tests sind grün. Der Dashboard-Produktionsbuild und drei Archivansicht-Tests sind grün. Das Schema-Snapshot wurde nur um die 15 Spalten der neuen Tabellen ergänzt; die frische Gesamtmigration bleibt CI-Gegenstand. Der erste Gate-Lauf fand ein Snapshot-Race und einen Dashboard-Timer-Nit; beides ist korrigiert. Ein weiterer unabhängiger Rust-Review fand die fehlende personenbezogene CLEARCHAT-Zielmarke; auch sie ist mit Spätzustellungs-Regression ergänzt. Der erneute Selbstreview-Gate auf dem korrigierten Code urteilte `ALLOW` und bestätigte den behobenen Snapshot-Race. Eine unabhängige Abnahme des finalen Codes folgt. Kein Merge oder Live-Deploy aus diesem Branch.
