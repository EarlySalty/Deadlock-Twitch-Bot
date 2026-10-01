# Markenzuordnung

## Geprüfte Quellen

Twitch-Bot-Repository:
`/home/nathanael/repos/Deadlock-Twitch-Bot`

| Bereich | Quelle |
| --- | --- |
| Farben und Schriften | `bot/dashboard_v2/src/index.css` |
| Materialkanten | `bot/shared-theme/industrial-gold.css` |
| Schriftlaufweiten | `bot/shared-theme/typography.css` |
| Verwendetes Logo | `bot/dashboard_v2/public/brand/deadlock-d-logo.png` |
| Verwendung des Logos auf der Streamer-Seite | `website/src/components/layout/Navbar.tsx` |

Der ursprüngliche generische Film-Button wird nicht mehr als Logo verwendet.
Das ältere `ddc-logo.svg` mit einem Blitz und einem Teal-Verlauf ist nicht
die Quelle dieser Revision.

## Logo

Die Quelle ist eine 192 × 192 Pixel große PNG-Datei. Für die Vorschau wurde
dieses Bild proportional auf 96 × 96 Pixel skaliert und als WebP gespeichert.
Das Motiv wurde nicht nachgezeichnet oder umgefärbt.

Original-PNG, SHA-256:

```text
9647e2a596cfa341a607f33e3f64071f0ef3515742156fcb0734948e367cc8c1
```

Display-WebP, SHA-256, gegen die Server-Konvertierung abgeglichen:

```text
d6ee8c4862e96df8789aec9cc7dd292a847a94fb7dfdf831958cbbfb7e550170
```

## Typografie

Produktintegration: Manrope für Bedienelemente, Sora für Überschriften.
Die KPI-Familie folgt dem vorhandenen Stack Outfit, Sora, Manrope; Outfit
wird nicht zusätzlich heruntergeladen, ohne vorhandenes Outfit greift Sora.

Die vorhandenen Endpunkte für Manrope und Sora wurden vom Projektserver
aus mit HTTP 200 und Content-Type `font/woff2` geprüft. Sie geben keinen
`Access-Control-Allow-Origin`-Header zurück. Daher kann eine lokal geöffnete
HTML-Datei diese Fonts nicht direkt laden.

Die selbstständige Vorschau enthält bewusst keine Fontdateien und entfernt
beim Build die Font-Ladeanweisungen. Die Font-Stacks und Produkt-Styles bleiben
erhalten. Die beiliegenden Screenshots zeigen den System-Fallback.

## Struktur

Die 216-Pixel-Seitenleiste, vier Bereichs-Tabs, Listen- und Kartenansicht,
196-Pixel-Listen-Thumbnails, Dialoggrößen und Formularabläufe wurden
beibehalten. Semantische Farbklassen ersetzen die Tailwind-Standardpaletten.
Das Community-Logo erscheint in der mobilen Kopfzeile, wenn die Seitenleiste
ausgeblendet ist.

TEXTNACHWEIS[DR-1]: Gedankenstriche 0 | ae/oe/ue/ss-Ersatz 0 | Absolutwörter 0 belegt | Senke: Gestaltungsvorschau, README.md, BRAND.md
