# Vertragsprüfung für Twitch #1035 und Bots #472

Stand: 2. Oktober 2026. Twitch-Zielbasis `fcc45ab10759013da66e007000fbc5bac6dca82e`, Herkunftskopf `d1a6666252b59d286ad5cb2e0f32ff93c8f02c44`. Der belegte Anteil und freigegebene Herkunftsfix wurden selektiv als `c759cfcb` übertragen. Producer-Gate R1 ist BLOCK; die anschließende Fixrunde ist noch ungetestet.

## Identität und Twitch-Verknüpfung

Discord-ID und Twitch-User-ID sind die Schlüssel. Logins dienen der Anzeige. Harte Verknüpfungen kommen aus `core.discord_platform_connections` auf der Bots-Seite. Der neue Twitch-Anteil enthält keinen `twitch-links`-Endpunkt. Paket A und dessen Broker-Vertrag müssen am heutigen Bots-Code geprüft werden. Punkte ohne Discord-Verknüpfung dürfen öffentlich weder einer Discord-ID zugerechnet noch namentlich angezeigt werden.

## Punkte-Tageswerte

Twitch liefert `GET /internal/twitch/v1/community-points/viewers` und `GET /internal/twitch/v1/community-points/streamers`. Authentifizierung: `X-Internal-Token` und Loopbackprüfung. Query: optionales `updated_since` als exklusiver RFC3339-Cursor und `limit` von 1 bis 5000, Standard 1000. Antwort: `rows`, `next_updated_since` als String oder null, `has_more` als Boolean.

Zuschauerzeile: `twitch_user_id`, `twitch_login`, `channel_twitch_user_id`, `day`, `watch_minutes`, `chat_messages`, `points_watch`, `points_chat`, `points_discovery`, `updated_at`.

Streamerzeile: `streamer_twitch_user_id`, `streamer_login`, optionale `discord_user_id`, `day`, `viewer_minutes`, `unique_viewers`, `raids_to_partners`, `updated_at`.

Die Zeilen sind ersetzbare Tageswerte nach Europe/Berlin. Der Consumer darf einen erneuten Abruf nicht als weiteren Punktegewinn buchen. Producerwerte zählen Twitch-Zuschauer vor der Discord-Zuordnung. Die Begrenzung auf verknüpfte Community-Mitglieder für Streamerpunkte muss der Consumer nachweisen; `viewer_minutes` allein enthält diese Einschränkung nicht.

## Clip-Einreichung

Twitch sendet `POST /internal/master/v1/clips/submit` mit `X-Internal-Token` und `X-Idempotency-Key`. Body: `source=twitch`, `clip_url`, `streamer_twitch_user_id`, `streamer_login`, optionale `submitted_by_twitch_user_id`, optionaler `title`, `idempotency_key=twitch-clip-<clip_id>`. Header und Body verwenden denselben Key.

Die Antwort ist ein Broker-Envelope mit `ok=true` und `result`. `result` enthält `status=accepted|duplicate|rejected`, optionale `submission_id` und optionalen `reason`. Twitch liest keinen nackten Ergebnisbody. Ein Duplikat mit `reason=idempotency_metadata_drift` muss dauerhaft als `duplicate` einschließlich ID und Grund gespeichert werden; danach darf derselbe Versuch keinen weiteren Brokeraufruf auslösen. Der übernommene Herkunftsfix erzwingt genau eine erfolgreich aktualisierte Zeile, bevor ein Erfolg gemeldet wird. Die Regression wird im nächsten bestätigten Slot ausgeführt.

Die Twitch-Vorprüfung verlangt einen aktiven Partner, einen Helix-geprüften Clip des eigenen Kanals, Broadcaster oder Moderator im Chat beziehungsweise eine echte Twitch-OAuth-Session des Kanals im Dashboard und höchstens drei Einreichungen je Kanal und Berliner Tag.

## Scout-Vorschläge und Ergebnisse

`POST /internal/twitch/v1/scout/community-suggestion`: Body `twitch_login`, `suggested_by_discord_id`, optionaler `reason`, `idempotency_key`. Antwort `status=created|already_known|already_partner|blocked|not_found` und optionale `twitch_user_id`. Ein anders belegter Idempotency-Key liefert HTTP 409 mit `idempotency_conflict`. Ungültige Eingaben liefern 400. Fehlende oder gestörte Helix-Anbindung liefert für neue Anfragen 503. Persistierter Replay wird nach Dienstauthentifizierung und Prüfung von ursprünglichem normalisiertem Login, Discord-ID und Grund ohne neue Helix-Abfrage zurückgegeben. Fremde Anfragebindung liefert 409 ohne gespeicherte Twitch-ID. Alle acht Listenprüfungen und Kandidatenupdates richten sich nach der Twitch-ID. Fehlende gespeicherte ID oder ein Kandidatenlogin mit bekannter anderer ID liefert 503 `identity_unresolved`, bis die zuständige Identitätsauflösung den Altstand klärt. Bestehende Entscheidungen werden nicht umgeschrieben.

`GET /internal/twitch/v1/scout/community-suggestions/outcomes` nutzt dieselben Cursorparameter und dieselbe Seitenstruktur wie Punkte. Zeile: `twitch_user_id`, `twitch_login`, `suggested_by_discord_id`, optionales `suggested_at`, `suggestion_count`, `candidate_status`, `is_partner_active`, optionales `partner_since`, `updated_at`. Der erste Vorschlagende bekommt bei erfolgreicher Partnerschaft einmalig die vereinbarten 150 Punkte. Die Adminfreigabe bleibt Voraussetzung für Outreach.

## Vorhandenes Gate und Integrationsfolge

Historisches Gate: BLOCK auf `671a460a`. Blockierender Fund: Watchtime reichte über `session.ended_at` hinaus. Weitere Befunde: Discovery-Entscheidung vor Writer-Lock, Clip-Replay bei geänderten Metadaten, zusätzliche Poolconnection bei Bannprüfung, fehlende UI-Screenshots.

Der lokale Commit `83e1cc51` begrenzt Tick- und Chatabdeckung am Sessionende, serialisiert das Lesen und Schreiben über dieselbe Transaktion und verlegt die Scout-Bannprüfung auf dieselbe Connection. Die erste gezielte Suite hat auf `c759cfcb` mit Rust/Cargo 1.98.1 alle 20 Tests gegen echte TimescaleDB bestanden. Das aktuelle R1-Gate bleibt wegen Scout-Identität und Replay BLOCK. Der schmutzige Clipfix ergänzt die dauerhafte Speicherung des Replay-Ergebnisses.

Gemeinsam prüfen und freigeben. Consumer #472 vor Producer #1035 integrieren und ausrollen. C9-Pfade und aktive Peer-Auslieferungen vorher abstimmen. Keine alte vollständige Branchbasis mergen, kein Einzeldeploy. Nach endgültigem Nachziehen der Basis Repo-Lock bis zum Live-Nachweis halten; Cargo zusätzlich unter den verbindlichen Hostlocks und im abgestimmten Slot ausführen.

## Interner Dashboard-Schreibpfad und Rollen

Der öffentliche Dashboard-Aufruf nimmt keine freie Clientidentität entgegen. Die Akteur-ID stammt aus der von der Auth-Schicht ausgewählten echten Twitch-OAuth-Session. Reine interne oder Discord-Admin-Sessions genügen nicht. Das Dashboard sendet Clip-DB-ID und serverseitig ermittelte Akteur-ID mit bestehendem Dienst-Token an `POST /internal/twitch/v1/clips/contest/submit` auf Loopback. Dort prüft der Producer den gespeicherten Clip-Eigentümer, die ID-gebundene aktive Partnerschaft, Helix-Ownership, Tageslimit und dauerhafte Idempotenz mit seiner schreibenden Botrolle.

Drei Communitymigrationen erzeugen vier neue Tabellen im Schema `public`: `twitch_community_points_viewer_daily`, `twitch_community_points_streamer_daily`, `twitch_clip_contest_forwards` und `twitch_scout_community_suggestions`. Die Punktmigration erzeugt zwei Tabellen. Die Vorschlagstabelle hat die Sequenz `public.twitch_scout_community_suggestions_id_seq`; die anderen drei Tabellen benötigen keine eigene Sequenz. Für `twitchdash` bleibt SELECT, Tabellen-Schreibrechte und Sequenz-USAGE/UPDATE werden nach allen allgemeinen Grants gezielt entzogen. Bestehende Clipverwaltungsrechte und Scout-Adminpfade bleiben erhalten.

Der echte Launcher führt sämtliche pending Dateien aus und danach die Rollenmatrix. Das read-only abgeglichene Manifest hat 161 angewandte Migrationen ohne Prüfsummenkonflikt. Neben den drei Communityversionen wäre `20260930220200_token_storage.sql` ausstehend. Dieser vorhandene Main-Anteil wird weder verändert noch als Community-Eigenanteil beansprucht; sein tatsächlicher Auslieferungsstand und seine Zuständigkeit sind vor dem späteren Lauf mit dem Hauptthread abzugleichen.
