# Gemeinsamer Vertrag Twitch #1035 und Bots #472

Stand: 3. Oktober 2026. Twitchintegration bei 3a7b1c91, kombinierte Botsquelle und alleiniger Botsdeploy bei c0b1d111. Beide vollständigen finalen SHAs, ihre Prüfungen und das Gruppenurteil werden vor Integration gebunden. Alte BLOCK-Urteile bleiben bis positiver neuer Abnahme verbindlich. Kein Einzeldeploy.

## Identität und Konfiguration

Twitch-User-ID und Discord-ID sind Schlüssel, Logins Anzeigenamen. Twitch-Präsenz speichert die Helix-ID atomar mit Chatters und Rollup. Der Punkte-Leser löst die Zuschaueridentität nicht anhand eines heutigen Logins neu auf. Community-Sync ordnet Twitch-Tageswerte über die tatsächliche Discord-Verknüpfung zu und berücksichtigt Privacy. Allgemeine Discord-Voice-/Ledger-Punkte setzen keinen Twitchlink voraus.

Die vorhandene Bot-TOML enthält [bot].community_points_aggregation_interval_seconds als ganze Sekunden von 1 bis 3600, Default 300. Der Wert steuert den Takt, keine Punktregel. Secrets bleiben im vorhandenen Infisical-/Credential-/FD-Zugang. Keine neue JSON- oder ENV-Konfigurationsquelle.

## Normale Punkte-Tageswerte

GET /internal/twitch/v1/community-points/viewers und /streamers verlangen X-Internal-Token und Loopback. updated_since ist ein exklusiver RFC3339-Cursor, limit liegt zwischen 1 und 5000, Default 1000. Antwort: rows, next_updated_since, has_more. Tageswerte nach Europe/Berlin werden ersetzt, nie als zusätzliche Gewinne addiert. updated_at ist je Tabelle monoton und mikrosekundengenau eindeutig.

Viewerfelder: twitch_user_id, twitch_login, channel_twitch_user_id, day, watch_minutes, chat_messages, points_watch, points_chat, points_discovery, updated_at. Streamerfelder: streamer_twitch_user_id, streamer_login, optionale discord_user_id, day, viewer_minutes, unique_viewers, raids_to_partners, updated_at. Streamerpunkte entstehen im Consumer aus den verknüpften und privacygeprüften Viewerzeilen; viewer_minutes des Producers sind dafür kein Ersatz.

Beide Rohquellen und Chatwertung verwenden ausschließlich [session.started_at, session.ended_at). Anwesenheitsintervalle enden spätestens am Sessionende. Rohänderungen markieren einen Berliner Tag transaktional. Der Aggregator verarbeitet auch markierte alte Tage und erneuert Viewerzeitstempel bei Rohänderungen trotz unveränderter gedeckelter Punkte. Eine Wiederholung ohne Rohänderung bleibt idempotent.

## Wiedereinwilligung und gebundene Rohaktivität

GET /internal/twitch/v1/community-points/viewers/activity verlangt die gleiche interne Authentifizierung. Query: twitch_user_id, day als Berliner YYYY-MM-DD und activity_since als gespeicherte RFC3339-Consent-Grenze mit höchstens Mikrosekundenauflösung. Antwort: twitch_user_id, day, activity_since, computed_at und rows mit den normalen Viewerfeldern. Die Bindung ist ausdrücklich Teil der Antwort. Kein separater Cursor für diesen einzelnen ID-/Tagesabruf.

Der Leseweg berechnet ausschließlich Samples und Nachrichten ab der Consent-Grenze. Ein alter Tick wird nicht durch Abschneiden zu neuer Aktivität. Chat-Cooldown und Duplikate werden innerhalb dieser neuen Rohaktivität angewandt; der normale Tagespfad behält seine Vorgeschichte. Ein früher am selben Tag gespeicherter Entdeckerbonus wird nicht erneut ausgegeben. Der Leseweg verändert keine gespeicherten Tageswerte.

Nach Löschung, bewusster datenschutz-optin-Entscheidung und erneuter OAuth-Verknüpfung bleibt der Twitch-ID-Hash als Altwertschutz erhalten. Normale kumulative Tageszeilen werden für solche IDs weiter gesperrt. Nur genau gebundene Aktivitätsantworten werden unter Identity-/Nutzerlocks und Consent-CAS übernommen. rows=[] ist bestätigte Null und ersetzt frühere Werte dieses Tages. computed_at verhindert ältere Antworten. Der aktuelle Berliner Tag wird unabhängig vom normalen Cursor regelmäßig angefragt. Historische geänderte Tage werden vor dessen Seitencommit gezielt gelesen. Bei HTTP-/Bindungsfehlern bleibt der Cursor für Retry stehen. Dieser Consumervertrag ist erst mit tatsächlichen DB-Regressionsbelegen abgenommen.

## Clips

Chat verlangt Broadcaster oder Moderator, Dashboard eine echte Twitch-OAuth-Session. Der Dashboard-Lesepool ruft nur die authentifizierte interne Producer-API; die Akteur-ID stammt serverseitig aus der Session. CSRF kommt aus dem bestehenden auth-status-Weg. Producer prüft aktive Partnerschaft, Clip-Datenbankzugehörigkeit nach Twitch-ID, Helix-Ownership, Tageslimit und Replay.

Broker POST /internal/master/v1/clips/submit nutzt bestehenden Token und identischen X-Idempotency-Key/Bodykey twitch-clip-<clip_id>. Body: source=twitch, clip_url, streamer_twitch_user_id, streamer_login, optionale submitted_by_twitch_user_id, submitted_at und title. submitted_at ist RFC3339 UTC mit DB-Mikrosekunden und stammt ausschließlich aus clock_timestamp() beim ursprünglichen serverseitigen INSERT in twitch_clip_contest_forwards. Retry und Replay verändern diesen Zeitpunkt nie. Unbelegter Altbestand bleibt NULL und wird ohne submitted_at weitergegeben. created_at behält unabhängig davon die bestehende Tageslimitsemantik. Der Consumer anonymisiert nach Erasure nur die Einreicher-ID, wenn Herkunft fehlt oder vor der gespeicherten Consentgrenze liegt. Bewusstes Opt-in plus erneute OAuth-Verknüpfung erlaubt nur neue Einreichungen ab dieser Grenze; alte Replaykeys stellen anonymisierte IDs nicht wieder her. Streamer-ID und Einreicher-ID dürfen voneinander abweichen. Antwort ist der bestehende ok/result-Envelope mit accepted|duplicate|rejected. First-write-wins: gleiche unveränderte Metadaten accepted; Drift duplicate/idempotency_metadata_drift mit bestehender Submission-ID; anderer Clip idempotency_conflict. Persistierter duplicate/drift beendet den Producer dauerhaft. Duplikattext nennt keine aktuelle Woche. Dashboardrolle twitchdash bleibt für die Communitydaten lesend.

## Scout und Privacykopien

POST /internal/twitch/v1/scout/community-suggestion nutzt bestehende interne Auth. Pflichtfelder: twitch_login, suggested_by_discord_id, idempotency_key; optional reason. Neue Consumerrequests ergänzen ursprüngliches submitted_at und privacy_epoch, gemeinsam vorhanden. Alte Requests ohne diese beiden Felder funktionieren nur vor der ersten Privacyoperation. Login wird nur für eine neue Anfrage über Helix aufgelöst. Gespeicherter Replay wird nach ursprünglicher Anfragebindung vor Helix zurückgegeben. Alle acht Guards und Kandidatenänderungen nutzen die echte Plattform-ID.

GET /internal/twitch/v1/scout/community-privacy/export?discord_user_id=String liefert discord_user_id, epoch, activity_since sowie suggestions und candidates. Vorschlagsfelder: idempotency_key, twitch_user_id, twitch_login, reason, result_status, created_at, submitted_at. Kandidatenfelder: twitch_user_id, streamer_login, suggestion_reason, suggested_at, status. Export betrifft ausschließlich neue Communitykopien dieser Aufgabe.

POST /internal/twitch/v1/scout/community-privacy/erase erhält discord_user_id, operation_id als UUID und positive i64 epoch. POST /consent erhält zusätzlich activity_since als gespeicherten DB-Zeitpunkt. Antwort: discord_user_id, operation_id und epoch als Anfrageecho, status=applied|replayed|stale, activity_since als aktuelle Grenze. Auch stale bindet genau die ursprüngliche Anfrageepoche; seine Consent-Grenze darf zum neueren aktiven Stand gehören. Gleiche Epoche mit anderer UUID oder anderem Inhalt ergibt 409/privacy_epoch_conflict. Ältere Epoche ergibt stale ohne Mutation. Ein Retry benutzt dieselbe dauerhaft gespeicherte UUID/Epoche. Ein Netzwerkfehler nach Remotecommit erzeugt keine neue Epoche.

Erasure, Replayprüfung und Schreiben halten denselben vorhandenen globalen Scout-Advisorylock vor Zeilenlocks. Erasure löscht die eigenen Vorschläge samt ID/Grund/Replay, entfernt eigene ID/Grund/Zeit aus den Kandidatenkopien und aktualisiert die Vorschlagendenzahl. Kanal, Freigabe und fremde Vorschläge bleiben erhalten. Gelöschte Replaykeys bleiben nur als domaingetrennte Hashes gesperrt. Die Discord-ID-Barriere enthält nur Hash, Epoche, UUID und Consent-Grenze. Nach Erasure sind nur exakt aktuelle Consent-Epoche und ursprüngliches submitted_at >= activity_since zulässig. privacy_blocked ist ein dauerhafter fachlicher Fehler. Ein alter Replaykey wird auch nach neuer Einwilligung nicht neu angelegt.

Der Consumer persistiert den geordneten Auftrag in seiner bestehenden Transaktion und verwendet den vorhandenen Retryloop. Offene Privacyaufträge sperren Weitergabe. Export ergänzt Remotecommunitykopien oder scheitert erkennbar. Erasure mit Remoteausfall meldet keinen vollständigen Erfolg. Verlorene Remoteantwort, zentraler Rollback, identischer Retry und verspätete alte Erasure nach Consent gehören zu den gemeinsamen DB-Tests.

## Auslieferung

Finale Intent-/Security-/Gruppenabnahme bindet Producer und kombinierte Consumerquelle einschließlich C9/Guide. Nur Discord liefert dl-bot, dl-web über FD3 sowie Punkte-Sync-Service/Timer aus. Consumer vor Producer. Twitch prüft direkt vor Integration aktuelles main, den echten SQLx-Stand und sämtliche pending Migrationen. Vollständiger Migrationslauf und anschließend Rollenmatrix erfolgen vor neuen Schreibpfaden. Communityleserechte, technische Privacyrechte, legitime Clipverwaltung und Watchdog-Collectorrechte werden effektiv geprüft. STT- und Lernpause einschließlich ef66e6a2 bleiben erhalten.

Der In-App-Browser ist derzeit nicht verfügbar. Vorhandene HTML- und Peer-Screenshotbelege sind historische beziehungsweise anders gebundene Belege, keine neue Clip-Sichtabnahme. Release/Neustart/Liveprüfung und Cleanup folgen erst nach positivem gemeinsamen Abschluss unter den geltenden Sperren.
