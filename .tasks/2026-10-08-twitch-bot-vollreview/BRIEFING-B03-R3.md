# B03: dritte frische Fixrunde

Auftrag: Den verbliebenen Audit-Akteursfehler beheben. Frischer Sol-Fixer, danach frischer Sol-Kritiker. Die ursprüngliche Reviewer- und doppelte Skeptikerkette bleibt in BRIEFING-B03.md erhalten. Keine neue Gegenprüfung für günstigere Urteile.

## Stand und konkreter Mangel

Eigener Worktree `/home/nathanael/.worktrees/tb-vollreview-audit-akteur`, Branch `fix/vollreview-audit-akteur`, aktueller Head `3a01d0c3b67ecc6ebc20bee2cc2140660276ecbb`. Basis der Runde 2: `a8b5b5e986a1de0b8e2f981651f83bda9cf400dd`. B02 wurde inzwischen separat als `e98b7f016dbab373a5a8dd9490d158b136c97fec` nach main integriert. Vor dieser Runde origin/main frisch holen und die eigene Fixbasis regulär abgleichen. Keine fremde Arbeit im Hauptcheckout verändern.

Runde 2: Sol-Gate ALLOW, frischer Kritiker BLOCK. Der Kritiker bestätigt in `admin_audit.rs:121-125` und `:69-78`: Audit klont Request-Teile vor der nachgelagerten Authentifizierung und erhält die dort gewählte AuthenticatedAdminSessionId nicht. Nach erfolgreicher Aktion wird nochmals authentifiziert.

Konkretes Szenario: Erstes Cookie gehört zu zentral gültiger Discord-ID 99 ohne lokalen Spiegel, zweites zu lokal gültiger ID 42. Bei der tatsächlichen Authentifizierung scheitert die Broker-Prüfung für 99 vorübergehend. Auth und CSRF wählen 42 und führen die Aktion aus. Beim späteren Audit funktioniert die Broker-Prüfung wieder. Die zweite Auswahl wählt 99, gespeichert wird der falsche Akteur. Der Testmock der Runde 2 liefert pro Sitzung konstante Antworten und erfasst den Wechsel nicht.

Soll: Audit übernimmt die für diese tatsächlich ausgeführte Aktion gewählte Identität. Berechtigungsprüfung, bestehender technischer Ausfallfallback und anonyme/interne Fallbacks bleiben erhalten. Eine zusätzliche unabhängige Authentifizierung ist kein Nachweis derselben Auswahl.

## Schreibgrenze

Weiterhin ausschließlich `rust/crates/tb-dashboard-api/src/admin_audit.rs`. A01 besitzt `auth/level.rs`, `auth/session.rs` und `auth/discord_admin_login.rs`; B04 besitzt `lib.rs`. Dort nicht schreiben, keine parallelen oder vorgezogenen Schnittstellenänderungen. Falls ein minimaler richtiger Fix die Übernahme der tatsächlich ausgewählten Identität außerhalb der eigenen Datei benötigt, präzisen Abhängigkeitsblocker mit erforderlichem Pfad und minimalem Datenvertrag zurückgeben. Keine unsichere Ein-Datei-Umgehung erzwingen. Astra ordnet nach Ende der bestehenden Dateieigentümer seriell zu.

## Prüfungen und Abnahme

Vorhandene Regeln und ursprüngliches Briefing lesen; Code-Suche zuerst Graphify. Keine neuen Features, Kommentare, Abhängigkeiten, Konfiguration oder Nebenbereinigung. Python bleibt unverändert. Keine Migrationen, produktiven Datenbankzugriffe, echten Kontoaktionen, Secrets oder ENV-Dateien. Keine Browserarbeit, kein ai-coach, keine Sessionnachrichten oder zusätzlichen Threads.

Regression mit wechselnder Broker-Antwort deterministisch belegen, sofern im erlaubten Umfang umsetzbar. Testdatenbank freiwillig: ohne Opt-in weiterhin überspringen, mit aktiviertem Setup Fehler hart melden. `RUSTUP_TOOLCHAIN=1.97.1-x86_64-unknown-linux-gnu`, `SQLX_OFFLINE=1`, bestehendes cargo-slot, Kompilierung mit `--jobs 1`, `TB_TEST_DATABASE_URL=postgres:///tb_bb_test?host=/var/run/postgresql`, `TB_TEST_REQUIRE_DB=1`. Keine fremden Slots/Prozesse verändern und keine normale Kompilierung nach einer pauschalen Frist abbrechen.

Quellstand und Diff vor den Prüfungen fest binden, vollständige Crate-Tests, Formatierung und Clippy gemäß AUFTRAG.md. Runde 2 hatte 1334 bestandene Tests und 35 identische Baselinefehler; ihre Logs gelten nicht automatisch für diese Runde. Paketformatierung hatte 265 gleiche Abweichungen, Clippy scheiterte vorher an fremder tb-chat-Diagnose. Belege in `/tmp/b03-r2-sol.AW8duh/` erhalten.

Bei tatsächlichem Fix: eigener Commit, sauberer nichtleerer Diff, expliziter Sol-Gate mit `--model gpt-6.1-sol --effort high --timeout 1080`, frische Kritik danach. Kein Modellfallback oder Schutzbypass. Jeder Git-Schritt einzeln mit literalen absoluten Pfaden. Kein Push, Merge, Releasebau, Deploy, Neustart oder Aufräumen in dieser Runde.

Modelle und finale Rückgaben der Runde 2 durch Astra geprüft: Fixer a710256042741ec48, 176 Sol-Datensätze, SHA256 f4d6906e27fc9e409824a0e26d4989b61c689ff09bd23d420ad5dc4093fe293d. Kritiker a7e3bce057453012a, 45 Sol-Datensätze, SHA256 bd770e27bdb4ee9abd82bce24f6029d55e0e45b8f9dd6e89725f5faedf01367d.
