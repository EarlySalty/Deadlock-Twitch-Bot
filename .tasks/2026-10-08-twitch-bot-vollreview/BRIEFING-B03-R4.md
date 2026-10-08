# B03: serielle Freigabe nach A01-Integration

Frischer Sol-Fixer, anschließend frischer Sol-Kritiker. Ausführungsrunde 4, nach zwei fachlich beanstandeten Codefassungen und einem reinen Abhängigkeitsblocker in Runde 3. Die ursprüngliche Reviewer- und doppelte Skeptikerkette in BRIEFING-B03.md bleibt maßgeblich.

## Abhängigkeit aufgelöst

A01 ist tatsächlich als f04c0ef03d47ce4893ee4d17cb98eeb4c2f6c473 auf main integriert. Astra prüfte den unveränderten 36780-Byte-Diff, sieben einzelne Git-Schritte, aktuellen Sol-Gate und regulären Push Exit 0; anschließendes ls-remote bestätigt diesen SHA. Kein Deploy oder Neustart. Der A01-Writer ist beendet. Damit erhält B03 jetzt seriell zusätzlichen Zugriff auf auth/level.rs.

Eigener Worktree /home/nathanael/.worktrees/tb-vollreview-audit-akteur, Branch fix/vollreview-audit-akteur. Erhaltener Head aa4509784c0e689dfc4e45b892222a6e01a5f8cc auf e98b7f016dbab373a5a8dd9490d158b136c97fec. Ursprungskorrektur nur in admin_audit.rs. Frisches main holen, eigenen Patch regulär konfliktfrei abgleichen und A01 vollständig erhalten. Alte ALLOWs sind keine Freigabe dieses neuen Gesamtdiffs.

Exklusive Schreibpfade:

- rust/crates/tb-dashboard-api/src/admin_audit.rs
- rust/crates/tb-dashboard-api/src/auth/level.rs

auth/session.rs, auth/discord_admin_login.rs und lib.rs sind nicht freigegeben. Keine andere Rolle schreibt aktuell an den beiden B03-Pfaden. Sonstige Pakete und Hauptcheckout unangetastet lassen.

## Konkreter ursprünglicher Restfehler

Runde 2 authentifiziert nach der erfolgreichen Aktion erneut und kann damit einen anderen Akteur speichern. Beispiel: erstes Cookie zentral gültige Discord-ID 99 ohne lokalen Spiegel, zweites Cookie lokal gültige ID 42. Brokerprüfung für 99 scheitert während tatsächlicher Authentifizierung technisch. Auth und CSRF wählen 42, Aktion erfolgt. Beim Audit funktioniert der Broker wieder, zweite Auswahl ergibt 99. Dieser Akteur hat die ausgeführte Aktion nicht authentifiziert.

Soll: Audit übernimmt die für die tatsächlich ausgeführte Aktion gewählte Identität. Runde 3 hat belegt, dass dies bei geklonten Request-Teilen einen request-lokalen gemeinsam sichtbaren Träger aus der tatsächlichen Auth-Auswahl benötigt. Der zusätzliche Pfad ist für genau diesen minimalen Datenvertrag freigegeben. Kein neuer Authmechanismus, keine geänderte Auswahlreihenfolge, kein pauschaler Abbau des technischen Ausfallfallbacks, keine neue Sessionpolitik.

## Umsetzung und Beweis

1. AUFTRAG.md, BRIEFING-B03.md und BRIEFING-B03-R3.md als historischen Mangelkontext lesen. Für die Schreibgrenze gilt dieses neuere Briefing. Vor Codesuche code-suche/Graphify. Tatsächliche Middleware-Reihenfolge und vorhandene Extensions am frischen SHA ermitteln.
2. Die reale erfolgreiche Auswahl in einem je Request geteilten Träger festhalten und beim Audit verwenden. Keine erneute unabhängige Authentifizierung als Ersatz. Anonyme, interne und bestehende legitime Fallbackpfade unverändert lassen. A01s explizite zentrale Ablehnung sowie optionaler Testvertrag müssen erhalten bleiben.
3. Deterministische Regression für wechselnden Brokerausgang und verschiedene Cookie-Identitäten. Vorhandene Auditfälle, zentrale Sitzung ohne lokalen Spiegel und A01-Regressionsverträge mitprüfen. Keine künstlich konstanten Brokerantworten als alleiniger Beweis für den konkreten Wechsel. Keine Prozess-globalen Akteursdaten.
4. Bestehende Crate-Suite, eigene Formatierung, Paketformat und Clippy gemäß Auftrag am gebundenen finalen Quellstand. Vollsuite mit --no-fail-fast, --include-ignored und dokumentiertem Setup. Passende neue main-Baseline liegt in A01-INTEGRATIONSVORBEREITUNG.md und /tmp/tb-a01-integration.qw1biK/ vor, aber Quell-/Kommandoidentität vor Wiederverwendung prüfen. 1341 bestandene und 35 vorbestehend rote Tests sind kein grüner Gesamtlauf.
5. Neue DB-Tests ohne freiwillige Aktivierung überspringen; ausdrücklich aktiviertes kaputtes Setup hart melden. Diesen Vertrag separat prüfen, nicht aus positiver DB-Ausführung ableiten. Sauberer eigener Commit, nichtleerer Gesamtdiff und Sol-Gate ausschließlich mit --model gpt-6.1-sol --effort high --timeout 1080. Bei neuem fachlichem BLOCK beenden und den Befund zurückgeben, keine nächste Korrektur im selben Kontext.

Nach erfolgreicher tatsächlicher Codekorrektur folgt ein frischer Kritiker für den gesamten B03-Diff. Bei weiter nötigem nicht freigegebenem Pfad präzisen Abhängigkeitsblocker zurückgeben statt Nebenumbau.

## Grenzen und Ausgabe

Keine Kommentare, Umbenennung, Konfiguration, Abhängigkeiten oder Nebenbereinigung. Rust 1.97.1, SQLX_OFFLINE=1, cargo-slot, --jobs 1. Test-DB ausschließlich synthetisch gemäß Auftrag. Eigene laufende Kompilierung nicht doppelt starten oder pauschal abbrechen, fremde Prozesse/Slots unangetastet. Ein Git-Schritt je Bash-Aufruf, literale absolute Pfade.

Kein Main-Push, Releasebau, Deploy, Neustart, Löschen, schreibender Prod-DB-Befehl, Migration, echte Kontoaktion, Secret-/ENV-Lektüre, ai-coach oder Browserarbeit. Brave verboten. Python-Anwendungscode unverändert. Keine weiteren Agenten, T3-Threads, ListAgents oder SendMessage. Kein Fallback oder Schutzbypass.

Einzige Task-Schreibdatei B03-R4-NACHWEIS.md. Rückgabe mit tatsächlicher Basis/Head, geändertem Umfang, gebundenen Prüfungen, Gate, offenen Lücken und eigenen Hintergrundaufgaben. Branch und Worktree erhalten.
