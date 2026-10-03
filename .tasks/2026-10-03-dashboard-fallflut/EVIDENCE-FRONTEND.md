# Prüfung der Nutzeroberfläche

Die vollständige Liste samt Karten, Transkriptdetails und Moderationsknöpfen wurde aus `ScamGuardSection.tsx` entfernt. Der Nutzerbrowser fordert die Queue nicht mehr an. Ausschließlich dafür verwendete API-Helfer und Typen wurden ebenfalls entfernt. Die Schutz-Einstellungen bleiben erhalten, und ihr Erklärungstext verspricht keine Fallbearbeitung mehr.

`npm run build` war im isolierten Worktree erfolgreich: TypeScript-Projektprüfung und Vite-Produktionsbundle. Der bestehende Verwaltungstab-Test war vollständig grün. Im zusätzlich ausgeführten DashboardShell-Test waren 17 von insgesamt 18 kombinierten Tests grün; die unveränderte Baseline hat bereits die beanstandete Klasse `max-w-4xl` in `App.tsx` Zeile 372.

Die lokale Browserprüfung verwendete Chromium mit Playwright sowie die bestehenden synthetischen Auth- und Home-Fixtures aus `adminMode.browser.test.mjs`. `/twitch/verwaltung#bot` wurde geöffnet. Der Scam-Schutz-Schalter, die Modi und beide Schwellen waren sichtbar; „Gemeldete Fälle“ fehlte, und kein Request ging an `/scam-guard/queue`. Der Screenshot wurde gelesen: `/home/nathanael/.claude/sichtpruefung/dashboard-fallflut-20261003.png`. Andere, nicht gemockte Abschnitte zeigten erwartete 404; der betroffene Scam-Abschnitt wurde erfolgreich geladen.

Das Einzelreview des Frontend-Commits erteilte ALLOW. Das erste gemeinsame Zwischenreview beanstandete die Entfernung der Fallverwaltung als Funktionsverlust. Diese Entfernung ist die ausdrückliche Nutzeranforderung aus `CONTRACT.md`; sie wird im finalen gemeinsamen Review erneut gegen diesen Auftrag geprüft. Es erfolgte kein Merge oder Deploy aufgrund des Zwischenreviews.
