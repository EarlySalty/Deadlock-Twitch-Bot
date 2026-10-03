# Prüfnachweise

- Ausgangsbasis: echter Remote-Stand `aeff9c21807b64916bd5dacdb41b0569dcc4c131`.
- Eigener Branch: `feat/trustpilot-hilfe-20261003`; Worktree `/home/nathanael/.worktrees/trustpilot-hilfe-20261003`.
- Lokale AGENTS.md und CLAUDE.md gelesen; Texte mit humanizer und no-em-dashes geprüft.
- Vorhandener Graph zu `StreamerNetworkPage`, `help`, `InternalHomeLanding` und `changelog` abgefragt. Der Graph enthält die Bot-Hilfekomponente noch nicht; deren aktuelle Quelle wurde danach direkt geprüft.
- Website-Build und Dashboard-Build erfolgreich. Vorhandene Warnungen zu externen Assets, Chunks und React-Keys bleiben bestehen.
- Website-Suite: 55 bestanden, keine Fehler.
- Dashboard-Suite: 419 bestanden, fünf Fehler. Dieselben fünf Testnamen auf unverändertem `aeff9c21` im separaten Baseline-Worktree reproduziert: drei Paletteprüfungen, eine Rahmenprüfung und eine OBS-Hilfeprüfung. Der temporäre Baseline-Worktree wurde danach entfernt.
- Logs: `/tmp/trustpilot-hilfe-website-tests.log`, `/tmp/trustpilot-hilfe-dashboard-tests.log`, `/tmp/trustpilot-hilfe-dashboard-baseline.log`.
- Kein eigener JSON-Scorefetch, keine eigenen Scoretypen oder zusätzliche Scoretests verblieben. Kein zusätzliches Caddy-connect-src erforderlich.
- Website-Devvorschau: `http://127.0.0.1:4178/streamer/`.
- Dashboard-Vorschau mit vorhandenem Fixturemodus: `http://127.0.0.1:4174/dashboard`. Kein produktiver Login umgangen und keine Authdaten gelesen.
- Die abschließende Sichtprüfung, feste Produkt-SHA, Selbstreview und Auslieferung stehen noch aus. Der echte Score-Embed ist separat offen.
