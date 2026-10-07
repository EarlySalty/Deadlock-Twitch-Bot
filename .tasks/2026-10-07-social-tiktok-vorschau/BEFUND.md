# TikTok-Freigabe wartet auf eine nicht gestartete Vorschau

status: aktiv, 2026-10-07

Aktenkopie des Erstbefunds der Hauptsession. Keine neue Laufzeitprüfung.

## Prüfung

Am 7. Oktober 2026 gegen 23:30 Uhr wurden ausschließlich lesende Abfragen gegen die produktive Datenbank `twitch_analytics` und den Quellstand der laufenden Dienste ausgeführt. Beide Dienste sind aktiv. Die ausführbaren Dateien von Bot und Dashboard stammen aus Release `2ead4d556327596fcc7d9feeaceb848e910cf8f8`. Kein Neustart, keine Datenänderung und keine Veröffentlichung wurden ausgelöst.

Der geteilte Checkout liegt hinter origin/main und enthält fremde Änderungen. Die Quellprüfung erfolgte deshalb mit `git show` gegen den produktiven Commit. Die drei Kernstellen TikTokPostDialog.tsx, social_media_tiktok_direct.rs und preview.rs unterscheiden sich zum lokal bekannten origin/main b0bd68248c3accc1771e938e6166c3a122ac154e nicht.

## Ursache

Die Meldung „Bitte warte, bis die Videovorschau fertig ist.“ ist irreführend. Beim betroffenen Clip 124768 wurde keine Vorschau angefordert: preview_status, preview_path, preview_error und preview_updated_at sind sämtlich NULL. Es läuft für diesen Clip kein Vorschauauftrag.

Der TikTok-Dialog ruft beim Öffnen ausschließlich fetchTikTokCreatorInfo auf. Der Knopf „Erneut laden“ wiederholt nur diese Abfrage. Keiner der beiden Wege fordert eine Vorschau an.

Der Rust-Handler duration verlangt dagegen PREVIEW_READY und meldet für jeden anderen Zustand einschließlich NULL pauschal, dass gewartet werden müsse. Der Vorschau-Worker verarbeitet nur pending oder veraltete rendering-Aufträge. Ein NULL-Zustand wird nicht von selbst verarbeitet. Dadurch entsteht eine dauerhafte Sackgasse.

## Live-Befunde

- Clip 124768, „BABA NOOO why not Soloman“: Status approved, keine Vorschau, keine gespeicherte TikTok-Freigabe.
- YouTube-Auftrag 5 wurde am 7. Oktober um 18:02 Uhr deutscher Zeit abgeschlossen.
- TikTok-Auftrag 6 scheiterte am 7. Oktober um 18:00 Uhr deutscher Zeit an fehlenden Veröffentlichungseinstellungen. Es handelt sich um lokale Validierung vor dem eigentlichen Upload, nicht um einen belegten TikTok-Ausfall.
- Der gleichnamige Clip 124767 ist für den 8. Oktober um 18:00 Uhr eingeplant und besitzt ebenfalls weder Vorschau noch TikTok-Freigabe.
- Beim Konto earlysalty stehen insgesamt 26 TikTok-Aufträge auf pending. Alle 26 haben NULL in tiktok_post_options und können den aktuellen Upload-Prüfpfad so nicht passieren. Weitere vier Einträge sind failed. Nicht alle vier sind vom aktuellen Vorfall.

## Quellbelege im produktiven Commit

- bot/dashboard_v2/src/components/socialmedia/TikTokPostDialog.tsx:23 und :77: Kontextabfrage und reiner Wiederholungsaufruf, kein Renderstart.
- rust/crates/tb-dashboard-api/src/handlers/social_media_tiktok_direct.rs:86 bis :98: fehlende Vorschau wird als laufende Vorschau gemeldet.
- rust/crates/tb-social-media/src/preview.rs:33: vorhandener request_preview-Pfad.
- rust/crates/tb-social-media/src/preview.rs:129: Worker übernimmt nur angeforderte Vorschauen.
- rust/crates/tb-social-media/src/upload_worker.rs:967 bis :981: fehlende Zustimmung oder Einstellungen blockieren den TikTok-Upload.
- bot/dashboard_v2/src/pages/SocialMedia.tsx:2308 und :2611: Im Clipmenü existiert „Vorschau rendern“ bereits.

BESTAND[BS-1]: ja | Fundort: rust/crates/tb-social-media/src/preview.rs:33 | Anknüpfung: bestehenden Vorschauauftrag und Statusabfrage im TikTok-Freigabeablauf verwenden.

## Empfohlene Korrektur

Beim TikTok-Freigabeablauf fehlende Vorschauen gezielt über den vorhandenen Renderweg vorbereiten und den echten Zustand anzeigen. NULL, pending, rendering und error dürfen nicht dieselbe Wartemeldung ergeben. Fertige Vorschauen wiederverwenden, laufende Aufträge nicht fortlaufend zurücksetzen. TikTok-Einstellungen und Zustimmung bleiben eine ausdrückliche Entscheidung des Nutzers. Bestehende Aufträge ohne diese Angaben müssen zur Freigabe geführt werden, statt erst am Veröffentlichungstermin zu scheitern. Ein bereits abgeschlossener YouTube-Upload darf dabei nicht wiederholt werden.

Als Zwischenweg lässt sich im Clipmenü „Vorschau rendern“ auslösen und nach Fertigstellung die TikTok-Freigabe erneut öffnen. Dieser Weg wurde anhand des Codes geprüft, nicht im Browser ausgeführt.

Noch kein Fix implementiert oder deployt.
