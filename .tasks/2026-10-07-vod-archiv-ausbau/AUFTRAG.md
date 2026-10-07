# VOD-Archiv ausbauen

Auftrag vom 7. Oktober 2026, Hauptthread d3a1741e-82bc-4a48-865b-2845c663dca7.

Bestehendes Rust-Archiv im Twitch-Bot zusammenführen, Verwaltungsseite im Social-Media-Manager ergänzen, Konfiguration zentralisieren. Fremde Streamer-Konten bleiben verbunden. Das Nebenrepo bleibt bis zur Nutzerfreigabe erhalten.

## Bereichsvertrag

Eigentum: tb-vod-archive, eigener Dashboard-Handler und Archiv-Tab, additive Config, neue Migration und Schema-Snapshot. R1-Upload-Worker, Retention der Social-Pipeline und A-Verbindungsmechanik werden nicht umgebaut. Vor dem Gate wird origin/main eingearbeitet.

## Nachweise

Funktionsvergleich des Nebenrepos, Datenabgleich nach Twitch-VOD-ID, Zugriffsprüfung nach Session-Twitch-ID, Formatierung, Clippy, bestehende Tests, Frontend-Build, Sichtprüfung, Merge-Gate und Live-Nachweis. Prod-Migration, Release und Neustart erfolgen erst nach ALLOW und Merge.
