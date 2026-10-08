# Zweite Gruppe bestätigter W02-Fixpakete

BRIEFING[WB-1]: Pflichtteile 5/5 | Timer 20 min | Worktree: eigener Paket-Worktree unter /home/nathanael/.worktrees/

Auftrag AUFTRAG.md bleibt verbindlich. Astra delegiert fünf Pakete mit getrennten Schreibpfaden. Je Befund liegen zwei unabhängige BESTÄTIGT-Urteile mit belegtem Soll vor. Nur diese bestätigten Fehler korrigieren. Keine allgemeine Modernisierung, Migration, neue Konfiguration, Abhängigkeit, Kommentare oder fachfremde Änderungen.

## Paketmatrix und Eigentum

Quellpfade relativ zu `rust/crates/tb-dashboard-api/`.

| Paket | Worktree | Branch | Exklusiver Schreibbereich |
|---|---|---|---|
| A02, Affiliate-Eigentümer | tb-vollreview-affiliate-eigentuemer | fix/vollreview-affiliate-eigentuemer | src/handlers/affiliate.rs, src/handlers/affiliate_portal.rs |
| B04, Routerverträge | tb-vollreview-router-vertraege | fix/vollreview-router-vertraege | src/lib.rs |
| B05, proaktiver Plattform-Refresh | tb-vollreview-plattform-refresh | fix/vollreview-plattform-refresh | src/handlers/platform_token.rs, src/handlers/platform_store.rs, src/handlers/plattform_oauth.rs |
| B06, Proxy-Antwortgrenze | tb-vollreview-proxy-antwort | fix/vollreview-proxy-antwort | src/proxy.rs |
| B07, Plan-Testfixture | tb-vollreview-plan-fixture | fix/vollreview-plan-fixture | tests/plan_stufen_gates.rs |

Vollständige Worktreepfade beginnen mit `/home/nathanael/.worktrees/`. Von frisch geholtem origin/main anlegen, einen vorhandenen eigenen Stand bei Wiederaufnahme zuerst prüfen. Zunächst feststellen, ob aktuelles main den Defekt bereits beseitigt hat. Fremde Änderungen bleiben unangetastet.

A01 besitzt weiterhin `auth/session.rs`, `auth/level.rs` und `auth/discord_admin_login.rs`. B02 besitzt `obs/bus.rs`/`obs/ws.rs`, B03 `admin_audit.rs`. Diese Dateien nicht ändern. Falls eine weitere Datei unabdingbar ist, den konkreten Bedarf an Astra zurückgeben, bevor die Eigentumsgrenze überschritten wird. Eine Datei in der Paketmatrix ist eine erlaubte Grenze, keine Pflicht, sie anzufassen.

## A02: wiedervergebener Login öffnet fremdes Affiliate-Konto

Befund `W02-DA02-S004-correctness-1`, Klasse A. Primärort `src/handlers/affiliate.rs:521` am Review-SHA `0ecae1370f1a80d1a101249b5c932663d69be8af`.

Eine Person legt mit Twitch-ID 111 das Affiliate-Konto unter Login L samt persönlichen Daten an. Eine andere Person erhält später denselben Twitch-Login mit ID 222 und meldet sich regulär über Affiliate-OAuth an. Der Callback und die geschützten Profilzugriffe wählen das bestehende Konto über L, ohne die bereits gespeicherte Eigentümer-ID 111 mit der authentifizierten 222 zu vergleichen. Dadurch kann `/twitch/api/affiliate/me` persönliche Daten des alten Kontos liefern. Verschlüsselung mit loginbasierter AAD, korrektes OAuth und gültige Sessions verhindern die falsche Kontozuordnung nicht.

Sollbelege: unveränderliche Identität `auth/session.rs:1410-1435`; gespeicherte Eigentümer-ID `handlers/affiliate.rs:782-792` und bestehende Tabellendefinition `rust/migrations/20260617030000_baseline_missing_tables.sql:21-40`; bestehender Fremdeigentumstest `handlers/affiliate.rs:2119-2233`. Migrationen nur lesen, niemals ändern oder ausführen.

Ziel: fremde vorhandene Affiliate-Konten nicht allein aufgrund eines wiedervergebenen Logins öffnen, ändern oder im Callback überschreiben. Vorhandene authentifizierte und gespeicherte IDs nutzen, gültiges Onboarding und bestehende korrekte Eigentümer erhalten. Bereits unter dem alten Fehler ausgestellte gültige Affiliate-Sessions mit falscher Kontozuordnung bei geschützten Zugriffen berücksichtigen. Kein automatischer Besitztransfer, keine Datenkorrektur oder Kontenzusammenlegung. Andere bestätigte Affiliate-OAuth-CSRF-Befunde sind noch nicht diesem Paket zugewiesen und werden nicht nebenbei umgesetzt. Bei unklaren Legacy-Zeilen die Grenze benennen statt eine Eigentümerregel zu erfinden.

Günstige Regression: gleicher Login, verschiedene unveränderliche IDs, keine fremden Profildaten und keine fremde Aktualisierung; gleicher richtiger Eigentümer und neues Konto funktionieren weiter. Kein tatsächlicher betroffener Nutzer und kein Live-Angriff wurde nachgewiesen.

## B04: zwei belegte Routerverträge

1. `W02-DA01-S004-correctness-1`, B, `src/lib.rs:1663`: Die registrierte Kündigungsroute mit literalem ü passt nicht zur vom Browser gesendeten URI `/twitch/abbo/k%C3%BCndigen`. Bestehendes browserseitiges Ziel muss den vorgesehenen Handler erreichen. Keine neue Produktseite, neue öffentliche Funktion oder Weiterleitungsarchitektur. Unterschied zwischen URI-Routing und Parameter-Decoding prüfen; vorhandene gültige Routen erhalten.
2. `W02-DA01-S004-security-2`, B, `src/lib.rs:1485`: Der Partner-Link-Teilrouter erhält die konfigurierte ExpectedToken-Extension nicht. Ein interner Client mit korrektem konfiguriertem `X-Internal-Token`, gültigem Body und ohne Browsercookies bekommt daher 403/admin_required. Soll: der vorhandene interne Admin-Vertrag gilt auch für die vorhandene Admin-Linkroute. Belege `auth/level.rs:3-16,329-335`, `handlers/partner_login.rs:6-10,94-108,202-209`, `tb-http-core/src/middleware/auth.rs:97-106,182-190`. Nur die fehlende Verdrahtung ergänzen, keine Guards abschalten oder neue Auth-Policy einführen. Falscher oder leerer Schlüssel, fremder Origin und nicht berechtigte Sessions bleiben abzuweisen.

Regressionen möglichst im bestehenden Testmodul derselben Datei. Die Plattform-Refresh-Funktion in lib.rs ist nicht Eigentum dieses Pakets zur fachlichen Änderung; B05 behebt deren belegte Ursachen in den Handler-Modulen.

## B05: zwei Fehler des proaktiven Plattform-Refreshs

1. `W02-DA01-S004-errors-1`, B, sichtbare Folge `src/lib.rs:2276`: Der echte OAuth-Client akzeptiert einen positiven i64-Ablaufwert, der die Chrono-Dauer oder das Zieldatum übersteigt. Die ungeprüfte Umwandlung panikt im proaktiven Kick-/YouTube-Refresh und beendet dessen Task. Beispiel: `expires_in=9223372036854776`, obwohl es in i64 passt. Ein normaler Anbieterfehler soll als Fehler zurückgegeben werden, spätere Verbindungen und Ticks weiterlaufen. Belege `lib.rs:2277-2296`, `handlers/platform_token.rs:357-368,546-550`, `handlers/plattform_oauth.rs:153-160`, `handlers/platform_store.rs:433-436`. Vorhandene gültige Ablaufwerte und die bisherige Behandlung negativer Werte erhalten. Kein allgemeiner TaskSupervisor-Umbau, keine neue Zeitbudget-Policy. Betroffene gemeinsame Ablaufumwandlungen und beide Anbieterpfade prüfen.
2. `W02-DA01-S004-errors-2`, B, sichtbare Folge `src/lib.rs:2289`: Bei gespeichertem, bald ablaufendem Kick-/YouTube-Zugang ohne konfigurierten Anbieterclient liefert der übersprungene Refresh Ok und wird als tatsächlich erneuert gezählt. Der Timer meldet Erneuerungen, obwohl weder Anbieteraufruf noch Speicherung stattgefunden haben. Das Überspringen selbst ist beabsichtigt. Nur den falschen Erfolg entfernen, keine neue Reauth- oder Fehlermeldungs-Policy. Vorhandener Vertrag `handlers/platform_store.rs:367-425,475-480`, Tests `636-669,701-721`: Erneuert setzt tatsächlich gespeicherte neue Werte voraus, NichtNoetig bleibt getrennt.

Regressionen für extreme akzeptierte Ablaufwerte ohne Panic, normale Werte sowie fehlenden Anbieter ohne falsche Erneuerungszählung. Keine Anbieteranfragen oder Zugangsdaten verwenden. Kein tatsächliches Auftreten der extremen Antwort in Produktion wird behauptet.

## B06: dokumentierte Proxy-Antwortgrenze

`W02-DA01-S005-errors-1`, B, `src/proxy.rs:202`. Die Fallback-Antwort wird ohne die belegte 16-MiB-Antwortgrenze gepuffert. Zwei Skeptiker bestätigen den Antwortvertrag und die konkrete Überschreitung. Vollständige Sollbelege aus ihren unten referenzierten Journalresultaten lesen; die Requestgrenze allein ist kein Antwortvertrag.

Den vorhandenen Antwortvertrag vor beziehungsweise während der Pufferung durchsetzen, einschließlich fehlender oder falscher Content-Length und der tatsächlichen vom Client gelieferten Bytes. Korrekte Antworten, Header-/Statussemantik und Requestgrenzen erhalten. Keine willkürliche neue Grenze oder Proxy-Neuarchitektur. Stalled-Request-Body und andere C-Szenarien sind nicht freigegeben. Günstige lokale Regression mit Grenzwert und Überschreitung, keine Produktionsaufrufe.

## B07: Testfixture erreicht den beabsichtigten Plan-Gate

`W02-DA01-S007-correctness-1`, B, `tests/plan_stufen_gates.rs:141`. Die gemeinsame Partnerfixture lässt die Twitch-ID leer. Dadurch scheitern Akzeptanztests am vorherigen Identitätsguard statt die vorgesehenen Feature-/Plan-Gates zu prüfen. Beide Skeptiker bestätigen den Fixturefehler und das Soll.

Nur die synthetische Fixture an den bestehenden unveränderlichen Identitätsvertrag anpassen. Keine produktiven Guards, Testassertionen oder Planregeln lockern. Kein Umbau fest benannter Testschemata, das ist ein gesonderter C-Befund. Betroffene vorhandene Tests ausführen und Vorher-/Nachhergrenze dokumentieren.

## Unabhängige Modell- und Urteilskette

Reviewer-Workflow `wf_cdc4c5ac-9bb`; 70 Einzelbelege in W02-KANDIDATEN.json unter model_proofs.reviews, von Astra unabhängig nachgeprüft. Reviewer der hier beauftragten Claims:

| Claims | Reviewer | Sol-Datensätze | Transcript-SHA256 |
|---|---|---:|---|
| A02 | ae1ce501d1d9ba73d | 91 | 741944ebcfb0241e5c12791af9b19e87d330536255cd06c15a3e0effe3925590 |
| B04 Kündigung | a53b34425596bfc4e | 55 | 8e2efd3da7230667979a631b52b0eca38f91a5c7f2f05f9cade54807f8e1c5c9 |
| B04 Admin-Link | a264d87612a6687a0 | 57 | 2a538ac8bbdae7d8f6f3a57a8fdeb6413fbe83f4119805e1e3aff12f3416256f |
| Beide B05-Claims | a972e17e0b3238a66 | 61 | e589704071f15ca88d958835862fce5aab6596de875a30b32ed7efbf6cb8d76a |
| B06 | acbcb0fa9b860f4f0 | 45 | 1079a1d586805cb18ece4c1b648b3007e1f63908999b49f165be229b333ee441 |
| B07 | a669b845f0d315088 | 61 | 6309da803787202d306e1b181ce466bb377cd033c203b8dceade37aef0ce894d |

Skeptiker-Workflow `wf_f9b737d9-fe8`. Jede folgende Rolle war frisch und erhielt nur Behauptung und Ort. Je Paar BESTÄTIGT, genannte Klasse, Soll belegt. Astra prüfte die Modellfelder und SHA256 der abgeschlossenen Transcripts; ausschließlich gpt-6.1-sol.

| Claim | Skeptiker | Sol-Datensätze | Transcript-SHA256 |
|---|---|---:|---|
| A02 | a833a33ce11e05b37 | 35 | fc975b2c8ed46e72779b96217551ecc01d085fdc50ff5a129e8ddfbc4a6496a9 |
| A02 | ac37c053cf7e2e6f0 | 43 | 531f3a528bfcea63c7e53bcf8af7aa107670386260b4f4a9b41f1371eda4e0a8 |
| B04 Kündigung | a8c15c47db0975444 | 31 | d3a5973af9b41e372d589214e056d254ad2fdc5abb60a97a829fcaef9dca23ee |
| B04 Kündigung | af8bd0ba61f4941e6 | 35 | a4c3e38221df8430e0564d9f26bc4a2df2874e7f081e85ab56710ff4e7735047 |
| B04 Admin-Link | ae102308a9d2410ed | 31 | da3c16e93575757df7044edd09ca936b31ffd06bacc67713224f3c90b10cae33 |
| B04 Admin-Link | a296ccdadaac69d62 | 37 | 2ff25c60b0a7af686bc5affa05437cd730ff5b76c97f8bf65e260ce86cb749aa |
| B05 extremer Ablaufwert | a2e02e4fc74d4013e | 32 | 1c2d62e530bfc02ae2af28cf70b5f31e62480b44af29210520d198881a8d06dd |
| B05 extremer Ablaufwert | a97a3a6720ac744df | 43 | 190e0592220354dd9a586abf9121a81b072df79fc0635a3124e41031d41a0084 |
| B05 falsche Erneuerungszählung | ac633ce1507dec721 | 27 | b79bc94ce3bfcf209e6d0a57633b8d7ca3950c81ee8d1939c720300e193fd951 |
| B05 falsche Erneuerungszählung | a33df847907013bff | 33 | 4df7e795168cc950c5c3ddb2b0892419c2a21cde9a142d6d6c789288d5ab585d |
| B06 | adc408c1e495cb6a4 | 31 | 374987e2d49ad64241677a66b8a2019ca8b684b24bc384f2448631940d1d93ac |
| B06 | a9d72ec7b62b76f5d | 39 | 2e419f77b3ac752a55a011450dc7f96c3d0885b283536569c2d4dd9e1d5286ae |
| B07 | aac95ae23d7aa929c | 26 | 20f8c428d725b697f722b571fed3f102ef098fddacd4dc06b86299dde0e95672 |
| B07 | a296432781e2dc20b | 30 | 880eb7ace03e45e9483f5787f22d305b60e5d0bf0a5c5c52fcef6ed6e0b4e0ce |

Vollständige Begründungen liegen als result-Einträge mit den Agent-IDs im jeweiligen nativen journal.jsonl unter `/home/nathanael/.claude/projects/-home-nathanael-repos-Deadlock-Twitch-Bot/f61905e7-f7ff-405b-a6d7-090dec371fcb/subagents/workflows/`. Nur eigene zugewiesene Resultate gefiltert lesen, keine ganzen Journale oder Rohtranscripts in den Kontext kopieren.

## Prüfungen, Git und Abgabe

Vor Suche code-suche und Graphify gegen `/home/nathanael/repos/Deadlock-Twitch-Bot/graphify-out/graph.json`, anschließend aktueller eigener Worktree oder fester Review-SHA. Keine Secrets/ENV-Dateien, Produktionsdatenbank-Aufrufe, Migrationen, echten Kontoaktionen, Browser oder `ai-coach`. Keine produktiven Python-Änderungen. Keine anderen Agenten/Threads, kein ListAgents/SendMessage, keine Nutzerfragen. Nur gpt-6.1-sol.

OPS-PREFLIGHT.md und rolle-test-waechter beachten. `RUSTUP_TOOLCHAIN=1.97.1-x86_64-unknown-linux-gnu`, `SQLX_OFFLINE=1`, vorhandenes `/home/nathanael/.local/bin/cargo-slot`, bei Kompilierung `--jobs 1`. Testdatenbank: `TB_TEST_DATABASE_URL='postgres:///tb_bb_test?host=/var/run/postgresql'`, `TB_TEST_REQUIRE_DB=1`. `rust/test-database.json` nur auf Existenz prüfen. Eigene Dateien formatieren, paketbezogen Formatprüfung und Clippy, bestehende betroffene Tests von tb-dashboard-api. Fremde Baselinefehler nicht nebenbei korrigieren. Eigene reguläre Kompilierung nicht nach wenigen Minuten abbrechen; keine fremden Builds oder Dienste ändern. Fehlende Testausführung ehrlich als offen melden.

Nur eigene Dateien stagen, ein Git-Schritt je Bash-Aufruf, literale absolute Pfade. Eigener Commit und Sicherung des Arbeitsbranches erlaubt. Trailer `Co-authored-by: GPT 6.1 Sol <gpt-6.1-sol@local>`. Kein main-Push, Merge nach main, Release, Deploy oder Restart. Der offene Wrapper-Konflikt bleibt bestehen.

Finaler Gate: `python3 /home/nathanael/Documents/.claude/gpt-workers/gate_hook.py --review --repo <eigener literaler Worktreepfad> --base origin/main --head HEAD --model gpt-6.1-sol --effort high --timeout 1080`. Keine Platzhalter ausführen. Kein Rückfallmodell, kein --chain, keine Hook- oder Zustandsänderung. Bei BLOCK zurückgeben; nächste Fixrunde mit frischem Kontext. Nach Abgabe folgt ein frischer Sol-Kritiker.

Rückgabe: Status je zugewiesenem Claim, Basis-/Head-SHA, geänderte Dateien, tatsächliche Prüfaufrufe mit Exit-Codes, Baseline, Gate, Sicherungsstatus und verbleibende Grenzen. Keine neuen Berichtdateien, Astra dokumentiert. Wenn eine Quelle auf main schon korrigiert ist, mit exaktem Beleg nicht erneut ändern.
