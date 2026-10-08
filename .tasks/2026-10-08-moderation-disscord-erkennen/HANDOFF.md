# Übergabe M/1

Stand: 2026-10-08. Implementierung und lokale Abnahme abgeschlossen; Integration und Deployment stehen noch aus. Nicht neu beginnen.

## Stand und Ursache

Worktree: `/home/nathanael/.worktrees/tb-moderation-disscord-erkennen`, Branch `fix/moderation-disscord-erkennen`. Basis: `6937e4a61f43a9c08174fa95c96f49da149ca859`. Erster Fix: `1dbcb4f3a16e48c569a24fa8c53983ae75ce3316`. Freigegebener Gate-Fix: `41d8d547ab1917be4aa9b907b9b56abab558c89f`.

Die alten Discord-Muster übersehen das doppelte s. Der Screenshot erreicht nur generic_praise(1), keine Einzelentscheidung. Im Gesprächswächter war natürliches Deutsch zudem ein kategorischer Freibrief. Der neue Kombinationsbefund wertet Lob, Follow-/Wiederkommen-Versprechen und ungefragten persönlichen Kontakt gemeinsam. Die Schreibvariante bleibt auf Kontaktlabels begrenzt. Kontaktspannen werden aus Inhaltsprüfungen ausgeklammert; Namen sind für das Urteil unerheblich. Keine neue Architektur, kein Modellwechsel, keine öffentliche Route, keine DB-Handkorrektur und keine neuen Produktionskommentare.

## Abnahme und Grenzen

Screenshot-Wortlaut durch echtes Rust-Scoring und `ScamPitchDetector::observe`: unbekanntes Alter ergibt Hint, zehn Tage ergibt PublicWarn, 200 Tage ergibt Hint. Der echte Pipeline-Aufrufer bestätigt Warnungen und die bestehenden Ausnahmen. Erstwarnungen löschen nicht und lösen weder Timeout noch Ban aus. Harmlose Gegenproben umfassen Lob ohne Kontakt, erfragte Namen, Spiel-/Community-Bezug, normale Gespräche und gleiche Schreibweise. Unabhängige Namen, mehrere Kontaktlabels, Rollen, Vertrauen, Verlauf und abgeschalteter Pitch-Schalter sind geprüft. Der Gesprächswächter behält seine eigenen Rollen- und Einstellungsausnahmen sowie die bestehenden Alters- und Aktionsregeln.

- Kontaktabnahme am freigegebenen Fix: 7 passed, 0 failed, 0 ignored, Exit 0. `cargo-slot test -p tb-chat --jobs 3 contact_ -- --nocapture --include-ignored`, SQLX_OFFLINE=true, TB_TEST_REQUIRE_DB=1, Wegwerf-DB auf 33084. Der Fixer hat den tatsächlichen Pipeline-Replay mit 18 Fällen sowie Vertrauens- und Einstellungsausnahmen ausgeführt. Belege in REVIEW.md und `/tmp/tb-contact-bait-m1-replay.log`.
- Paket-Clippy `--no-deps --lib`: Fix und unveränderte Basis jeweils Exit 0, dieselbe einzelne Borrow-Warnung. Strict-Clippy: beide Exit 101 mit demselben vorhandenen result_unit_err in tb-raid. Format der eigenen Rust-Dateien: Exit 0. Logs: `/tmp/tb-contact-bait-m1-clippy-package.log`, `...-baseline-package-clippy.log`, `...-clippy.log`, `...-baseline-clippy.log`.
- Gesamtsuite vor dem endgültigen Gate-Fix: 911 passed, 43 failed, 2 ignored; unveränderte Basis: 906 passed, 42 failed, 2 ignored. Alle 42 Baseline-Fehler sind im Fixlauf enthalten. Die zusätzliche Abweichung ist PoolTimedOut an unverändertem `commands.rs:3144`. Genau dieser Test läuft isoliert sowohl auf Basis als auch auf dem freigegebenen Fix mit 1 passed, 0 failed, 0 ignored und Exit 0. Die Gesamtsuite ist weiterhin nicht grün. Logs: `/tmp/tb-contact-bait-m1-final-test.log`, `...-baseline-test.log`, `...-baseline-extra-pool.log`; der isolierte Fixlauf ist im Worker-Transcript aufgezeichnet.
- KI-Transport ausschließlich mit Wiremock geprüft. Dessen Urteil ist ein Stub, kein reales KI-Urteil. Keine tatsächliche Nachricht, Löschung, kein Timeout und kein Ban an echten Konten ausgelöst; keine Community-Daten als Funktionstest an externe Modelle gesendet.

TESTNACHWEIS[TW-1]: 7 passed, 0 ignored | Baseline: 42 rot

## Gate und Abschlussweg

Runde 1, Reviewer gpt-6.1-sol: BLOCK wegen Kontaktname als falschem Spielkontext. Vorgeschriebener frischer nativer Fixer hat den Befund und den Anfrage-Zwilling behoben. Runde 2 am Fix-SHA 41d8d547: ALLOW, derselbe Reviewer, Exit 0. Originalbefunde, Befundlösung, Befehle und Protokolle in REVIEW.md. Kein unabhängiger Review-Thread und kein Hook umgangen.

Nächster Schritt: eigene Prüfprotokolle committen, origin/main frisch holen und integrieren. Im eigenen sauberen Integrationsstand denselben Gate-Reviewer prüfen lassen. Danach main mit HEAD:main aktualisieren. Nur den aktuellen origin/main-SHA im eigenen Worktree bauen, sämtliche erforderlichen ELF-Herkunftsmarken prüfen, Artefakte in den eigenen unabhängigen Staging-Clone übernehmen und über deploy-twitch-release ausliefern. Der schmutzige geteilte Hauptcheckout bleibt unberührt. Vor Cleanup eigene Belege und wertvolle ignorierte Artefakte sichern, Branch nur nach geprüftem merge-base-Exit löschen. Thread erst nach vollständigem Abschluss selbst settlen.

## Ressourcen und Live-Ausgang

Eigene Testcontainer `tb-contact-bait-m1` und `tb-contact-bait-m1-baseline`, Wegwerfports 33084 und 33086. Eigener detached Baseline-Worktree `/home/nathanael/.worktrees/tb-contact-bait-m1-baseline`. Eigener unabhängiger Staging-Clone `/home/nathanael/repos/twitch-release-contact-bait-m1`; dortiger Main-Stand bei Erstellung `51c8a674a371d0e623687940e6b8ca3492f96c92`. Compilerprüfungen ausschließlich über cargo-slot mit drei Jobs. Ein Lauf mit --features testing war mangels dieses Features vor Teststart gescheitert; erfolgreiche Läufe nutzen die vorhandenen Tests, keine Suite wurde abgeschwächt.

Live-Ausgang mit deploy-twitch-release --pruefen bestätigt: Release `b0bd68248c3accc1771e938e6166c3a122ac154e`, vier Dienste aktiv und keine gelöschte exe. PIDs: Bot 3022811, Dashboard 3022680, Coaching-Watch 3023797, Collector 3023840. Frontend-Quellen gegenüber b0bd6824 bis zum Staging-Main unverändert; vor Übernahme der Bundles erneut gegen den endgültigen SHA prüfen, niemals nach Datei-Alter entscheiden. Noch kein eigener Merge, Push oder Deploy erfolgt.
