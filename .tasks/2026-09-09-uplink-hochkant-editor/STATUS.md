# Status: Uplink-Hochkant-Editor

datum: 2026-09-09
zustand: umgesetzt, lokal auf feat/uplink-hochkant-editor, nicht gepusht

## Geliefert

- `bot/dashboard_v2/src/components/uplink/hochkantLayout.ts`: Typen und reine Funktionen (hochkantAnfang, hochkantPruefen, seitenverhaeltnisSperren, rahmenBegrenzen, rahmenZiehen, zielverhaeltnisse, zielPixel, vorschauAusschnitt, hochkantGleich, naechsterEntwurf).
- `bot/dashboard_v2/src/components/uplink/UplinkHochkantEditor.tsx`: Komponente mit zwei Vorschauen, drei Modi, Kamera-Schalter, Bandregler, Standanzeige, Pixelanzeige, Tastaturbedienung.
- `bot/dashboard_v2/tests/uplinkHochkantLayout.test.ts`: 14 Tests der reinen Funktionen.
- `bot/dashboard_v2/tests/uplinkHochkant.test.tsx`: 8 Tests der Komponente ueber renderToStaticMarkup.

## Nachweise

- `npm test`: 278 Tests, alle gruen.
- `npx tsc -p tsconfig.app.json --noEmit`: sauber.
- `npm run build`: erfolgreich.
- Sabotage-Gegenprobe: Klemmung aus rahmenBegrenzen entfernt macht "rahmenBegrenzen haelt an allen vier Kanten" rot; geaenderter Leertext macht "ohne Stand zeigt der Editor den Anfangswert und den Leertext" rot. Beide nach Ruecksetzen wieder gruen.

## Abweichungen vom Vertrag

- `hochkantPruefen` traegt wie im Prompt-Auftrag die Quelle als dritten Parameter (`hochkantPruefen(layout, ziel, quelle)`); der Editor-Vertrag nennt nur `(layout, ziel)`, die Seitenverhaeltnispruefung braucht die Quellpixel.
- `vorschauAusschnitt` gibt Pixelwerte relativ zur uebergebenen `zielflaeche` zurueck; die Komponente misst jede Vorschauflaeche per ResizeObserver.
