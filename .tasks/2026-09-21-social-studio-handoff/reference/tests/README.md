# Prüfungen

`npm test`: 18 Logik-, Marken- und Strukturtests.

`python tests/browser_check.py`: Browserprüfung der gebauten
`preview.html`, mit Python-Playwright und `/usr/bin/chromium`.
Der Browserlauf enthält keine Produkt-API-Aufrufe.

`node tests/check-react.cjs`: Syntaxprüfung der React-Dateien.
Benötigt TypeScript; alternativ den Paketpfad in `TYPESCRIPT_PATH` setzen.
Es handelt sich nicht um einen vollständigen React-Integrationstest.

`browser-results.json` und `syntax-results.json` enthalten den ausgeführten Stand.
`BRAND.md` dokumentiert die Font-Fallbacks und die Logo-Prüfsumme.
