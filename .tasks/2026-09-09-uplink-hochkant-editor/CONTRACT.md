# Contract: Uplink-Hochkant-Editor als Dashboard-Komponente

status: aktiv
datum: 2026-09-09
klasse: mittel
repo: Deadlock-Twitch-Bot

Dieser Contract ist der Maßstab für Implementierung und Merge-Kritiker. Nach dem
Anlegen ist er unveränderlich: der Hook lässt nur noch die `status:`-Zeile und
Anhänge unter `## Amendments` zu. Wer ein REQ oder INV ändern will, schreibt ein
Amendment mit Begründung; Produkt-, API- oder Datenänderungen entscheidet der User.

Vertrag: Uplink-Repo `docs/hochkant-vertrag.md` (Worktree `/home/nathanael/.worktrees/uplink-hochkant`).

## Ziel

Ein Streamer legt im Uplink-Dashboard fest, welcher Ausschnitt seines Querformat-Streams als Gameplay und welcher als Kamera in das Hochkantbild kommt, wählt gestapelt oder Bild-im-Bild, sieht eine Vorschau und speichert das als neue Revision.

## Anforderungen (user-sichtbares Verhalten)

- REQ-01: Ohne gespeichertes Layout startet der Editor mit dem Anfangswert aus dem Vertrag (Bild-im-Bild, Gameplay mittig, Kamera rechts oben); mit `stand.layout` zeigt er dieses.
- REQ-02: Drei Modi (`nur_gameplay`, `gestapelt`, `bild_im_bild`) und ein Schalter Kamera an/aus; Kamera aus erzwingt `nur_gameplay`.
- REQ-03: Rahmen lassen sich per Zeiger verschieben und an Ecken vergrößern; sie bleiben in der Fläche, halten die Mindestkante 0,05 und das gesperrte Seitenverhältnis nach Vertrag.
- REQ-04: Zwei Vorschauen nebeneinander: Quelle mit Rahmen (Standbild oder Raster) und Zielbild, das den späteren Schnitt aus denselben Rahmen nachrechnet.
- REQ-05: "Speichern" liefert ein gültiges `HochkantLayout` an `onSpeichern`; bei Verstoß gegen die Regeln ist Speichern gesperrt und der Grund steht sichtbar daneben. "Zurücksetzen" ruft `onZuruecksetzen`. Beide Knöpfe sind bei `beschaeftigt` gesperrt.
- REQ-06: Revision und aktive Revision werden angezeigt ("Gespeichert als Stand 3, im laufenden Stream aktiv: Stand 2"); der Hinweis, dass ein neuer Stand erst beim nächsten Stream gilt, ist sichtbar.
- REQ-07: Fehlt `quelle`, steht sichtbar, dass 1920×1080 angenommen wird; `standbildHinweis` wird unter der Vorschau gezeigt.

## Invarianten (darf sich nicht ändern)

- INV-01: Kein Fetch, kein OAuth, keine Tokenverwaltung; Werte und Callbacks nur über Props.
- INV-02: Keine Änderung an `components/socialmedia/**`, `utils/socialMediaLayout.ts`, `pages/**`, `api/**`, Rust; `package.json` nur um die neue Testdatei.
- INV-03: Bestehende Tests bleiben unverändert und grün.
- INV-04: Echte Umlaute, keine Gedankenstriche, keine Code-Kommentare, bestehende Tailwind-Klassen der Uplink-Karten.
- INV-05: Alle Geometrie in normierten Koordinaten `0..1`; Pixel nur in der Umrechnungsfunktion `zielPixel`.

## Nicht-Ziele

- Speicherroute, Bot-Routen, Sessionstart-Übernahme, Standbildpfad (Codex).
- Layoutwechsel während eines laufenden Streams.
- Änderung des Social-Media-Editors.

## Erlaubter Änderungsbereich

- bot/dashboard_v2/src/components/uplink
- bot/dashboard_v2/tests/uplinkHochkant.test.tsx
- bot/dashboard_v2/tests/uplinkHochkantLayout.test.ts
- bot/dashboard_v2/package.json
- .tasks/2026-09-09-uplink-hochkant-editor

## Verbotene Änderungen

- bot/dashboard_v2/src/components/socialmedia/**, bot/dashboard_v2/src/utils/**, bot/dashboard_v2/src/pages/**, bot/dashboard_v2/src/api/**, rust/**, website/**, bestehende Tests

## Offene Produktfragen

- keine (Layoutwechsel im laufenden Stream ist ausdrücklich Nicht-Ziel)

## Amendments
