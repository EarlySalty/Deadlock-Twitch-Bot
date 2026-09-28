status: aktiv (2026-09-28)

# Review-Runde 1: Patchfeed C, Commit 7593923f

Prüfer: T3-Thread `0e665ed3-6e4d-4446-8a39-adf211215555`, GPT 6 Astra. Urteil: fertig N, Fix nötig J. Keine Produktivfreigabe. A, B und D brauchen danach noch ein gemeinsames Integrationsreview.

1. **P1, Feed blockiert nach Ablauf:** `rust/bin/tb-bot/src/patch_feed.rs:215-228`. Pending 286 ist älter als 120 Sekunden und fehlt im Index oder sein Artikel liefert dauerhaft 404. Der Code erreicht den Callback nie und stoppt 287 auf Dauer. Abgelaufene Pending-Zeile atomar mit sichtbarem Grund terminal abschließen, ohne `observed_at` zu erneuern; jüngere Ereignisse danach bearbeiten. Während der gültigen Frist weiter fail-closed stoppen.
2. **P1, ID ist nicht Publikationsreihenfolge:** `rust/bin/tb-bot/src/patch_feed.rs:222-226,284,319`. Nach Baseline 285 scheitert Web-Veröffentlichung 286, 287 gelingt, 286 wird danach erstmals veröffentlicht. `id <= last_processed_patch_id` verwirft 286 dauerhaft. Der bestehende Publisher verarbeitet nach Fehlern weiter (`/home/nathanael/.worktrees/patchnotes-live-main/web_publish.py:568-594`). Bestehende IDs beim Start und spätere erstmals gesehene IDs einzeln und dauerhaft unterscheiden; die numerische Cursor-Tabelle ist überholt. Test `patch_feed.rs:668-678` darf den Verlust nicht mehr festschreiben.
3. **P2, leerer Erstindex:** `rust/bin/tb-bot/src/patch_feed.rs:213-214,241-245`. Ein vorübergehend leeres HTTP-200-Array wird als Baseline 0 gespeichert; der wiederkehrende Altbestand würde als neu gelten. Bei leerem Erstindex keinen Initialisierungsmarker schreiben. Eine bereits vorhandene Baseline nicht löschen.

Prüfgrenze: Die zehn C-Tests sind Fakes; kein Test lief gegen die produktiven SQL-Abfragen oder Parallelität. C und B müssen DDL, Transaktionen und einen isolierten PostgreSQL-Lauf zusammen belegen. Stabile `source_url`, HTTPS-Domain, Weiterleitungsablehnung und Artikelprüfung waren unauffällig. Ein bereits erfolgreich verarbeiteter Callback kann nach Datenbankfehler erneut erreicht werden; B muss per stabilem Ereignisschlüssel und vorab gespeichertem Versuch eine zweite externe Sendung verhindern.
