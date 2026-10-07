# Auftrag F: Manuelle Cliptexte ohne automatische Anreicherung

Auftraggeber: Haupt-Orchestrator, Thread d3a1741e-82bc-4a48-865b-2845c663dca7.
Worker: diese Sitzung, keine Delegation. Umfang: mittel nach Korrektur vom 7. Oktober 2026.

## Vertrag

- Enrichment-Worker nicht starten. Manueller Start-Endpunkt startet weder LLM noch STT.
- Pipeline-Code, Tabellen und vorhandene Daten bleiben erhalten.
- Editor bleibt für manuelle Titel, Beschreibungen und Hashtags. Automatische Vorschläge, Transkript und Neu-generieren-Aktion entfallen.
- Zuletzt gespeicherte Hashtags je Twitch-User-ID aus clip_last_hashtags wiederverwenden, per Klick einfügen. Keine neue Tabelle erforderlich.
- Vorschau und Upload rendern ohne Transkript-Untertitel; Branding, Titel, Layout, Download und Upload bleiben erhalten.
- Start meldet einmal, dass Clip-Anreicherung und Transkription abgeschaltet sind.
- Fremde Änderungen A, D, E nicht anfassen. Vor dem Gate auf frisches origin/main rebasen.

## Bestand

BESTAND[BS-1]: ja | Fundort: rust/crates/tb-social-media/src/clip_templates.rs:260 | Anknüpfung: vorhandene clip_last_hashtags-Speicherung über Twitch-User-ID und vorhandene manuelle Plattformfelder

Enrichment-Aufrufer: Bot-Start, Dashboard-Start-Endpunkt und der Harvest-Pfad in clip_context_learn. Alle drei Einstiegspunkte sind stillgelegt, Bibliothekscode bleibt erhalten. Gemeinsamer Renderpfad: render_clip_vertical, verwendet von Vorschau und Upload. Alte fertige Vorschauen werden logisch ausgeblendet; vorhandene Dateien und Daten bleiben unverändert. Neue Vorschauen verwenden den Namen *_preview_manual_v1.mp4.

Der Upload wartet nicht auf den Enrichment-Status. Der vorgeschaltete Freigabe-Selektor schloss jedoch Deadlock-Clips mit aktivierter Kategorie-Anreicherung aus. Diese Kategoriebedingung entfällt, damit ausstehende Clips auch ohne Transkript die Freigabe und danach die Upload-Warteschlange erreichen. ClipPrepWorker lädt lokale Dateien ohne Enrichment-Statusvoraussetzung.

## Prüfung und Abschluss

cargo fmt, cargo clippy, betroffene cargo test, Dashboard-Build. Gate ohne eigene Reviewer-Threads. Bei ALLOW Push HEAD:main, Release-Deploy über Wrapper, Neustart Bot und Dashboard, Journal und Dashboard prüfen. Danach eigene Arbeitskopie und Branch entfernen, Bericht und Selbstabschluss.
