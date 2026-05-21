## #43 — Automatische Onboarding-Tour durch Dashboard, Pläne und Analytics

- Neue Nutzer werden nach der Dashboard-Tour automatisch zur Abo-Seite weitergeleitet
- Auf der Abo-Seite erklärt eine Spotlight-Tour die drei Plan-Stufen (Free, Basic, Extended) mit je einer kurzen Erklärung
- Nach der Pricing-Tour geht es direkt weiter zum Analyse-Dashboard mit einer eigenen Tour
- Die Analytics-Tour zeigt die wichtigsten Tabs: Übersicht, Streams, Wachstum und Chat
- Alle drei Touren können einzeln übersprungen werden und zeigen sich nur beim ersten Besuch

## #42 — Pricing-Seite überarbeitet: Vergleich korrigiert & Auswahl klarer

- Feature-Vergleichstabelle zeigt jetzt echte Plan-Spalten: Free / Werbefrei / Raid Boost / Analyse / Alles drin — statt ungenauer Tier-Labels
- „Bot-Werbung deaktivieren" steht jetzt korrekt beim Werbefrei-Plan, nicht erst beim Bundle
- Trial-Plan zeigt keine Chat-Werbung-Deaktivierung mehr (war schon im Backend korrekt, ist jetzt auch im Vergleich sichtbar)
- Feature-Kacheln auf der Pricing-Seite haben jetzt einen klaren Hinweis „+ Auswählen" und einen erklärenden Titel darüber

## #41 — Bundle-Preise angepasst

- Werbefrei + Raid Boost: 5,99 → 6,99 €/Mo. (geringerer Rabatt, da Raid Boost von anderen Streamern abhängt)
- Werbefrei + Analyse: 11,49 → 10,49 €/Mo. (2 € Rabatt statt bisher 99 ct)
- Alles drin und Analyse + Raid Boost bleiben unverändert

## #40 — AGB: Name und Links korrigiert

- Name „EarlySalty / Deadlock Partner Network" → „Deutsche Deadlock Community" in AGB und Seitentitel
- Zurück-Button und Footer-Link auf der AGB-Seite zeigen jetzt korrekt auf `/twitch/pricing`
- Kündigung in §5 verlinkt jetzt auf das Dashboard statt auf die veraltete Abo-Seite

## #39 — Jahresplan: Sofortabbuchung, 14 Monate Zugang und Rechtssicherheit

- Jahrestarif wird jetzt direkt beim Kauf abgebucht (kein Trial-Zeitraum mehr)
- Käufer eines Jahresabos erhalten automatisch 2 Bonusmonate on top — insgesamt 14 Monate Zugang
- Widerrufsbelehrung nach § 356 Abs. 5 BGB direkt im Checkout sichtbar und bestätigungspflichtig
- AGB aktualisiert: neuer Abschnitt zu sofortiger Leistungserbringung, Jahresbonus klar beschrieben, „6-Monats"-Option entfernt
- „30 Tage kostenlos testen"-Button auf der Pricing-Seite ist jetzt vollständig funktional (einmalig pro Account)
- Feature-Picker auf der Pricing-Seite ersetzt die alten Plan-Karten

## #38 — Neue Abo-Pläne, Jahrestarif und überarbeitete Pricing-Seite

- Zwei neue Bundle-Pläne: „Werbefrei + Analyse" (11,49 €/Mo.) und „Alles drin" (13,99 €/Mo.)
- Jahrestarif (12 Monate) mit 20 % Rabatt auf allen bezahlten Plänen wählbar
- Pricing-Seite vollständig überarbeitet: übersichtliches Toggle Monatlich/Jährlich, keine Informations-Überflutung
- Alte `/twitch/abbo`-Seite leitet dauerhaft auf `/twitch/pricing` weiter
- Stripe-Produkte und -Preise für die neuen Bundles automatisch angelegt

## #37 — Security-Scanner-Alerts bereinigt

- 271 offene Code-Scanning-Alerts (Semgrep + CodeQL) vollständig bearbeitet
- Echte Bugs behoben: undefinierte Variable in Tests, ungenutzte Variable, Lambda-Zuweisung
- Alle False-Positive-Alerts mit korrekten Suppression-Kommentaren versehen (`# nosemgrep`, `# lgtm[...]`)
- 4 Discord-Snowflake-ID-Alerts als False Positive über GitHub API dismissed

## #36 — Werbefrei-Plan für 3,99 € + besseres Onboarding und FAQ

- Neuer Plan „Werbefrei" für 3,99 €/Monat: Die Bot-Discord-Einladung in deinem Chat ist dauerhaft aus — auch wenn ein Admin gerade einen globalen Aktions-Text aktiv hat
- Combo „Werbefrei + Raid Boost" für 5,99 €/Monat (spart 2 € gegenüber Einzelkauf)
- Onboarding-Tour im Dashboard erweitert: erklärt jetzt auch was der Bot grundsätzlich macht, deinen aktuellen Plan, Werbung-Einstellungen und wo du Hilfe findest
- Neue Sidebar-Sektion „Hilfe" mit Direktlink zur FAQ und Knopf zum Neu-Starten der Tour
- FAQ um drei neue Sektionen ergänzt: „Was macht der Bot eigentlich?", „Pläne & Preise" und „Chat-Werbung"

## #35 — Clips ohne Alterswarnung und Twitch-Embeds funktionsfähig

- Clips von xradoo_ und miracleghost9 ersetzt — beide hatten eine Twitch-Alterswarnung, die das Einbetten verhinderte
- Ersetzt durch Clips von friduzockt und einsbezi aus der Community
- Caddy-Konfiguration repariert: Twitch-Embeds auf der /streamer/-Seite wurden durch eine zu restriktive Content-Security-Policy geblockt — jetzt erlaubt
- Demo-Dashboard kann wieder in die Streamer-Website eingebettet werden (gleiches CSP-Problem behoben)

## #34 — Branding der öffentlichen Website vereinheitlicht

- Titel und Link-Vorschau zeigen jetzt „Deutsche Deadlock Community" statt „EarlySalty"
- Kaputte Clips (gelöschte Medal-Links, tote EarlySalty-Clips) durch echte Community-Clips ersetzt
- Favicon auf allen Seiten einheitlich — EarlySalty-„E" im Bot-Favicon entfernt
- Logos über alle Projekte auf denselben Stand gebracht
- „DDC"-Abkürzung im gesamten öffentlichen Website-Text durch ausgeschriebenen Namen ersetzt

## #33 — Dashboard zeigt Twitch-Auth nach Erstanmeldung korrekt als aktiv

- AutoRaid-Status wird nach der ersten Twitch-Autorisierung sofort als aktiv angezeigt
- Bisher blieb das Dashboard auf „inaktiv", obwohl die Auth korrekt gespeichert war und Raids funktionierten
- Betrifft nur neue Streamer beim allerersten Auth-Vorgang, nicht Re-Auth

## #32 — Bot-Absturz beim Start behoben

- Twitch-Bot und Dashboard starten wieder fehlerfrei
- Fehler trat auf, weil eine neue Storage-Funktion intern vergessen wurde zu verknüpfen

## #31 — GitHub Actions Minutenverbrauch deutlich reduziert

- Fünf tägliche Security-Workflows auf reine Event-Trigger umgestellt (kein Schedule mehr)
- Security-Scans und Secret-Scanning laufen jetzt nur noch bei Push und Pull Request
- Semgrep bricht den Build nicht mehr bei jedem Fund ab — Ergebnisse werden als Artifact gespeichert

## #30 — Sicherheitslücke in Test-Abhängigkeit geschlossen

- pytest in der CI-Test-Pipeline auf Version 9.0.3 angehoben — schließt eine Privilege-Escalation-Lücke über das /tmp-Verzeichnis

## #29 — Security-Scan: 390 Alerts bereinigt

- Sensible Werte (User-IDs, Streamer-Logins, Dateipfade) werden jetzt überall vor dem Logging bereinigt — verhindert Log-Injection
- Discord-Nutzer-IDs im Code sind als öffentliche IDs markiert, nicht als Secrets
- Rund 350 False-Positive-Alerts aus dem Semgrep-Scanner (SQL-Queries, Logger-Credential, HTML-Format, Dynamic-Imports) wurden mit präzisen Suppression-Kommentaren ausgestattet, damit echte neue Probleme künftig auffallen
- 63 unbenutzte Imports, 41 unbenutzte Variablen und weitere kleine Stil-Verstöße automatisch bereinigt

## #28 — Hintergrund-Konsolidierungen (Audit-Cleanup Phase 2)

- Streamer-Plan- und Billing-Lookups laufen jetzt über eine gemeinsame Quelle — ein Test, der wegen Schema-Drift rot war, ist wieder grün
- Schnellere Streamer-Suche im Dashboard durch zwei neue Datenbank-Indexe für case-insensitive Logins
- Datenbank-Pool gehärtet: Default-Verbindungen 4 → 10, Connect-Timeouts gesetzt, automatischer Retry bei seltenen Postgres-Deadlocks
- Internal-API-Server -319 Code-Zeilen schlanker (doppelte Helper-Logik in policy.py konsolidiert)
- Doku auf Stand gebracht: korrekte Routen-Übersicht in INDEX/API.md, Stream-Report-Sektion ergänzt, veraltete „geplant"-Marker entfernt

## #27 — Stabilität verbessert (Audit-Cleanup Phase 1)

- Social-Media-Uploads laufen nicht mehr in endlose Wartezeiten, wenn TikTok-, Instagram- oder Login-Provider hängen — alle externen Calls haben jetzt feste Timeouts
- Bot-Reload entfernt nicht mehr versehentlich noch laufende Hintergrund-Module aus dem Speicher — weniger sporadische Crashes nach Cog-Reloads
- Doppelte HTTP- und KI-Client-Initialisierungen zusammengezogen, künftige Wartung einfacher
- Test- und Linter-Konfiguration vereinheitlicht, fehlende Dependency `cryptography` korrekt deklariert

## #26 — KI Chat-Analyse (MiniMax Deep) funktioniert jetzt

- "Analyse starten"-Button im Chat-Analytics-Dashboard war kaputt — der Backend-Endpoint crashte sofort
- Ursache: fehlendes `import json` im Backend-Modul
- Zusätzlich: TypeScript-Buildfehler behoben (fehlende `streamer`-Prop und Typcast im Donut-Chart)
- Dashboard neu gebaut und Bot neu gestartet

## #25 — Security-Deep-Scan verschlankt, kein Sicherheitsverlust

- Python-Security-, JavaScript-Security- und Semgrep-Scans aus dem Deep-Scan entfernt — diese laufen bereits täglich in der Security-Fortress
- Deep-Scan fokussiert sich jetzt auf Trivy-Filesystem-Scan und OSSF-Scorecard — beides hat keinen Doppelläufer
- Weniger doppelte CI-Minuten, gleiche Abdeckung

## #24 — CI-Laufzeiten optimiert, kein Sicherheitsverlust

- Security-Scans (Container, IaC, Supply-Chain) laufen jetzt wöchentlich statt täglich — Schutz bleibt vollständig durch Push-/PR-Trigger
- Security-Incident-Automation läuft jetzt täglich statt alle 6 Stunden — 75 % weniger Runs
- Dependency-Review hat keinen sinnlosen Tages-Schedule mehr

## #23 — CI-Artifacts werden nach 30 Tagen automatisch gelöscht

- Alle automatisch erzeugten CI-Berichte (Security-Scans, Dependency-Reports, Logs) werden ab jetzt nach 30 Tagen automatisch von GitHub entfernt
- Verhindert, dass sich der GitHub-Actions-Speicher dauerhaft volläuft

# Changelog

## #22 — Werbung sensibler für neue Zuschauer + Viewer-Trigger auch bei Normalzahlen

- Discord-Einladung wird jetzt schon bei 3 Chat-Nachrichten im Fenster ausgelöst statt 5
- Viewer-basierter Trigger greift ab sofort auch wenn die Zuschauerzahl einfach auf normalem Niveau liegt — kein "Spike" mehr nötig
- Cooldown für den Viewer-Trigger auf 60 Minuten gesenkt (war 90)
- Neue Bedingung: Promo wird nur gesendet, wenn mindestens 2 Chatter im aktuellen Fenster die letzte Werbung noch nicht gesehen haben — verhindert, dass dieselbe Zuschauerschaft mehrfach dieselbe Meldung bekommt; nach 2 Stunden gilt ein Zuschauer wieder als "neu"
- Im Admin-Dashboard gibt es jetzt Felder für Start- und Endzeit der globalen Promo-Überschreibung — die zeitbefristete Steuerung funktioniert damit korrekt

## #20 — Stream-Reports: Rating-System, neues Report-Layout + Auto-Retry

- Jeder Report hat jetzt Bewertungs-Buttons (Gut / Neutral / Schlecht) mit optionalem Kommentar — direkt unter dem jeweiligen Report sichtbar
- Reports zeigen jetzt alle Analyse-Abschnitte aus dem neuen Minimax-Schema: Snapshot, Kritische Momente, Audience, Chat-Diagnose, Wachstum, Vergleich und Maßnahmen
- Keine chinesischen Zeichen mehr in Reports — Minimax bekommt jetzt eine explizite Sprachanweisung
- Fehlgeschlagene Reports werden jetzt automatisch alle 30 Minuten bis zu 3x erneut versucht
- Minimax-Anfragen brechen nach 3 Minuten automatisch ab statt ewig zu hängen

## #19 — Stream-Reports für alle Partner freigegeben

- Stream-Reports sind jetzt für alle aktiven Partner sichtbar — kein kostenpflichtiger KI-Plan mehr nötig
- Fehlerbehebung: Dashboard-Service wurde nach Code-Änderungen nicht neu gestartet, Reports haben deshalb nicht geladen

## #18 — Stream-Reports: Neues Analyse-Schema + Minimax komplett aufgedreht

- Report-Prompt komplett neu geschrieben: 5 konkrete Analyse-Aufgaben (Kritische Momente, Audience-Qualität, Chat-Diagnose, Wachstums-Signale, Ehrlicher Vergleich)
- Minimax darf jetzt deutlich mehr schreiben: Token-Limit von 6.000 auf 16.000 erhöht
- Report-Ausgabe folgt jetzt einem klaren deutschen Schema (snapshot, momente, audience, chat_diagnose, wachstum, vergleich, massnahmen)
- Fehler-Fallback passt sich dem neuen Schema an — Dashboard bricht nicht mehr bei Parse-Fehlern

## #17 — Stream-Reports: Backfill beim Start + weitere SQL-Bugfixes

- Beim Bot-Start werden automatisch die letzten 3 Sessions pro Streamer mit einem Minimax-Report nachgefüllt, falls noch keiner existiert
- Wöchentliche Titel-Insights: zweiter SQL-Bug behoben (Session-Lookup ging an falscher Spalte, Sessions wurden nie geladen)
- Bisherige Stream-Reports ohne Fehler werden nicht doppelt generiert

## #16 — Stream-Reports mit Minimax funktionieren jetzt für alle Streamer

- Nach jedem Stream-Ende erstellt Minimax automatisch einen detaillierten Report mit Viewer-Kurve, Chat-Analyse und Vergleich zu früheren Sessions
- Reports werden für alle Streamer generiert — kein kostenpflichtiger Plan mehr nötig, um die Funktion zu nutzen
- Admins können alle Reports im Dashboard einsehen, unabhängig vom Streamer-Plan
- Wöchentliche Titel-Insights waren wegen eines Datenbankfehlers kaputt — dieser ist jetzt behoben
- Tabellen für KI-Reports werden beim ersten Start automatisch angelegt, falls die Migration noch nicht gelaufen ist

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
