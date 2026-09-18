# Pakete: werbemanager-budget-smart

Schnitt disjunkt nach Dateien. Gemeinsamer Review beider Branches vor dem Merge, weil Schreib- und Lesepfad zusammengehören.

| Paket | Inhalt | Dateien | Worktree | Branch |
|---|---|---|---|---|
| A | Backend: Budget-Planer, Entscheider, Signale, Verlauf, API, Migration, Doku | alles unter `rust/`, `docs/streamer/WERBEMANAGER.md` | `~/.worktrees/tb-werbemanager-a` | `feat/werbemanager-budget-backend` |
| B | Dashboard: Status-Karte, zwei Strategien, Budget, Verlauf, Feineinstellungen | `bot/dashboard_v2/src/components/verwaltung/AdManagerSection.tsx`, `bot/dashboard_v2/src/api/adManager.ts`, neue Dateien unter `bot/dashboard_v2/src/components/verwaltung/` | `~/.worktrees/tb-werbemanager-b` | `feat/werbemanager-budget-dashboard` |

Schnittstelle: `API-VERTRAG.md`. Beide Worktrees starten von `origin/main`.
