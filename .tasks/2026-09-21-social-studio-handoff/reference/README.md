# Social Studio: Industrial Gold

Die übernommene Struktur bleibt erhalten: Pipeline, Auto-Pilot & Zeitplan,
Templates & Layouts sowie Konten & Einstellungen. Diese Revision betrifft
Farben, Materialwirkung, Typografie und Community-Logo.

## Vorschau

`preview.html` lässt sich lokal im Browser öffnen. Die Aktionen ändern
Beispieldaten im Arbeitsspeicher. Es werden keine Clips hochgeladen oder
veröffentlicht und keine Konten verbunden.

Die Logo-Grafik, Oberflächenstile und das JavaScript sind in der Vorschau
eingebettet. Es gibt keine Netzwerkanfragen.

**Schriften:** Die eigenständige HTML-Vorschau verwendet den System-Fallback.
Die Produkt-Styles referenzieren Manrope und Sora vom vorhandenen
Community-Host. Auf der Community-Domain werden diese Schriften gleichnamig
verwendet. Es sind keine Fontdateien oder eingebetteten Fonts im Paket.

## Anpassungen

- Dashboard-Tokens: `#0d0d0d`, `#121212`, `#161616`, Antikgold `#C5A059`,
  Messing `#D6B676`, Text `#f2eee6` und `#9d968a`.
- Goldene Primäraktionen mit dunkler Schrift, goldene aktive Navigation,
  dezente Materialkanten. Statusfarben bleiben vom Navigationsakzent getrennt.
- Vorhandenes Deadlock-D-Logo statt des generischen App-Symbols, auch mobil.

Die Farben stammen aus `bot/dashboard_v2/src/index.css`, die Materialwirkung
aus `bot/shared-theme/industrial-gold.css` und die Laufweiten aus
`bot/shared-theme/typography.css` des Twitch-Bot-Repositories.
Herkunft und Prüfsummen stehen in `BRAND.md`.

## Dateien

| Datei | Zweck |
| --- | --- |
| `preview.html` | Interaktive, selbstständige Gestaltungsvorschau |
| `src/brand-theme.css` | Semantische Tailwind-Tokens |
| `src/styles.css` | Komponentenmaterialien, Typografie und Formularstile |
| `src/react/DashboardShell.jsx` | Globale Navigation und vier Bereichs-Tabs |
| `src/react/CommunityBrand.jsx` | Wiederverwendbare Community-Marke |
| `src/react/ClipQueueCard.jsx` | Clip-Karte mit Freigabe und Aktionsmenü |
| `src/react/AutoPilotSchedule.jsx` | Zeitplanformular |
| `src/react/WorkspaceDialog.jsx` | Fokussierter Editor-Dialog |
| `INTEGRATION.md` | Bestehender Vertrag für die Produktanbindung |

## Build und Tests

```bash
npm install
npm run build
npm test
npm run check
```

Die Browserprüfung der HTML-Vorschau benötigt Playwright und Chromium.
Sie liegt unter `tests/browser_check.py`; Details stehen in `tests/README.md`.
Sie prüft auch die Farben des gebauten Artefakts, das geladene Logo sowie
die vier Bereiche bei 320 und 390 Pixel Breite.

Die React-Dateien werden mit `tests/check-react.cjs` syntaktisch geprüft.
Das ersetzt keinen React-Integrationstest im Produktprojekt.

## In das Produkt einbinden

Die vorhandene App definiert die Markenfarben bereits. Dort diese Tokens
wiederverwenden, statt einen zweiten globalen Farbsatz einzuführen.
In einer eigenständigen Tailwind-v4-App:

```css
@import "tailwindcss";
@import "./src/brand-theme.css";
@import "./src/styles.css";
```

Die vorliegenden API-Adapter und die bestehenden Editoren bleiben die
Integrationspunkte. Diese Revision ändert deren Verträge nicht.

## Stand

Dies ist eine überarbeitete Gestaltungsvorschau mit Komponentenquellen.
Das Produkt-Repository und die Live-Seite wurden in dieser Revision nicht verändert.
