[Orchestrator]

# Discord-Kontaktköder zuverlässig erkennen

status: erledigt
Datum: 2026-10-08
Abschluss: Release 3341098f live geprüft; Belege in status/M/1/004-deployment.md, Ressourcen- und Threadabschluss im REGISTER.md.
Stufe: mittel
Paket: M
Versuch: 1
Auftraggeber: Hauptsession db0eedc0-967c-4dcc-a0ae-32c550f54753

## Nutzerziel und Grenze

Der Nutzer hat nach der Prüfung eines Chat-Screenshots ausdrücklich beauftragt: „okay dann nschau das es erkannt wird“.

Der Wortlaut aus dem Screenshot lautet:

> Richtig nice Stream! Hab direkt gefollowt. Komme nächstes Mal gerne mit Freunden wieder - Disscord: united_247

„Disscord“ hat zwei s. Der Nutzername im Screenshot ist für die Erkennung unerheblich und darf weder in Regeln noch in Sperrlisten eingebaut werden. Ein Kanal, Kontoverlauf und Kontoalter sind unbekannt.

Behebe die belegte Erkennungslücke im vorhandenen Rust-Moderationspfad. Die konkrete Kombination aus pauschalem Lob, Follow-/Wiederkommen-Versprechen und ungefragtem Discord-Kontakt muss bei einem neuen, nicht geschützten Chatter nachvollziehbar erkannt werden. Nur nachzuweisen, dass irgendein KI-Aufruf möglich wäre, erfüllt den Auftrag nicht. Erkenne die Masche allgemein, nicht den einen Volltext oder Kontaktnamen. Bewahre die vorhandenen Schutzrollen, Vertrauensprüfungen, Kanaleinstellungen und die Regeln für Löschung, Warnung, Timeout und Ban. Ein Discord-Hinweis oder lockeres Deutsch allein ist kein Sperrgrund.

Die Sprachannahme im Gesprächswächter muss korrigiert werden: Natürliches Deutsch darf kein kategorischer Freibrief sein. Echte Gesprächsbezüge, beantwortete Kontaktfragen, normale Community-Einladungen und etablierte Chatter bleiben geschützt. Keine neue Moderationsarchitektur, kein zweiter KI-Connector, kein Modellwechsel, keine neue öffentliche Route und keine Datenbank-Handkorrektur.

## Belegter Ausgangspunkt

Die vorausgehende read-only-Prüfung wurde unabhängig gegengeprüft. Der damals laufende Bot hatte Release b0bd68248c3accc1771e938e6166c3a122ac154e. Der geteilte Checkout stand auf einem älteren Commit und enthielt fremde Änderungen. Nicht dort arbeiten. Prüfe die aktuellen Dateien und den Live-Stand erneut; die Fundstellen unten beziehen sich auf den genannten Release.

- rust/crates/tb-chat/src/spam_filter.rs:759 und :815: feste Regeln ergeben beim Screenshot Score 0, sofern keine gelernten Datenbankmuster passen. Erstnachricht und Kontoalter allein machen daraus kein hartes Spam-Signal.
- rust/crates/tb-chat/src/scam_pitch.rs:305, :345, :354 und :995: nur generic_praise(1); die exakten Discord-Muster übersehen „Disscord“. Der Kontaktname ohne @ trifft die allgemeine Handle-Regel nicht. Einzelne Nachricht bekommt keine Aktion.
- rust/crates/tb-chat/src/conversation_scam.rs:26: Prompt verlangt bei flüssigem Umgangsdeutsch clean oder höchstens unsure. Diese absolute Entwarnung ist fachlich nicht haltbar.
- conversation_scam.rs:257, :329, :337, :358 und :941: 110 Zeichen reichen bei einem neuen, nicht ausgenommenen Chatter für eine sofortige Gesprächsprüfung. Ob die KI dieses Beispiel tatsächlich als Scam wertet, wurde nicht getestet.
- rust/crates/tb-chat/src/pipeline.rs:898: Gesprächswächter ist ein eigener Pfad; Spam-AutoBan ist nicht sein Einstellungsschalter.
- rust/crates/tb-chat/src/sus_invite.rs:32: prüft discord.gg-Links, keine Kontaktnamen.

Vor jeder weiteren Code-Suche Skill code-suche laden und Graphify benutzen. Die vorhandenen Normalisierungs-, Scoring-, Test- und Moderationsbausteine wiederverwenden. Tippfehler-Normalisierung eng begrenzen; kein globales Zusammenziehen von Buchstaben in Nutzernamen oder normalem Text.

## Eigentum und Arbeitsstand

Rolle: ausführender Worker für genau dieses Paket, zugleich Integrationsverantwortlicher. Keine weiteren T3-Threads oder zusätzliche Orchestratorebene. Native frische Fixer sind ausschließlich für die vorgeschriebene Gate-Fixschleife erlaubt. Keine fremden Sessions koordinieren oder verändern.

Worktree: /home/nathanael/.worktrees/tb-moderation-disscord-erkennen
Branch: fix/moderation-disscord-erkennen
Ausgangs-HEAD: 6937e4a6, frisch von origin/main am 2026-10-08 angelegt.
Fach-Repo: /home/nathanael/repos/Deadlock-Twitch-Bot

Eigene Schreibbereiche:
- rust/crates/tb-chat/src/scam_pitch.rs
- rust/crates/tb-chat/src/conversation_scam.rs
- rust/crates/tb-chat/src/spam_filter.rs
- bei notwendiger Verdrahtung rust/crates/tb-chat/src/pipeline.rs und rust/crates/tb-chat/src/lib.rs
- unmittelbar zugehörige vorhandene tb-chat-Testdateien und bei Bedarf ein kleiner geteilter Rust-Helfer innerhalb tb-chat
- .tasks/2026-10-08-moderation-disscord-erkennen/REVIEW.md, HANDOFF.md und status/M/1/

AUFTRAG.md und REGISTER.md schreibt allein die Hauptsession. Diese beiden Dateien gehören zum eigenen Task-Commit und dürfen mitgenommen werden. Keine globalen Formatänderungen, keine nebenbei gefundenen Umbauten, keine fremden uncommitteten Änderungen anfassen. Keine Code-Kommentare hinzufügen. Produktivcode ausschließlich Rust. Keine Secrets lesen, ausgeben oder in Dateien ablegen; keine ENV-Dateien oder neue ENV-Konfiguration.

## Beweisziel und Abschluss

1. Prüfe die aktuelle Ursache und korrigiere den vorhandenen Erkennungspfad sowie die kategorische Deutsch-Entwarnung.
2. Führe den Screenshot-Wortlaut durch die tatsächlichen Rust-Erkennungsfunktionen und den relevanten Aufrufer. Zeige vorher/nachher beziehungsweise den konkreten Treffer und die daraus folgende vorhandene Entscheidung. Keine Assertion nur auf einen künstlichen Helper ohne Aufrufer.
3. Prüfe harmlose Gegenbeispiele: normales Lob ohne Kontakt, ein erfragter Kontaktname, echte Discord-Community-Einladung mit Spielbezug und dieselbe Schreibweise in einem normalen Gespräch. Prüfe die bestehenden Vertrauens- und Rollen-Ausnahmen sowie den Schalterzustand. Das ist der sachliche Abnahmenachweis dieses Fixes, keine allgemeine Pflicht zu neuen Tests für jeden Auftrag.
4. Passende bestehende Rust-Prüfungen, Format und Clippy ausführen. Skill rolle-test-waechter und die Repo-Testregeln beachten. Compiler-Prüfungen über cargo-slot mit --jobs 3; Release nur im eigenen Worktree. Fehler selbst beheben, vorbestehende rote Befunde getrennt nachweisen. Keine eigenen Build-Warteschleifen oder fremden Prozesse stoppen.
5. Keine tatsächlichen Chat-Nachrichten, Löschungen, Timeouts oder Bans an echten Konten als Test auslösen. Keine Community-Daten an externe Modelle schicken. Für den Funktionstest lokale bestehende Replays, Mocks oder isolierte Testdaten verwenden. Behaupte kein reales KI-Urteil, wenn nur ein Stub verwendet wurde.
6. Einziger Code-Reviewer ist der lokale Merge-Gate. gate_hook.py --review prüfen lassen, Funde im REVIEW.md. Bei BLOCK pro Runde frischen nativen Fixer einsetzen; derselbe Gate-Reviewer bleibt maßgeblich. Bis ALLOW autonom korrigieren. Keine separaten Review-T3-Threads und kein Umgehen von Schutz-Hooks. Nach fünf erfolglosen Runden als echter Blocker melden.
7. Nach ALLOW den eigenen Fix nach main mergen, HEAD:main pushen und den aktuellen origin/main-Stand über den vorhandenen Deploy-Wrapper deployen. Vorher laufenden Release belegen; hinterher Bot neu starten und Prozess/Release sowie die wirksame Erkennung nachweisen. Deploy- und Test-Skills nutzen. Keine PRs, keine GitHub-Actions-Aufträge.
8. Git-Schritte einzeln pro Bash-Aufruf mit absoluten literalen Pfaden. Nur eigene Dateien adden. Keine fremden Branches oder Worktrees verändern. Den veralteten schmutzigen Hauptcheckout nicht umstellen. Wenn nötig den dokumentierten sauberen Merge-/Clone-Weg nutzen. Branch nur nach merge-base-Nachweis und geprüftem Exit-Code löschen. Wertvolle ignorierte Artefakte vor Worktree-Cleanup prüfen.
9. Melde kurz: Ursache, behobene Wirkung, Prüfungen mit Exit/Testzahlen, Gate, Merge-SHA, laufender Release-SHA, ehrlicher Funktionsnachweis und Cleanup. Pflichtzeile MERGEPROTOKOLL[MS-1] mitliefern. Erst nach vollständigem Abschluss als letzten Schritt den eigenen T3-Thread mit t3-thread.py settle --selbst abschließen.

## Rückfragen und Status

Die Hauptsession bleibt verantwortlich und prüft etwa alle 20 Minuten den eigenen Worker. Keine Frage direkt an den Nutzer. Was Auftrag und Bestand beantworten, selbst entscheiden. Ein unvermeidbarer Architektur-/Produktwechsel, eine Geldwirkung oder eine echte Schutzgrenze geht als FRAGE AN ORCHESTRATOR an die Hauptsession mit Empfehlung, Worktree, Branch und SHA. Keine Zwischenberichte je Fixrunde.

Produzent der Paketereignisse: Worker M, Versuch 1. Kurze unveränderliche Statusdateien unter status/M/1/; TODO.md nicht selbst verwalten. Fachlicher Endbericht im T3-Thread, Belege im Task-Verzeichnis. Bei Ausfall HANDOFF.md mit Stand und Fortsetzung schreiben; Arbeit nicht neu beginnen.

Browser ist für den Auftrag nicht nötig. Falls doch: zuerst /home/nathanael/Documents/claude-config/wissen/agent-browser.md lesen, ausschließlich Moli verwenden. Brave und der persönliche Browser bleiben unangetastet.
