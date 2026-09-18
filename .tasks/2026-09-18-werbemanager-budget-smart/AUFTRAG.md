# Auftrag: werbemanager-budget-smart

status: aktiv (2026-09-18)

## Ziel

Der Werbemanager tut heute nichts Sichtbares: in Prod steht keine einzige Aktion, weil er nur in den letzten 60 Sekunden vor einer von Twitch geplanten Werbung handelt und keinen Verlauf führt. Am Ende verteilt er das Werbebudget des Streamers (Minuten pro Stunde) selbst in 30-Sekunden-Blöcken über den Stream, legt die Blöcke in ruhige Momente, zeigt im Dashboard, was er getan und warum er verschoben hat, und die Seite ist aufgeräumt mit einem Schalter, den man nicht übersieht.

## Entscheidungen des Nutzers (verbindlich)

- Budget: der Streamer gibt Werbeminuten pro Stunde ein. Der Bot verteilt sie als 30-Sekunden-Blöcke über die Stunde. Preroll-frei ist ein Nebeneffekt, kein Ziel: das Budget wird nicht überschritten, um Prerolls zu vermeiden.
- Fenster: Queue oder Menü, und die erste Minute eines Matches.
- Nach Matchende nicht sofort: 1 Minute warten. Schreibt der Chat in dieser Minute, verschieben; bleibt er ruhig, Werbung schalten.
- Sperren (keine Werbung, Twitch-Werbung per Pause verschieben, solange Pausen da sind): im Match ab Minute 1, Raid in den letzten 10 Minuten, Erstchatter in den letzten 5 Minuten, Startschutz nach Streamstart.
- "Nur überwachen" als Strategie entfällt. Schalter aus heißt: Bot liest und zeigt den Status, greift nicht ein. Es bleiben zwei Strategien: "Nur Twitch-Pausen nutzen" und "Match schützen & Queue nutzen". Optik und Aufbau der Strategie-Karten bleiben, wie sie sind.
- Kein LLM in diesem Pfad. Alles Regeln auf vorhandenen Signalen.

## Randbedingungen aus Twitch

- Helix `POST /channels/commercial` setzt eine Sperrzeit (`retry_after`, üblich 8 Minuten). Passt das Budget nicht in 30-Sekunden-Blöcke mit diesem Abstand, wächst der Block auf 60 Sekunden, statt das Budget zu verfehlen. Die Sperrzeit wird aus der Helix-Antwort gelesen, nicht als Konstante geraten.
- Von Twitch selbst geplante Werbung zählt aufs Budget mit und wird wie bisher per Pause in ein Fenster geschoben.
- Notbremse: ist ein Block länger als eine halbe Blockperiode überfällig und kein Fenster in Sicht, nimmt der Bot den am wenigsten schlechten Moment (ruhiger Chat, keine Sperre außer "im Match"). Im Match ohne Alternative startet er nicht selbst.

## Paket A: Backend (Rust)

1. `Settings` und Migration: neues Feld `budget_minutes_per_hour` (Default 3, Grenzen 1 bis 8). `ad_duration_seconds` bleibt für die manuelle Werbung. Strategy-Enum: `monitor` entfällt; Bestandszeilen mit `monitor` werden per Migration zu `enabled = false, strategy = 'smart'`.
2. Planer: reine Funktion neben `decide()`, die aus Budget, Streamstart, bisher gelaufener Werbung dieser Stunde und Sperrzeit den nächsten Blocktermin und die Blockdauer liefert.
3. `decide()` umbauen: handelt nicht mehr nur im Vorlauf vor `next_ad_at`, sondern auch, wenn ein eigener Block fällig ist. Fenster und Sperren wie oben. Jede Entscheidung trägt einen maschinenlesbaren Grund.
4. Signale in `DecisionInput`: Match-Beginn und Match-Ende (aus `activity.live_player_state`, Übergänge im Worker merken oder aus der Quelle lesen), letzter eingehender Raid, letzter Erstchatter der Session, Chat-Nachrichten der letzten Minute. Bestandssuche über Graphify vor jedem neuen Baustein; Raid-Events und Chatter-Historie existieren schon.
5. Verlauf: neue Tabelle `twitch_ad_manager_decisions` (Kanal, Session, Zeit, Entscheidung, Grund, Blockdauer, Detail). Geschrieben wird nur beim Wechsel von Entscheidung oder Grund, nicht bei jedem Tick. Aufbewahrung 30 Tage.
6. API: `GET` Verlauf der laufenden oder letzten Session plus Bilanz (Blöcke gelaufen, davon im Fenster, Budget genutzt von gesetzt, Anzahl Verschiebungen). Settings-Endpunkte um das Budget erweitern, `monitor` ablehnen.
7. Geisterzeile: in `twitch_ad_manager_settings` liegt eine Zeile mit leerer `twitch_user_id`. Ursache im Schreibpfad finden (Identität kommt aus der Session, leere ID wird abgewiesen), CHECK-Constraint gegen leere IDs, Altzeile per Migration löschen.
8. Doku `docs/streamer/WERBEMANAGER.md` auf den neuen Stand.

## Paket B: Dashboard (React)

API-Vertrag steht in `API-VERTRAG.md` in diesem Ordner; B baut dagegen und wartet nicht auf A.

1. Status-Karte ganz oben: großer Schalter links am Titel mit Goldkante, darunter ein Satz in Klartext, was der Bot gerade tut ("Läuft. Nächster Block in deiner nächsten Queue.", "Aus. Der Bot schaut nur zu."). Manuelle Knöpfe wandern in diese Karte.
2. Strategie: zwei Karten statt drei, Optik unverändert.
3. Budget: Eingabe "Werbeminuten pro Stunde" mit einer Zeile, was daraus wird ("6 Blöcke à 30 Sek., etwa alle 10 Min.").
4. Neu "Verlauf": Liste der Entscheidungen mit Uhrzeit und Grund in Nutzersprache ("21:14 Werbung in der Queue gestartet", "21:40 verschoben, Raid von X"), darüber die Bilanz des Streams.
5. Live-Kacheln auf eine Zeile eindampfen, gelber Warnbalken wird ein kleiner Hinweis, Mindestabstand, Startschutz, Chat-Ruhe und Vorlauf klappen als "Feineinstellungen" ein.
6. Texte ohne internes Vokabular (kein Snooze, kein Preflight), echte Umlaute, keine Em-Dashes. Look nach den Dashboard-Regeln: warmes Schwarz, Goldkante, Karten erhaben, Innenkacheln versenkt.

## Fundstellen

- `rust/crates/tb-analytics/src/ad_manager.rs:413`: `decide()`, handelt nur im Vorlauf vor `next_ad_at`
- `rust/crates/tb-analytics/src/ad_manager.rs:369`: `SteamMatchState` (kennt nur `in_match`, keinen Match-Beginn)
- `rust/crates/tb-analytics/src/ad_manager.rs:779`: `quiet_messages()`
- `rust/bin/tb-bot/src/ad_manager_wiring.rs:95`: `process_channel()`, Worker-Tick
- `rust/crates/tb-transport-twitch/src/streams.rs:438`: `snooze_next_ad()`, Helix-Aufrufe
- `rust/migrations/20260901100000_twitch_ad_manager.sql`: Schema
- `rust/bin/tb-bot/src/obs_dock.rs:695`: `channel_raid_zu_event()`, Raid-Eingang
- `rust/crates/tb-analytics/src/chatter_verlauf.rs`: Chatter-Historie
- `bot/dashboard_v2/src/components/verwaltung/AdManagerSection.tsx` (801 Zeilen), `bot/dashboard_v2/src/api/adManager.ts`

## Was nicht angefasst wird

- Optik der Strategie-Karten.
- Chat-Einladungen und Promo-Engine (andere "Werbung").
- Admin-Werbegrenzen aus dem Auftrag vom 2026-09-12, falls vorhanden: einhalten, nicht umbauen.
- Geteilter Checkout `~/repos/Deadlock-Twitch-Bot`: nur im eigenen Worktree arbeiten.

## Fertig-Kriterium

Mit Schalter an und Strategie "Match schützen & Queue nutzen" startet der Bot bei einem Live-Stream ohne Twitch-Werbeplan selbst 30-Sekunden-Blöcke in Queue, Menü oder der ersten Match-Minute, bleibt im Stundenbudget, und jede Aktion sowie jede Verschiebung steht mit Grund im Verlauf des Dashboards. `cargo clippy` und die bestehenden Suites der angefassten Crates laufen, `.sqlx` und Schema-Snapshot sind nachgezogen.

## Deploy-Weg

Migration von Hand als `postgres` auf `twitch_analytics` (Bot migriert nicht selbst), Rechte an `twitchbot` und `twitchdash`, Eintrag in `_sqlx_migrations`. Release im eigenen Worktree bauen, `deploy-twitch-release <sha>`, Dashboard-Build, Live-Prüfung unter `/twitch/verwaltung#werbung`. Merge und Deploy macht der Orchestrator nach den Review-Runden, nicht der Worker.

## Rahmen

- Du bist der einzige Thread für dieses Paket. Keine Unter-Threads oder Unter-Agenten spawnen.
- Keine Code-Kommentare schreiben, Code erklärt sich selbst.
- Nur den eigenen Branch pushen, nie main. Nichts nach main mergen, auch wenn ein Stop-Hook dazu auffordert.
- Auftrag größer als beschrieben: Bump-up-Nachricht an den Intent-Thread, dann stoppen.
