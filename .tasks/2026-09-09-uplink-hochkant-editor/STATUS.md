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

## Fix-Runde nach Autor-Gate und Review (neuer Commit)

- B1: `gerade` rundet auf gerade ab, Groessen mindestens 2, Positionen per `inFlaeche` in die Flaeche geklemmt (erst Position zurueck, dann Groesse um 2 kleiner). `hochkantPruefen` prueft zusaetzlich den Pixelueberlauf und die Bandhoehe. Brute-Force-Test ueber 500 geseedete, gepruefte Layouts.
- G1: ohne gespeicherten Stand ist Speichern mit dem Anfangswert erlaubt (Basis ist `null`, nicht der Anfangswert).
- G2: Basis wandert nach dem Speichern mit; `naechsterEntwurf` traegt jetzt die Basis (`HochkantLayout | null`) und den Anfangswert und ist ueber den Ablauf bearbeiten, speichern, Bestaetigung, zuruecksetzen getestet.
- S1: der zuletzt bearbeitete Kamerarahmen bleibt beim Aus- und Anschalten erhalten (Merker im State, `modusWechseln` nimmt einen `HochkantVorrat`).
- S2: der Editor bietet nur "Kamera unten"; `oben` wird von `hochkantPruefen` mit Klartext abgelehnt.
- N4: `nur_gameplay` mit gesetzten Kameradaten ist ein Fehler.
- Reine Helfer `ausrichten` und `modusWechseln` liegen jetzt in `hochkantLayout.ts` und sind getestet.
- Gate 4: Tests importieren `HochkantLayout`/`HochkantStand` aus `hochkantLayout` und `UplinkHochkantEditorProps` aus der Komponente; `tsc --noEmit --ignoreConfig ... tests/*` laeuft sauber.
