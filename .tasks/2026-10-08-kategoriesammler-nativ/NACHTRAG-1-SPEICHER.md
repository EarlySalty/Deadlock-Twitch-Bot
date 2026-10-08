# Nachtrag 1: Speicherform des Kategoriesammlers

Gehört zu diesem Auftrag. Er betrifft denselben Code, deshalb liegt er beim selben Agenten. Reihenfolge: zuerst den Umbau in den Bot (AUFTRAG.md) live bringen, dann diesen Nachtrag als eigene Commits auf demselben Branch oder als direkt folgenden Branch. Nicht vermischen, falls das den Gate-Diff sprengt (Schwelle etwa 150 KB je Gate-Lauf).

## Messwerte vom 2026-10-08

- Chat: rund 700.000 Nachrichten am Tag, rund 600 MB am Tag inklusive Index, hochgerechnet etwa 18 GB im Monat. Eine Zeile belegt im Schnitt 638 Byte. Davon entfallen 35 Byte auf `message_text` und 464 Byte auf die rohen IRC-Tags in `tags jsonb`.
- Snapshots: Jede Minute wird für jeden Live-Stream eine volle Zeile geschrieben, das sind etwa 10 GB im Monat. Am letzten Tag waren es 733.639 Zeilen bei nur 6.987 unterschiedlichen Kombinationen aus `(stream_id, title, tags, thumbnail_url, is_mature, language)`. Die statischen Felder sind damit zu 99 % Wiederholung.

## Nutzerentscheidung

1. **Partner lokal, alle anderen nur in der Cloud.** Chat und Stream-Snapshots von Kanälen, die kein Partner sind, liegen dauerhaft nur in Google Drive. Bei Partnern bleiben beide lokal in Postgres. Maßgeblich ist der Partnerstatus über die Twitch-User-ID aus dem bestehenden Partnermodell des Bots, nicht der Login.
2. **Snapshots normalisieren, auch den Altbestand.** Ein einziges Format, kein Nebeneinander von alt und neu. Statische Stream-Felder (Titel, Tags, Vorschaubild, Mature, Sprache) werden nur bei Änderung geschrieben, etwa als Versionstabelle mit `valid_from`. Pro Minute bleiben nur `snapshot_at`, `stream_id`, `viewer_count` und Verweise. Die vorhandenen 8,3 Mio. Zeilen seit 2026-09-27 werden einmalig verlustfrei in dieses Format überführt. Beweis vor dem Entfernen der alten Tabelle: Eine Rekonstruktion aus neuem Format ergibt Zeile für Zeile denselben Inhalt wie das Original (Zählung und Prüfsumme je Tag). Erst danach wird die alte Form entfernt und der Platz freigegeben. Dashboard und Auswertungen lesen direkt das neue Format. Nur neue additive Migrationen verwenden, alte Migrationen nie ändern.

## Vorgaben für die Cloud-Ablage

- Bestehenden Drive-Weg wiederverwenden: rclone, Remote `gdrive:`. Er wird vom VOD-Archiv schon benutzt, siehe `Docs/workspace/twitch-bot.md`. Keinen zweiten Google-Connector bauen. rclone muss für den Dienstnutzer des Bots eingerichtet sein. Der Drive hat 4,7 TiB frei.
- **Pflicht: vor dem Upload clientseitig verschlüsseln** (rclone-`crypt`-Remote über `gdrive:` oder gleichwertig). Der Schlüssel liegt in Infisical. Hintergrund: Laut den harten Regeln gehen Community-Daten nie unverschlüsselt an externe Anbieter. Google darf nur Chiffretext sehen.
- Format: je UTC-Tag eine komprimierte Datei pro Datenart (zstd-JSONL oder Parquet). Die Roh-Tags dabei auf die ausgewerteten Felder eindampfen.
- Ablauf je abgeschlossenem Tag: Nicht-Partner-Zeilen exportieren, hochladen, per Prüfsumme bzw. `rclone check` verifizieren, erst danach lokal entfernen. Bei einem Fehler bleibt alles lokal liegen, und es kommt eine entprellte Meldung (höchstens eine pro Tag). Der Ablauf muss idempotent sein und nach einem Neustart wieder aufsetzen. Ein Manifest pro Tag (Datei, Zeilen, Prüfsumme) bleibt lokal in Postgres.
- Stundenaggregate (`category_chat_rollup`) und Kennzahlen bleiben für alle Kanäle lokal, damit Dashboard und Kategorie-Analyse weiter funktionieren. Ein Rückholweg für einen Tag aus Drive als CLI-Unterbefehl gehört dazu.
- Die Doku-Zusage „keine Alterslöschung“ wird angepasst: Lokal wird nach der verifizierten Auslagerung gelöscht. Der Gesamtbestand bleibt in Drive erhalten. Die DB-Rechte für das lokale Entfernen eng fassen, nur für den Auslagerungspfad.
- Vor dem ersten echten Löschlauf einen Trockenlauf mit Zahlen vorlegen (Zeilen und Bytes je Tag, Partner gegen Nicht-Partner). Danach den Altbestand seit 2026-09-27 einmalig auslagern.
- Hinweis: rclone nutzt noch die geteilte Google-Client-ID, die Google 2026 zurückzieht. Das ist nicht Teil dieses Auftrags, gehört aber in den Bericht als offenes Risiko.
