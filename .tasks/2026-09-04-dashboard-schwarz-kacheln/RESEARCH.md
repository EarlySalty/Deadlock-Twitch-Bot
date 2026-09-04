# Research: Schwarz und Kacheln der Streamer-Landingpage ins Dashboard übernehmen

status: erledigt
datum: 2026-09-04
klasse: mittel

## Auftrag

Das Streamer-Dashboard (alle Shell-Routen) trägt denselben neutralen schwarzen
Rasterhintergrund und dieselben flachen dunklen Kacheln mit feiner Kante wie die
Landingpage `/streamer`; die warmen Gold-Auren und die Gusseisen-Kacheln fallen weg.

## Beobachtungen (belegt, Datei:Zeile)

- `/streamer` lädt index.css plus theme-v2.css und setzt `data-theme="v2"` (website/index.html:5, website/src/main.tsx:3,5). Die aktiven Vorlagenwerte stehen also im v2-Scope von website/src/theme-v2.css, nicht im warmen Basis-index.css.
- Die schwarzen Flächen-Tokens des Dashboards stimmen bereits mit der Vorlage überein: `--color-bg:#0b0b0b`, `--color-background:#101010`, `--color-card:rgba(20,20,20,0.82)`, `--color-card-hover:rgba(32,32,32,0.88)`, `--gradient-bg` und `--gradient-hero` sind identisch (bot/dashboard_v2/src/index.css:23-26,65-66 gegen website/src/theme-v2.css:12-15,26-27). Der warme Eindruck kommt nicht aus den Tokens.
- Der warme Eindruck kommt aus vier Overlay-Ebenen, die die Vorlage nicht hat: Body-Radials plus Holzmaserung (index.css:86-99), goldenes Raster (index.css:105-117), zwei animierte Gold-Aura-Ebenen `.internal-home-vibe::before/::after` (index.css:609-680) und die farbigen `BackgroundBlobs` (DashboardShell.tsx:6-12,30).
- Die Vorlage-Kachel ist flach: ein dünner Goldfilm über `rgba(18,18,18,0.86)`, feine Kante `rgba(239,212,157,0.18)`, ein weicher Schatten `0 14px 40px rgba(0,0,0,0.5)` (website/src/theme-v2.css:62-70). Das Dashboard trägt stattdessen Gusseisen-Streifen, Lichtabfall, Bevel-Insets und Eck-Nieten (index.css:194-209,223-256).
- Farb-Tokens werden per Tailwind-v4-`@theme` in index.css gepflegt (index.css:18); tailwind.config.js/.ts sind leer. `bg-card`, `border-border`, `text-text-secondary` mappen automatisch aus den `@theme`-Variablen. ddc-design-tokens.css ist nicht importiert und damit tot (EVIDENCE, ddc-design-tokens.css:14-18).
- Die Sidebar- und Header-Flächen laufen über Tokens (`bg-background/60`, `border-border`) und folgen einer Token-Änderung automatisch (DashboardSidebar.tsx:229,255,266; Header.tsx:117,145); Gold-Akzente sind bewusst gesondert (`bg-primary/10`, `from-primary/30`) und bleiben.
- dashboardShell.test.ts verlangt den Klassennamen `internal-home-vibe` an der Shell; die Klasse muss bleiben, nur ihre Pseudo-Element-CSS wird neutralisiert.
- brandPalette.test.ts prüft ausschließlich `#rrggbb`-Literale gegen eine Whitelist (die Patch-Schwarz-Werte und Textfarben sind schon drin) sowie weißen Text auf hellen Gold-Flächen; `rgba(...)`-Werte sind von der Hex-Prüfung nicht erfasst.

## Hypothesen (unbelegt, nie als Fakt weiterreichen)

- Das statische Körnungs-Overlay (`body::after`, index.css:121-129, opacity 0.028) hat die Vorlage nicht; es ist so schwach, dass es vermutlich stehenbleiben kann, ohne den flachen Look zu stören. Prüfen per Screenshot.
- Die Vorlage-Kachel nutzt Website-Gold `rgba(201,168,106,...)`; das Dashboard-Gold ist `#C5A059` = `rgba(197,160,89,...)`. Für Markentreue vermutlich besser den Dashboard-Goldwert im Kachelfilm nehmen. Optisch minimal, per Screenshot gegen die Vorlage prüfen.

## Wahrscheinlich zu ändernde Dateien

- bot/dashboard_v2/src/index.css : Body-Hintergrund, Raster, `.internal-home-vibe::before/::after`, `.glass`, `.panel-card`, `.panel-card::after`, Border-Alpha-Tokens.
- bot/dashboard_v2/src/components/layout/DashboardShell.tsx : `BackgroundBlobs` entfernen, Klasse `internal-home-vibe` behalten.

### Konkreter Token- und Regel-Vorschlag (alt -> neu, Vorlagenwert mit Fundstelle)

Flächen-Tokens (bg, background, card, card-hover): keine Änderung nötig, Dashboard und Vorlage sind bereits gleich.

| Element | Dashboard alt | neu (Vorlage) | Vorlage-Fundstelle |
|---|---|---|---|
| `--color-border` (index.css:54) | `rgba(239,212,157,0.22)` | `rgba(239,212,157,0.16)` | theme-v2.css:18 |
| `--color-border-strong` (index.css:55) | `rgba(239,212,157,0.40)` | `rgba(239,212,157,0.34)` | theme-v2.css:19 |
| `--color-border-hover` (index.css:56) | `rgba(239,212,157,0.34)` | `rgba(239,212,157,0.28)` | theme-v2.css:20 |
| Body-Hintergrund (index.css:86-99) | 3 Gold-Radials plus Holzmaserung plus `var(--gradient-bg)` | nur `var(--gradient-bg)` | theme-v2.css:46 |
| Raster `body::before` (index.css:110-115) | Gold `rgba(197,160,89,0.05)`, opacity `0.35` | weiß `rgba(255,255,255,0.045)`, opacity `0.72`, Größe 36px und Maske bleiben | theme-v2.css:51-54, index.css(website):122-124 |
| Kachel-Fläche `.panel-card`/`.glass` (index.css:194-242) | Gusseisen-Streifen, Lichtabfall, Bevel-Insets | `background:linear-gradient(0deg, rgba(197,160,89,0.05), rgba(197,160,89,0.05)), rgba(18,18,18,0.86)` | theme-v2.css:65-67 |
| Kachel-Kante `.panel-card`/`.glass` | `1px solid var(--color-border)` plus Bevel | `1px solid rgba(239,212,157,0.18)` | theme-v2.css:68 |
| Kachel-Schatten `.panel-card`/`.glass` | dreiteiliger Schatten mit Insets | `0 14px 40px rgba(0,0,0,0.5)` | theme-v2.css:69 |

Zu streichen bzw. neutralisieren:
- `.panel-card` Gusseisen-`background-image` (Lichtabfall plus Schleifspuren) und die zwei Bevel-`inset`-Schatten (index.css:228-241).
- `.panel-card::after` Eck-Nieten komplett (index.css:245-256).
- `.glass` Gusseisen-`background-image` und Bevel-Insets (index.css:196-208); flache Fläche wie oben, `backdrop-filter:blur(14px)` kann bleiben.
- `.internal-home-vibe::before` samt Animation `internal-home-gradient-flow` (index.css:585-630): Gold-Auren raus. Entweder Pseudo-Element entfernen oder auf `background:none` setzen.
- `.internal-home-vibe::after` (index.css:631-680): Gold-Auren raus; falls ein Raster gewünscht bleibt, nur die neutralen weißen Gridlinien und eine dezente Vignette behalten, sonst entfernen (das Body-Raster deckt das Netz bereits ab).
- `BackgroundBlobs` (DashboardShell.tsx:6-12) und dessen Aufruf (Zeile 30) entfernen; die Klasse `internal-home-vibe` an der Wrapper-`div` (Zeile 29) bleibt unverändert.
- `.card-glow` und `.card-glow::before` (index.css:259-303) bleiben (REQ-03: goldener Hover-Glow), die Kante folgt der neuen `--color-border`-Alpha.

## Risiken / Seiteneffekte

- brandPalette.test.ts: rot bei jedem neuen `#hex` außerhalb der Whitelist. Der Vorlagen-Kachelvorschlag und alle Raster/Aura-Werte sind `rgba(...)`, werden also nicht geprüft; kein neues Hex-Literal einführen. `rgba(18,18,18,...)` statt `#121212` schreiben, sonst müsste die Whitelist erweitert werden.
- dashboardShell.test.ts: bricht, falls die Klasse `internal-home-vibe` aus der Shell verschwindet oder eine Seite sie selbst setzt. Klasse behalten, nur CSS neutralisieren; das Entfernen von `BackgroundBlobs` ist ungetestet und unkritisch.
- scoreColors.test.ts: nicht betroffen (Scoring-Util, kein CSS).
- Kontrast (REQ-05): `--color-text-secondary:#9d968a` auf der Kachel `rgba(18..32,...)` über `#0b0b0b` liegt bei rund 6:1, die Vorlage nutzt exakt dieselben Werte; kein Kontrastverlust. Statusfarben (`#43b581`, `#E8A33D`, `#FF5A3C`) bleiben unverändert und liegen weiterhin auf schwarzer Fläche. Der frühere Kontrastgewinn kam aus der `--color-card`-Aufhellung, die bereits im Dashboard steht und nicht angefasst wird.
- Token-Stellen ohne Wirkung: ddc-design-tokens.css und tailwind.config.* sind tot bzw. leer; dort investierte Änderungen hätten keinen Effekt. Nur `@theme` in index.css zählt.
- Harte Farbwerte, die nicht über Tokens laufen: die Gold-Radials in Body und `.internal-home-vibe` tragen `rgba(197,160,89,...)` und `rgba(241,210,153,...)` direkt im CSS; sie werden hier explizit entfernt, nicht über Token gesteuert. Ausserhalb der Shell (pages/) liegen weitere harte Flächen, die laut Contract (Verbotener Bereich pages/) nicht angefasst werden und daher warm bleiben könnten; Sichtprüfung REQ-06 muss zeigen, ob eine Seite noch aus der Reihe fällt.

## Offene Fragen

- Ob das schwache Körnungs-Overlay (`body::after`) und ein neutrales Raster in `.internal-home-vibe::after` erhalten bleiben oder entfallen sollen, entscheidet die Screenshot-Serie gegen `vorlage-streamer.png` (REQ-06).
