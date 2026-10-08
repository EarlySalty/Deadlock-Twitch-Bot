# Abschluss M/1

Stand: 2026-10-08. Fix auf main veröffentlicht, produktiv ausgeliefert und geprüft. Ressourcenabschluss läuft; danach eigenen T3-Thread abschließen.

## Wirkung

Lob, Follow- oder Wiederkommen-Versprechen und ungefragter Discord-Kontakt werden im vorhandenen Rust-Scoring gemeinsam erkannt, einschließlich „Disscord“. Kontaktnamen liefern weder Gesprächskontext noch andere semantische Signale. Natürliches Deutsch ist im vorhandenen Gesprächswächter kein kategorischer Freibrief mehr. Schutzrollen, Vertrauen, Verlauf, Einstellungen und bestehende Aktionsregeln bleiben erhalten. Keine neue Architektur, keine neue Route, kein Modellwechsel und keine DB-Handkorrektur.

Screenshot-Wortlaut durch echtes Scoring und observe: unbekanntes Kontoalter ergibt Hint, zehn Tage ergibt PublicWarn, 200 Tage ergibt Hint. Echter Pipeline-Aufrufer mit 18 Fällen sowie Vertrauens- und Einstellungsausnahmen geprüft. Harmlose Kontaktantworten und Community-Einladungen erzeugen keine Aktion. Erstwarnungen löschen nicht und erzeugen keinen Timeout oder Ban.

## Veröffentlichung und Abnahme

Code- und Release-SHA: `3341098f3a953e231c0ec5731141996b10f1b9b6`. Veröffentlichung aus dem eigenen sauberen Main-Clone mit `HEAD:main`. Lokaler Gate mit gpt-6.1-sol: ALLOW, Exit 0. Erste BLOCK-Runde durch frischen nativen Fixer korrigiert, derselbe Reviewer bestätigt. Origin/main unmittelbar vor Build und Deploy frisch geprüft. Acht ELF-Herkunftsmarken exakt gleich dem sauberen Release-SHA; ausgelieferte Dateien entsprechen per SHA-256 dem Build.

Integrierte Kontaktabnahme: 7 passed, 0 failed, 0 ignored, Exit 0. Format Exit 0. Paket-Clippy Exit 0 mit einem vorhandenen Borrow-Hinweis. Gesamtsuite vor endgültigem Gate-Fix: 911 passed, 43 failed, 2 ignored; unveränderte Basis: 906 passed, 42 failed, 2 ignored. Zusätzlicher PoolTimedOut-Test isoliert auf Basis und Fix grün. Strict-Clippy scheitert auf Basis und Fix am selben bestehenden tb-raid-Befund. Keine grüne Gesamtsuite behauptet.

Deploy-Wrapper Exit 0. Vier Dienste aktiv auf dem Release-SHA ohne gelöschte exe, NRestarts jeweils 0. Bot-PID 1005470 zu 1830251; weitere PIDs in status/M/1/004-deployment.md. Journal mit -p err seit Deploy leer. Neuer ASCII-Anker im ausgelieferten Bot vorhanden, im Vorrelease nicht vorhanden. Frontend-Quellen gegenüber Vorrelease 51c8a674 unverändert; drei übernommene Bundles bytegenau geprüft.

Funktionsnachweis ist ein isolierter Replay des ausgelieferten Quellstands über echte Rust-Erkennung und den echten Pipeline-Aufrufer. Der KI-Transport wurde mit Wiremock geprüft; dessen Urteil ist ein Stub. Keine Live-Moderation gegen echte Konten und kein reales KI-Urteil als Test ausgelöst. Browser nicht benötigt. Ort ist der Rust-Moderationspfad des laufenden Twitch-Bots, keine neue UI.

## Belege und Abschluss

Belege dauerhaft unter `abschlussbelege/`, Gate-Runden in REVIEW.md, vollständiger Auslieferungsnachweis in status/M/1/004-deployment.md. AUFTRAG.md und REGISTER.md wurden nicht verändert. Abschlusscommit sichert Dokumentation, keinen weiteren Anwendungscode; produktiver Code bleibt Release 3341098f.

Baseline-Testcontainer und Baseline-Worktree entfernt. Verbleibenden eigenen Testcontainer, Fix-Worktree und gemergten Branch nach Sicherung der Belege entfernen. Branch-Löschung nach geprüftem merge-base-Exit. Der eigene unabhängige Staging-Clone wurde durch den Wrapper rootkontrolliert nach `/opt/deadlock/twitch/builds/3341098f3a953e231c0ec5731141996b10f1b9b6` verschoben. Release- und Build-Verzeichnisse bleiben für den laufenden Betrieb erhalten. Der schmutzige geteilte Checkout bleibt unberührt. Abschließende Aktion: eigenen T3-Thread mit `t3-thread.py settle --selbst` abschließen.
