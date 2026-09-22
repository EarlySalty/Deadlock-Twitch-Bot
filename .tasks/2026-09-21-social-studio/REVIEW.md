# Review: Social Studio Integration

## Befunde des eigenen Reviews vor dem Merge

1. `studio.css` deklarierte `@font-face` für `/brand/fonts/manrope-latin.woff2` und `sora-latin.woff2`. Eine zwischenzeitliche Umstellung auf die im Index geladenen Google-Fonts-Familien wurde nach Prüfung der Produktion zurückgenommen: Caddy liefert `/brand/*` aus dem zentralen Marken-Paket `Website/dl-brand` aus (HTTP 200, `font/woff2`), und der Browserlauf bestätigt geladene Studio-Fonts ohne 404. Die Marken-Paket-Variante bleibt.
2. Status-Semantik geprüft: terminale und veröffentlichende Zustände werden über `queueStage` nie freigebbar; alter `approval.state` kann das nicht kippen (Test abgedeckt).
3. Kennzahlen: aus vollständigem Bestand berechnet, nicht aus einer 24er-Stichprobe; `reicht_fuer_tage = null` wird als „Manuell" angezeigt, nicht als 0 Tage.
4. Teilfehler beim Zeitplan-Speichern: Serverstand wird neu gelesen, Entwurf erhalten, Meldung unterscheidet teilweise/fehlgeschlagen; Vollautomatik wird erst nach den Plattform- und Kategorienänderungen geschrieben.
5. Kanalwechsel: `key={streamer}` remountet, Query-Keys kanalgebunden, Bestätigungsdialog bei ungespeicherten Entwürfen; Bestandsabfrage bricht mit Fehler ab, statt irreführende Zahlen anzuzeigen.
6. Konten: `uses_global_fallback` wird als Sammelverbindung ausgewiesen; das Backend trennt die Sammelverbindung nur mit explizitem `__global__`-Scope (Handler geprüft).
7. Fremde Branches (`feat/twitch-ddc-brand-20260921`) und fremde Worktrees unberührt; Haupt-Checkout auf `feat/player-multi-steam-rank-me` unberührt.

## Baseline-Fehler, bewusst nicht angefasst

`brandPalette` (Verwaltungs-/Admin-Dateien), `dashboardProfileCache` (Sidebar-Skeleton), `uplinkEmpfehlung` (OBS-Hilfe) fehlten auf sauberem main identisch. Keine fremden Dateien für diese Suite umgeschrieben.
