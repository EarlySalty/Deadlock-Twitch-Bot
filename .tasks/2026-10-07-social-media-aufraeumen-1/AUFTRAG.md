# Auftrag: Social-Media aufräumen, Runde 1 (Inventur-Punkte 1 bis 3)

Auftraggeber: Haupt-Orchestrator (Claude-Hauptsession, Thread d3a1741e-82bc-4a48-865b-2845c663dca7)
Rolle: Blatt-Worker. Gate-Fixschleife selbst fahren (frische Fixer-Subagenten bis ALLOW). Keine T3-Threads starten. Meldung erst bei Abschluss oder wenn du feststeckst.
Stufe: mittel. Repo: Deadlock-Twitch-Bot. Eigener Worktree von aktuellem origin/main, Branch `fix/social-aufraeumen-1`.

Grundlage: Inventur auf Branch `docs/social-media-inventar` (Commit cfedfe84), `.tasks/2026-10-07-social-media-inventar/INVENTAR.md`, Abschnitte 6 und 9 ("Jetzt", Punkte 1 bis 3). Die Inventur stand auf e0b0dbaf; seitdem sind D (07f511a3, Upload-Wartezustand) und E (4c830249, 9315b3cf, TikTok Direct Post) gemergt. Jeden Befund gegen aktuelles main neu prüfen.

## Ziel

1. **Admin-Freigabe sendet falsche Identität:** `bot/admin_dashboard/src/api/client.ts` (um 1376) schickt `streamer_login`, der Rust-Handler (`handlers/social_media.rs` um 3882) verlangt `twitch_user_id`. Auf die Twitch-ID umstellen wie der Studio-Client. Überholte Runtime-/Startaussagen in der Doku (Python-Social-Runtime) entfernen.
2. **Toter Code:** den unaufgerufenen alten Social-Redirectstub und das doppelte `ensure_schema` entfernen (Aufrufer vorher gegen aktuelles main prüfen, Migrationen bleiben).
3. **Ein Download-Weg, vollständige Aufräumfrist:** den zweiten, nicht atomaren Download im Upload-Worker (um 429 bis 446) auf den atomaren Kern von Prep/Preview umstellen; die Löschfrist (`retention_worker.rs`) erfasst auch abgeleitete Renderdateien (z. B. `branded_v2`). Nichts allein nach Dateialter löschen.
4. Kleinigkeit aus Abschnitt 6: Clip-Fetch gibt `result.error` an die Antwort weiter, statt Fehler als Erfolg zu melden.

Nicht in diesem Auftrag: Alt-API-Familien, Referenzoberfläche, VOD-Archiv-Zusammenführung, Python-Detektor (warten auf Nutzerentscheidung).

## Grenzen

Keine Code-Kommentare. Kein Python. Migrationen nur neu. `.sqlx` und Schema-Snapshot mitziehen. fmt, clippy, betroffene Tests, Dashboard- und Admin-Build.

## Abschluss

Selbstprüfung `gate_hook.py --review`, Gate bis ALLOW, Merge per `git push origin HEAD:main`, Release über `deploy-twitch-release <sha>`, Neustart Bot und Dashboard, Live-Beweis (Admin-Freigabe speichert mit Twitch-ID, Journal ohne Fehler, Upload-Worker lädt über den atomaren Weg). Branch und Worktree löschen, Bericht an den Haupt-Orchestrator, `t3-thread.py settle --selbst`.
