# Partner-Challenges: Einsatz statt Reichweite

Die Rust-Crate `tb-effort` enthält die gemeinsame Punkte-, Quest-, Streak- und Achievement-Engine. `tb-bot` betreibt den Collector über den vorhandenen Task-Supervisor; `tb-dashboard-api` stellt ausschließlich lesende, persönliche Antworten bereit. Zuschauerzahlen, Followerzahlen und Raid-Größen gehen nicht in die Berechnung ein. Es gibt keinen Versandpfad zu Twitch-Chat oder Discord.

## Quellen und bestehende Verträge

Vor der Implementierung wurden der lokale Graphify-Graph (`8c800bb`) und der globale Graph ausgewertet und die gefundenen Verträge im aktuellen Quellcode geprüft. Ausgangspunkte waren `docs/community-coplay.md`, der Community-Handler, die Stream-Historie, `tb-social-media`, die Steam-Präsenz und der gespeicherte GC-Match-Verlauf. Co-Play-Empfehlungsscores sind keine Teilnahmebeweise und werden nicht in Einsatzpunkte umgerechnet.

| Ereignis | Bestehender Beleg | Vergabe und Identität |
|---|---|---|
| `qualified_invite` | `activity.twitch_invite_qualifications`, Status `qualified` | 10 Punkte an die Twitch-ID aus `bot.twitch_streamer_invites`; eindeutige `join_event_id`; ein abweichender Einladender bleibt als Viewer-Twitch-ID erhalten. |
| `streamer_referral` | `activity.streamer_referral_credits` | 50 Punkte an die ausdrücklich gespeicherte Referrer-ID. `added_by`, Loginähnlichkeit und administrative Partneranlage sind keine Empfehlung. |
| `co_stream` | Helix `GET /shared_chat/session`, echte Session-ID und aktive, gleichzeitig live Partner | 8 Punkte einmal pro Twitch-Stream. 30 Minuten fortlaufend bestätigte Beobachtungen; Ausfall, Abwesenheit, Session-Wechsel oder zu große Poll-Lücke unterbrechen den Nachweis. |
| `party_play` | `voice.deadlock_party_members.party_id`, frische `activity.live_player_state` und beide abgeschlossenen GC-Historien | 5 Punkte einmal pro Partner und Match; höchstens vier bezahlte Matches je Berlin-Woche. Bloße Präsenz oder gleichzeitiges Streamen genügt nicht. |
| `clip_submitted`, `clip_top3` | `twitch_clip_contest_effort_outbox` aus dem Clip-Contest | 2 Punkte, höchstens zwei bezahlte Einreichungen pro Woche; Top 3 mit 20/15/10. `metadata.submission_id` löst die stabile Broadcaster-ID der bestehenden Einreichung auf. |
| `quest_done` | Persistierte Wochenaufgabe und bestätigter Fortschritt | 5 Punkte pro Aufgabe, 10 zusätzliche Punkte für alle drei; keine frei aufrufbare Quest-Buchung. |

Die Einladungs- und Empfehlungsquellen gehören zur Arbeit „Qualified Discord Invites“, die Contest-Outbox zur Arbeit „Clip Contest“. Diese Tabellen werden hier weder nachgebaut noch automatisch erzeugt. Fehlt eine Abhängigkeit, wird sie als nicht verfügbar geführt. Der öffentliche Lesepfad antwortet dann mit 503 statt erfundenen Nullständen. Das bestehende Clip-System `twitch_clips_social_media` bleibt Eigentümer der Clip-Daten.

Steam-Match-Belege werden aus bereits abgeschlossenen `steam.steam_tasks` des Typs `GC_GET_MATCH_HISTORY` gelesen. Es werden keine neuen GC-Abfragen angelegt. Die Engine prüft den Steam-Eigentümer der Antwort, eine gültige Match-ID, den Startzeitpunkt und ein abgeschlossenes Ergebnis. Beide Personen müssen denselben Match-Eintrag besitzen. Zur Zuordnung zum beobachteten Party-Spiel dient die vorhandene Rich-Presence-Spielzeit; die 90-Sekunden-Toleranz berücksichtigt deren Minutenrundung. Fehlende Historie führt nicht zu einer ersatzweisen Vergabe. Noch nicht bestätigte Beobachtungen werden bis zu sieben Tage später erneut abgeglichen. Danach werden die temporären Party-Beobachtungen gelöscht; das append-only Punkte-Ledger und die Achievements bleiben erhalten.

Die Identitätskette verwendet `twitch_streamer_identities`, die vorhandene Auswahl in `twitch_player_steam_links` und bestätigte `core.steam_links`. Ein deaktivierter Lookup oder eine ausdrücklich entfernte Primärauswahl wird respektiert. Identische Discord- oder Steam-Konten zählen nicht als Mitspieler des eigenen Kontos. Nicht-Partner brauchen zusätzlich die bestätigte Community-Mitgliedschaft aus der bestehenden Identität; eine zurückgebliebene Steam-Verknüpfung nach dem Austritt genügt nicht.

## Ledger und Wiederholungssicherheit

Es gibt genau eine neue Migration: `20260927110000_partner_effort_engine.sql`. Das Ledger bleibt `partner_effort_events` und ist damit mit den gesonderten Dashboard- und Monats-Raid-Boost-Arbeiten kompatibel. Punkte und Konfigurations-Hash werden bei der Vergabe festgehalten. Geänderte Punktwerte schreiben die Vergangenheit nicht um.

Postgres sperrt konkurrierende Vergaben je Partner und Woche. Eine Quellen-ID ist je Ereignis und Partner eindeutig; Einladungen, Empfehlungen und Contest-Ereignisse können zusätzlich nicht mehreren Partnern zugeordnet werden. Eine identische Wiederholung ist ein No-op; widersprüchliche Wiederholungen werden abgewiesen. Dauerhafte Empfangsbelege machen Quellenwiederholungen ungefährlich. Jede Quelle verwendet zwei dauerhaft gespeicherte Lesepositionen: einen zeitlich geordneten Pfad für neue Bestätigungen und einen zyklischen historischen Abgleich nach Join-ID. Je Pfad wird pro Poll höchstens eine begrenzte Seite verarbeitet; nach jedem bestätigten Datensatz wird die Position gespeichert. Ein Neustart oder Timeout setzt den Scan nicht auf null zurück. Der historische Pfad findet auch verspätet sichtbare ältere IDs oder zurückdatierte Bestätigungen. Während des ersten noch unvollständigen Abgleichs bleibt die API auf 503, statt unvollständige Lifetime-Zahlen als vollständig auszugeben.

Über dem Wochenlimit bleiben echte Teilnahmeereignisse mit null Punkten erhalten. Sie erhöhen keine Punktesumme, bleiben aber als Teilnahme für Quests und Achievements belegbar. UPDATE, DELETE und TRUNCATE sind für das Ledger durch Datenbank-Trigger gesperrt. Erreichte Achievements und zugewiesene Questdefinitionen sind ebenfalls unveränderlich. `twitchdash` erhält nur SELECT; der Bot erhält die notwendigen Schreibrechte, ohne Änderungs- oder Löschrecht am Ledger.

## Wochen und dauerhafter Fortschritt

Alle Wochen beginnen Montag 00:00 in `Europe/Berlin`. Grenzen werden als lokale Kalendertage berechnet und erst dann in UTC umgerechnet; Wochen mit Zeitumstellung dauern korrekt 167 beziehungsweise 169 Stunden.

Die Auswahl der drei Aufgaben wird mit SHA-256 aus Partner-Twitch-ID, Wochenbeginn und Aufgabenkennung sortiert und dauerhaft gespeichert. Party-Aufgaben brauchen die verknüpften Mitspieler, Stream Together einen weiteren aktiven Partner. Der Contest ist nur bei erreichbarem Einreichungsfenster und verfügbarem Monatskontingent im Pool. Sind tatsächlich weniger als drei unterschiedliche Aufgaben erreichbar, meldet die Engine diesen Zustand als Fehler, statt eine unerfüllbare Aufgabe vorzutäuschen.

Die Zeitaufgabe bedeutet: **Deadlock-Minuten dieser Woche über dem Mittel der vier vorherigen vollständigen Berlin-Wochen**, plus die konfigurierten Zusatzminuten. Der Referenzwert und die Belohnungen werden bei Zuweisung eingefroren. Änderungen an der Konfiguration verändern eine laufende Aufgabe nicht. Fortschritt wird aus dem Ledger und den Zeitbelegen gelesen; auch verspätet belegte, bereits zugewiesene Vorwochenaufgaben werden abgerechnet. Für nie zugewiesene Ausfallwochen werden keine erfundenen Aufgaben erzeugt.

Deadlock-Zeit kommt aus den vorhandenen `category_stream_snapshots`. Überlappende Messintervalle werden vereinigt, an Wochenwechseln geteilt und je Stream/Woche dauerhaft zusammengefasst. Ein langes Variety-Stream-Intervall oder mehrere kurze Streams werden nicht als ein einzelner 30-Minuten-Deadlock-Stream ausgegeben.

Eine Streak-Woche benötigt mindestens einen Stream mit 30 belegten Deadlock-Minuten in dieser Woche und mindestens ein Einsatzereignis. Die noch laufende Woche wird nicht vorzeitig als verpasst gewertet. Eine verpasste abgeschlossene Woche pro Kalendermonat wird automatisch überbrückt; maßgeblich ist der Monat ihres Montags. Ein Freeze erhält die Serie, erhöht sie aber nicht. Aktuelle und längste Serie sowie der verwendete Freeze werden gespeichert. Achievement-Schwellen der Ausdauer verwenden die längste erreichte Serie.

Lifetime-Punkte werden niemals beim Monatswechsel zurückgesetzt. Bereits verdiente Achievement-Stufen bleiben auch bei geänderten Schwellen sichtbar. Das nächste Ziel enthält fehlende Punkte und eine Route mit möglichst wenigen weiteren qualifizierenden Aktionen, unter Berücksichtigung erreichbarer Aufgaben, ihrer Belohnungen, des Drei-Aufgaben-Bonus und verbleibender Wochenlimits. „Schnell“ bezeichnet hier die Anzahl der Aktionen, keine garantierte Dauer oder garantierte Vermittlung eines neuen Partners; ungewisse Top-3-Platzierungen werden nicht als planbare Route empfohlen.

## Persönliche API

`GET /twitch/api/v2/challenges/me` liefert drei Quests, Fortschritt und nächste Wochengrenze, Streak/Freeze, Lifetime-Level, nächstes Ziel, Achievement-Stufen, „mit uns“-Werte und den eigenen Monatsrang.

`GET /twitch/api/v2/challenges/viewers` liefert die maximal 50 besten eigenen Viewer-Werber mit qualifizierten Einladungen und über den vorhandenen Helix-Client aufgelösten Anzeigenamen. Ein von Twitch nicht mehr geliefertes Konto behält seine ID und erhält keinen erfundenen Anzeigenamen.

Die eigene Identität stammt ausschließlich aus der bestehenden authentifizierten Sitzung. Partner sehen nur sich selbst. Angemeldete Admins können mit `?streamer=<login>` einen aktiven Partner auswählen; die Auswahl verändert nicht ihre eigene Identität. Ein internes Token ohne persönliche Sitzung reicht nicht. Unbekannte Query-Felder werden abgelehnt. Antworten sind `private, no-store`; ohne Anmeldung 401, fremder Partnerzugriff 403, inaktiver Partner 404, fehlende oder nicht aktuelle Quellen 503. GET-Endpunkte schreiben keine Belohnungen.

„Mit uns“ zählt dauerhaft qualifizierte Einladungen, vereinigt echte zeitliche Überschneidungen mit anderen Personen in den vorhandenen Community-Voice-Sitzungen und zählt erfolgreiche empfangene Raids nach Twitch-ID. Die Zahl der anderen Personen oder Zuschauer multipliziert weder die Stunden noch Punkte.

Die Monatsrangliste summiert die in diesem Berlin-Kalendermonat aufgetretenen Ledger-Ereignisse ausschließlich für aktive Partner. Gleichstand: mehr qualifizierte Einladungen, früherer Zeitpunkt des letzten positiven Punkteereignisses, schließlich Twitch-ID. Der dauerhafte Monatsabschluss und die zeitlich begrenzten Raid-Grants bleiben Aufgabe des separaten Raid-Boost-PRs.

## Konfiguration und Prüfung

`docs/partner-effort-config.toml` ist der Abschnitt für die vorhandene gemeinsame Bot-/Dashboard-TOML. Es gibt keine zweite Laufzeit-Konfigurationsquelle. Zugang zur Zentraldatenbank wird über den bereits bestehenden Infisical-Startpfad mit `DEADLOCK_CENTRAL_DSN` eingespeist; Werte werden weder protokolliert noch in Git gespeichert. Der daraus erzeugte Pool ist ausschließlich lesend, hat begrenzte Verbindungszahl und Statement-Timeout. Die Feature-Arbeit selbst ändert keine produktiven Secrets oder Dienste.

Die PostgreSQL-Tests benötigen ausdrücklich `TB_TEST_DATABASE_URL` einer Wegwerf-Datenbank mit TimescaleDB. Sie erstellen eigene Datenbanken, führen die vollständigen Repository-Migrationen aus und prüfen unter anderem Parallelität, Quellenwiederholung, Limits, Unveränderlichkeit, inaktive Partner, Lebenszeit/Monatsgrenzen, Shared Chat und die zweistufige Steam-Match-Bestätigung. Ohne Test-DSN schlagen sie sichtbar fehl statt still übersprungen zu werden. Die CI führt diese Tests, die API-Scope-Tests, Schema-Abgleich, Formatierung und Clippy aus. SQLx-Offlinedaten werden aus einer tatsächlich migrierten Testdatenbank erzeugt.

Dieser Auftrag endet mit einem offenen Pull Request. Kein Merge, keine produktive Migration, kein Deploy und kein Dienstneustart.
