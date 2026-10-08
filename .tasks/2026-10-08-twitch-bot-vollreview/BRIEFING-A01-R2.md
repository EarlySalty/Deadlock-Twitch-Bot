# A01, frische Fixrunde 2

BRIEFING[WB-1]: Pflichtteile 5/5 | Timer 20 min | Worktree: /home/nathanael/.worktrees/tb-vollreview-session-widerruf

## Auftrag und belegter Restfehler

BRIEFING-A01.md bleibt der verbindliche Grundauftrag. Zwei unabhängige Bestätigungen je Teilbefund liegen vor. Dies ist die Korrektur des ersten lokalen Gate-BLOCK, kein neuer Auth-Umbau.

Erster Fixcommit: `3718481e9d58c94d1864012fd6c6d6acc55cb10c`, Basis `51c8a674a371d0e623687940e6b8ca3492f96c92`. Der Fixer ist beendet. Modellnachweis: `adc95e54a00eba87d`, Workflow `wf_6dc2eaf0-c73`, 113 echte Sol-Modellnachrichten. Astra prüfte den Transcript-SHA256:
`98290c16309686bc594c274911efc1e9c8ffd7ca2bbd0015b5c73cdde33dbf4f`.

Der explizite Sol-Gate endete mit Exit 1:
`BLOCK: A later broker outage restores explicitly rejected admin access.`

Fundort: `rust/crates/tb-dashboard-api/src/auth/level.rs:444` am ersten Fixstand. Die direkte zentrale Antwort `valid=false` wird zwar jetzt abgewiesen, ihre lokale Sitzungskopie bleibt jedoch erhalten. Bei einem späteren technischen Brokerausfall erhält derselbe bereits abgelehnte Sitzungswert über den lokalen Fallback wieder Adminrechte. Damit ist A01b nicht vollständig beseitigt.

Der frische Kritiker der ersten Runde liest ausschließlich den festen ersten Commit und kann noch weitere belegte Mängel liefern. Seine laufende Read-only-Prüfung blockiert deine Korrektur des bereits belegten Gate-Befunds nicht. Keine Dateien oder Referenzen des Kritikers ändern; endgültige Kritik dieser zweiten Runde erfolgt anschließend neu.

## Minimales Ziel

Eine bereits ausdrücklich abgelehnte Sitzung darf durch einen späteren technischen Fehler nicht lokal wieder zugelassen werden. Den bekannten lokalen Spiegel wirksam widerrufen oder dessen spätere Zulassung auf dem vorhandenen Widerrufspfad verhindern. Keine neue Widerrufstabelle, Konfiguration, Persistenzart oder globale Fallback-Abschaltung. Falls der notwendige Vertrag unter Datenbankausfall nicht minimal belegbar ist, die Grenze konkret zurückgeben, keine neue Produktregel erfinden.

Nachweise für die zeitliche Folge ergänzen: lokaler Spiegel vorhanden, tatsächlicher Brokerclient liefert `valid=false`, danach technischer Brokerfehler mit demselben Cookie. Es darf keine erneute lokale Adminzulassung geben. Eine andere bislang gültige Sitzung muss bei einem bloß technischen Brokerausfall weiterhin den bisherigen Fallback verwenden können.

A01a und seine gültigen Sitzungs-, TTL- und Cacheverträge erhalten. Ersten Diff auf die ursprünglichen beiden bestätigten Fehler begrenzen, keine weiteren jetzt parallel untersuchten A/B-Kandidaten nebenbei umsetzen.

## Eigentum und Basis

Vorhandenen eigenen Branch `fix/vollreview-session-widerruf` und Worktree fortsetzen, nicht neu anlegen. Arbeitsstand zuerst prüfen und frisches origin/main einbeziehen. Der Remote-Zeiger lief während Runde 1 weiter; vor dem neuen Gate die tatsächliche Basis feststellen. Keine fremden Änderungen oder Prozesse anfassen.

Schreibrecht unverändert ausschließlich diese drei Dateien unter `rust/crates/tb-dashboard-api/src/auth/`:

- `session.rs`
- `level.rs`
- `discord_admin_login.rs`

Der unveränderte eigene Baseline-Worktree `/home/nathanael/.worktrees/tb-vollreview-session-widerruf-baseline` liegt auf der ersten Basis und blieb sauber. Nicht als aktuellen Baselinevergleich ausgeben, wenn die Fixbasis inzwischen wechselt. Keine Baseline- oder Fix-Worktrees löschen, bevor ihre Sicherung und Ancestry geprüft sind.

## Prüfungen und Gate

Runde 1: drei eigene Dateien rustfmt und git diff --check grün. Paket-fmt, Clippy, Baseline und Tests sind nicht ausgeführt, weil kein Buildslot frei wurde. Der erste Fixer beendete seine eigene wartende Baseline nach dem Gate-BLOCK. Keine laufende Kompilierung wurde dabei abgebrochen. Fünf Regressionstests sind ergänzt, aber noch nicht ausgeführt. Keine Testzahlen oder Baselinefehler aus dieser fehlenden Ausführung ableiten.

Toolchain, Testdatenbank und cargo-slot wie im Grundbriefing. `rust/test-database.json` fehlt im ersten Fixworktree; nicht anlegen oder Secrets suchen, bestehende erlaubte Testkonventionen benutzen. Reguläre eigene Kompilierung bis zum Ergebnis laufen lassen. Fremde Slots, Prozesse, Dienste und Datenbanken unangetastet lassen. Keine Gate-/Hook- oder Umgebungsänderung zur Umgehung.

Nach minimaler Korrektur eigenen Commit erstellen, Arbeitsbranch sichern erlaubt. Trailer: `Co-authored-by: GPT 6.1 Sol <gpt-6.1-sol@local>`. Kein main-Push, Merge nach main, Release, Deploy oder Restart. Ein Git-Schritt pro Bash-Aufruf, literale absolute Pfade, nur eigene Dateien stagen.

Neuer Gate ausschließlich:

```text
python3 /home/nathanael/Documents/.claude/gpt-workers/gate_hook.py --review --repo /home/nathanael/.worktrees/tb-vollreview-session-widerruf --base origin/main --head HEAD --model gpt-6.1-sol --effort high --timeout 1080
```

Bei erneutem BLOCK oder fehlendem Urteil mit Belegen zurückgeben. Die nächste Runde erhält frischen Kontext. Keine zweite eigene Korrekturrunde im selben Kontext, kein Modellrückfall und kein `--chain`.

## Rückgabe und Grenzen

Status je ursprünglichem Teilbefund, tatsächliche Basis/Head-SHAs, Dateien, Prüfaufrufe und Exit-Codes, Gate-Urteil samt Beleg, Baseline- und verbleibende Prüfgrenzen. Frischer Sol-Kritiker folgt automatisch. Astra dokumentiert; keine Berichtdateien schreiben.

Ausschließlich `gpt-6.1-sol`, keine Delegation, Threads, Sessionnachrichten oder Nutzerfragen. Vor Codesuche code-suche und Graphify. Keine Secrets/ENV-Dateien, Produktionsdatenbank-Aufrufe, Migrationen, echten Kontoaktionen, Browser oder `ai-coach`. Deploy bleibt wegen des ungeklärten Wrapper-Konflikts gesperrt.
