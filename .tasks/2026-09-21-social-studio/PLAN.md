# Plan: Social Studio Industrial Gold

## Schritte

1. Recon: Git-Zustand, Übergabepaket, Skills, Produktcode, Deploy-Architektur.
2. Review der vorhandenen Integrationsarbeit gegen die Anforderungen (Status-Semantik, Kennzahlen, Teilfehler, Kanalwechsel, Konten).
3. Korrekturen: Font-Stacks in `studio.css`.
4. Tests: fokussierte Suite, Produktbuild, vollständige Suite gegen Baseline.
5. Browserprüfung am gebauten Produkt gegen die echte API: Desktop, 390, 320, Tastatur, Dialoge, Fonts, Logo, Konsolenfehler.
6. Merge-Schleuse: Commit, Push, Gate, `HEAD:main`.
7. Deploy über `deploy-twitch-release` mit Release-Checkout, Live-Nachweis auf der Zielroute.
8. Cleanup: eigene Branches und Worktrees, Abschlussbericht.

## Vertragspunkte

- Vier Bereiche über `SOCIAL_MEDIA_TABS`, Route `social` in `DashboardShell` scoped, Seitenleiste wiederverwendet.
- Kennzahlen aus vollständigem Bestand (`loadQueueSnapshot`), Pagination clientseitig über den gesamten Bestand (24 pro Seite).
- Speichern des Zeitplans über `PostingPlanDraft`: geänderte Felder einzeln, Vollautomatik zuletzt, kanonischer Plan aus Serverantwort, Entwurf bei Teilfehler erhalten.
- Keine Freigabe aus terminalen oder veröffentlichenden Zuständen (`queueStage`).
