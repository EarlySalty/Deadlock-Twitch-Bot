# Changelog

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
