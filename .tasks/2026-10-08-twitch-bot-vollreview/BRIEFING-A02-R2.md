# A02: frische Fixrunde 2 nach aktuellem Gate-BLOCK

Ausschließlich gpt-6.1-sol. Der bisherige Fix und die ursprünglichen Reviewer-/Skeptikerbelege bleiben erhalten. Auftrag ist die minimale Behebung der jetzt belegten Testregression innerhalb des eigenen Fixes, keine neue Produktänderung.

## Ausgangspunkt und Mangel

Eigener Worktree /home/nathanael/.worktrees/tb-vollreview-affiliate-eigentuemer, Branch fix/vollreview-affiliate-eigentuemer. Erhaltener sauberer Head e6765a5dd5440fe32dbd4896d1c85db1af3a54ef auf e98b7f016dbab373a5a8dd9490d158b136c97fec. Exklusive Dateien:

- rust/crates/tb-dashboard-api/src/handlers/affiliate.rs
- rust/crates/tb-dashboard-api/src/handlers/affiliate_portal.rs

A02-INTEGRATIONSVORBEREITUNG.md dokumentiert unveränderten Patch und vollständige final gebundene Prüfungen. Der aktuelle Sol-Gate ist BLOCK: New tests break database-free test runs. Drei neue Tests erzwingen trotz optionalem DB-Vertrag mit expect("Testdatenbank") einen Pool, an affiliate.rs:1847,1926 und affiliate_portal.rs:453. Mit ausdrücklich aktivierter Testdatenbank bestanden diese Tests; der freiwillig nicht aktivierte Pfad blieb unberücksichtigt.

Gate-Log /tmp/tb-a02-integration-20261008.GocqTo/sol-gate.log, SHA256 ad78d762778d468fb1b2b4b6210707ba5e62bf8901081210c067fe71bfe5a1ae. Gatezustand cfef2f5df6d1e813.json bestätigt festes Paar, Sol-Modell und eine BLOCK-Runde. Vorbereitungsrolle aae9b47ffea978cb3, 114 echte Sol-Datensätze, Transcript-SHA256 a33709bb6f8608fea63bfdc95260d7fb960e09a5cad8717a02b93887ac0ecdc7. Astra hat Modell, fertigen Hash und originale Rückgabe geprüft; ABSCHLUESSE-11.json. Frühere ALLOWs heben diesen aktuellen BLOCK nicht auf.

## Minimaler Auftrag

1. AUFTRAG.md, ursprünglichen A02-Abschnitt in BRIEFING-W02-FIXGRUPPE-02.md und aktuellen Gatebefund lesen. Vor Codesuche code-suche/Graphify. Bestehenden optionalen DB-Testvertrag und diese drei neuen Stellen gezielt prüfen.
2. Den vorhandenen Skip ohne freiwillige DB-Aktivierung erhalten. Bei ausdrücklich eingeschaltetem, fehlerhaftem Setup muss der Test weiterhin hart scheitern. Erfolgreiche DB-Regressionsprüfungen und ihre Assertions nicht abschwächen. Keine pauschale Änderung alter Fixtures, kein neuer Schalter, keine Produktionslogikänderung.
3. Beide Verträge gezielt nachweisen: ohne Aktivierung regulärer Skip, mit synthetischer DB vollständige drei Regressionen. Zusätzlich ausdrücklich aktiviertes ungültiges Setup darf nicht still grün werden. Kein Zugriff auf Produktionsdaten oder vorhandene Zugangsdaten. Vorhandene Helperverträge respektieren.
4. Bestehende Crate-Suite, Clippy und Formatprüfung gemäß Auftrag auf finalem Quellstand abschließen. Vollsuite mit --no-fail-fast, Flags und Setup dokumentieren. Passende gebundene Baseline aus dem vorhandenen Nachweis verwenden, wenn Quellstand und Ausführung wirklich passen; neue Quelländerung braucht neue Fixprüfung. Vorbestehende 35 Fehler sind kein grüner Gesamtlauf.
5. Sauber committen, nichtleerem Diff gegen aktuelle passende Vorfahrbasis zuordnen, lokaler Gate ausschließlich gpt-6.1-sol, --effort high --timeout 1080. Kein Gate-Neuwürfeln oder Fallback. Bei weiterem fachlichen BLOCK zurückgeben; eine weitere Korrektur erhält wieder einen frischen Fixer. Danach prüft ein anderer frischer Sol-Kritiker den gesamten A02-Diff einschließlich des optionalen Testvertrags.

Originaler Eigentumsfix darf nicht verloren gehen: Die unveränderliche Twitch-ID bleibt entscheidend, auch bei bereits ausgestellten Affiliate-Sitzungen. Kein neuer Eigentums-, OAuth- oder Migrationsauftrag.

## Grenzen

Eigenen Worktree und aktuelle Basis zuerst prüfen. Kein paralleler Writer auf den beiden Dateien. Main kann fortschreiten; konfliktfreien Abgleich regulär ausführen, Quellbindung nachführen, keine fremden Änderungen zurücknehmen. Jede Git-Aktion einzeln mit literalem absolutem Pfad. Kein Main-Push, Deploy, Neustart, Löschen oder Releasebau. Branch und Worktree erhalten.

Rust 1.97.1, SQLX_OFFLINE=1, cargo-slot, --jobs 1. Keine laufende eigene Kompilierung duplizieren oder pauschal abbrechen, fremde Prozesse/Slots unverändert. Keine Secrets/ENV-Dateien lesen oder ausgeben, keine schreibenden Prod-DB-Befehle, Migrationen, echten Konten, ai-coach, Python-Anwendungsänderung oder Browserarbeit. Brave verboten. Keine zusätzlichen Agenten, T3-Threads, ListAgents oder SendMessage. Keine Codekommentare, keine Nebenbereinigung.

Einzige Task-Schreibdatei der Fixrolle A02-R2-NACHWEIS.md. Vollständige Rückgabe mit Basis/Head, tatsächlichem Diff, Prüfkommandos und Quellbindungen, Gate, offenen Lücken und eigenen Hintergrundaufgaben. Keine neue Rolle selbst starten.
