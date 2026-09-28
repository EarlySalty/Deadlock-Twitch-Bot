status: aktiv (2026-09-28)

# Patchnotes im Twitch-Chat, unabhängig vom Rust-Port des Patchnotes-Bots

## Ziel
Wenn eine neue echte deutsche Patchnotes-Seite veröffentlicht wurde, bekommen aktive Partner, die zu diesem Zeitpunkt mit Deadlock live sind, genau eine kurze Bot-Nachricht in ihrem eigenen Twitch-Chat mit Link auf genau diese Website-Seite. Nach Neustart und unsicheren Sendefehlern keine doppelten Nachrichten. Historische Patches und fremde Kanäle werden nie nachträglich beworben. Die neue Funktion ist Rust-Code im bestehenden Twitch-Bot; keine Änderung am produktiven Python-Patchnotes-Bot.

## Bestand und Grenzen
- Uncommittierter Altentwurf, ausschließlich lesbare Vorlage: `/home/nathanael/.worktrees/twitch-patch-hype-20260920/`, 226 Commits hinter dem am 28.09. geprüften `origin/main` (`992e265961048ee72d03a483673c92ef3c49e715`). Sein Receiver in `rust/crates/tb-internal-api/src/handlers/patch_announcement.rs:25-85,175-315` nutzt noch einen Discord-Link, snapshotet aber aktive Deadlock-Partner und persistiert Sendeversuche. Nicht den gesamten Altbranch mergen oder dort fremde uncommittierte Dateien ändern.
- Laufender Website-Publisher: `/home/nathanael/.worktrees/patchnotes-live-main/web_publish.py:401-409,529-534`. Die Einzel-HTML-Seite wird vor dem öffentlichen `https://deutsche-deadlock-community.de/patchnotes/index.json` geschrieben. Das JSON antwortete am 28.09. mit HTTP 200 und 46 Einträgen; neuester Eintrag ist ID 285. Eintragsfelder: `id`, `posted_at`, `url`; `/patchnotes/patch-<id>/meta.json` enthält `source_url`. Diese Website-Schnittstelle soll beim laufenden parallelen Rust-Port erhalten bleiben.
- User-wirksamer Bot-Text, nur bei echtem neuen Patch: `Neuer Deadlock-Patch ist da 🔥 Die Änderungen auf Deutsch: {url}`. `url` ist die individuelle kanonische DE-Seite auf unserer Domain, keine Discord-Adresse, kein Archiv- oder Original-Link. Text vor dem Senden auf Länge und Zeichen prüfen. Kein persönlicher Post im Namen des Betreibers.
- Sicherheitsgrenze: nur verbundene aktive Partnerkanäle mit aktiver Deadlock-Session und erlaubter Bot-Ausgabe, keine beliebigen Twitch-Streamer. Quelle nur feste HTTPS-Website-Adresse, keine freien Redirects. Kein Testversand an echte Partner, kein altes Event replayen.
- Unabhängige andere T3-Arbeit `17337791-d5fe-49c8-ad55-2e31a2e016d2` portiert den Patchnotes-Bot in Rust; deren Repo, Branches, Config und Services sind tabu. Kommunikation über den Website-Vertrag, keine gemeinsamen Code-Dateien. Keine neuen Python-Features oder produktiven Python-Fixes, keine Code-Kommentare schreiben.

## Pakete
- A: Twitch Source-only Chat-Transport und Schutzregeln im Rust-Bot (Altentwurf prüfen, passend auf aktuelle main-Basis übertragen).
- B: validierter Receiver, unveränderliche Empfängerauswahl, migrationsgestützte Zustellquittungen. Beginnt nach A, damit alle Crates einzeln bauen.
- C: Rust-Website-Beobachter im Twitch-Bot: nur veröffentlichte neue Artikel, langlebiger Cursor, kein Altpatch, feste URL-Validierung. Läuft parallel zu A.
- D: Wiring und Integration nach A/B/C, gemeinsame Tests/Review/Gate, Migration/Release/Live-Beweis. Pro Paket ein eigener T3-Thread/Worktree, Datei-Zäune in `PAKETE.md`.

## Fertig-Kriterium
Kombinierter Rust-Code kompiliert, fmt/clippy und betroffene vorhandene Tests laufen; frischer Kritiker prüft gegen diesen Intent und das lokale Merge-Gate entscheidet. Migration vor Start im echten Twitch-Postgres; aktueller `origin/main`-SHA nach Merge gebaut/deployt; `deadlock-twitch-bot-rust` neu gestartet; read-only Live-Beweis: öffentliche JSON-Antwort und neuer Artikelstatus, Dienst aktiv, Cursor auf bestehende ID 285 initialisiert, keine historische Chat-Nachricht. Eine echte Live-Zustellung ist erst beim nächsten realen Patch verifizierbar. Python-Detektor-Recovery und der ungeklärte Forumskandidat vom 23.09. sind Aufgabe des parallelen Rust-Port-Threads; sie nicht als von diesem Paket gelöst melden.
