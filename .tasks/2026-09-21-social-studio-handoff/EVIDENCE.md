# Nachweis der Übergabe

## Umfang

Anlass ist die Bitte des Nutzers, Entwurf und Integrationsauftrag über einen PR oder Branch an einen anderen Agenten zu übergeben, weil Datei-Uploads gerade nicht möglich sind.

Dieser Branch enthält die Task-Akte und die Referenzdateien unter `.tasks/2026-09-21-social-studio-handoff/`. Die Produkt-App, deren API-Verträge, Konten, Freigaben und Produktionsdienste wurden durch diese Übergabe nicht verändert. Der Draft-PR ist keine abgeschlossene Produktintegration.

## Herkunft

Originales Chat-Paket: `social-studio-industrial-gold.zip`.

SHA-256 des ursprünglichen ZIP:

```text
d57b0f9d5c514213e903d8a8af6d627bf882e4d75eab659feb43b7ef76001973
```

28 Quelldateien inklusive Logo, Komponenten, Styles, Dokumentation und ursprünglichen Tests wurden aus dem Paket übernommen. Das komprimierte Übertragungspaket wurde vor dem Entpacken auf dem Projekthost gegen SHA-256 geprüft:

```text
30bcd53938c2bf02f3020c433c01dc292b5754eeb3166fa0f3ab45753320e4a0
```

Einzelprüfsummen stehen in `reference-manifest.json`. Es wurden keine Fontdateien übernommen. Die ursprünglichen PNG-Screenshots wurden nicht binär übertragen; die acht Bilder unter `checks/screenshots/` wurden aus der Referenz auf dem Projekthost neu gerendert.

## Aktuell ausgeführte Prüfungen

| Prüfung | Ergebnis |
| --- | --- |
| Referenzabhängigkeiten | Installation mit `--ignore-scripts`, Tailwind 4.1.10 per Lockfile festgehalten |
| `reference/npm run build` | HTML-Vorschau und Tailwind-Stylesheet gebaut |
| `reference/npm test` | 18 Tests bestanden, 0 fehlgeschlagen |
| `reference/npm run check` | JavaScript-Syntaxprüfung bestanden |
| Ursprünglicher JSX-Syntaxprüfer mit vorhandenem Dashboard-TypeScript | 7 JSX-Dateien geparst und transpiliert |
| `checks/npm run check` | 28 importierte Dateien unverändert; 2 gebaute Artefakte bytegleich mit dem Chat-Paket |
| Begrenzte Veröffentlichungsprüfung im selben Quellenlauf | Keine Font-/Umgebungs-/Schlüsseldateien und keine Treffer der geprüften Credential-Muster |
| `checks/npm test` | 13 Browser-Prüfgruppen bestanden |
| Browser-Netzwerk und JavaScript | 0 Netzwerkrequests, 0 JavaScript-Fehler |
| Responsive Prüfung | Vier Bereiche bei 320 und 390 Pixel Breite ohne Dokumentüberlauf |
| Screenshots | Acht gesettelte Viewport-Aufnahmen, keine Live-Bilder |

Die Browserprüfung deckt in der Demo Freigaben, Suche, Leerzustände, Ansichtswechsel, Dialogfokus, Escape, Rückkehr zum Auslöser, fehlgeschlagene Freigabe, Formularvalidierung, erfolgreiche und gescheiterte Speicherung, erhaltene Entwürfe, Tastatur-Tabs und schematische Layout-Einstellungen ab.

Der erste neue Browserlauf versuchte, den auf Mobilgeräten ausgeblendeten Ansichtswechsler zu bedienen. Der Prüfablauf wurde korrigiert: Die Listen-/Kartenumschaltung wird auf Desktop geprüft, auf Mobilgeräten werden Kopfansicht und Clip-Bereich als Viewport-Serie aufgenommen. Anschließend bestand der vollständige Browserlauf. An den importierten Referenzquellen war dafür keine Änderung erforderlich.

## Bytegleiche gebaute Artefakte

```text
preview.html
54f480a73604723afa4b00f4ffceee6884adff27a7c30464cea46e347d3acf25

dist/tailwind.css
e353aab485b855136e566827d324a3b35ef4368b82a558796453fea0d34da007
```

Die aktuellen maschinenlesbaren Ergebnisse stehen in `checks/source-results.json` und `checks/browser-results.json`. Die ursprünglichen Ergebnisdateien im Referenzordner werden nicht als Produktnachweis ausgegeben.

## Nicht nachgewiesen und weiterhin offen

Die React-Komponenten sind noch nicht in das Produkt eingebunden. Es gab in dieser Übergabe keine Tests gegen produktive OAuth-Flows, echten Upload, Scheduler oder Medienrenderer. Die Offline-Referenz verwendet eine Ersatzschrift; das tatsächliche Laden der Produktfonts bleibt Teil der Integration. Der Credential-Mustercheck ersetzt keinen umfassenden Sicherheitsreview.

Kein Merge auf `main`, kein Produktdeploy und kein Dienstneustart. Die Folgearbeit einschließlich Gates und Live-Nachweis ist in `AUFTRAG.md` beschrieben. Der Übergabe-Branch und sein Arbeitsbaum bleiben für den übernehmenden Agenten erhalten.

TEXTNACHWEIS[DR-1]: Gedankenstriche 0 | ae/oe/ue/ss-Ersatz 0 | Absolutwörter 0 als pauschale Erfolgszusicherung | Senke: neue Übergabe-Dokumente; unverändert importierte Referenz ausgenommen
