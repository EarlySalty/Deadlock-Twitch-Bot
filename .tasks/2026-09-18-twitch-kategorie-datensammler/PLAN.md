# Plan: Twitch-Kategorie-Datensammler Deadlock (global, alle Sprachen)

Stand 2026-09-18. Klasse: mittel bis hoch (neuer Dauerdienst plus Dashboard-Seite,
aber fast nur Wiederverwendung vorhandener Bausteine).

## Ziel

Alle oeffentlich ohne Bot-Konto verfuegbaren Daten der gesamten Twitch-Kategorie
Deadlock dauerhaft sammeln und speichern, plus eine Auswertungsseite im Twitch-
Admin-Dashboard, die zeigt, aus welcher Region (also Sprache) wie viel Aktivitaet
kommt. Der Bot sendet nichts, liest Chat nur anonym.

## Harte Randbedingungen

- Chat ausschliesslich ueber anonyme `justinfan`-Verbindungen (siehe
  `irc_lurker.rs::anonymous_nick`). Nie ueber das authentifizierte Bot-Konto.
  Grund: der Bot ist ueberall Mod/Gast und darf durch Massen-Joins in fremde
  Kanaele kein Ban-Risiko aufbauen. Deshalb laeuft der Sammler als eigener
  Dienst mit eigener Identitaet, getrennt vom `tb-bot`-Prozess.
- Nur lesen und zaehlen. Kein Senden, kein Moderieren, keine Whispers.
- Persistenz Postgres (`twitch_analytics`), keine SQLite, keine ENV-Config.

## Was Twitch ohne Bot hergibt (ehrliches Dateninventar)

Verfuegbar anonym / mit App-Token:
- Helix Get Streams (Kategorie): stream_id, user_id, user_login, title,
  viewer_count (Aggregat), started_at, language, thumbnail_url, tags, is_mature.
- Helix Get Users / Get Channel Information: Profil, created_at (Kontoalter),
  broadcaster_type, Beschreibung, Broadcaster-Sprache.
- Helix Get Videos / Get Clips: VOD- und Clip-Listen je Kanal.
- Chat (anonym IRC): jede geschriebene Nachricht mit Tags (user-id, display-name,
  room-id, emotes, badges), also wer schreibt was wann in welchem Kanal.

NICHT verfuegbar ohne Bot-Rechte auf dem jeweiligen Kanal (ehrlich benennen, nie
als Feature versprechen):
- Echte Zuschauer-Geolokation. Twitch gibt keine Viewer-Laenderdaten heraus.
  "Region" ist ausschliesslich ueber Sprache annaeherbar (Stream-Sprache plus
  erkannte Chat-Nachrichtensprache), nie echte Geografie.
- Vollstaendige Zuschauer-/Lurker-Liste. Get Chatters braucht moderator-Auth.
  Ohne Bot kennen wir nur die Schreiber im Chat plus das viewer_count-Aggregat.
- Follower-Gesamtzahl je Kanal (braucht moderator:read:followers) und Sub-Daten.

Konsequenz fuer die Dashboard-Frage "aus welcher Region wie viele Daten":
Auswertung nach Sprache, klar als Sprache beschriftet, nicht als Land. Das ist
die maximale ehrliche Aussage aus oeffentlichen Daten.

## Architektur (Wiederverwendung zuerst)

Neuer Dauerdienst `tb-category-collector` (eigenes Bin in Deadlock-Twitch-Bot),
getrennt vom Bot-Prozess, drei Schleifen:

1. Discovery (Helix): `tb-transport-twitch::streams` paginiert Get Streams fuer
   die Deadlock game_id im Minutentakt. Ergebnis ist die aktuelle Kanalliste
   plus ein Stream-Snapshot je Kanal.
2. Snapshot-Store: jeder Poll schreibt eine Zeitreihen-Zeile je Live-Stream
   (viewer_count, title, language, started_at, tags). Analog zu
   `tb-analytics::stream_kennzahlen`, aber kategorieweit statt nur Partner.
3. Anonymer Chat-Lurker: die Kanalliste aus Schritt 1 speist `irc_lurker`/
   `scout_chat`-Sharding (anonyme Verbindungen, >100 Kanaele werden bereits
   ge-shardet). Jede PRIVMSG wird als Roh-Chatzeile gespeichert, plus je
   Nachricht eine Sprachdetektion fuer die Regions-Auswertung.

Keine zweite Helix- oder IRC-Implementierung. Der Collector importiert
`tb-transport-twitch` und `tb-monitoring` als Crates.

## Datenmodell (twitch_analytics, Migration als postgres)

- `category_stream_snapshots`: snapshot_at, stream_id, user_id, user_login,
  viewer_count, title, language, started_at, tag_ids, is_mature. Zeitreihe.
- `category_channels`: Stammdaten je Kanal (user_id PK, login, display_name,
  created_at, broadcaster_type, first_seen, last_seen, broadcaster_language).
- `category_chat_messages`: sent_at, room_user_id, chatter_user_id,
  chatter_login, message_text, detected_lang, emote_count. Der Storage-Treiber.
- `category_chat_rollup`: stuendliche Aggregate je Kanal und Sprache
  (messages, distinct_chatter, avg_len) fuer die Langzeit-Auswertung.

Retention: Roh-`category_chat_messages` 90 Tage, danach nur Rollup behalten.
Snapshots dauerhaft (klein). So bleibt der Plattenbedarf im niedrigen zweistelligen
GB-Bereich (Host-Platte ist knapp, Retention ist Pflicht, nicht optional).

## Dashboard-Auswertung (Twitch-Admin)

Neue Seite in `dashboard_v2` in der bestehenden Shell (gleiche Sidebar/Optik),
Backend-Route in `tb-dashboard-api` bzw. `tb-web/src/routes/analytics.rs`,
serverseitige Admin-Auth. Inhalte:
- Aktivitaet nach Sprache: Streams, Sendestunden, Zuschauer-Schnitt,
  Chat-Nachrichten je Sprache, Zeitraum 7d/30d/90d. Kernblatt fuer die
  Regions-Frage, klar als Sprache beschriftet.
- Kategorie-Trend: gleichzeitige Streams und Zuschauer ueber Zeit.
- Top-Kanaele je Sprache (Impact aus Sendezeit und Schnitt-Zuschauer).
- Chat-Volumen je Sprache und Tageszeit.

Farbwelt und Karten nach den bestehenden Dashboard-Regeln (Schwarz-Gold,
erhabene Karten, gesaettigte warme Diagrammfarben, keine Rot-Gelb-Gruen-Ampel).
Neue oeffentliche Pfade in Caddy `@public_twitch`/`@dashboard_paths` eintragen.

## Skalierung und Twitch-Limits

- Anonyme IRC-Joins: Twitch drosselt Joins pro Verbindung (~20 pro 10s). Das
  vorhandene Sharding in `scout_chat.rs` verteilt Kanaele auf mehrere anonyme
  Verbindungen und haelt das ein. Bei 100-300 Kanaelen mehrere Shards.
- Helix Rate-Limit 800 Punkte/min reicht dick fuer Kategorie-Paginierung im
  Minutentakt.
- Kanalliste aendert sich staendig: Joins/Parts inkrementell gegen den letzten
  Poll, nicht jedes Mal alles neu verbinden.

## Kosten (Erinnerung)

Laufende Kosten praktisch null: kein Video, kein Audio, kein STT, keine Cloud,
keine API-Gebuehren. Einziger realer Posten ist Plattenplatz, durch die
Retention gedeckelt. Eine kleine Dauer-CPU-Last fuer den Collector auf dem
bestehenden Host.

## Offene Entscheidungen (fuer den Nutzer)

1. Chat-Rohtext wirklich speichern (fuer spaetere Textanalysen) oder nur
   Zaehl-Aggregate je Sprache/Kanal? Rohtext ist der Storage-Treiber.
2. Retention Roh-Chat: 90 Tage Vorschlag, oder kuerzer/laenger?
3. Sammel-Umfang: nur Chat plus Snapshots, oder zusaetzlich VOD- und Clip-Listen
   je Kanal mitziehen?

## Umsetzungsschritte

1. Echte Kategorie-Concurrency ueber 24-48h messen (Helix), damit Shard-Zahl und
   Storage-Schaetzung belastbar sind.
2. Migration `category_*`-Tabellen (als postgres, Rechte an twitchbot/twitchdash).
3. `tb-category-collector`-Bin: Discovery-Schleife (Helix) plus Snapshot-Store.
4. Anonymen Chat-Lurker anschliessen (irc_lurker/scout_chat-Sharding), Rohzeilen
   plus Sprachdetektion schreiben, stuendlicher Rollup-Job, Retention-Job.
5. Dashboard-Route und -Seite (Sprach-Auswertung, Trend, Top-Kanaele).
6. systemd-User-Dienst mit Restart-Backoff und OnFailure-Notify, getrennt vom
   Bot-Prozess. Live-Beweis: Kanalzahl, Snapshot-Zeilen, Chat-Zeilen, Dashboard.
