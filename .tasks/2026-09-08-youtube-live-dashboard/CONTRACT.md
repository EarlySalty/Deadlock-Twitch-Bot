# Contract: Dashboard-Komponente YouTube-Live für Uplink

status: aktiv
datum: 2026-09-08
klasse: mittel
repo: Deadlock-Twitch-Bot

Dieser Contract ist der Maßstab für Implementierung und Merge-Kritiker. Nach dem
Anlegen ist er unveränderlich: der Hook lässt nur noch die `status:`-Zeile und
Anhänge unter `## Amendments` zu. Wer ein REQ oder INV ändern will, schreibt ein
Amendment mit Begründung; Produkt-, API- oder Datenänderungen entscheidet der User.

Auftrag: `/home/nathanael/Documents/.tasks/2026-09-08-uplink-neubau/CLAUDE-YOUTUBE-LIVE.md`.
Schnittstellenvertrag: Uplink-Repo `docs/youtube-live-vertrag.md`, Abschnitt Dashboard-Komponente.

## Ziel

Ein Streamer sieht in der YouTube-Zielkarte eine Karte, in der er Titel, Sichtbarkeit und Auto-Start/Stop wählt, YouTube bewusst für Live freigibt, den Broadcast-Zustand sieht und ihn beendet.

## Anforderungen (user-sichtbares Verhalten)

- REQ-01: Ohne verbundenes YouTube-Konto zeigt die Komponente nur den Hinweis, dass zuerst verbunden werden muss, und kein Formular.
- REQ-02: Anfangsauswahl ohne gespeicherte Einstellungen ist Sichtbarkeit privat, Auto-Start aus, Auto-Stop aus, Freigabe aus.
- REQ-03: Ohne Freigabe steht sichtbar, dass Uplink bei OBS-Start keinen YouTube-Broadcast anlegt; die Freigabe ist ein eigenes Kontrollkästchen mit klarem Text.
- REQ-04: Der Zustand (`inaktiv`, `vorbereitet`, `sendet`, `live`, `beendet`, `blockiert`, `fehler`, `unklar`) wird in Nutzersprache angezeigt; `live` heißt ausdrücklich "auf YouTube live", `sendet` bestätigt keine Veröffentlichung.
- REQ-05: "Speichern" ruft `onSpeichern` mit dem Entwurf, "Beenden" ruft `onBeenden` und ist nur bei `vorbereitet`, `sendet`, `live` und `unklar` aktiv; während `beschaeftigt` sind beide Knöpfe gesperrt.
- REQ-06: Keine technischen Kennungen (Broadcast-ID, Run-ID) in der Hauptansicht; die Broadcast-ID erscheint nur in einer aufklappbaren Detailzeile.

## Invarianten (darf sich nicht ändern)

- INV-01: Kein eigener Fetch, kein OAuth, keine Tokenverwaltung in der Komponente; Werte und Callbacks kommen ausschließlich über Props.
- INV-02: Keine Änderung an `UplinkZiel.tsx`, `api/uplink.ts`, `Uplink.tsx`, Rust-Handlern, `package.json` außer dem Test-Skript-Eintrag.
- INV-03: Bestehende Tests bleiben unverändert und grün.
- INV-04: Echte Umlaute, keine Gedankenstriche, bestehende Tailwind-Klassen der Uplink-Karten (`rounded-xl border border-border bg-background/70`, `text-text-secondary`, `accent-primary`).

## Nicht-Ziele

- Anbindung an `UplinkZiel.tsx` und Bot-Routen (Codex).
- Übersetzungen über `i18n/dictionary.ts`.

## Erlaubter Änderungsbereich

- bot/dashboard_v2/src/components/uplink
- bot/dashboard_v2/tests/uplinkYouTubeLive.test.tsx
- bot/dashboard_v2/package.json
- .tasks/2026-09-08-youtube-live-dashboard

## Verbotene Änderungen

- bot/dashboard_v2/src/pages/**, bot/dashboard_v2/src/api/**, rust/**, website/**, bestehende Tests

## Offene Produktfragen

- keine

## Amendments
