# VOD-Archiv im Social-Media-Manager

Im [Social-Media-Manager](https://deutsche-deadlock-community.de/twitch/social-media?view=archiv) stehen im Tab **VOD-Archiv** fertige Sicherungen und offene Uploads. Für einen eigenen Kanal gilt die bestehende Social-Media-Freigabe; die Verwaltung kann mehrere Kanäle sehen. **Erneut versuchen** merkt den Upload für den nächsten Archivlauf vor, **Auf Drive ausweichen** wählt Drive ausdrücklich, **Aus der Liste ausblenden** entfernt den Eintrag aus der Ansicht und erhält die Sicherung.

## Routenübergang

Der Archivtab liegt mit dem Manager unter `/twitch/social-media?view=archiv`. Die bisherigen Manageradressen leiten direkt dorthin weiter; offene Tabs können ihre bisherige API weiter verwenden. Verträge und geschützte Plattformadressen stehen in [Twitch-Pfadmigration](TWITCH_PATH_MIGRATION.md).

## Betriebsdatei

Die Archiveinstellungen stehen unter `[bot.vod_archive]` in der normalen TOML-Betriebsdatei. Ohne eigenen Abschnitt gelten diese Defaults:

```toml
[bot.vod_archive]
download_dir = "data/vod-archive"
max_downloads_per_run = 6
max_uploads_per_run = 3
min_free_gb = 80
download_timeout_seconds = 21600
interval_hours = 12
ffmpeg = "ffmpeg"
ffprobe = "ffprobe"
rclone = "rclone"
category_id = "20"
title_template = "{title} [{date}]{part}"
```

`rate_limit` und `playlist_id` sind optionale Zeichenketten. Die Playlist-Zuordnung prüft vor dem Hinzufügen, ob das Video bereits enthalten ist. Eine gesetzte Playlist benötigt entsprechende Rechte des verbundenen YouTube-Kontos. Der Default enthält keine Playlist.

Relative `download_dir`-Pfade beziehen sich auf das Arbeitsverzeichnis des Botdienstes. Der bisherige Default erreicht dessen bestehenden Daten-Bind-Mount. `yt_dlp_binary` und `vod_export_remote_base` bleiben die bereits vorhandenen typisierten Botoptionen. Der Standard für den Drive-Zielpfad ist `gdrive:Deadlock/Twitch-VODs`; rclone benötigt die beim Dienstkonto eingerichtete Verbindung.

Die bisherigen `TB_VOD_ARCHIVE_*`-Betriebsvariablen werden vom Archiv nicht mehr gelesen. Zugangsdaten bleiben im bestehenden Infisical- und verschlüsselten Datenbankpfad. Ein fehlender YouTube-Zugang hält den Upload an, aber nicht den lokalen Download. Drive wird dadurch nicht von selbst ausgewählt.

## Temporäre Dateien

YouTube-Kopien werden erst nach bestätigter Verarbeitung sämtlicher Teile lokal entfernt. Bei Drive müssen Übertragung, Dateigröße am Ziel und Ziellink bestätigt sein. Fehler erhalten die lokale Kopie für einen weiteren Versuch. Das Dashboard gibt keine lokalen Pfade oder rohen Werkzeugfehler aus.

Der frühere EventSub-Drive-Export wird nicht mehr als Laufzeitpfad aufgebaut. Die automatische Sicherung wird durch den vorhandenen Kanalschalter `social_media_vod_archive.enabled` gesteuert. Das neue Dashboard führt dieselben Archivdatensätze weiter; der historische SQLite-Bestand wird nicht neu eingespielt. Der detaillierte Abgleich steht in `.tasks/2026-10-07-vod-archiv-ausbau/VERGLEICH.md`.
