# Vertrag und Nachweisziele

- Admin: Streamer-Detailseite übergibt `detail.twitchUserId`; Hook und Client verwenden dieselbe ID; JSON enthält `twitch_user_id`. Kein Schreibaufruf bei fehlender ID oder ungeladenem Freigabestatus.
- Schema: Der entfernte `ensure_schema` wurde im aktuellen Stand lediglich vom eigenen Test aufgerufen. Migrationen und Schema-Snapshot bleiben unverändert, weil keine Datenbankstruktur geändert wird. Der zugehörige tote Schematest entfällt zusammen mit der Komponente; die vorhandenen Migrationstests bleiben erhalten.
- SQLx: Die doppelte Upload-UPDATE-Abfrage entfällt. Ihr Cache `c537a3e6…` wird entfernt; der vorhandene Cache von `register_local_file` bleibt unverändert und wird mit Offline-Kompilierung geprüft.
- Download: Upload benutzt den gleichen atomaren Kern wie Prep und Vorschau; ein registrierter Cachetreffer löscht frühere Downloadfehler. Ein DB-Fehler wird nicht als erfolgreicher Upload-Vorbereitungsschritt behandelt. Fehlgeschlagene Downloads entfernen ihre eigene temporäre Datei.
- Retention: Clipfrist und Veröffentlichungs-/Verwerfstatus bleiben die Löschentscheidung. Beide gespeicherten Originalpfade und die daraus berechneten Plattformrenderpfade werden dedupliziert. Vorschau gehört ebenfalls dazu. Bei Dateifehlern bleibt die Clipzeile erhalten. Keine Verzeichnissuche nach Dateialter.
- Fetch: HTTP 502, `success: false`, Fehler des Diensts und aktueller Kontingentstand. Erfolg mit null Clips bleibt HTTP 200.

Funktionstests: realer yt-dlp-Download einer lokal per HTTP bereitgestellten MP4 mit echtem PostgreSQL; Cachetreffer, fehlgeschlagene Registrierung, atomare Konkurrenz und Teilfehler; Retention über reale Dateien und nicht löschbare Renderpfade; Fetch-Antwortstruktur.
