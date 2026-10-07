# Auftrag E: TikTok Direct Post

## Veröffentlichungsvertrag

1. Das Dashboard lädt beim Öffnen aktuelle TikTok-Kontoeinstellungen. Die Sichtbarkeit hat keinen voreingestellten Wert. Interaktionen und Zustimmung sind zunächst ausgeschaltet.
2. Die Beschreibung enthält den vorgeschlagenen Titel, Beschreibungstext und Hashtags und bleibt vor der Bestätigung editierbar. Das gemeinsame Textfeld ist auf 2200 UTF-16-Einheiten begrenzt.
3. Die vollständige Freigabe enthält Konto-Zuordnung, Sichtbarkeit, Interaktionen, Werbekennzeichnung, Zustimmung und Prüfsumme der Videovorschau. Die serverseitig ermittelte Vorschau-Datei wird zusammen mit den Angaben gespeichert. Ein Trigger übernimmt die Angaben atomar in neue TikTok-Aufträge.
4. Vor dem Start werden die gespeicherte Konto-Zuordnung und der Bereich video.publish geprüft. Die Reservierung verhindert, dass während der Übertragung eine neue Freigabe denselben Auftrag verändert. Veröffentlicht wird eine Arbeitskopie des freigegebenen Videos, nicht ein neu gerenderter Clip.
5. Der Uploader lädt vor jeder Initialisierung die Kontoeinstellungen erneut und prüft Videodauer, Prüfsumme, Sichtbarkeit, Interaktionen und Werbung. Ohne gültige vollständige Freigabe gibt es keine Initialisierung.
6. Die Initialisierung verwendet ausschließlich /v2/post/publish/video/init/ und post_info. Keine automatische Umleitung ins TikTok-Postfach. Eine Beschränkung nicht auditierter Clients wird nicht umgangen.
7. Die Vorgangskennung wird vor der Dateiübertragung gespeichert. Unklare Ergebnisse werden mit derselben Kennung nachgeprüft. Nur PUBLISH_COMPLETE setzt den Clip auf veröffentlicht. FAILED und fehlende Bestätigung erscheinen im Dashboard. Frühere Postfach-Aufträge behalten ihre Status-Nachprüfung.

## Eingriffe und Grenzen

Neue Migration: 20261007042000_social_media_tiktok_direct_post.sql. Frühere Migrationen bleiben unverändert. Die drei neuen Spalten erscheinen im Schema-Snapshot. Neue Abfragen verwenden dynamisches SQL und keine neuen SQLx-Makros.

Keine Änderungen an den Bereichen der Aufträge A und D. Vor dem Gate folgt ein Rebase auf den aktuellen origin/main-Stand. Kein zusätzlicher Thread und kein eigenes Review.

## Gesonderte Live-Freigabe

Am 7. Oktober 2026 hat der Haupt-Orchestrator ausdrücklich genau einen privaten Testpost nach ALLOW, Merge und Deploy auf earlysalty erlaubt. SELF_ONLY muss manuell ausgewählt werden. Beschreibung und Musiknutzungsbestätigung werden im Dashboard bestätigt. Das Video muss ein bereits aufbereiteter eigener earlysalty-Clip mit Deadlock-Gameplay sein; die Vorschau wird angesehen und darf keine erkennbar fremde Musik enthalten. Andere Konten, weitere Posts und öffentliche Sichtbarkeit bleiben ausgeschlossen. Die Clip-ID gehört in den Abschlussbericht. Bisher wurde kein Post ausgeführt.
