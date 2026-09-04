# Plan: Partner-Übersicht nach Deadlock-Live und Impact

status: aktiv
datum: 2026-09-04
klasse: mittel
contract: CONTRACT.md
evidence: EVIDENCE.md

Branch: `feat/partnernetz-sortierung` von origin/main (1540aebb), Worktree
`~/.worktrees/tb-partnernetz-sortierung`. Tests: `cd website && node --test
tests/*.test.mjs` (Baseline 30 grün). Build: `npm run build`.

## M1: Regressionstest rot

`website/tests/partnerNetzSortierung.test.mjs` (neu): reine Funktionen aus
`website/src/lib/partnerNetwork.ts` per Quelltext-Prüfung und, wo möglich,
durch Ausführen einer nach `/tmp` transpilierten Kopie (oder direkte
Typescript-freie Logik in einer `.mjs`-Hilfsdatei unter `website/src/lib/`,
die die TSX-Datei importiert). Mindestens:
- `istDeadlock({game:"Deadlock"})` true, `"deadlock"` true, `"WARDOGS"` false, undefined false.
- `gliedere(liste)` liefert `{embeds, weitereDeadlock, allePartner}`: embeds max 3 Deadlock-live nach Zuschauern, weitereDeadlock Rest der Deadlock-live, allePartner alles andere nach Impact.
- `impactScore` 50/50 gegen Maxima; bei max 0 kein NaN.
- PartnerNetwork.tsx enthält Kopfzeilen "weitere streamen gerade Deadlock" und "Alle" plus einen Klapp-Knopf (`aria-expanded`).
Erwartung: rot, Namen und Meldungen unten eintragen.

## M2: Logik in lib/partnerNetwork.ts

`istDeadlock`, `impactScore`, `gliederePartner`; Hook-Sortierung auf die neue
Gliederung stützen oder unverändert lassen (Gliederung sortiert selbst).
Validierung: tsc grün, M1-Logiktests grün.

## M3: PartnerNetwork.tsx umbauen

- Embeds nur `embeds` (max 3, Deadlock-live).
- `Ausklappliste`-Baustein (Kopfzeile mit Zähler, Knopf mit `aria-expanded`,
  Chevron, Standard eingeklappt, Raster 4/2/1 Spalten, Zeilen wie `OfflineTile`
  plus LIVE-Punkt und Zuschauer bzw. Kennzahlen).
- Leerzustand "Gerade streamt kein Partner Deadlock." bei 0 Embeds.
- `LivePreview` entfernen, wenn kein Nutzer mehr.
Validierung: Tests 30 + neue grün, tsc, Build.

## M4: Abschluss

Commit(s) auf dem Branch, push, Status hier nachtragen. Merge, Deploy
(rsync dist) und Branch-Löschung macht die Hauptsession.

## Roter Lauf (M1)

`node --test tests/partnerNetzSortierung.test.mjs` am 2026-09-04 rot: die
gesamte Datei bricht beim Import ab.

- Test: `tests/partnerNetzSortierung.test.mjs`
- Meldung: `SyntaxError: The requested module '../src/lib/partnerNetwork.ts' does not provide an export named 'gliederePartner'`
- Ergebnis: tests 1, pass 0, fail 1

## Status

- M1: fertig (roter Lauf oben, Test rot vor Code)
- M2: offen
- M3: offen
- M4: offen

## Test-Ansatz

Node 22.23 strippt TS-Typen ohne Flag, `node --test tests/*.test.mjs` importiert
`../src/lib/partnerNetwork.ts` direkt. Reine Funktionen dort nur mit
`import type`, damit kein Runtime-Import auf den Alias `@/` laufen muss.
