# Briefing: Review Runde 1, Pakete A und B gemeinsam

[Orchestrator] Du bist Reviewer, nicht Autor. Du änderst keinen Code. Ergebnis ist eine vollständige Mängelliste in `/home/nathanael/.worktrees/tb-werbemanager-a/.tasks/2026-09-18-werbemanager-budget-smart/REVIEW.md` (nur diese Datei schreibst du, nicht committen).

## Gegenstand

- Paket A: Worktree /home/nathanael/.worktrees/tb-werbemanager-a, Branch feat/werbemanager-budget-backend, Diff `git diff origin/main...HEAD`
- Paket B: Worktree /home/nathanael/.worktrees/tb-werbemanager-b, Branch feat/werbemanager-budget-dashboard, Diff `git diff origin/main...HEAD`
- Maßstab: AUFTRAG.md, der Schnittstellenvertrag im selben Ordner, NACHTRAG-1.md bis NACHTRAG-3.md im Akten-Ordner. Nachtrag 3 streicht `plan.suggestion` aus Nachtrag 2. Pakete C und D sind nicht Gegenstand.

Beide Branches zusammen prüfen: Schreib- und Lesepfad gehören zusammen, Feldnamen und Grundcodes müssen auf beiden Seiten deckungsgleich mit dem API-Vertrag sein.

## Prüfpunkte

1. Abnahme gegen den Nutzer-Wunsch: fertig ja oder nein, Abweichungen. Kern: Bot handelt auch ohne Twitch-Plan (eigene 30-Sekunden-Blöcke im Stundenbudget), mit Twitch-Plan bewegt er nur (vorziehen, pausieren); Fenster sind Queue, Menü, erste Match-Minute; nach Matchende 1 Minute warten und nur bei ruhigem Chat; Sperren Match ab Minute 1, Raid 10 Minuten, Erstchatter 5 Minuten, Startschutz; "Nur überwachen" ist weg; Verlauf mit Grund; Schalter nicht zu übersehen.
2. Wirkung statt Erfolgsmeldung: kann der Entscheider real je eine Werbung auslösen (Helix-Sperrzeit aus der Antwort, Mindestabstand, Idempotenz, Lease), kann er doppelt auslösen (zwei Ticks, Neustart, Twitch-Werbung und eigener Block kurz nacheinander), überschreitet er das Budget, startet er je selbst im Match. Verlauf schreibt nur beim Wechsel, nicht je Tick. Gates fail-closed bei DB-Fehlern.
3. Sicherheit: Identität nur aus der Session, keine leere Twitch-User-ID mehr schreibbar, History-Endpunkt gibt keine fremden Kanäle heraus, `monitor` wird abgelehnt.
4. Migration: läuft auf Prod-Bestand (eine Zeile mit leerer ID, eine mit `smart`), ändert keine bereits angewandte Migration, Rechte-Hinweis für `twitchbot` und `twitchdash`, Schema-Snapshot passt.
5. Frontend: ehrlicher Leerzustand ohne `history` oder `plan`, unbekannte Grundcodes neutral, keine internen Wörter (Snooze, Preflight, Reason-Codes), echte Umlaute, keine Em-Dashes, Look nach Dashboard-Regeln, Strategie-Karten optisch unverändert.
6. Keine Code-Kommentare neu, keine toten Reste von `monitor`, keine harten Timeout- oder Modell-Konstanten, kein LLM im Pfad.

## Format von REVIEW.md

Je Mangel: Nummer, Schwere (BLOCKER, MANGEL, NIT), Paket, `pfad:zeile`, was falsch ist, was stattdessen gelten muss. Oben ein Urteil: FREIGABE oder FIX NÖTIG, dazu die Abnahme-Antwort fertig J/N. Keine Unter-Agenten. Danach hier im Thread kurz melden und stoppen.
