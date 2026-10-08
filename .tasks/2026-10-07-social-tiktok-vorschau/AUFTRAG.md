[Orchestrator]

# TikTok-Freigabe und Vorschau reparieren

status: aktiv, 2026-10-07

## Ziel und Auftrag

Der Nutzer meldete mit Screenshots, dass die TikTok-Veröffentlichung im Social-Media-Dashboard dauerhaft bei „Bitte warte, bis die Videovorschau fertig ist.“ stehen bleibt. Nach belegter Ursachenanalyse lautet die ausdrückliche Freigabe: „Spawne agents die das fixen“.

Du bist Teil-Orchestrator und Integrationsverantwortlicher für diesen einen zusammenhängenden Fix. Starte native Bauagenten mit getrennten Schreibzuständigkeiten für Vorschau/Freigabe und Upload-Warteschlange. Kein zweiter T3-Thread und keine weitere Orchestrierungsebene. Eng gekoppelte Änderungen gemeinsam abnehmen und zusammen veröffentlichen, nie getrennte Deploys. Du hältst Integration, Gate, Deploy und Belege. Astra und Fable dürfen nur delegieren, nicht selbst bauen; Sonnet ist verboten. Modelle aus der freigegebenen Pyramide, keine ungefragten kostenpflichtigen Wechsel.

## Eigentum und Ausgangsstand

- Repo und eigener Worktree: `/home/nathanael/.worktrees/tb-tiktok-preview-freigabe-20261007`.
- Branch: `fix/tiktok-preview-freigabe-20261007`.
- Ausgangs-SHA nach frischem fetch: `b0bd68248c3accc1771e938e6166c3a122ac154e`.
- Beim Anlegen war der Worktree sauber. Nur die von der Hauptsession geschriebenen Dateien in `.tasks/2026-10-07-social-tiktok-vorschau/` kommen hinzu.
- Der geteilte Checkout `/home/nathanael/repos/Deadlock-Twitch-Bot` liegt zurück und enthält fremde Änderungen. Ihn niemals umstellen, bereinigen, stashen oder daraus bauen. Fremde Branches, Worktrees und Threads bleiben unverändert.
- Schreibbereich: bestehende Social-Media-Vorschau, TikTok-Freigabe, betroffene Upload-/Approval-Pfade sowie ihre bestehenden UI-Komponenten und Prüfungen. Keine VOD-Archiv-Änderungen, kein OAuth-Umbau, keine zusätzlichen Plattformen oder allgemeine UI-Überarbeitung.
- Du darfst eigene Änderungen nach den bestehenden Prüf- und Gate-Regeln committen, pushen, nach main integrieren, deployen und die betroffenen Dienste neu starten. Keine PRs oder GitHub-Actions-Aufträge, keine Hook-Umgehung.
- Die Hauptsession schreibt ausschließlich das zentrale REGISTER.md. Deine Unteragenten und deren Dateien hältst du in BEREICHSREGISTER.md fest. REVIEW.md und ABSCHLUSS.md gehören dir. Kein konkurrierendes Schreiben in REGISTER.md.

## Bereits belegter Bestand

Vollständiger Erstbefund: `/home/nathanael/repos/Deadlock-Twitch-Bot/.tasks/2026-10-07-social-tiktok-vorschau/BEFUND.md`. Lesen und als eigene Aktenkopie übernehmen, damit der Befund mit dem Fix gesichert wird. Keine erneute breite Bestandsaufnahme erforderlich; aktuelle Zustände und betroffene Verträge gezielt bestätigen. Vor Codesuche Skill code-suche und Graphify verwenden.

Prüfung am 07.10.2026 gegen 23:30 Uhr, produktiver Commit `2ead4d556327596fcc7d9feeaceb848e910cf8f8`:

- Bot und Dashboard waren aktiv; `/proc/<pid>/exe` zeigte beide auf das genannte Release.
- Clip 124768, „BABA NOOO why not Soloman“: approved, preview_status/path/error/updated_at sämtlich NULL, tiktok_post_options NULL. Kein Renderauftrag läuft für diesen Clip.
- YouTube-Auftrag 5 erfolgreich am 07.10. um 18:02 Uhr deutscher Zeit. TikTok-Auftrag 6 scheiterte um 18:00 Uhr an fehlenden Veröffentlichungseinstellungen.
- Gleichnamiger Clip 124767 für 08.10. um 18:00 Uhr: ebenfalls weder Vorschau noch TikTok-Freigabe.
- Beim Konto earlysalty: 26 pending-TikTok-Aufträge, alle ohne tiktok_post_options. Vier failed-Aufträge, nicht alle aus diesem Vorfall.
- Diese Bestandsdaten sind Diagnose, keine Erlaubnis zum Veröffentlichen oder zur Zustimmung anstelle des Nutzers.

Fundstellen im produktiven Commit:

- `bot/dashboard_v2/src/components/socialmedia/TikTokPostDialog.tsx:23,77`: Öffnen und „Erneut laden“ rufen nur fetchTikTokCreatorInfo auf, nie den Renderauftrag.
- `rust/crates/tb-dashboard-api/src/handlers/social_media_tiktok_direct.rs:86-98`: duration verlangt PREVIEW_READY, behandelt aber NULL, pending, rendering und error mit derselben Wartemeldung.
- `rust/crates/tb-social-media/src/preview.rs:33,129`: bestehender request_preview-Pfad und Worker, der pending oder veraltete rendering-Aufträge übernimmt. NULL startet nichts.
- `bot/dashboard_v2/src/pages/SocialMedia.tsx:2308,2611`: vorhandener separater Menüpunkt „Vorschau rendern“ samt Statusabfrage.
- `rust/crates/tb-social-media/src/upload_worker.rs:967-981`: TikTok verlangt gespeicherte Optionen mit ausdrücklicher Zustimmung, sonst lokale Validierung vor dem Upload.
- In den drei Kernstellen TikTokPostDialog.tsx, social_media_tiktok_direct.rs und preview.rs gab es zum Ausgangs-origin/main keinen neueren Fix.

BESTAND[BS-1]: ja | Fundort: rust/crates/tb-social-media/src/preview.rs:33 | Anknüpfung: bestehenden Renderauftrag, Statusabfrage und TikTok-Freigabe reparieren.

## Verbindlicher Sollzustand

1. Der TikTok-Freigabeablauf kommt ohne versteckten Umweg durchs Clipmenü vom Clip ohne Vorschau zu einer fertigen Vorschau und bedienbaren Veröffentlichungseinstellungen. Den bestehenden Renderweg benutzen, keine parallele Pipeline.
2. Zustände fehlend, angefordert, läuft, fertig und fehlgeschlagen unterscheiden. Kein endloses Warten ohne Auftrag. Echte Renderfehler zeigen einen verständlichen Wiederholungsweg. Polling endet bei Fertigstellung/Fehler und beim Schließen; keine Neuanforderung bei jedem Poll oder Render.
3. Fertige gültige Vorschauen wiederverwenden; parallele Klicks/Fenster, veraltete Artefakte, Neustart und erneutes Öffnen sicher behandeln. GET-Abfragen nicht beiläufig zu ungeschützten Schreibwegen machen.
4. TikTok-Einstellungen und Zustimmung werden weiter ausdrücklich vom Nutzer gewählt. Keine vorausgefüllte Zustimmung, kein erfundenes Einverständnis, kein automatisches Veröffentlichen der 26 Altaufträge. Videodigest-/Konto-/Scope-Prüfungen bleiben erhalten.
5. Alte oder automatisch erzeugte TikTok-Aufträge ohne notwendige Freigabe nachvollziehbar zur Freigabe führen. Nicht weiterhin bis zum Termin als bereit wirken und dann immer wieder scheitern. Ursache im schreibenden/ausführenden Pfad korrigieren, nicht per periodischem Datenbank-Reparaturskript oder manuellem SQL-Zurechtbiegen.
6. Bereits erfolgreiche YouTube-Veröffentlichungen bleiben erfolgreich und dürfen beim Nachholen der TikTok-Freigabe nicht wiederholt oder umgeplant werden. Andere Plattformen dürfen nicht durch fehlende TikTok-Freigabe blockiert werden.
7. Bestehende Termine nach Möglichkeit erhalten; verstrichene Termine nicht ohne nachvollziehbaren bestehenden Vertrag sofort veröffentlichen. Bei einer tatsächlich offenen Produktentscheidung kurze Empfehlung an Hauptorchestrator statt eigenmächtiger Außenwirkung.
8. Knappe, natürliche deutsche Nutzertexte mit echten Umlauten. Kein Rohtext „validation failed“ als Bedienhilfe, keine Gedankenstriche, keine Code-Kommentare. Keine neuen Python-Anwendungs- oder Servicepfade; Backend und dauerhafte Logik in Rust, bestehende Dashboard-Komponenten gezielt anpassen.

## Beweis und Abschluss

- Graphify zuerst, gezielte aktuelle Code-/DB-/Laufzeitprüfung danach. Secrets niemals lesen/ausgeben oder in Dateien schreiben, kein /proc/*/environ, keine ENV-Dateien. Datenbankabfragen auf nötige Spalten begrenzen.
- Browser ausschließlich Moli `/home/nathanael/.local/bin/moli`. Vorher `/home/nathanael/Documents/claude-config/wissen/agent-browser.md` lesen und allen nativen Agenten weitergeben. Brave MUST NOT gestartet, übernommen oder als Rückfall benutzt werden. Persönlichen Browser und fremde Dienste nicht anfassen.
- Passende Formatter-, Compiler-, Linter- und vorhandene Tests durchführen, vorbestehende Fehler separat nachweisen. Neue Tests sind nicht pauschal Pflicht; die tatsächlichen Fehlerzustände müssen nachvollziehbar geprüft sein. Rust über cargo-slot mit --jobs 3, höchstens ein Release-Build, keine selbstgebauten Sperrschleifen.
- Frontend-Wirkung im Browser zeigen: fehlende Vorschau startet, laufende Vorschau bleibt ein Auftrag, fertig öffnet Auswahl, Fehler hat einen funktionierenden Wiederholungsweg. Für kontrollierte Fehlerfälle isolierte Fixtures/Wegwerfkonten nutzen.
- Live-Beweis ohne echte Social-Media-Veröffentlichung und ohne Ändern von Nutzerzustimmungen. Einen notwendigen Funktionsbeweis darfst du am bestehenden authentisierten Vorbereitungsweg machen, nicht durch ungefragtes Posten. Keine Tokenrotation, kein Trennen echter Konten.
- Gemeinsamer integrierter SHA: unabhängige Intent-Abnahme nach aktuellem ABLAUF.md, danach ausschließlich zentraler Merge-Gate für Bug-/Security-Review. Kein eigener paralleler Code-Review-Thread. Bei BLOCK frischer nativer Fixer je Runde, Selbstprüfung inklusive Gate, höchstens fünf erfolglose Runden bis Eskalation.
- Vor Deploy Herkunft der laufenden Binaries prüfen. Release nur im eigenen Worktree bauen. Über vorhandenen Deploy-Wrapper aktuellen origin/main-SHA deployen, betroffene System-Units Bot/Dashboard neu starten und Release-SHA, aktive Prozesse und Funktion belegen. Keine fremden Builds/Deploys per Chat koordinieren.
- Branch/Worktree erst nach Merge, Push und Live-Beweis entfernen; vor Branchlöschung merge-base --is-ancestor samt Exit prüfen. Eigene Akte und Beweise vor Cleanup im Fachrepo sichern. Niemals große Videos/Binaries committen.

## Routing und Rückmeldung

Auftraggeber ist die Hauptsession `d71ef3f0-d1c3-420d-948c-320ff0cc9670` im T3-Nutzerauftrag zur TikTok-Vorschau. Paket T, Versuch 1, benannter Produzent: dieser Teil-Orchestrator. Status und kurze Entscheidungen in `.tasks/2026-10-07-social-tiktok-vorschau/`, Hauptsession liest per t3-thread.py read; kein ListAgents oder SendMessage an fremde Sessions. Eigene native Agentenergebnisse bleiben bei dir.

Melde nur echten Blocker oder fertigen Stand mit SHA, Prüfungen, Gate-Urteil, Deploy-/Live-Beleg, Cleanup und verbleibenden Nutzeraktionen. Keine Frage an Nutzer via AskUserQuestion. Falls nötig: `FRAGE AN ORCHESTRATOR:` mit Lage, Empfehlung und Arbeitsstand. Keine Wiederbelebung fremder oder alter Threads. Nach verifiziertem Abschluss und Schlussbericht als letzten Schritt ausschließlich deinen eigenen Thread mit `t3-thread.py settle --selbst` abschließen.

BRIEFING[WB-1]: Pflichtteile 5/5 | Timer 20 min | Worktree: /home/nathanael/.worktrees/tb-tiktok-preview-freigabe-20261007
ORCHESTRIERUNG[OR-1]: Stufe mittel | Schritt dispatch | Artefakt: .tasks/2026-10-07-social-tiktok-vorschau/AUFTRAG.md
