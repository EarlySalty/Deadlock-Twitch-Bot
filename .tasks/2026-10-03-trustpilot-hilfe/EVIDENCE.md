# Prüfnachweise

- Ausgangsbasis: echter Remote-Stand `aeff9c21807b64916bd5dacdb41b0569dcc4c131`.
- Vor dem festen Reviewstand konfliktfrei auf `8db73ec3779239cd237262c4392a2c612f16c6e0` nachgezogen. Die zusätzlichen Installer-/Configänderungen stammen aus dem geprüften Peerpaket, nicht aus diesem UI-Auftrag.
- Eigener Branch: `feat/trustpilot-hilfe-20261003`; Worktree `/home/nathanael/.worktrees/trustpilot-hilfe-20261003`.
- Lokale AGENTS.md und CLAUDE.md gelesen; Texte mit humanizer und no-em-dashes geprüft.
- Vorhandener Graph zu `StreamerNetworkPage`, `help`, `InternalHomeLanding` und `changelog` abgefragt. Der Graph enthält die Bot-Hilfekomponente noch nicht; deren aktuelle Quelle wurde danach direkt geprüft.
- Website-Build und Dashboard-Build erfolgreich. Vorhandene Warnungen zu externen Assets, Chunks und React-Keys bleiben bestehen.
- Website-Suite: 55 bestanden, keine Fehler.
- Dashboard-Suite: 419 bestanden, fünf Fehler. Dieselben fünf Testnamen auf unverändertem `aeff9c21` im separaten Baseline-Worktree reproduziert: drei Paletteprüfungen, eine Rahmenprüfung und eine OBS-Hilfeprüfung. Der temporäre Baseline-Worktree wurde danach entfernt.
- Dashboard-Suite nach der Preview-Fixturekorrektur erneut mit 419 bestandenen Tests und denselben fünf Fehlern abgeschlossen. Logs: `/tmp/trustpilot-hilfe-website-tests.log`, `/tmp/trustpilot-hilfe-dashboard-tests-final.log`, `/tmp/trustpilot-hilfe-dashboard-baseline.log`.
- Kein eigener JSON-Scorefetch, keine eigenen Scoretypen oder zusätzliche Scoretests verblieben. Kein zusätzliches Caddy-connect-src erforderlich.
- Website-Devvorschau: `http://127.0.0.1:4178/streamer/`.
- Dashboard-Vorschau mit vorhandenem Fixturemodus: `http://127.0.0.1:4174/dashboard`. Kein produktiver Login umgangen und keine Authdaten gelesen.
- Eigene Brave-Prüfung der Hauptsession: Desktop 1260 × 900, mobil 390 × 844 und geringe Höhe 640 × 360, Collector und Eingabe nutzbar, kein horizontaler Überlauf; Escape schließt die Hilfe und stellt den Fokus auf den Launcher zurück. Dashboard mit 1920 × 1080 und 390 × 844 geprüft, vollständiger vierter Changelog-Eintrag erreichbar. Screenshots unter `/home/nathanael/.claude/sichtpruefung/trustpilot-dashboard-{desktop,mobil,changelog}-20261003.png`.
- Selbstreview des ersten festen UI-Snapshots: `ALLOW`. Der konkrete Hinweis zum Fokusverlust beim Neuigkeiten-Umschalten wurde anschließend behoben: derselbe Anker bleibt beim Öffnen und Einklappen vorhanden. Der neue Stand wird erneut selbst und unabhängig geprüft.
- Abschließende unabhängige Teilabnahme auf `9f8a907d`: Positions-, Collector- und Dashboardteil fertig, keine weiteren Fixes nötig. Beide echten Enter-Umschaltungen behalten den Fokus auf demselben Anker, alle vier Vorschau-Neuigkeiten bleiben erreichbar. Desktop 1440 × 900 mit Labelmitte bei 55 Prozent geprüft; mobile Platzierung direkt über der Hilfe hält den Hero-Text lesbar. Kleine Höhe 640 × 360 und äquivalenter Platz bei 200 Prozent Zoom geprüft.
- Erneuter Selbstreview auf `9f8a907d`: `ALLOW`. Fehlende automatisierte Screenshotkonfiguration durch echte Browserprüfungen abgedeckt. Der verbleibende Hinweis zu `replaceState` folgt der bereits bestehenden zustandslosen App-/Home-Konvention; kein bestätigter Fehler. Nach dieser Abnahme ausschließlich den Dokumentationsnachweis vervollständigt, keine weitere Produktänderung.
- Der echte Score-Embed ist separat offen. Die Hauptsession hat die Auslieferung der fertigen Positions- und Dashboardänderung unabhängig davon autorisiert. Kein Fertignachweis für den Score-Teil.
