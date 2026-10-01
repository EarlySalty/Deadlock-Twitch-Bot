# Auftrag: Rangliste & Erfolge fällt komplett aus (503 not_current)

Intent: Hauptsession (Claude, Documents), Nutzer-Meldung 2026-10-01 22:40.
Du bist der einzige Thread für dieses Paket. Keine Unter-Threads oder Unter-Agenten spawnen.

## Symptom

`/twitch/challenges` zeigt "Rangliste und Erfolge konnten nicht geladen werden".
`GET /twitch/api/v2/challenges/me` und `/challenges/viewers` antworten 503,
Log `tb_dashboard_api::handlers::challenges: Challenge-Abfrage fehlgeschlagen error=source unavailable: not_current`.

## Belegte Ursachenkette

1. `tb-category-collector.service` (System-Unit) ist am 2026-10-01 06:15 UTC gestorben.
   Beim Neustart scheiterte systemd mit `Failed to set up mount namespacing: Cannot allocate memory`
   bzw. `Failed to set up credentials: Protocol error` (Host ist ein OpenVZ-Container am
   physpages-Limit 48 GB, Page-Cache zählt mit). Nach 5 Versuchen griff `StartLimitBurst=5`
   in `StartLimitIntervalSec=300`, die Unit blieb 14 h `failed`. Der Notify-Dienst
   `tb-category-collector-failure` scheiterte aus demselben Grund. Niemand hat es gemerkt.
   Die Hauptsession hat den Collector um 22:44 CEST per `reset-failed` + `start` wieder gestartet.
2. `tb-effort` `Engine::ensure_ready` (`rust/crates/tb-effort/src/lib.rs`) verlangt alle 7 Quellen
   in `partner_effort_source_state` gesund; `category_collection` und `engine` sind seit 06:16 UTC
   `healthy=false, error_code=source_unavailable`.
3. `category_collection_coverage` (`rust/crates/tb-effort/src/evidence.rs`) verlangt lückenlose
   Collector-Läufe (`bool_and(... gaps_ok)`) vom Programmstart bzw. 4 Wochen vor Wochenbeginn bis jetzt.
   Folge: eine einzige Collector-Lücke macht die gesamte Rangliste-und-Erfolge-Seite für bis zu
   rund 4 Wochen unbenutzbar, auch nachdem der Collector wieder läuft. Bitte live prüfen, ob die
   Seite nach dem Neustart wirklich wieder lädt; die Erwartung ist nein.

## Zu tun

A. Ausfallverhalten: Eine historische Datenlücke darf die Seite nicht als Ganzes abschalten.
   Fehlende Abdeckung betrifft nur Wertungen, die vom Lückenzeitraum abhängen. Löse das so,
   dass Rangliste und Erfolge weiter angezeigt werden und betroffene Kategorie-Wertungen ehrlich
   als unvollständig markiert oder für den Lückenzeitraum ausgesetzt werden, statt still falsch
   zu zählen. Fail-closed für Belohnungs- oder Geldwirkung (z. B. `tb-raid/src/monthly_raid_boost.rs`,
   der dieselbe 7-Quellen-Liste nutzt) bleibt erhalten; prüfe beide Leser zusammen.
   Das Frontend soll statt der Komplett-Fehlerkarte den Teilzustand zeigen (kurzer Nutzertext,
   echte Umlaute, keine Em-Dashes).
B. Selbstheilung: Der Collector darf nach einem Startfehler nicht dauerhaft liegen bleiben.
   Unit im Repo (Quelle der installierten `/etc/systemd/system/tb-category-collector.service`
   suchen) so anpassen, dass systemd es weiter versucht (z. B. `StartLimitIntervalSec=0` mit
   gestaffeltem `RestartSec`, oder ein Timer, der `failed` zurücksetzt). Prüfe, ob andere
   Twitch-Units dieselbe Falle haben (`deadlock-twitch-bot-watchdog.service` scheitert gerade
   ebenfalls mit 226/NAMESPACE) und nenne sie im Bericht.
C. Sichtbarkeit: Der Ausfall der Quelle muss den Betreiber einmal erreichen (entprellt, eine
   Meldung je Vorfall), auch wenn der OnFailure-Notify selbst nicht starten kann. Prüfe den
   bestehenden Watchdog-/Alarmpfad und nutze ihn, statt einen neuen zu bauen.

## Rahmen

- Eigener Worktree `~/.worktrees/tb-challenges-not-current`, Branch `fix/challenges-not-current-20261001`
  von frischem `origin/main`. Release nur im eigenen Worktree bauen.
- Keine Code-Kommentare. Rust only. `cargo fmt`, `clippy`, betroffene Tests (tb-effort, tb-raid,
  tb-dashboard-api) grün; tb-effort-Postgres-Tests laufen gegen den Docker-Test-Container.
- Neue Migration nur, wenn nötig; nie eine angewandte ändern. Prod-Migration von Hand als `postgres`.
- Vor Abgabe `gate_hook.py --review` gegen die eigene Arbeit. Dann Abschluss nach AGENTS.md:
  Merge-Gate, `git push origin HEAD:main`, Deploy über `deploy-twitch-release <sha>`, Unit-Änderung
  installieren + `daemon-reload`, Live-Beweis: `/twitch/api/v2/challenges/me` liefert 200 bzw. den
  Teilzustand, Collector-Unit zeigt die neue Restart-Policy. Branch und Worktree löschen.
- Blocker oder Bump-up als Nachricht an den Nutzer im eigenen Thread.
