status: aktiv
Datum: 2026-09-28

# Arbeitszweige außerhalb der offenen PRs

84 registrierte Twitch-Worktrees zu Beginn. 30 davon hatten Änderungen an getrackten Dateien und sind ohne Sicherung nicht löschbar. Der geteilte `main`-Checkout hat fremde ungetrackte Dateien.

| Zweig | Patch-Stand gegenüber `origin/main` | Zustand |
|---|---:|---|
| `fix/partner-profile-official-logo` | 2 einzigartige Patches | sauber, fachlich prüfen |
| `codex/fix-code-scanning-20260924-clean` | 6 | sauber, mit geschlossenem Security-PR abgleichen |
| `feat/streamer-voice-join` | 1 | sauber, Voice-Flow prüfen |
| `feat/titel-studio-costream` | 9 | sauber, letzter Commit ausdrücklich WIP |
| `feat/twitch-ddc-brand-20260921` | 1 | sauber, Textänderung prüfen |
| `feat/twitch-toml-admin-live-20260920` | 1 | sauber, Betriebszugriff prüfen |
| `feat/twitch-toml-dashboard-rollout-20260920` | 2 | 40 getrackte Änderungen, geschützt |
| `feat/category-admin-api-20260918` | 2 | sauber, Collector-Abhängigkeit prüfen |
| `feature/category-collector-20260918` | 1 | sauber, überlappende Kategorie-Zweige prüfen |
| `feat/category-collector-retention-20260918` | 1 | sauber, überlappende Kategorie-Zweige prüfen |
| `fix/category-final-release-20260918` | 2 | sauber, überlappende Kategorie-Zweige prüfen |
| `fix/code-review-4edaf32` | 1 | sauber, Review-Fixes prüfen |

Weitere Remote-Zweige gehören zu geschlossenen PRs, Sicherungen oder alten Integrationsversuchen. Weder `git cherry` noch ein gleicher Titel beweisen allein, dass ein Worktree entbehrlich ist. Vor jedem Aufräumen: Diff, offene Dateien, PR-Entscheidung, Branch-Ancestry und Live-Stand prüfen.
