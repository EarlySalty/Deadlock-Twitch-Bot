# Review: Schwarz und Kacheln der Landingpage ins Dashboard

status: erledigt
datum: 2026-09-04
reviewer: frischer Agent (Opus 4.8), read-only
diff: git diff origin/main...HEAD
urteil: FREIGABE

## Urteil

FREIGABE. Alle sechs REQ und fuenf INV erfuellt, Werte 1:1 zur tatsaechlich aktiven Vorlage (theme-v2.css), Scope exakt eingehalten, Regressionstest faengt den Rueckfall, Screenshots zeigen den schwarzen Rasterlook mit flachen Kacheln.

## Werte-Treue (E1 / REQ-01 / REQ-02)

- Grund-Verlauf: bot/dashboard_v2/src/index.css:65 `--gradient-bg: linear-gradient(180deg, #0f0f0e 0%, #0b0b0b 55%, #101010 100%)` == Vorlage website/src/theme-v2.css:26. Body zieht nur noch diesen Verlauf (index.css:84), die drei Gold-Radials und die Holzmaserung sind raus. Treffer.
- Raster: index.css:93-99 `rgba(255, 255, 255, 0.045)`, `background-size: 36px 36px`, `mask-image: radial-gradient(ellipse at top, black 40%, transparent 75%)`, `opacity: 0.72` == Vorlage theme-v2.css:50-55 (Farbe/Deckkraft) plus index.css:120-125 (Groesse/Maske). Treffer.
- Kachel-Fuellung (`.glass` index.css:162-169, `.panel-card` index.css:182-191): `background: linear-gradient(0deg, rgba(201, 168, 106, 0.05), rgba(201, 168, 106, 0.05)), rgba(18, 18, 18, 0.86)`. Das ist der EXAKTE Wert der echten Vorlage theme-v2.css:64-66. Der Reviewer-Hinweis auf `rgba(239,212,157,0.05)` ist nicht der Ist-Zustand der Vorlage: PLAN E4 nennt diesen Wert faelschlich, EVIDENCE (theme-v2.css:62-70) und die Datei selbst nennen `rgba(201,168,106,0.05)`. Der Implementierer hat gegen die reale Quelle gebaut, nicht gegen die falsche PLAN-Zeile. Kein Mangel.
- Kachel-Kante: index.css:167 / :189 `border: 1px solid rgba(239, 212, 157, 0.18)` == theme-v2.css:67. Treffer.
- Kachel-Schatten: index.css:168 / :190 `box-shadow: 0 14px 40px rgba(0, 0, 0, 0.5)` == theme-v2.css:68. Treffer.
- Border-Tokens: index.css:54-56 `0.16 / 0.34 / 0.28` == theme-v2.css:18-20. Alt war 0.22/0.40/0.34. Treffer.
- Radius: Vorlagen-Kachelblock setzt keinen eigenen Radius (erbt), die neuen `.glass/.panel-card` aendern den Radius ebenfalls nicht. Konsistent.
- Nieten (`.panel-card::after`), Gusseisen-Streifen (`repeating-linear-gradient`), Bevel-Insets und die alten Karten-Backgrounds sind ersatzlos entfernt.

## Vollstaendigkeit (REQ-04)

- Alle in EVIDENCE als "muessen weg" gelisteten warmen Flaechen sind entfernt: body-Radials, Holzmaserung, `body::after`-Koernung, `body::before`-Goldlinien (jetzt weiss), `.internal-home-vibe::before/::after` samt Keyframe `internal-home-gradient-flow` (grep leer), `BackgroundBlobs` in DashboardShell.tsx (Funktion und Aufruf raus, Zeile 6-12 und 30 der alten Fassung).
- Verbliebene Gold-Werte in index.css sind semantische Akzente, nicht die Shell-Grundtoenung, und liegen ausserhalb der Kartenklassen/Shell-Hintergrund-Regeln: `--color-primary-soft` (29), Glow-Schatten-Tokens (67-68), `internal-home-log-card`-Tonleiste (606, 4px-Indikator), Hero-Aura (362-383, eigenes Bauteil). Diese fallen unter REQ-03 (Akzente bleiben) und liegen ausserhalb des erlaubten Aenderungsbereichs (INV-01), korrekt unangetastet.
- `.internal-home-vibe` bleibt als Klasse vorhanden (index.css:514, DashboardShell.tsx:20). Test-Anker intakt.
- Kein Bruch abgeleiteter Klassen: `.card-glow` bleibt (index.css:196ff), `.panel-card`/`.glass` behalten Struktur; keine Regel setzt entfernte Pseudo-Elemente voraus.

## Kontrast (REQ-05)

- Neue Kachelflaeche `rgba(18,18,18,0.86)` ist minimal dunkler/deckender als das alte `--color-card` `rgba(20,20,20,0.82)`. Helle Textfarben (`--color-text-primary #f2eee6`, `--color-text-secondary #9d968a`) und Statusfarben (`#43b581 / #E8A33D / #FF5A3C`) gewinnen auf dunklerem Grund an Kontrast, verlieren nirgends. Keine Klasse wird zu dunkel. Border-Alpha sinkt (0.22->0.16), betrifft Kantensichtbarkeit, nicht Textkontrast, und ist die vorgeschriebene Vorlagen-Alpha.

## Regressionen / Tests

- Neue Datei tests/dashboardLook.test.ts, in package.json nur `scripts.test` erweitert (kein anderer Skript-Eingriff). INV-04 gewahrt.
- Testschaerfe: `.panel-card::after`, `.internal-home-vibe::before/::after`, `BackgroundBlobs` per `doesNotMatch` gegen echten Quelltext; `repeating-linear-gradient` nur im ausgeschnittenen `.glass{`/`.panel-card{`-Block (block()-Helper schneidet `{`..erstes `}`, in beiden Bloecken kein verschachteltes `}`, faengt also einen Rueckfall). Raster- und `--gradient-bg`-Regex matchen die exakten Vorlagenwerte. Bei Revert der index.css wird jeder Punkt rot. Echte Regression, keine Regex-Falle.
- `dashboardShell.test.ts` unveraendert und weiter im Testlauf; `internal-home-vibe` vorhanden -> bleibt gruen.
- Keine neuen Hex-Literale im src (Kachel/Border/Grund als rgba; `--gradient-bg`-Hex war bereits in origin/main vorhanden, kein Neuzugang) -> brandPalette.test.ts unberuehrt.
- Keine neuen Kommentare (Diff-Scan auf `/* // */` in `+`-Zeilen leer), entfernte Kommentare in angefassten Bloecken raus. INV-05 erfuellt.
- Scope: geaenderte Dateien = index.css, DashboardShell.tsx, tests/dashboardLook.test.ts, package.json plus Artefakte/Screenshots. Alle im erlaubten Bereich. DashboardSidebar.tsx, App.css, uplinkHelp.css unberuehrt (nicht noetig).

## Screenshots (REQ-06)

- Sieben Route-Screens (analyse, dashboard, overlay, pricing, social, uplink, verwaltung) plus vorlage-streamer.png vorhanden.
- Sicht: Hintergrund neutral schwarz mit feinem weissem Raster und Ellipsen-Maske, Karten flach mit dezenter heller Kante, Sidebar-Kachel gleich. Gold-Akzente (aktiver Nav-Eintrag, Buttons, Badges, Freischalten-Buttons, Verlaufstext) erhalten. Keine Wärme-Flut, keine Gusseisen-Streifen, keine Niet-Punkte. Deckt sich mit vorlage-streamer.png.

## Ausserhalb Scope (nur bestaetigt)

- bot/dashboard_v2/src/components/socialmedia/LayoutEditor.tsx:243/245 tragen weiter warme Diagonal-Streifen (`repeating-linear-gradient(45deg, rgba(197,160,89,0.18) ...)`). Ausserhalb Shell/Kartenklassen, nicht im Diff, korrekt unangetastet. Kandidat fuer eine Folgeaufgabe, kein Mangel dieser Aenderung.
