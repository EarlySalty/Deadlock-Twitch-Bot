# Changelog

## #15 — Voice-Reaction: Bot führt jetzt echte Gespräche nach Outreach & Raids

- Nach jedem Outreach und jedem Outreach-Boost-Raid kann der Bot eigenständig im Streamer-Chat antworten — wie ein echter Community-Mensch, nicht wie ein Sales-Bot
- Der Bot hört kurz in den Stream rein und reagiert sinnvoll auf das, was der Streamer gerade sagt oder im eigenen Chat schreibt
- Bei klar interessierten Streamern landet automatisch eine Discord-Notification beim Team, damit ein Mensch persönlich übernimmt
- Standardmäßig deaktiviert — wird im Staging mit Trockenlauf aktiviert, bevor live geantwortet wird
- Komplettes Audit-Log pro Konversation, damit das Verhalten manuell durchgesehen und nachjustiert werden kann

## #14 — Beta: Auto-Highlight-Clips per Discord DM (EarlySalty)

- Nach jedem Deadlock-Match werden automatisch Highlights erkannt (Triple Kill, Multi Kill, Team Fights)
- Clips werden direkt aus dem Twitch-VOD ausgeschnitten und per Discord DM gesendet
- Erkennt Multi-Kills (≥3 Kills in 10 Sek) und Team-Fights (≥4 Kills in 15 Sek)
- Prüft alle 10 Minuten auf neue Matches der letzten 24 Stunden

## #13 — Bot taucht nicht mehr in Analyse-Daten auf

- Der Bot selbst (`deutschedeadlockcommunity`) wird jetzt überall aus Chat-Statistiken und Analyse-Dashboards herausgefiltert
- Viewer-Rankings, Publikumsauswertungen und Chat-Tiefenanalysen zeigen keine Bot-Einträge mehr

## #12 — Dashboard-Login funktioniert wieder + sauberes Partner-Status-Gating

- Dashboard-Login war komplett kaputt: SQL-Query referenzierte nicht-existierende Spalten (`is_partner`, `archived_at`, `created_at`) und brach mit 503 ab. Login funktioniert jetzt wieder zuverlässig.
- Wer sich erfolgreich einloggt und nicht permanent gesperrt ist, wird automatisch wieder als aktiver Partner geführt — kein manuelles Reset mehr nötig nach Re-Auth.
- Departnered/archivierte Streamer können sich jetzt einloggen und kommen ins Dashboard, sehen aber nur Verwaltung, Pläne und Affiliate-Bereich. Analyse, Social Media und Title-Generator bleiben gesperrt bis ein gültiger Twitch-OAuth durchläuft (außer Bot-Bann oder permanenter Block).
- Verwaltung-Seite zeigt einen klaren Hinweis-Banner mit „Jetzt neu autorisieren"-Button, wenn der Partner-Status nicht aktiv ist.

## #11 — Plan-Preise um 50% gesenkt

- Raid Boost: 7,99 € → **3,99 €** pro Monat
- Analyse Dashboard: 16,99 € → **8,49 €** pro Monat
- Bundle (Analyse + Raid Boost): 22,99 € → **11,49 €** pro Monat
- 6-Monats- und 12-Monats-Tarife folgen automatisch (mit den bestehenden 10% / 20% Mehrjahresrabatten)
- Stripe-Preise synchronisiert; bestehende Abos rechnet Stripe weiter zum bisherigen Betrag ab, neue Buchungen laufen automatisch auf die halbierten Preise

## #10 — Social-Media-Dashboard ist jetzt eine eigene Seite

- Im Analyse-Dashboard gibt es keinen „Social Media"-Tab mehr; das Tooling sitzt unter der eigenen URL `https://deutsche-deadlock-community.de/social-media-admin`
- Die neue Seite hat einen schlanken eigenen Header (Admin-Badge + Rück-Link auf `/analyse`) und zeigt direkt die Clip-Pipeline ohne Tab-Navigation drumherum
- Partner sehen die Seite weiterhin nicht; ohne Admin-Recht kommt eine klare „Admin-Zugriff erforderlich"-Meldung
- Caddy ist um die neue Route erweitert, der Login-Redirect kehrt nach erfolgreicher Twitch-Auth direkt auf das Social-Media-Dashboard zurück

## #9 — Discord-Freigabe für Clips + Auto-Approve pro Plattform

- Fertig angereicherte Clips landen jetzt zuerst in einer Freigabe-Schleife, statt sofort in die Upload-Pipeline zu rutschen
- Ein Admin bekommt pro Clip eine Discord-DM mit Vorschau, plattformspezifischen Hashtags und den Aktionen „Posten", „Bearbeiten" oder „Skip"
- Beim Freigeben lassen sich YouTube Shorts, TikTok und Instagram Reels einzeln auswählen
- Zusätzlich gibt es im Social-Media-Dashboard neue Auto-Approve-Schalter pro Plattform, damit bestimmte Ziele nach einer Freigabe immer automatisch mit in die Queue gelegt werden
- Cross-Posting startet erst nach Freigabe oder Auto-Approve und nicht mehr schon vor dem Approval-Schritt

## #8 — Social-Media-Phase 3: Performance-Tracking, LLM-Reports und Analytics-Tab

- Veröffentlichten Clips werden jetzt pro Plattform in 24h-, 7d- und 30d-Buckets nachgezogen, inklusive Views, Likes, Comments, Shares, Watch-Time, CTR und Engagement-Rate
- Jede Woche kann automatisch ein deutscher LLM-Report für einzelne Streamer entstehen; zusätzlich gibt es einen monatlichen Cross-Streamer-Report sowie einen wöchentlichen Admin-Report per Discord-DM
- Im Admin-Dashboard gibt es jetzt einen eigenen Analytics-Bereich mit Charts pro Clip und einer Report-Liste für gespeicherte Streamer-, Cross- und Admin-Reports
- Migration weiter separat: vor dem ersten Einsatz einmal `python bot/migrations/social_media_phase3_analytics.py` ausführen, damit die Analytics-Spalten und die neue Tabelle `social_media_reports` angelegt werden

## #7 — Social-Media-Dashboard 2.0: Phase 0–2 (Layout-Editor, Auto-Aufbereitung)

- Bestehender Tab „Streams" wurde in „Social Media" umbenannt und ist vorerst nur für Admins sichtbar
- Clips bekommen jetzt automatisch ein vertikales 9:16-Layout mit Game- und Cam-Box, das pro Streamer als Default speicherbar ist und pro Clip übersteuert werden kann
- Eigene MP4s lassen sich direkt im Dashboard hochladen und werden 14 Tage aufbewahrt, bevor sie automatisch aufgeräumt werden
- Neue Auto-Aufbereitung: Clips werden lokal transkribiert, Deadlock-Begriffe (Helden, Items, Abilities, Slang) werden korrigiert und ein lokales LLM (Ollama auf dem Server, kein Datenabfluss) schlägt Title, Description und Hashtags je YouTube/TikTok/Instagram vor
- Externe LLMs (z. B. MiniMax oder Claude Haiku) bleiben standardmäßig aus und werden nur genutzt, wenn ein Admin den Schalter „External-LLM-Consent" ausdrücklich aktiviert
- Migration nicht automatisch — vor dem ersten Lauf einmal `python bot/migrations/social_media_phase2_enrichment.py` ausführen, damit die neuen Tabellen `deadlock_vocab` und `social_media_clip_enrichment` angelegt sind

## #6 — Changelog im Dashboard zeigt jetzt die letzten Updates

- Die Sektion „Was gibt's Neues" auf dem Streamer-Dashboard ist jetzt befüllt
- Alle bisherigen Verbesserungen (#1–#5) sind dort als Einträge sichtbar
- Künftige Updates erscheinen automatisch dort, sobald sie veröffentlicht werden

## #5 — Aktive Tab-Buttons im Analyse-Dashboard ohne kaputten 1px-Halo

- Aktiver Tab (z. B. „Übersicht") hatte einen harten cyan 1px-Strich am Rand, der mit dem Card-Highlight kollidierte und broken aussah
- Border ersetzt durch weichen Inset-Highlight + sanfteren Außen-Glow

## #4 — Glow-Tuning und feines Hintergrund-Grid

- Mini-KPI-Karten (Ø Viewer, Follower, Chat-Aktivität, Stream-Stunden) leuchten jetzt dauerhaft in ihrer Trendfarbe und nicht erst beim Hover
- Health-Score-Karte hat ein deutlich dezenteres Glow, damit es nicht mehr in den Bereich daneben überstrahlt
- Subtiles Gitternetz-Pattern im Dashboard-Hintergrund — wirkt weniger leblos, aber bleibt im Hintergrund

## #3 — Build-Toolchain auf aktuelle Node-LTS aktualisiert

- Build-System läuft jetzt auf Node 22 LTS statt der alten Node-18-Version
- Frontend-Build ist etwas schneller und ohne Versionswarnungen
- Keine Auswirkungen auf die Bot-Funktionalität, rein interne Aufräumung

## #2 — Streamer-Dashboard mit deutlich mehr Vibe

- Karten haben jetzt einen weichen farbigen Glow am Rand und heben sich beim Hover sichtbar an
- Header bekommt eine subtile rotierende Aura im Hintergrund
- Health-Score-Ring leuchtet farblich passend (grün/gelb/rot) mit Drop-Shadow
- Wochen-KPIs bekommen pro Karte eine farbige Trend-Aura (grün bei +, rot bei -)
- Sparkline-Linien glühen leicht in ihrer Trendfarbe
- Last-Stream-Mini-Stats (Ø Viewer, Peak, Follower, Chat) bekommen Hover-Spotlight und Text-Glow
- Activity-Items haben jetzt einen vertikalen Akzent-Streifen, farblich nach Typ (Raid grün, Ban rot, Warnung gelb)
- Letzte Streams Liste hat einen blau-violetten Akzent-Streifen pro Eintrag
- Live-Indikator pulsiert mit zusätzlichem roten Außenglow

## #1 — Streamer-Dashboard schneller, schöner und mit funktionierender Navigation

- Dashboard lädt deutlich schneller (Doppelter API-Request entfernt, Backend in mehrere parallele Aggregationen aufgeteilt)
- Sidebar-Links zu Overview, Streams und Chat funktionieren wieder und springen direkt auf den richtigen Tab
- Beim Laden erscheint sofort eine animierte Vorschau (Skeleton) statt eines leeren Spinners
- Neuer Live-Indikator im Header zeigt, ob du gerade live bist, mit aktueller Viewer-Zahl und Stream-Titel
- Wochen-KPIs (Ø Viewer, Follower, Chat, Stream-Stunden) haben jetzt eine Mini-Trendlinie der letzten 7 Tage
- Neue Sektion „Letzte Streams" listet die letzten 5 Streams mit Datum, Dauer, Ø Viewer, Peak und Follower-Zuwachs
- Aktivitäts-Feed lässt sich nach Raids, Bans und Warnungen filtern und mit „Mehr laden" ausklappen
- Sanftere Hintergrund-Animation, dezentere Optik und mehr Mikro-Animationen in Sidebar, Cards und Listen
