## #161 — EventSub 403-Spam gestoppt (xoralle, berserkkoo)

**Ausgangslage:** Für Kanäle, bei denen der Bot-Account gebannt wurde oder der Streamer externe EventSub-Subscriptions gesperrt hat, schlägt Twitch mit HTTP 403 zurück. Der Reconciler lief alle 30 Minuten durch alle Partner-Kanäle und versuchte es jedes Mal erneut — mit demselben Ergebnis. Das erzeugte konstante Warn-Logs und löste stündlich Discord-Alerts aus, obwohl kein Code-Bug vorlag.

**Was wurde geändert:** Der `SubscriptionManager` führt jetzt ein In-Memory-Set `perm_failed`. Beim ersten 403 wird der betroffene `(sub_type, broadcaster_id)`-Schlüssel dort eingetragen und eine einmalige Warn-Meldung geloggt ("Bot gebannt oder Kanal gesperrt — kein weiterer Retry bis Neustart"). Alle folgenden Reconcile-Läufe skippen diesen Eintrag lautlos auf Debug-Level. Beim Bot-Neustart wird einmal erneut versucht — bleibt das 403, landet der Kanal wieder im Set.

**Jetzt:** xoralle und berserkkoo werden beim nächsten Reconcile-Lauf (spätestens in 30 min) ein letztes Mal mit Warn-Log quittiert und danach aus dem Alert-Radar verschwinden, solange der Bot-Account in diesen Kanälen gebannt bleibt.

## #160 — Chat-Bot komplett auf Rust umgestellt (Welle B: Cutover vollzogen)

**Ausgangslage:** Der Twitch-Chat (Moderation, Spam-Filter, Commands, Promos, Chatter-Tracking) lief als letzter großer Brocken noch im Python-Worker — inklusive der Hoheit über das Bot-Token. Der Rust-Bot bediente bereits Monitoring, Raids und die interne API, musste für jede Chat-Aktion aber den Umweg über Python nehmen.

**Was wurde geändert:**

- Die komplette Chat-Verarbeitung läuft jetzt nativ im Rust-Bot. Statt einer dauerhaften WebSocket-Verbindung wie im alten Bot abonniert der neue Bot Chat-Nachrichten als Webhook-Events über die bestehende EventSub-Strecke — „einem Kanal beitreten" heißt jetzt: eine Subscription anlegen. Beim Umschalten wurden 46 Partner-Kanäle abonniert und alle Subscriptions von Twitch bestätigt.
- Die Nachrichten-Pipeline arbeitet jede eingehende Nachricht in derselben festen Reihenfolge ab wie vorher: eigene Nachrichten verwerfen → bekannte Service-Bots nur zählen → Kanal einordnen (Partner / nur beobachtet / Deadlock live?) → globale Bannliste prüfen → Scam-Anschreiben erkennen → Spam bewerten → verdächtige Discord-Links flaggen → Spaß-Antworten → Chatter zählen → Promos → Commands.
- Der Spam-Filter eskaliert exakt wie bisher: Konto jünger als 90 Tage und Erstnachricht zählen nur dann als Verdachtspunkte, wenn bereits ein hartes Signal (bekannte Spam-Domain oder -Marke) vorliegt — eine harmlose erste Nachricht eines neuen Zuschauers kann also weiterhin nicht zum Bann führen. Ab 3 Punkten wird gebannt, bei hartem Signal darunter nur gelöscht.
- Bans, Timeouts und Lösch-Aktionen laufen über einen einzigen gemeinsamen Pfad — egal ob Spam-Filter, globale Bannliste oder Sweep sie auslösen. Dadurch greifen Chat-Hinweis (sofern nicht per !silentban stumm geschaltet), Review-Protokoll und Discord-Alert an die Moderatoren überall identisch.
- Das Chatter-Tracking schreibt weiter dieselben Daten (wer chattet wann in welcher Session, Erstkontakt-Erkennung, Roh-Nachrichten für die Analytics) — inklusive des Gesundheits-Heartbeats, den das Admin-Dashboard zur Überwachung der Chat-Erfassung liest.
- Die Bot-Token-Verwaltung ist mit umgezogen: Der Rust-Bot validiert das Token beim Start, erneuert es bei Bedarf sofort über das Refresh-Token und hält es im 30-Minuten-Takt frisch. Der Wechsel passierte atomar — erst Python-Chat aus, dann Rust-Chat an — damit sich nie zwei Prozesse um dasselbe Token streiten.
- Die Begrüßung nach einer Partner-Autorisierung sendet jetzt direkt der Rust-Bot statt über den Python-Umweg.

**Wie es jetzt funktioniert:** Twitch liefert jede Chat-Nachricht als signiertes Webhook-Event, der Bot prüft die Signatur, arbeitet die Pipeline ab und handelt bei Treffern sofort (löschen, bannen, warnen, antworten). Alle 30 Minuten gleicht er die Kanal-Abonnements mit der Partnerliste ab — neue Partner werden so automatisch „betreten". Kanäle, die den Bot nicht autorisiert haben, werden sauber übersprungen statt Fehler zu werfen. Für Streamer und Zuschauer ändert sich am Verhalten nichts — Commands, Moderation und Promos funktionieren wie gewohnt, nur schneller und aus einem Guss.

**Bewusst noch beim Alten:** !title (Titel-Generator) und die Engagement-Funktionen folgen in einer eigenen Phase.

## #159 — Sicherheitsseite: Responsible Disclosure, Meldeformular + Opus-Analyse

**Ausgangslage:** Die `/twitch/sicherheit`-Seite endete mit einem knappen Einzeiler „Hinweise willkommen, bitte vertraulich melden". Kein klarer Rahmen, wer was darf, und kein direkter Meldeweg — wer eine Lücke gefunden hat, musste selbst herausfinden, wie er sie loswird.

**Was wurde geändert:** Zwei Ergänzungen in einem Zug.

Erstens ein vollständiger *Security Testing & Responsible Disclosure*-Abschnitt: White-Hat-Testing ist jetzt ausdrücklich erlaubt. Die Regeln stehen klar da — kein Schaden, keine Datenexfiltration, kein Social Engineering gegen Nutzer, dafür Pflicht zu einem reproduzierbaren Report. Wer sich daran hält, hat keine Konsequenzen zu befürchten; DoS, Backdoors und echte Exfiltration fallen ausdrücklich nicht darunter.

Zweitens ein eingebettetes Meldeformular direkt auf der Seite: Kurztitel, detaillierter Reproduktionsweg (Pflichtfeld, mindestens 100 Zeichen), optionaler Kontakt. Die Hinweisbox macht explizit klar, dass nur echte, selbst geprüfte Lücken erwartet werden — keine KI-Halluzinationen, keine Vermutungen ohne Eigenprüfung. Nach dem Absenden sendet der Rust-Handler (`tb-dashboard`, Port 8769) direkt über die Discord-HTTP-API eine DM an den Bot-Eigentümer — kein Umweg über Python-Cogs, kein Zwischen-Service.

Drittens ein lokaler Opus-Analyse-Flow: Sobald die Eingangs-DM raus ist, startet `tb-dashboard` im Hintergrund einen `tokio::spawn`-Task, der via `spawn_blocking` das Claude-CLI (`claude -p --model opus --dangerously-skip-permissions`) headless aufruft. Opus liest den Report, prüft den relevanten Code im Repo auf die beschriebene Lücke, bewertet Schweregrad und Echtheit, und committet einen Fix falls sicher möglich. Das Ergebnis kommt als zweite Discord-DM mit dem vollständigen Analysebericht. Die HTTP-Response geht sofort zurück — der Nutzer wartet nicht auf Opus.

## #158 — Spam-Bot-Bans werden jetzt im Live-Ban-Feed sichtbar

**Ausgangslage:** Die „Spam-Schutz in Echtzeit"-Sektion zog ihre Zahlen und den Feed aus einer Tabelle, die nur Bans erfasst, die über Twitch-Benachrichtigungen (EventSub) reinkommen — und die sind nur für sehr wenige Kanäle aktiv. Die eigentliche Kernleistung, das automatische Bannen von Viewer-Bot- und Spam-Accounts im Chat, wurde dort gar nicht protokolliert (nur in eine interne Logdatei). Deshalb zeigte der Feed praktisch nichts und „Bans heute" stand auf 0, obwohl der Bot laufend Spam-Bots entfernt.

**Was wurde geändert:** Jedes Mal, wenn die Auto-Moderation einen Account wegen Spam bannt, wird dieser Ban jetzt zusätzlich in der Statistik-Tabelle protokolliert — und die gesamte Historie wurde aus der vorhandenen Moderations-Logdatei rückwirkend nachgetragen. Der öffentliche Feed filtert außerdem sauber auf echte Bans (frühere Entsperrungen tauchen nicht mehr fälschlich als „Ban" auf).

**Wie es jetzt funktioniert:** Sobald der Bot eine Spam-Nachricht erkennt und den Account bannt, schreibt er nach dem erfolgreichen Bann einen Eintrag mit Account-Name, Spam-Inhalt und Zeitpunkt in die Ban-Ereignis-Tabelle — als zusätzlicher Schritt, der den eigentlichen Bann-Vorgang nie blockiert (schlägt das Protokollieren fehl, läuft die Moderation normal weiter). Der Bot hat seine Auto-Bans schon immer in einer Logdatei mitgeschrieben; daraus wurden die zurückliegenden Bans einmalig in die Statistik-Tabelle übernommen (idempotent, ohne Duplikate), sodass der Feed sofort die echte Historie zeigt statt erst über die Zeit zu füllen. Die öffentliche Statistik-Abfrage zählt und zeigt nur noch Einträge vom Typ „Ban". Im Feed steht jetzt der tatsächliche Spam-Text der gebannten Bots (z. B. die typischen „billige Viewer"-Werbungen). Zusammen mit der bereits korrigierten Zahl der geschützten Kanäle spiegelt die Sektion damit die tatsächliche Schutzleistung wider — aktuell rund 50 entfernte Spam-Bots in den letzten 30 Tagen über alle Partner-Kanäle.

## #157 — Streamer-Seite: Sicherheits-Sektion, Discord-Beitritt, Demo-Vorschau repariert, Ban-Zahl korrigiert

**Ausgangslage:** Auf der Streamer-Landingpage fehlte ein sichtbarer Hinweis darauf, wie sicher der Bot mit den Mod-Rechten umgeht. Einen echten Discord-Beitritts-Button gab es nirgends — die Community-Sektion erwähnte Discord nur. Die eingebettete Dashboard-Demo lud nicht (leerer, kaputter Rahmen), und die „Spam-Schutz"-Sektion zeigte unglaubwürdige Zahlen, u. a. „1 geschützter Kanal", obwohl der Bot viele Partner moderiert.

**Was wurde geändert:** Die Streamer-Seite hat eine neue Sicherheits-Sektion bekommen, die kurz und ehrlich zeigt, was der Bot darf und was nicht — mit Verweis auf das vollständige Sicherheitskonzept. Discord ist jetzt an zwei prominenten Stellen direkt beitretbar (eigener Block in der Community-Sektion und Button im Schluss-Aufruf). Die Demo-Vorschau wurde repariert, die Navigationsleiste überarbeitet, und die Zahl der geschützten Kanäle zeigt jetzt den echten Wert.

**Wie es jetzt funktioniert:** Die Demo lud nicht, weil die Sicherheitsrichtlinie der Seite das Einbetten der Demo-Adresse blockierte — die Freigabe für die eigene Domain hat gefehlt; sie ist jetzt ergänzt, die Vorschau lädt wieder. Die „geschützte Kanäle"-Zahl wurde bisher falsch aus den protokollierten Ban-Ereignissen abgeleitet (daher 1); sie kommt jetzt aus der Zahl der aktiv betreuten Partner-Kanäle (aktuell 47) — das ist die korrekte Bedeutung, denn der Bot schützt jeden Partner-Kanal, unabhängig davon, ob dort zuletzt etwas gebannt wurde. Diese öffentliche Statistik-Abfrage wird seit diesem Schritt nativ von der Rust-Schicht beantwortet. In der Navigationsleiste überlappten sich bei mittlerer Fensterbreite Logo, Menü und Buttons; sie klappt jetzt früher ins kompakte Menü und hat einen klareren Demo-Button.

## #156 — Stats-Leaderboard Slow-Query-Fix

**Problem:** Das Stats-Leaderboard im Admin-Dashboard brauchte bei jedem Aufruf 1,3–3,2 Sekunden statt unter 100 ms. Betraf alle drei Teilabfragen (Top-Streamer-Ranking, Stunden-Verteilung, Wochentag-Verteilung) gleichzeitig — also 9 langsame Queries pro Dashboard-Besuch.

**Ursache:** Die Abfragen liefen über ein `UNION ALL`-CTE-Muster (`WITH source_rows AS (SELECT ... FROM twitch_stats_tracked UNION ALL SELECT ... FROM twitch_stats_category)`). TimescaleDB erkennt in einem CTE keine echten Hypertable-Referenzen — es kann deshalb die 30-Tage-Grenze nicht für Chunk-Exclusion nutzen und scannt alle Chunks beider Tabellen komplett durch. Mit ~3500–3560 Ergebniszeilen und wachsendem Datenbestand wurde das zunehmend langsamer.

**Fix:** UNION ALL CTE entfernt. Jede Abfrage fragt jetzt direkt die zuständige Tabelle an (`twitch_stats_tracked` für Partner-Daten, `twitch_stats_category` für den Rest). TimescaleDB kann so den Zeitraum-Filter auf die relevanten Chunks (ca. 5 Stück bei 7-Tage-Interval) beschränken statt alle zu scannen. Zusätzlich neue DB-Migration mit `LOWER(streamer)`-Indizes auf beiden Tabellen, die beim nächsten Start automatisch angewendet wird.

## #155 — Streamer-Analytics repariert + drei Analytics-Routen nativ in Rust

**Ausgangslage:** Die Analytics-Ansicht im Streamer-Dashboard war für jeden Streamer mit Daten kaputt — und zwar schon seit der Umstellung der Datenbank auf Postgres: Die zentrale Session-Abfrage nutzte eine SQLite-Funktion (`TIME(...)`), die Postgres nicht kennt. Jede Anfrage endete mit „Internal error", das Dashboard zeigte dauerhaft „keine Daten". Zusätzlich standen drei interne Lese-Routen (/stats, Streamer-Analytics, Session-Detail) noch auf dem Python-Umweg, weil frühere Rust-Versuche die Antwort-Struktur nicht exakt trafen.

**Was wurde geändert:** Alle drei Routen sind jetzt shape-genau nativ in Rust — gebaut aus zeilengenauen Verträgen gegen den Python-Quelltext und gegen die laufende Python-API mit echten Produktionsdaten verglichen. Die Session-Detail-Route liest wie Python dynamisch alle Spalten (künftige Felder kommen automatisch mit). Die Stats-Route liefert exakt die Python-Sektionen inklusive Monetization (Werbepausen mit Viewer-Drop-Berechnung, Hype Trains, Bits, Subs). Beim Vergleich flogen zwei weitere schlafende Python-Bugs auf: Die Sub-Geschenk-Zählung verglich ein Ja/Nein-Feld mit einer Zahl (schlug still fehl, Subs waren immer 0 — in Rust korrekt umgesetzt), und der Verifikations-Status der Partner wurde in einem früheren Rust-Entwurf durch einen Typ-Fehler still verschluckt (alle Partner erschienen als unverifiziert — vor dem Release gefixt).

**Wie es jetzt funktioniert:** Streamer öffnen ihr Dashboard → die Analytics laden wieder echte Daten (Sessions, Retention, Chat-Aktivität, Vergleich mit ähnlichen Kanälen). Verifiziert wurde mit echten Produktions-Sessions: Session-Details sind feldgenau identisch zur Python-Antwort, die Stats-Aggregate ebenso; nur die Live-Zähler unterscheiden sich um die Sekunden zwischen zwei Abfragen.

## #154 — Sicherheitskonzept: Passwortmanager als Phishing-Schutz

**Ausgangslage:** Der Abschnitt zur menschlichen Schutzebene nannte den Passwortmanager bisher nur als Speicher für lange, einzigartige Passwörter. Sein vielleicht stärkster Effekt fehlte: Er ist selbst ein Phishing-Schutz.

**Was wurde geändert:** Die Seite `/twitch/sicherheit` erklärt jetzt, dass der Passwortmanager Zugangsdaten an die echte Adresse (Domain) bindet und auf gefälschten Seiten deshalb gar nicht erst ausfüllt.

**Wie es jetzt funktioniert:** Ein Passwortmanager merkt sich zu jedem Zugang die echte Domain und füllt das Passwort nur dort automatisch ein. Landet man über einen gefälschten Link auf einer täuschend echt aussehenden Betrugsseite, bleibt das Feld leer — die Adresse ist dem Manager unbekannt. Damit dort überhaupt ein Passwort hineingerät, müsste man es bewusst von Hand eintippen oder die falsche Seite aktiv freigeben. Genau dieses ausbleibende Auto-Ausfüllen wirkt als zweite Warnstufe: Es erzwingt einen aktiven, bewussten Schritt, statt dass die Anmeldung unbemerkt durchläuft.

## #153 — Sicherheitskonzept: Tresor statt .env, Server-Praktiken, menschliche Schutzebene

**Ausgangslage:** Das Sicherheitskonzept beschrieb den Secret-Manager nur knapp und ließ drei Dinge offen, nach denen ein sicherheitsbewusster Nutzer fragt: Wie genau werden Geheimnisse verwahrt (und warum nicht der übliche, riskante Weg über Klartext-Dateien), welche gängigen Server-Schutzpraktiken laufen, und wie ist die menschliche Ebene abgesichert — also die Zugänge des Betreibers selbst, die in der Praxis das häufigste Angriffsziel sind.

**Was wurde geändert:** Drei Bereiche der öffentlichen Seite `/twitch/sicherheit` wurden ausgebaut. Der Abschnitt zu Zugangsdaten erklärt jetzt, was eine .env-Datei ist, warum dieser verbreitete Weg riskant ist und dass wir stattdessen einen verschlüsselten Tresor nutzen. Beim Server kamen gängige Praktiken dazu. Und es gibt einen neuen Abschnitt zur menschlichen Schutzebene. Außerdem wird jetzt ehrlich eingeordnet, warum bei uns die gezielte Feld-Verschlüsselung wichtiger ist als eine pauschale Festplattenverschlüsselung.

**Wie es jetzt funktioniert:** Eine .env-Datei ist eine Textdatei, in der Geheimnisse im Klartext stehen und beim Start als Umgebungsvariablen übergeben werden — riskant, weil sie lesbar auf der Platte liegt, leicht in Backups oder die Versionsverwaltung rutscht und von anderen Prozessen mitlesbar ist. Stattdessen liegen alle Geheimnisse verschlüsselt in einem zentralen Tresor; im Klartext existieren sie nur flüchtig im Arbeitsspeicher des laufenden Dienstes und werden beim Start frisch geholt. Zur Datenbank wird klargestellt: Festplattenverschlüsselung schützt nur die ausgeschaltete Platte vor physischem Diebstahl — gegen einen kopierten Datenbank-Auszug oder ein verlorenes Backup hilft sie nicht. Genau dort greift die Feld-Verschlüsselung, die die sensibelsten Werte einzeln verschlüsselt (Schlüssel außerhalb der Datenbank). Bei den Server-Praktiken werden Dienste mit minimalen Rechten in getrennten Prozessen, durchgehende TLS-Verschlüsselung, automatische Neustarts mit Überlastschutz und regelmäßige Backups genannt — unter dem Leitprinzip Verteidigung in mehreren Schichten. Die neue menschliche Ebene beschreibt: einzigartige, zufällig erzeugte Passwörter im Passwortmanager (das Bot-Konto rund 40 Zeichen), Zwei-Faktor überall, E-Mail-Benachrichtigung bei jedem Login und bewusste Wachsamkeit gegen Social Engineering. Alle Texte synchron in Python-Fallback und Rust-Live-Modul, byte-identisch verifiziert.

## #152 — Sicherheitskonzept vertieft (Token-Verschlüsselung im Detail) und doppelter Checkout-Pfad entfernt

**Ausgangslage:** Das Sicherheitskonzept erklärte die Token-Verschlüsselung nur in einem Satz. Gerade dieser Punkt ist aber zentral, weil der Bot OAuth-Tokens der Streamer hält. Außerdem gab es zwei Code-Pfade, die eine Stripe-Bezahlsession bauen konnten: den echten (über die Bezahlseite, mit Pflicht-Häkchen zur AGB- und Widerrufs-Zustimmung) und eine ungenutzte JSON-Schnittstelle, der genau dieses Häkchen fehlte. Letztere wurde von keinem Frontend aufgerufen, war aber eine wartungsgefährliche Doppelung: Hätte je ein Bedienelement darauf umgestellt, wäre der Widerrufs-Verzicht stillschweigend weggefallen.

**Was wurde geändert:** Das öffentliche Sicherheitskonzept (`/twitch/sicherheit`) erklärt jetzt im Detail, wie die OAuth-Tokens geschützt sind, und zeigt ein echtes Format-Beispiel. Neue Abschnitte zu Zahlungsdaten, zur Echtheitsprüfung eingehender Twitch-Ereignisse und zum Server-Schutz kamen dazu. Die ungenutzte JSON-Checkout-Schnittstelle samt zugehöriger Hilfsverweise wurde vollständig entfernt — es gibt jetzt nur noch genau eine Stelle im Code, die eine Bezahlsession erzeugt, und die hat die Zustimmungs-Abfrage zwingend eingebaut.

**Wie es jetzt funktioniert:** Der vertiefte Abschnitt beschreibt die fünf Eigenschaften der Feldverschlüsselung: jeder Wert bekommt einen frischen Zufallswert (gleiche Tokens sehen verschlüsselt völlig verschieden aus), trägt ein Echtheitssiegel (jede Manipulation lässt die Entschlüsselung kontrolliert fehlschlagen), ist fest an Tabelle, Spalte und Twitch-Konto-ID gebunden (ein Token von Streamer A kann technisch nie als Token von Streamer B entschlüsselt werden, ein Refresh-Token nie als Access-Token), der Schlüssel liegt getrennt vom Datenbestand, und jeder Wert trägt eine Versions-Kennung für Schlüsselwechsel. Dazu ein Format-Beispiel mit Wegwerf-Schlüssel, damit man sieht, wie so ein Wert in der Datenbank aussieht — unlesbar ohne den Master-Schlüssel. Beim Bezahlen baut nun nur noch der Bezahlseiten-Pfad die Stripe-Session, mit dem Pflicht-Häkchen „Ich stimme den AGB zu und erkenne an, dass die Leistung sofort beginnt und mein Widerrufsrecht erlischt". Alle Sicherheitstexte sind synchron in Python-Fallback und Rust-Live-Modul gelandet (byte-identisch verifiziert).

## #151 — Re-Autorisierung setzt Partner-Einstellungen nicht mehr zurück (Python-Fix)

**Ausgangslage:** Der in #149 beschriebene Bug im Python-Pfad: Bei jeder Re-Autorisierung oder erneuten Partner-Aktivierung wurden Silent-Ban, Silent-Raid, die Live-Ping-Rolle und das Discord-Link-Pflicht-Flag still auf ihre Standardwerte zurückgesetzt. Ursache war das Aufräumen der alten Streamer-Tabelle: Die dort gedroppten Spalten liefern seitdem nur noch Festwerte, und der als Sicherheitsnetz gedachte Rückgriff auf den bestehenden Partner-Datensatz konnte nie greifen, weil ein Default-Wert ihn verdeckte.

**Was wurde geändert:** Die fünf betroffenen Felder greifen bei nicht explizit übergebenen Werten jetzt direkt auf den aktiven Partner-Datensatz zurück — exakt das Verhalten, das die Rust-Seite seit #149 hat. Ein neuer Regressionstest legt einen Partner mit gesetzten Einstellungen an, re-promotet ihn und prüft, dass alles erhalten bleibt.

**Wie es jetzt funktioniert:** Streamer autorisiert den Bot erneut → Partner-Status wird aufgefrischt (verifiziert, aktiv, Raid-Bot an) → alle individuellen Einstellungen bleiben unverändert stehen. Bereits zurückgesetzte Einstellungen aus dem Bug-Zeitfenster lassen sich nicht automatisch wiederherstellen — betroffene Partner müssen sie einmalig neu setzen.

## #150 — AGB klar auf den Twitch-Bot eingegrenzt, Server-Schutz im Sicherheitskonzept

**Ausgangslage:** Die frisch erweiterten AGB sprachen pauschal von den „digitalen Diensten der Deutschen Deadlock Community" — damit war unklar, ob sie auch für die anderen Angebote (Discord-Bots, Steam-Bot, Community-Server) gelten sollen. Und im Sicherheitskonzept fehlte die Server-Ebene: Wie die Maschine selbst geschützt ist, stand dort noch nicht.

**Was wurde geändert:** § 1 der AGB grenzt den Geltungsbereich jetzt ausdrücklich ein: Sie gelten ausschließlich für den Twitch-Bot samt zugehöriger Web-Dienste (Dashboard, Statistik-Seiten, Abo-Verwaltung) — die Discord-Bots, der Steam-Bot und der Community-Discord-Server sind explizit ausgenommen. Im Sicherheitskonzept gibt es einen neuen Abschnitt „Wie der Server geschützt wird".

**Wie es jetzt funktioniert:** Der neue Abschnitt erklärt die Schutzschichten des Servers: die doppelte Firewall (Rechenzentrums-Firewall bei IONOS vor dem Server, zweite Firewall auf dem Server selbst — offen ist nur das Minimum), Fernzugriff nur über ein privates VPN-Netz, Schutz vor SQL-Injection durch ausschließlich parametrisierte Datenbank-Abfragen (Nutzereingaben werden nie Teil des SQL-Befehls), Schutz vor eingeschleustem Browser-Code durch Ausgabe-Maskierung plus Content-Security-Policy und automatische Betriebssystem-Sicherheitsupdates inklusive kontrolliertem Neustart. Beide Änderungen sind synchron in der Python- und der Rust-Implementierung gelandet (byte-identisch verifiziert), live ausgeliefert wird weiter über Rust.

## #149 — Streamer-Autorisierung: Setup-Followups in Rust + Einstellungs-Wipe-Bug entdeckt

**Ausgangslage:** Der OAuth-Callback (Streamer autorisiert den Bot) war in Rust zwar fertig portiert, durfte aber nicht live gehen: Nach dem Token-Speichern startet das System Hintergrund-Schritte — Bot als Moderator einsetzen, Begrüßungsnachrichten in den Chat, Partner-Status setzen, Discord-Rolle vergeben, Trial-Zeitstempel — und die liefen nur im Python-Prozess. Ein nativer Callback hätte den Einmal-OAuth-Code verbrannt und diese Schritte verschluckt.

**Was wurde geändert:** Die kompletten Followups laufen jetzt nativ in Rust. Bei einer Erst-Autorisierung passiert in dieser Reihenfolge: (1) Partner-Status wird gesetzt bzw. reaktiviert (inkl. Übernahme der Discord-Verknüpfung, Anzeigename kommt über den Master-Broker direkt vom Discord-Bot), (2) der Erst-Login-Zeitstempel für die Trial-Logik wird einmalig festgehalten (Doppelausführung kann ihn nicht überschreiben), (3) der Bot setzt sich mit dem frischen Streamer-Token selbst als Moderator ein (war er es schon, wird das erkannt statt als Fehler behandelt), (4) drei Begrüßungsnachrichten gehen in den Chat — interim noch über den Python-Chat-Prozess geschickt, bis der Chat selbst auf Rust umzieht. Bei einer Re-Autorisierung mit Discord-Verknüpfung läuft nur der Partner-Sync. Jeder Schritt ist fehler-isoliert: Scheitert einer, laufen die übrigen trotzdem.

**Bug dabei gefunden und in Rust gefixt:** Der bisherige Python-Pfad setzt bei jeder Re-Autorisierung still die Partner-Einstellungen zurück — Silent-Ban/Silent-Raid auf aus, die Live-Ping-Rolle auf leer. Grund: Beim Aufräumen der Streamer-Tabelle wurden die Quell-Spalten durch Festwerte ersetzt, und die als Sicherheitsnetz gedachten Rückgriffe auf den bestehenden Partner-Datensatz greifen durch einen Default-Wert nie. Die Rust-Version bewahrt die bestehenden Einstellungen; der Python-Pfad (läuft noch fürs Streamer-Dashboard) hat den Bug weiterhin und wird separat gefixt.

**Verifikation:** 13 neue Datenbank-Tests gegen das echte Produktionsschema, Live-Vergleich aller gefahrlos testbaren Callback-Fehlerpfade gegen die Python-Antworten (inhaltlich identisch), Wiederholungsschutz (Idempotenz) live bestätigt. Die sofortige Offline-Erkennung nach der Autorisierung übernimmt der Rust-Poller innerhalb von ~15 Sekunden — der bisherige Sonderpfad dafür entfällt ersatzlos.

## #148 — Richtige AGB für den Bot, öffentliches Sicherheitskonzept und Legal-Seiten in Rust

**Ausgangslage:** Die AGB deckten bisher nur den Bezahl-Shop ab (Raid Boost, Analyse-Abos, Stripe-Checkout). Die eigentliche Bot-Nutzung — automatische Moderation, die kanalübergreifende Bannliste, Auto-Raids, KI-Funktionen, das kostenlose Partnerprogramm — war nirgends geregelt, obwohl genau das der Kern des Dienstes ist. Und wie das Projekt mit Sicherheit umgeht (Tokens, Mod-Rechte, Schutzmechanismen), konnte man nirgendwo nachlesen, obwohl der Bot in jedem Partner-Kanal Moderator-Rechte trägt.

**Was wurde geändert:** Die AGB wurden zu vollständigen Nutzungsbedingungen ausgebaut, wie man sie von großen Bots (Nightbot, StreamElements) kennt — 14 Paragraphen statt 10. Neu geregelt sind: die kostenlose Basisnutzung (Chat-Bot, Raid-Netzwerk, Moderation, Statistiken), die Teilnahme am Partnerprogramm inklusive OAuth-Autorisierung und deren Widerruf, die automatische Moderation mit der kanalübergreifenden Bannliste samt Einspruchsweg (Mail oder Discord, dann prüft ein Mensch), die KI-Funktionen, die Pflichten der Nutzer und die Haftung bei kostenlosen Leistungen. Dazu gibt es eine neue, frei zugängliche Seite: das Sicherheitskonzept unter `/twitch/sicherheit`. Beides ist im Website-Footer verlinkt. Unter der Haube wurden die kompletten Legal-Seiten zusätzlich in Rust neu geschrieben und live geschaltet.

**Wie es jetzt funktioniert:** Impressum, Datenschutz und AGB bleiben hinter dem bekannten Human-Gate (Cloudflare-Prüfung, 10-Minuten-Cookie) — das Sicherheitskonzept ist bewusst ohne Gate und für Suchmaschinen freigegeben, weil es gelesen werden soll. Es erklärt auf Konzeptebene, wie der Bot selbst abgesichert ist: getrennte Berechtigungsebenen (der Bot-Account hat keine Broadcaster-Rechte, mächtigere Aktionen brauchen die ausdrückliche Streamer-Autorisierung), verschlüsselte Token-Ablage, nur lokal erreichbare interne Dienste, Schutz gegen Prompt-Injection bei den KI-Funktionen, Fehlbann-Schutz im Spam-Filter und die laufende Überwachung mit stündlicher Log-Prüfung und täglichem Audit. Die Rust-Portierung liefert dabei byte-identische Seiten wie vorher der Python-Renderer — geprüft per direktem Vergleich beider Ausgaben — und übernimmt ab sofort den Live-Traffic für alle Legal-Pfade.

## #147 — !invite-Command in Rust, Auto-Erkennung entfernt

**Ausgangslage:** Der Bot hatte eine Regex-basierte Auto-Erkennung für Deadlock-Zugangsfragen und antwortete automatisch mit dem Discord-Invite-Link. Die Logik war zu breit: Das Wort "play" reichte als Zugangs-Signal aus, sodass normaler Kommentar-Text mit "play Deadlock" und einem Fragezeichen irgendwo im Satz die Antwort auslöste. Dazu lief die gesamte Logik noch in Python.

**Was wurde geändert:** Die automatische Regex-Erkennung wurde vollständig entfernt. Stattdessen gibt es jetzt einen expliziten `!invite`-Command. Wenn jemand `!invite` in den Chat schreibt, fragt Python den neuen Rust-Endpoint `POST /internal/twitch/v1/chat/command` an. Rust prüft dort in `twitch_live_state`, ob der Kanal gerade Deadlock streamt (`is_live = 1` und `last_game` enthält "deadlock"). Ist das der Fall, wird die Discord-Invite-URL geladen — zuerst streamer-spezifisch aus `twitch_streamer_invites`, dann als Fallback aus der Env-Var `PROMO_DISCORD_INVITE` — und als fertiger Reply-Text zurückgegeben. Python schickt ihn per IRC in den Chat.

**Wie es jetzt funktioniert:** `!invite` im Chat → Python schickt Request an Rust → Rust prüft ob Deadlock läuft → bei Ja: Invite-Link zurück, Python postet Antwort. Kein Deadlock live → keine Antwort, kein Spam. Pro User und Kanal gilt 1h Cooldown. Die gesamte Entscheidungslogik liegt in Rust.

## #146 — Discord-Link-Click: Typ-Mismatch bei clicked_at behoben

**Ausgangslage:** Jedes Mal wenn ein User auf den Discord-Live-Link klickte, schlug die Datenbank-Eintragung mit einem Typfehler fehl: Die Spalte `clicked_at` in der DB ist `TIMESTAMPTZ`, aber der Code schickte den Zeitstempel als formatierten Text-String. PostgreSQL macht keine implizite Text→Timestamp-Konvertierung, also scheiterte jeder Schreibversuch — der Klick wurde nicht gespeichert, der Nutzer bekam trotzdem ein `ok: true` zurück.

**Was wurde geändert:** Der Klick-Zeitstempel wird jetzt direkt als `DateTime<Utc>` an die Datenbank übergeben, nicht mehr als formatierter String. sqlx weiß, wie es einen nativen Zeitstempel-Typ korrekt an eine `TIMESTAMPTZ`-Spalte bindet. Gleichzeitig wurde das Test-Schema mit dem Prod-Schema synchronisiert (`TEXT` → `TIMESTAMPTZ`).

**Wie es jetzt funktioniert:** Jeder Discord-Link-Click landet sauber in der DB — der Zeitstempel wird typ-korrekt als Timestamp with time zone gespeichert, ohne Konvertierungsumweg über einen String.

## #145 — OAuth-Anmeldung fertig portiert + Systemsuche nach weiteren Halb-Portierungen: vier stille Lücken im Raid-Tracking geschlossen

**Ausgangslage:** Nach der #140-Korrektur stellte sich die Frage, ob es im Rust-Teil weitere Stellen gibt, die Arbeit versprechen, aber nicht (ganz) tun. Zwei Dinge wurden angegangen: Erstens die OAuth-Anmeldung der Streamer — deren Rust-Fassung brach bisher mitten im Ablauf ab, weil nach dem Token-Tausch der Schritt fehlte, der ermittelt, *wem* das frische Token gehört (die Twitch-Token-Antwort enthält keine Identität; dafür braucht es einen zweiten Abruf mit dem frischen Token). Zweitens ein systematischer Audit (84 unabhängige Prüf-Agenten, jeder Befund dreifach gegengeprüft) über den gesamten Rust-Baum nach genau solchen Mustern: Erfolgsmeldungen ohne Wirkung, Datenbanktyp-Abweichungen, eigenerfundene Antwortformate, zu schmale Schnittstellen.

**Was wurde geändert:**

- **OAuth-Anmeldung komplett:** Die gesamte Kette ist jetzt nativ — Code-Tausch bei Twitch, Identitäts-Abruf mit dem frischen Token, Falscher-Account-Prüfung (wenn Streamer X sich anmelden sollte, aber Account Y den Link benutzt → klare Fehlermeldung statt stillem Speichern unter falschem Namen), Berechtigungs-Prüfung gegen das erwartete Profil, verschlüsseltes Speichern der Tokens und die Erfolgsseite mit Weiterleitung. Dazu der Wiederholungsschutz: Ein Netz-Retry mit demselben Schlüssel bekommt die gespeicherte Antwort, statt den nur einmal gültigen Anmelde-Code ein zweites Mal zu verbrennen. Sieben Tests decken Erfolg, beide Mismatch-Arten, fremde Berechtigungen, Twitch-Ausfall und ungültigen State ab — jeweils mit der Prüfung, dass im Fehlerfall nichts gespeichert wird. Die Route bleibt vorerst bewusst beim alten System: Python startet nach dem Speichern Folgeaktionen (Bot als Moderator einsetzen, Chat-Begrüßung, sofortige Offline-Überwachung), die noch nicht portiert sind — und der echte Anmelde-Weg der Streamer läuft ohnehin über das Dashboard, nicht über diese interne Route.

- **Vier stille Lücken im Raid-Ankunfts-Tracking geschlossen (liefen LIVE):** Erstens wurden Zweit-Signale eines bereits erkannten Raids (z. B. wenn nach der Chat-Ankündigung auch das offizielle Raid-Ereignis eintrifft) komplett verworfen — die Signal-Liste und die „Raid wurde zurückgezogen"-Markierung in der Datenbank blieben dauerhaft unvollständig. Jetzt wird der jüngste passende Ankunfts-Eintrag (innerhalb von 10 Minuten) gefunden und fortgeschrieben. Zweitens gingen Raids, die NUR über die Chat-Ankündigung sichtbar wurden (ohne vorheriges offizielles Ereignis), vollständig verloren — jetzt werden sie 15 Sekunden lang auf Korrelation gewartet und danach als eigenständige Ankunft verbucht, exakt wie im alten System. Drittens wurde das Ergebnis der Score-Buchung bei bestätigten Partner-Raids stillschweigend weggeworfen — ein Datenbankfehler wäre unsichtbar geblieben; jetzt wird er protokolliert. Viertens fehlte die regelmäßige Komplett-Neuberechnung aller Partner-Scores (alle 5 Minuten im alten System) — Rust rechnete nur bei Online/Offline-Ereignissen; Partner mit verpassten Ereignissen veralteten dauerhaft. Der neue 5-Minuten-Durchlauf hat direkt beim ersten Lauf 52 Partner-Scores aktualisiert (live verifiziert).

- **Weitere Typ-Fallen gegen die echte Datenbank entschärft:** Mehrere Stellen lasen Ganzzahl-Spalten im falschen Bit-Format oder Text-Spalten als Zeitstempel — grüne Tests, aber Absturz beim echten Aufruf, weil die Testdatenbank andere Spaltentypen hatte als die echte. Die betroffenen Abfragen wurden korrigiert und die Testdatenbanken an die echten Typen angeglichen, inklusive eines Testaufbaus, der alte Tabellen verschleppte und deshalb neue Spalten nie bemerkte.

- **Zukünftiges Dashboard (noch nicht live) abgesichert:** Die öffentliche Raid-Liste zeigte auch fehlgeschlagene Raids (Filter fehlte) und 20 statt 10 Einträge — gefixt. Die Browser-Schutzregeln (CORS) standen auf „jede fremde Website darf zugreifen" — auch für Admin-Routen; jetzt gilt das wie im alten System nur für die drei öffentlichen Routen. Das Audit-Urteil zum restlichen Dashboard-Port ist ehrlich dokumentiert: Vor einem Umstieg fehlen die Partner-Anmeldung, rund 40 Routen und mehrere Antwortformate — das ist eingeplante Arbeit für die Dashboard-Welle, kein stiller Mangel mehr.

**Wie es jetzt funktioniert:** Das Raid-Ankunfts-Tracking erfasst wieder alle drei Signalwege (offizielles Ereignis, Chat-Ankündigung als Zweitsignal, Chat-Ankündigung allein), Partner-Scores bleiben auch ohne Ereignisse maximal 5 Minuten alt, und die OAuth-Anmeldung ist als vollständiger, getesteter Baustein bereit — der Umstieg ist ein Einzeiler, sobald die Folgeaktionen portiert sind. Der Audit hat zusätzlich bestätigt, was schon sauber war: Von 19 geprüften Verdachtsmomenten der schweren Kategorie waren 6 bereits durch die Tagesarbeit erledigt und 7 betrafen ausschließlich den noch nicht aktiven Dashboard-Port.

## #144 — Markt-Dominanz: Viewer- und Kanal-Dimension getrennt + Partner-Bestand

**Ausgangslage:** Die Seite zeigte nur die Viewer-Dimension. Dass z. B. 4 von 6 live geschalteten DE-Kanälen zum Netzwerk gehören (Kanal-Dominanz 67 %), war nirgends ablesbar — genauso wenig wie der Bestand: wie viele Partner überhaupt unter Vertrag sind und wie viele davon im Zeitraum wirklich gestreamt haben.

**Was wurde geändert:** Die Kennzahlen sind jetzt in zwei beschriftete Reihen aufgeteilt. „Viewer": Marktanteil live, Netzwerk-Viewer live, Markt-Viewer gesamt, Dominanz-Zeit. „Streamer": Partner unter Vertrag (aktive, ohne Opt-out/Pause), davon im Zeitraum mit Deadlock-Stream aktiv, live jetzt (Netzwerk-/Markt-Kanäle mit Prozent), Ø Kanal-Anteil über den Zeitraum. Dazu zwei neue Verlaufs-Charts: Kanal-Anteil des Netzwerks (wie viele der gleichzeitig live geschalteten Markt-Kanäle gehören uns — unabhängig von deren Größe) und die Live-Kanal-Verteilung Netzwerk vs. Rest. Die Kanal-Sicht zeigt die Netzwerk-Stärke fairer als die Viewer-Sicht, in der ein einzelner großer externer Kanal alles dominiert.

## #143 — Markt-Dominanz: 100-%-Phasen sichtbar + neue Dominanz-Zeit-Kennzahl

**Ausgangslage:** Die Mindestmarkt-Schwelle (20 Viewer) sollte nächtliche 0↔100-%-Sprünge glätten — hat dabei aber genau die wertvollsten Momente versteckt: die Phasen, in denen ausschließlich Netzwerk-Kanäle live sind und der Marktanteil real 100 % beträgt. Diese Zeiten tauchten weder in der Linie noch im Peak auf.

**Was wurde geändert:** Die Schwelle ist komplett raus — die Anteil-Linie zeigt jetzt jeden Mess-Zeitraum mit aktivem Markt, inklusive der 100-%-Phasen; die Einordnung (wie groß der Markt dabei war) liefert das Viewer-Panel direkt darunter. Die Peak-Karte wurde zur Dominanz-Karte: Sie zeigt, in wie viel Prozent der gemessenen Zeit das Netzwerk mindestens die Hälfte aller Markt-Viewer hielt, plus den Spitzenwert mit absoluten Zahlen (z. B. „100 % — 3 von 3 Viewern"), damit klein-aber-100 % von groß-und-40 % unterscheidbar bleibt.

## #142 — Markt-Dominanz: Chart in zwei lesbare Panels aufgeteilt

**Ausgangslage:** Marktanteils-Linie und Viewer-Flächen teilten sich ein Diagramm mit zwei Y-Achsen. Ergebnis: Die Prozent-Linie zerfiel durch die nächtlichen Mindestmarkt-Lücken in abgehackte Stücke, und die Viewer-Flächen waren neben einzelnen Tages-Spitzen kaum noch ablesbar.

**Was wurde geändert:** Das Diagramm ist jetzt zweigeteilt — oben der Marktanteil als durchgehende Fläche mit eigener Prozent-Skala (Phasen unter 20 Markt-Viewern werden überbrückt statt als Loch zu erscheinen, mit Hinweistext darunter), darunter die Viewer-Verteilung Netzwerk vs. Rest mit eigener Skala. Zusätzlich sind die Zeit-Buckets gröber (24 h → 15 min, 7 Tage → 2 h, 30 Tage → 6 h), was das Zickzack glättet und die Kurven ruhiger macht.

## #141 — Markt-Dominanz: DE-Erkennung jetzt über die Stream-Sprache statt über Tags

**Ausgangslage:** Die deutschsprachige Marktsicht erkannte Streams über die frei wählbaren Stream-Tags („Deutsch"/„German"). Das war zu grob: Internationale Streamer, die mehrsprachig taggen, rutschten in den DE-Markt, obwohl sie englisch streamen — und blähten dessen Viewer-Basis künstlich auf, sodass der eigene Anteil viel kleiner wirkte als er ist.

**Was wurde geändert:** Die Kategorie-Zeitreihe speichert ab sofort pro Stream die offizielle Stream-Sprache aus der Twitch-API (neue Spalte, der Poller schreibt sie bei jedem ~17-Sekunden-Tick mit). Die DE-Markt-Definition ist jetzt exakt dieselbe wie bei der Bot-Discovery: Stream-Sprache `de` — oder Partner, die zählen immer dazu. Für ältere Datenpunkte ohne Sprachinfo bleibt ein Fallback: vor dem 10.06.2026 zählen sie komplett (die Erhebung war damals selbst schon auf Deutsch gefiltert), für die zwei Tage danach gilt näherungsweise der Tag-Filter. Die Top-Stream-Tabelle zeigt jetzt den echten Sprachcode pro Stream.

**Wie es sich auswirkt:** Der mehrsprachig getaggte englische Stream mit 400 Viewern fällt aus dem DE-Markt raus — übrig bleibt der echte deutschsprachige Markt. Direkt nach der Umstellung: 12 von 29 DE-Viewern beim Netzwerk, also rund 41 % Marktanteil statt scheinbarer 2 %.

## #140 — Korrektur zu #137: Routen waren nie aktiv — jetzt wirklich nativ, geprüft gegen das Live-System

**Ausgangslage:** Eintrag #137 behauptete, zwölf interne API-Routen liefen nativ in Rust. Das stimmte nicht: Der Code lag zwar im Repo, war aber nie in das Programm eingebunden — die neuen Module waren nicht einkompiliert und keine einzige der zwölf Routen im Router registriert. Alles lief still weiter über den Python-Umweg, ohne dass es jemand merkte, weil der Fallback-Mechanismus genau dafür gebaut ist. Ein adversarialer Voll-Review (70 unabhängige Prüf-Agenten, jeder Befund von drei Skeptikern gegengeprüft) fand zusätzlich 19 bestätigte Abweichungen vom Python-Verhalten, einige davon auf Routen, die bereits live liefen.

**Was wurde geändert:**

- **Live-Routen repariert:** Die globale Bannliste normalisiert Eingaben jetzt wie das alte System (akzeptiert `@Name` und alternative Feldnamen, weist Ungültiges mit 400 statt kommentarlos ab) und liefert wieder die vollständige Antwort inkl. Login und Begründung. Der Health-Endpunkt gab Host, Port, Datenbankname und DB-Benutzer im Klartext aus — jetzt erscheinen sie wie im alten System ausschließlich als nicht zurückrechenbare Hashes (mit dem Original abgeglichen, Hash für Hash identisch).
- **Die 19 Review-Befunde behoben,** darunter: Der OAuth-Abschluss hätte Fehler als HTTP-Status gemeldet, obwohl die Auth-Oberfläche immer Status 200 mit dem Ergebnis im Inhalt erwartet. Eine leere Scope-Schutzliste hätte den Schutz komplett abgeschaltet statt alles zu sperren (fail-open statt fail-closed). Der Zufalls-Token für den OAuth-Schutz kam aus einer schwachen Eigenbau-Mischung statt aus dem Krypto-Generator des Betriebssystems. Vier Auswertungs-Abfragen wären beim ersten echten Aufruf mit einem Datenbankfehler geplatzt (Zahl mit Text verkettet — die Testdatenbank verzieh das, die echte nicht). Am gefährlichsten: Eine fehlgeschlagene Verifizierung hätte den Streamer dauerhaft **verifiziert** statt ihn zu entfernen, weil unbekannte Modi pauschal als „dauerhaft bestätigen" behandelt wurden — jetzt mutieren unbekannte und noch nicht portierbare Modi gar nichts und antworten ehrlich.
- **Wiederholungsschutz nachgebaut:** Das alte System dedupliziert kritische Schreib-Anfragen über einen mitgeschickten Schlüssel (gleiche Anfrage zweimal = einmal ausführen, zweite bekommt die gespeicherte Antwort; gleicher Schlüssel mit anderem Inhalt = Konflikt-Fehler; parallele Doppel-Anfrage wartet aufs Ergebnis der ersten). Dieser Mechanismus existiert jetzt als ein geteilter Baustein in Rust — exakt nach dem Python-Regelwerk inkl. 15-Minuten-Gedächtnis, Obergrenze und der Regel, dass Fehler nie gespeichert werden, damit ein erneuter Versuch wirklich neu ausgeführt wird.
- **Sieben Routen wirklich aktiviert** — jede einzelne vor der Freischaltung im laufenden Betrieb gegen die Python-Antwort verglichen, bis die Antworten byte-identisch waren: die Raid-Auth-Lesestrecke (Auth-Link erzeugen, Auth-Status, Block-Status, Link-Weiterleitung), die Liste aktiver Live-Ankündigungen, das Link-Klick-Logging (mit dem neuen Wiederholungsschutz) und die Vergleichs-Auswertung.
- **Bewusst NICHT aktiviert** (laufen unverändert über Python, nichts geht verloren): der OAuth-Abschluss selbst — die native Fassung ist am letzten Schritt unfertig und hätte beim Fehlschlag den einmalig gültigen Anmelde-Code verbrannt, womit die Streamer-Anmeldung kaputt statt nur langsamer gewesen wäre; der Anforderungs-Versand (braucht den Discord-Direktnachrichten-Versand, den nur Python kann); drei Auswertungs-Routen, deren Rust-Fassung beim Live-Vergleich eine andere Antwortstruktur lieferte als das Original.
- **Beim Live-Vergleich zwei schlafende Typ-Fehler gefunden:** Zwei Datenbankspalten haben in der echten Datenbank andere Typen als in der Testdatenbank — die Tests waren grün, die echte Route stürzte ab. Beides behoben und die Testdatenbank an die echten Typen angeglichen, damit diese Fehlerklasse künftig im Test auffällt.

**Wie es jetzt funktioniert:** Sieben weitere interne Routen beantwortet der Rust-Kern direkt — nachweislich mit identischen Antworten wie das alte System, verifiziert am echten Datenbestand. Alles, was der Rust-Kern noch nicht vollständig kann, läuft automatisch weiter über Python; es gibt keine halb portierten Pfade mehr, die Erfolg melden und nichts tun. Und als Konsequenz aus dem #137-Fehler gilt ab jetzt: „nativ" heißt erst dann nativ, wenn die Route im Router registriert ist und der Live-Vergleich gegen das alte System identische Antworten zeigt.

## #139 — Markt-Dominanz: deutschsprachige Sicht repariert und als Hauptsicht ausgebaut

**Ausgangslage:** Die frisch gebaute Markt-Dominanz-Seite hatte in der deutschsprachigen Sicht drei Fehler. Erstens zeigten die Live-Kennzahlen und die Top-Stream-Tabelle trotz DE-Auswahl die globale Kategorie (internationale Streams in der Liste). Zweitens fielen Partner ohne gesetzten „Deutsch"-Tag aus dem DE-Markt heraus, und die historischen Daten vor dem 10.06.2026 — die bei der Erhebung bereits sprachgefiltert waren — wurden durch den Tag-Filter größtenteils weggeworfen, obwohl Daten bis Oktober 2025 vorliegen. Drittens sprang die Anteils-Linie nachts wild zwischen 0 und 100 %, weil bei einem fast leeren Markt (2 von 2 Viewern) jeder Anteil bedeutungslos ist.

**Was wurde geändert:** Der DE-Markt ist jetzt einheitlich definiert als „Deutsch-Tag oder Partner" — in der Zeitreihe wie im Live-Block. Daten vor dem 10.06.2026 zählen im DE-Scope komplett (sie waren schon deutschsprachig erhoben), damit reicht die Historie nutzbar bis zum Datenbeginn im Oktober 2025 zurück; dafür gibt es neue Zeitraum-Optionen 180 Tage und „Alles". Live-Karten und Top-Stream-Tabelle respektieren den gewählten Scope. Anteils-Linie und Peak gelten nur noch ab einer Mindestmarktgröße von 20 Viewern — darunter zeigt das Chart eine ehrliche Lücke statt eines 100-%-Ausschlags, und der Peak nennt keinen Nachts-Mini-Markt mehr. Der Hinweis zur unvollständigen Vor-Cutover-Basis erscheint nur noch in der globalen Sicht, wo er tatsächlich gilt.

## #138 — Markt-Dominanz-Seite im Admin-Dashboard

**Ausgangslage:** Wie viel der Deadlock-Zuschauerschaft auf Twitch tatsächlich bei unseren Partner-Streamern läuft, war bislang nirgends sichtbar — die Daten lagen zwar längst in der Kategorie-Zeitreihe (der Poller schreibt seit Oktober alle ~17 Sekunden jeden Live-Stream der Kategorie samt Viewerzahl und Partner-Flag in die Datenbank), aber es gab weder eine Auswertung noch eine Visualisierung darüber.

**Was wurde geändert:** Das Admin-Dashboard hat unter Community eine neue Seite „Markt-Dominanz". Sie zeigt als Zeitverlauf ein gestapeltes Flächendiagramm (Viewer im Partner-Netzwerk vs. Rest der Kategorie) mit überlagerter Marktanteils-Linie in Prozent, dazu Kennzahlen-Karten (Marktanteil live, Live-Streams Partner/gesamt, Kategorie-Viewer, Peak-Anteil im Zeitraum) und eine Live-Tabelle der aktuellen Top-Streams mit Partner-Badge. Zeitraum ist wählbar (24 h / 7 / 30 / 90 Tage), und ein Scope-Schalter wechselt zwischen globaler Kategorie und der deutschsprachigen Teilmenge.

**Wie es funktioniert:** Die Berechnung läuft nativ im Rust-Worker als neuer interner Endpoint. Pro Zeit-Bucket (10 Minuten bis 1 Tag, automatisch je nach Zeitraum) werden die Viewer-Summen über die Poll-Ticks gemittelt — geteilt wird durch die Anzahl der Ticks im Bucket, damit das Ergebnis unabhängig von der Abtastrate stimmt. Marktanteil = Partner-Viewer geteilt durch Gesamt-Viewer des Buckets. Die deutschsprachige Sicht filtert über die Stream-Tags („Deutsch"/„German"), weil die Zeitreihe keine Sprachspalte führt — eine Näherung, aber Partner taggen durchgängig deutsch. Das Dashboard reicht die Admin-Anfrage nur durch (Admin-Login bzw. Localhost nötig); zwei Einordnungen zeigt die Seite ehrlich an: Daten vor dem 10.06.2026 enthalten nur die damals sprachgefilterte Teilmenge der Kategorie (der Kategorie-Poll lief bis dahin gefiltert), und der Peak-Anteil nennt immer die absoluten Viewerzahlen dazu, damit ein „100 % nachts bei 3 Viewern" nicht wie Dominanz aussieht.

## #137 — Raid-OAuth, Telemetrie und Analytics nativ in Rust (12 Routen, Welle B)

**Ausgangslage:** Trotz der bisherigen Portierungswellen liefen noch zwölf interne API-Routen über den Fallback-Proxy zu Python-8779 weiter: die komplette Raid-OAuth-Strecke (sechs Schritte von der Auth-URL-Generierung bis zum fertigen OAuth-Callback), zwei Telemetrie-Routen (Liste aktiver Live-Announcements, Link-Click-Logging), vier Analytics-Routen (Bot-Statistiken, Streamer-Einzelanalyse, Vergleichsauswertung, Session-Details). Außerdem fehlten der Verify-Route die befristete und löschbare Verifizierungs-Variante, und ArchiveMode kannte keine Toggle-Block-Variante — beide boten damit nicht die volle Python-Semantik.

**Was wurde geändert:** Alle zwölf Routen laufen jetzt direkt im Rust-Kern:

- **Raid-OAuth (6 Routen):** `GET /raid/auth-url`, `/raid/auth-state`, `/raid/block-state`, `/raid/go-url`, `POST /raid/requirements`, `/raid/oauth-callback` — der OAuth-Flow-Stack (`StateStore`, `AuthWriter`, `TwitchTokenClient`) wird per Composition-Root `raid_oauth_impl.rs` verdrahtet und über den Trait `RaidOAuthPort` mit den Handlern verbunden. Discord-Scope-Guard (Allowlist-Prüfung für Guild-/Channel-/Role-IDs) ist identisch zur Python-Implementierung eingebaut. Offener Punkt: Idempotenz-Layer für `requirements` + `oauth-callback` fehlt noch (OAuth-Codes sind Single-Use; ein zweiter Aufruf mit demselben Code schlägt beim Token-Exchange fehl statt eine gecachte Antwort zu liefern).

- **Telemetrie (2 Routen):** `GET /live/active-announcements` liest aktive Discord-Announcements direkt aus DB (JOIN auf State + Config, nur Zeilen mit Message- und Tracking-Token, alphabetisch sortiert). `POST /live/link-click` schreibt Link-Klicks in `twitch_link_clicks`; ein In-Memory-Idempotenz-Cache (15 min TTL, max 2000 Einträge, cleanup on write) verhindert Doppel-Writes bei Wiederholungsanfragen — Parität zu Pythons In-Process-Cache. `GET /debug/observability` und `/debug/chatters/:login` bleiben weiterhin proxied, da sie Live-Bot-Laufzeitstatus brauchen.

- **Analytics (4 Routen):** `GET /stats` (Bot-weite Kennzahlen), `/analytics/streamer/:login` (30-Tage-Statistiken + letzte Sessions), `/analytics/comparison` (Vergleichskategorien + Top-Streamer), `/sessions/:session_id` (Session-Detail) — alles direkte DB-Abfragen, kein Proxy-Umweg.

- **Verify-Modus:** `POST /streamers/:login/verify` unterstützt jetzt `mode: permanent | temp | clear`. `temp` setzt eine 30-Tage-Frist (`manual_verified_until = NOW()+30d`), `clear` löscht die Verifizierung, alles andere (inkl. leer/fehlt) verhält sich wie bisher als `permanent`.

- **ArchiveMode:** `parse()` ist jetzt infallibel — unbekannte Werte fallen auf `Toggle` durch statt 400 zu liefern (Python-Parität: unbekannte modi landen immer auf „toggle"). Neue Varianten `ToggleBlock` und `Toggle` ergänzen `Archive`, `Unarchive`, `Block`, `Unblock`.

**Wie es jetzt funktioniert:** Die zwölf Routen werden vom Rust-Kern direkt beantwortet; für Nutzer und Streamer ändert sich nichts Sichtbares. Der Proxy-Fallback zu Python-8779 wird damit ein weiteres Stück kleiner — die verbleibenden proxied Routen sind nur noch solche, die echten Live-Bot-Laufzeitstatus (Chat-Verbindung, In-Process-State) brauchen.

## #136 — Paritäts-Härtung der nativen Routen nach Voll-Review

**Ausgangslage:** Ein adversarialer Voll-Review des Rust-Umbaus (mehrere unabhängige Prüfer, jeder Befund gegengeprüft) hat die frisch nativisierten internen Routen Feld-für-Feld gegen das alte Python-Verhalten gestellt. Drei kleine, aber echte Abweichungen kamen zum Vorschein — nichts Kaputtes, aber Verhalten, das in Randfällen vom Original abwich.

**Was wurde geändert:** Die Self-Explainer-Discord-Log-Route gibt bei einem nicht lesbaren Anfrage-Body jetzt wieder exakt die alte Fehlerform zurück (klarer „ungültiges JSON"-Fehler mit Statuscode 400) statt der generischen Framework-Antwort, und sie behandelt eine leere oder Null-Kanal-ID jetzt wie früher als ungültige Eingabe statt sie durchzulassen. Die Liste der noch nicht mit Discord verknüpften Streamer (Quelle für den automatischen Namens-Abgleich) liefert bei einem kurzzeitigen Datenbank-Schluckauf jetzt wieder eine leere Liste statt eines harten Fehlers — genau wie das alte System, damit der Abgleich-Prozess robust bleibt und nicht abbricht.

**Wie es jetzt funktioniert:** Die drei Routen verhalten sich in genau diesen Randfällen wieder byte-gleich zum Python-Original. Für Nutzer und Streamer ändert sich nichts Sichtbares; es ist reine Angleichung an das bewährte Verhalten, abgesichert mit zusätzlichen Tests für die Fehlerfälle.

## #135 — Zwei weitere interne Routen nativ in Rust: Link-Kandidaten + Self-Explainer-Log

**Ausgangslage:** Im Zuge der Rust-Umstellung liefen noch mehrere interne API-Routen über die alte Python-Schicht (Fallback-Proxy). Zwei davon: die Liste der Streamer ohne Discord-Verknüpfung (Quelle für den automatischen Discord-Namens-Abgleich) und das Weiterreichen einer Self-Explainer-Antwort als Discord-Nachricht.

**Was wurde geändert:** Beide Routen laufen jetzt nativ im Rust-Kern. Die Link-Kandidaten-Liste liest direkt aus der Datenbank — identische Abfrage wie vorher, inklusive der Feinheiten: leerer Verknüpfungs-Eintrag zählt als unverknüpft, die Identitäts-Tabelle dient als zweite Wahrheitsquelle, archivierte Streamer fallen raus, Sortierung alphabetisch. Ein Paritäts-Detail wurde dabei gleich geradegezogen: das Feld für die Twitch-User-ID erscheint jetzt — wie im alten System — immer im Ergebnis (leer = `null`) statt bei fehlendem Wert ganz zu verschwinden, damit der Discord-Matcher nicht über ein fehlendes Feld stolpert. Die Self-Explainer-Log-Route ist ein reiner Weiterleiter: sie nimmt das fertige Discord-Embed entgegen und schickt es über den zentralen Nachrichten-Broker an Discord — mit demselben Dedup-Schlüssel-Verfahren wie zuvor (kanonisches JSON → Hash), sodass echte Wiederholungen derselben Frage+Antwort nicht doppelt gepostet werden. Beide mit Vertragstests gegen das alte Verhalten abgesichert.

**Wie es jetzt funktioniert:** Der automatische Discord-Namens-Abgleich und das Posten von Self-Explainer-Antworten laufen unverändert — nur über den Rust-Kern statt über die alte Python-Schicht. Für Nutzer und Streamer ändert sich nichts Sichtbares; der Proxy wird ein weiteres Stück kleiner.

## #134 — Raid-Sperrliste läuft jetzt nativ in Rust (Python-Proxy schrumpft)

**Ausgangslage:** Bei der schrittweisen Umstellung auf Rust laufen die Verwaltungs-Routen der Raid-Sperrliste — Eintragen, Entfernen, Prüfen und Auflisten der Kanäle, die nie angeraidet werden — noch über die alte Python-Schicht. Der Rust-Kern reicht diese Anfragen bislang über einen internen Seitenport an den Python-Worker durch (Übergangs-Klempnerei aus der „Strangler-Fig"-Migration, bei der nach und nach Route für Route von Python nach Rust wandert). Damit hängt ein Stück Admin-Funktion ohne sachlichen Grund weiter an Python.

**Was wurde geändert:** Die vier Sperrlisten-Routen sind jetzt direkt im Rust-Kern implementiert und greifen selbst auf die Datenbank zu — der Umweg über Python entfällt für sie. Damit das Verhalten exakt identisch bleibt, wurde der Datenbankzugriff mit demselben SQL nachgebaut (gleiche Antwortfelder, gleiche Reihenfolge, gleiche Standard-Begründung). Die Login-Normalisierung — die aus `@Name`, einem bloßen Kanalnamen oder einer kompletten `twitch.tv/...`-Profil-URL immer denselben kanonischen Namen macht und reservierte URL-Pfade (z. B. `/videos`) sowie fremde Hosts abweist — wurde als geteilter Baustein in die Domänenschicht gezogen, sodass Streamer-, Raid- und Sperrlisten-Routen dieselbe Logik nutzen statt je eine eigene Kopie. Abgesichert mit Vertragstests, die jedes Antwortfeld, beide Eingabe-Varianten, die Groß-/Kleinschreibung und die Fehlerantwort gegen das alte Verhalten prüfen.

**Wie es jetzt funktioniert:** Eintragen, Entfernen, Prüfen und Auflisten der Raid-Sperrliste beantwortet der Rust-Kern direkt — Eingaben werden wie zuvor auf den kanonischen Kanalnamen normalisiert, ungültige Eingaben mit derselben Fehlermeldung abgewiesen, die Liste kommt neuester Eintrag zuerst. Für Nutzer und Streamer ändert sich nichts Sichtbares; der Unterschied ist allein, dass eine weitere Funktion nicht mehr über die alte Python-Schicht läuft und der Proxy ein Stück kleiner wird.

## #133 — Performance- und Robustheits-Welle: schnellerer Score-Refresh, NULL-sichere Telemetrie, ehrliche Tests

**Ausgangslage:** Mehrere Befunde aus dem Audit ohne akute Außenwirkung, aber relevant für sauberen Dauerbetrieb. Der regelmäßige Score-Refresh lud bei jedem Lauf die komplette Stream-Historie aller Partner aus der Datenbank und warf den Großteil erst danach im Code weg — bei wachsender Tabelle unnötige Last. Beim Mitschreiben von Hype-Train-Ereignissen konnte, wenn der Startzeitpunkt eines Events nicht lesbar war, statt einer aktualisierten Zeile eine verwaiste Doppel-Zeile entstehen. Die Herkunfts-Einstufung eines eingehenden Raids („kennen wir den Sender per ID oder nur per Name?") wertete versehentlich den übergebenen Parameter statt des tatsächlichen Datenbankwerts aus. Und die Testbasis war doppelt unzuverlässig: Tests konnten ohne Datenbank still als „bestanden" durchlaufen, und bei wiederverwendeter Test-Datenbank blieben alte Tabellen stehen, sodass Tests gegen ein veraltetes Schema liefen.

**Was wurde geändert:** Der Score-Refresh filtert das 45-Tage-Fenster jetzt schon in der Datenbank-Abfrage (der bisherige Code-Filter bleibt als Sicherheitsnetz, das Ergebnis ist identisch). Das Produktions-Binary wird mit Link-Time-Optimierung gebaut (kleiner und schneller). Das Hype-Train-Update vergleicht den Startzeitpunkt jetzt NULL-sicher, sodass keine verwaisten Doppel-Zeilen mehr entstehen. Die Raid-Herkunfts-Einstufung nutzt den echten Datenbankwert und entspricht damit wieder dem alten System. Die Test-Helfer legen ihr Schema vor jedem Lauf frisch an (kein Verschleppen alter Tabellen), und es gibt jetzt eine durchgehende CI, die Build, Lints und die komplette Testsuite gegen eine echte Datenbank ausführt.

**Wie es jetzt funktioniert:** Der Score-Refresh überträgt nur noch die wirklich benötigten Daten; das Binary läuft etwas schlanker. Hype-Train-Statistiken bleiben sauber, Raid-Herkunft wird korrekt klassifiziert. Und „grün" in den Tests bedeutet künftig verlässlicher auch „wirklich geprüft", weil die Testdatenbank in der CI immer steht und jeder Testlauf mit frischem Schema startet.

## #132 — !dldc / !dlde: Discord-Link-Command

**Ausgangslage:** Zuschauer fragten in mehreren Partner-Kanälen nach dem Discord-Link des Streamers — bisher keine Bot-Antwort, Streamer musste manuell tippen oder hatte es nicht im Profil hinterlegt.

**Was wurde geändert:** Neuer Chat-Command `!dldc` (Alias `!dlde`) — gibt den für den jeweiligen Streamer bereits generierten Discord-Invite-Link aus. Die Lookup-Logik wurde als nativer Rust-Endpoint in die interne API gebaut (`GET /internal/twitch/v1/streamer/:login/discord-invite`), der direkt die `twitch_streamer_invites`-Tabelle liest. Python empfängt den Chat-Command und ruft nur noch diesen Endpunkt auf — keine Datenbanklogik in Python.

**Wie es funktioniert:** Wer `!dldc` oder `!dlde` tippt, bekommt sofort den hinterlegten Discord-Invite-Link des Streamers als Chat-Antwort. Ist für diesen Kanal noch kein Invite generiert worden, antwortet der Bot mit einem kurzen Hinweis. Fehlt der Eintrag, läuft der Command lautlos durch (kein Chat-Noise).

## #131 — Härtungswelle: Token-Erneuerung race-sicher, Helix-Zeitlimits, klare HTTP-Fehler

**Ausgangslage:** Drei robustheitskritische Befunde aus dem Audit plus eine Vorab-Korrektur. Erstens: Beim Erneuern eines Raid-Tokens konnte ein Wettlauf mit dem parallel laufenden Wartungsprozess dazu führen, dass mit einem bereits verbrauchten Refresh-Token erneuert wird — Twitch entwertet den alten Token bei Benutzung, also stirbt dann die ganze Token-Kette und der Partner wird grundlos zur Neu-Autorisierung aufgefordert. Zweitens: Die Aufrufe an die Twitch-API hatten kein Zeitlimit; ein hängender Twitch-Server konnte den gesamten Überwachungs-Durchlauf einfrieren. Drittens: Bei Fehlerantworten (z. B. Rate-Limit oder ungültige Zugangsdaten) parste der Code direkt das Antwort-JSON und meldete dann ein kryptisches „Feld fehlt" statt des echten HTTP-Status. Viertens (intern, noch nicht aktiv): die künftige Dashboard-Lese-API dekodierte mehrere Datenbankspalten im falschen Typ und wäre beim Scharfschalten sofort gebrochen.

**Was wurde geändert:** Die Token-Erneuerung liest jetzt unter einer prozessübergreifenden Sperre erst den frischesten Stand aus der Datenbank neu und erneuert nur dann — mit genau diesem Token; hat ein anderer Prozess in der Zwischenzeit schon erneuert, wird übersprungen. Abgesichert mit einem Test, der genau diesen Wettlauf nachstellt. Die Twitch-Aufrufe bekommen ein 10-Sekunden-Zeitlimit. Antworten werden zuerst auf ihren HTTP-Status geprüft und erst dann geparst — über einen gemeinsamen Helfer statt des an mehreren Stellen kopierten Musters; die Fehlermeldungen tragen den Status, niemals den Token-Wert. Die Dashboard-Typen sind an das echte Datenbankschema angeglichen, und die zugehörigen Tests laufen jetzt gegen dieselben Spaltentypen wie in der Produktion.

**Wie es jetzt funktioniert:** Die Token-Erneuerung kann sich nicht mehr selbst aussperren — grundlose Neu-Autorisierungs-Aufforderungen durch diesen Wettlauf fallen weg. Ein langsamer oder hängender Twitch-Server bremst den Bot nicht mehr aus, sondern läuft nach spätestens zehn Sekunden in einen sauberen Fehler. Und Betriebsfehler stehen im Log sofort als das da, was sie sind (echter Statuscode), statt als irreführender Parsing-Fehler.

## #130 — Auto-Raid gehört fest zur Partnerschaft: Abschalt-Befehl entfernt

**Ausgangslage:** Partner konnten ihren Auto-Raid per Chat- bzw. Discord-Befehl komplett abschalten. Das lief dem Grundgedanken des Partner-Netzwerks zuwider — gegenseitiges Anraiden beim Offline-Gehen ist genau der Mechanismus, von dem alle profitieren. Ein einzelner Abschalter nimmt nicht nur dem Bot, sondern dem ganzen Netz Reichweite weg.

**Was wurde geändert:** Die Abschalt-Befehle (samt ihrer Kurzformen) sind ersatzlos entfernt — sowohl im Twitch-Chat als auch als Discord-Befehl. Der Befehl zum Autorisieren/Einschalten bleibt unverändert: Wer den Bot noch nicht autorisiert hat, bekommt darüber weiterhin den OAuth-Link, und wer ihn früher abgeschaltet hatte, kann ihn damit jederzeit wieder anschalten. Geblockte Partner und solche mit abgelaufener Autorisierung raiden weiterhin nicht — das ist eine andere Mechanik (Moderation bzw. fehlende Berechtigung), kein abschaltbarer Schalter.

**Wie es jetzt funktioniert:** Sobald ein Partner den Bot autorisiert hat, raidet er beim Offline-Gehen automatisch den passenden anderen Partner — und das bleibt so. Es gibt keinen Selbst-Abschalter mehr; wer raus will, klärt das über einen Admin.

## #129 — Audit-Nachzieher: Raid-Ankunfts-Tracking repariert, Global-Bann von der Raid-Sperrliste getrennt

**Ausgangslage:** Ein zeilengenaues Vollaudit des auf Rust umgestellten Bot-Kerns hat mehrere Altlasten aufgedeckt; drei davon mit direkter Wirkung. Erstens: Das Mitschreiben jedes Raid-Ankunftsereignisses (wer nach einem Raid tatsächlich rübergekommen ist) schlug seit dem Umzug bei jedem Versuch fehl. Die Datenbankspalte für die laufende Nummer ist ein 32-Bit-Ganzzahltyp, der neue Kern erwartete beim Zurücklesen aber 64 Bit — die Datenbank wies den Schreibvorgang strikt ab. Der Fehler wurde an dieser Stelle obendrein stillschweigend verschluckt und tauchte in keinem Log auf, während die Schwester-Funktion daneben denselben Fehler korrekt meldete. Zweitens: Wurde ein Chatter global gebannt, landete sein Kanal fälschlich auch auf der Raid-Sperrliste — obwohl „global gebannter Chatter" und „Kanal, den wir nicht anraiden" zwei verschiedene Dinge sind, die schon im alten System getrennt waren. Drittens: Ein altes Komfort-Schlupfloch aus der Anfangszeit gab Admin-Rechte allein aufgrund einer lokalen Verbindung — hinter einem vorgelagerten Proxy wäre das gefährlich geworden.

**Was wurde geändert:** Der Schreibvorgang fürs Ankunfts-Tracking liefert die laufende Nummer jetzt im passenden Format zurück und schreibt wieder zuverlässig; schlägt ein Insert doch mal fehl, wird er sichtbar geloggt statt verschluckt. Der zugehörige Test läuft jetzt gegen denselben Spaltentyp wie die echte Datenbank, damit genau dieser Fehler nicht erneut unbemerkt durchrutscht. Der Global-Bann spiegelt nicht mehr in die Raid-Sperrliste — beide Listen werden wieder getrennt geführt. Das lokale Admin-Schlupfloch ist ersatzlos entfernt; Admin-Zugriff läuft nur noch über die echte Anmeldung per Token bzw. künftig die Discord-/Twitch-Identität.

**Wie es jetzt funktioniert:** Raid-Ankünfte werden wieder lückenlos erfasst, die Statistik dahinter stimmt wieder. Ein global gebannter Chatter sperrt keinen Kanal mehr versehentlich von Raids aus. Und wer sich nicht echt authentifiziert, bekommt keine Admin-Rechte mehr — unabhängig davon, von wo die Anfrage kommt.

## #128 — Discord-Rollen-Sync beim Speichern des Discord-Profils

**Ausgangslage:** Im Admin-Dashboard kann man für einen Partner eine Discord-User-ID hinterlegen. Die Streamer-Rolle im Discord-Server wurde aber nur bei der initialen Verifizierung vergeben — wer erst nachträglich verknüpft wurde oder dessen ID später gesetzt/korrigiert wurde, bekam die Rolle nie automatisch.

**Geändert:** Sobald im Dashboard eine Discord-ID gespeichert wird, löst der Bot jetzt unmittelbar `sync_streamer_role` aus. Wenn die Person bereits im Server ist, landet die Rolle sofort. Wenn nicht, passiert nichts (kein Fehler, kein Absturz).

**Jetzt:** Discord-Profil speichern = Rolle wird in einem Zug gesetzt, ohne dass ein zweiter manueller Schritt nötig ist.

## #127 — Raid-Priorität aus Plänen zählt wieder im Scoring

**Ausgangslage:** Wer einen Plan mit Raid-Priorität hat (Raid Boost, die Bundles oder „Alles drin"), soll bei der automatischen Raid-Ziel-Auswahl bevorzugt werden. Seit dem Umzug der Score-Berechnung auf den neuen Kern wurde aber nur noch das direkte Boost-Häkchen in der Datenbank geprüft — die Zuordnung „Plan → enthält Raid-Priorität" (inklusive manuell vergebener Pläne mit Ablaufdatum) war schlicht noch nicht mit umgezogen und im Code als offene Lücke vermerkt. Partner, deren Boost allein aus ihrem Plan kommt, wurden im Scoring wie Free-Nutzer behandelt.

**Was wurde geändert:** Die Plan-Zuordnung ist jetzt im neuen Kern nachgebaut, exakt nach derselben Tabelle wie im alten System: Boost gilt, wenn das Datenbank-Häkchen gesetzt ist ODER der Plan-Name auf einen Plan mit Raid-Priorität zeigt ODER ein manuell vergebener Plan mit Raid-Priorität aktiv (nicht abgelaufen) ist. Abgesichert mit Tests, die das Verhalten des alten Systems Punkt für Punkt nachstellen — inklusive Groß-/Kleinschreibung und der verschiedenen Datumsformate beim Ablaufdatum.

**Wie es jetzt funktioniert:** Beim regelmäßigen Score-Refresh bekommen alle Partner mit Raid-Priorität — egal ob per Häkchen, Plan oder manuellem Override — wieder ihren Boost-Multiplikator in der Raid-Ziel-Auswahl. Abgelaufene manuelle Pläne verlieren ihn automatisch.

## #126 — Stream-Reports scheitern nicht mehr an unsauberem KI-Ausgabeformat

**Ausgangslage:** Die automatische Nach-Stream-Analyse (der strukturierte Report nach Stream-Ende) schlug regelmäßig fehl — 15 Mal in den letzten Tagen. Das KI-Modell liefert das Ergebnis als JSON-Datenblock, hängt aber manchmal seinen „Denkprozess" in einem eigenen Block davor oder lässt ein überzähliges Komma vor einer schließenden Klammer stehen. Beides brachte den Parser zum Abbruch: Beim Denkblock griff die Extraktion den falschen Textabschnitt, beim Komma scheiterte das Einlesen — der Streamer bekam dann nur einen leeren Ersatz-Report („Bewertung: gemischt", keine Inhalte). Auch die Wiederholungsversuche scheiterten, weil sie denselben Text durch denselben Parser schickten. Nebenbefund aus demselben Audit: Ein Fehler im Hintergrund-Token-Refresh wurde mit leerer Fehlermeldung geloggt und war damit nicht diagnostizierbar.

**Was wurde geändert:** Vor der JSON-Extraktion werden Denkblöcke des Modells entfernt. Schlägt das Einlesen trotzdem fehl, läuft ein Reparatur-Versuch, der überzählige Kommas vor schließenden Klammern entfernt und es erneut probiert — echte Syntaxfehler werfen weiterhin sauber einen Fehler, nichts wird still geschluckt. Die Härtung greift an allen drei Stellen, die KI-Antworten einlesen (Report, Wortgruppen-Analyse, Kurz-Report). Der Token-Refresh loggt Fehler jetzt mit vollem Fehlertyp und Stacktrace.

**Wie es jetzt funktioniert:** Liefert das Modell sauberes JSON, ändert sich nichts. Liefert es einen Denkblock oder ein überzähliges Komma — die beiden mit Abstand häufigsten Fehlerbilder —, wird der Report trotzdem korrekt erzeugt statt durch den leeren Ersatz-Report ersetzt zu werden.

## #125 — Nachzieher zum Kern-Umzug: Admin-Aktionen und Discord-Verknüpfung repariert

**Ausgangslage:** Nach dem heutigen Umzug auf den neuen Bot-Kern steckten neben dem Onboarding-Fehler (#124) weitere stille Brüche in den Funktionen, die der neue Kern selbst beantwortet — dort greift das Auffangnetz aus #124 nämlich nicht. Ein systematischer Abgleich von altem und neuem Verhalten hat gefunden: Der Archiv-Knopf im Admin-Dashboard schlug fehl (der neue Kern kannte den „Umschalten"-Modus nicht, den das Dashboard standardmäßig sendet). Discord-Profil-Speichern und die automatische Discord-Verknüpfung liefen ins Leere — alte Aufrufer senden Feldnamen mit Unterstrichen, der neue Kern erwartete sie in Binnengroßschreibung, dadurch kamen alle Werte als „leer" an; gespeichert wurde nichts, das Mitglieds-Häkchen wurde dabei sogar jedes Mal entfernt, und die Antwort behauptete trotzdem „ok". Der Discord-Flag-Schalter gab einen Fehler. Chat-Nachrichten und Ankündigungen aus dem Dashboard scheiterten, weil der neue Kern eine beim Start eingefrorene Kopie des Bot-Logins benutzte — das eigentliche Chat-System erneuert diesen Login aber laufend, die Kopie war also nach kurzer Zeit tot. Verifizierungs-Modi (temporär, zurücksetzen) blieben wirkungslos. Und das Dashboard meldete bei jedem Start fälschlich „verschiedene Datenbanken", weil alter und neuer Kern zwei unterschiedliche Prüfsummen-Verfahren benutzten.

**Was wurde geändert:** Alle Streamer-Verwaltungs-Schreibwege beantwortet vorerst wieder der bewährte bisherige Dienst — der neue Kern reicht sie über das Auffangnetz aus #124 durch. Jede dieser Funktionen kehrt erst dann in den neuen Kern zurück, wenn ein Vertragstest beweist, dass sich Anfrage und Antwort exakt gleich verhalten. Die Datenbank-Prüfsumme berechnet der neue Kern jetzt mit genau demselben Verfahren wie das Dashboard, abgesichert durch Referenzwerte aus dem alten System.

**Wie es jetzt funktioniert:** Archivieren, Verifizieren, Discord-Flag, Discord-Profil und Chat-Aktionen laufen wieder über den Pfad, der das seit Jahren korrekt macht — inklusive aller Modi und mit dem stets frischen Bot-Login. Die automatische Discord-Verknüpfung speichert wieder echte Werte statt nichts. Der Datenbank-Fehlalarm beim Dashboard-Start ist weg; geprüft wird weiterhin, nur vergleichen jetzt beide Seiten dasselbe.

## #124 — Raid-Bot-Autorisierung im Onboarding repariert

**Ausgangslage:** Seit dem heutigen Umzug der Bot-Innereien auf den neuen Rust-Kern war die Raid-Bot-Autorisierung kaputt: Wer auf der Streamer-Seite bzw. im Onboarding auf „Raid-Bot verbinden" klickte, bekam statt der Twitch-Anmeldung nur die Fehlermeldung „Raid bot not initialized". Grund: Beim Umzug hat der neue Kern den internen Schnittstellen-Port komplett übernommen, dort aber nur die bereits portierten Funktionen angeboten — der komplette Autorisierungs-Ablauf (Login-Link erzeugen, Twitch-Rückmeldung verarbeiten), die Raid-Sperrliste und ein paar Statistik-Abfragen liefen ins Leere. Das Dashboard interpretierte die fehlende Antwort als „Raid-Bot nicht da".

**Was wurde geändert:** Der neue Kern hat jetzt ein Auffangnetz: Jede Anfrage an eine Funktion, die er selbst (noch) nicht kennt, reicht er unverändert an den bisherigen Dienst weiter, der dafür auf einem internen Nebenkanal weiterläuft. Antwort geht denselben Weg zurück — für das Dashboard sieht alles aus wie vorher.

**Wie es jetzt funktioniert:** Klick auf „Raid-Bot verbinden" → Dashboard fragt den neuen Kern → der erkennt „kenne ich nicht", reicht an den bisherigen Dienst weiter → der erzeugt den Twitch-Login-Link wie gewohnt. Sobald eine Funktion nativ im neuen Kern nachgebaut ist, beantwortet er sie automatisch selbst — das Auffangnetz greift dann nur noch für den Rest. Verifiziert: exakt der Klick, der heute Nachmittag mit Fehler abbrach, liefert jetzt wieder die Twitch-Anmeldeseite mit den korrekten Berechtigungen.

## #123 — !raid funktioniert jetzt auch direkt nach Stream-Ende

**Ausgangslage:** `!raid` hat mit „Du musst live sein" abgebrochen, sobald der Stream offline war — also genau in dem Moment, in dem man den Befehl am dringendsten braucht: Der Stream ist gerade zu Ende, der Auto-Raid hat aus irgendeinem Grund nicht gefeuert, und manuell nachsteuern ging nicht. Dabei startet der Auto-Raid selbst seinen Raid auch erst nach dem Offline-Gehen — Twitch lässt den Raid-Start im Nachlauf des Streams zu.

**Was wurde geändert:** Die harte Live-Sperre ist raus. Wenn Twitch den Kanal als offline meldet, nimmt der Bot jetzt den letzten bekannten Stream-Zustand als Grundlage (Spiel, Zuschauerzahl, Startzeit) und versucht den Raid ganz normal. Die Deadlock-Regel bleibt: `!raid` geht weiter nur, wenn der (letzte) Stream Deadlock war oder gerade erst von Deadlock auf Just Chatting gewechselt wurde.

**Wie es jetzt funktioniert:** `!raid` direkt nach Stream-Ende läuft durch die normale Ziel-Auswahl (Partner zuerst, sonst deutsche Deadlock-Streamer) und startet den Raid. Ist das Twitch-Fenster für Raids schon zu (zu lange offline), lehnt Twitch den Versuch ab und der Bot meldet die echte Fehlermeldung im Chat — statt vorher pauschal zu blocken. Nur wer noch nie gestreamt hat, bekommt weiterhin einen Hinweis, dass es keinen Stream gibt, von dem aus geraidet werden kann.

## #122 — Admin-Chat-Aktion: Versenden funktioniert jetzt wieder vollständig

**Ausgangslage:** Nach dem Fix des 503-Fehlers (#121) schlug das Versenden einer Nachricht immer noch fehl — Fehlermeldung: „Chat-Aktion für [login] konnte nicht gesendet werden". Das Dashboard läuft als eigenständiger Python-Prozess ohne lokalen Twitch-Chat-Bot, weshalb der Fallback-Pfad über die interne Rust-API geht. Der nötige Endpoint `POST /internal/twitch/v1/streamers/{login}/chat-action` war im Rust-Bot aber noch nicht implementiert — stand zwar im HTTP-Vertrag, war aber nicht als Handler gebaut.

**Was wurde geändert:** Der fehlende Endpoint wurde im Rust-Bot (`tb-internal-api`) implementiert. Er liest den Bot-Token und die Bot-User-ID aus Umgebungsvariablen (identisch zur Python-Logik), holt die `twitch_user_id` des Ziel-Streamers direkt aus der DB und sendet die Nachricht über die Twitch Helix API — mit dem Bot-Token des Chat-Accounts.

**Wie es jetzt funktioniert:** Drei Modi werden unterstützt: normaler Chat (`mode=chat`), `/me`-Aktion (`mode=action`, sendet `/me …` als Prefix) und Announcement (`mode=announcement`, nutzt `/helix/chat/announcements` mit wählbarer Farbe). Fehlt die `twitch_user_id` für einen Streamer in der DB, kommt ein 404. Antwortet Helix mit einem Fehler, wird dieser als 502 weitergereicht. Der neue Endpoint ist per Auth-Token + Loopback-Guard gesichert wie alle anderen internen Endpoints.

## #121 — Admin-Chat-Aktion: 503 bei kurzzeitigem DB-Fehler behoben

**Ausgangslage:** Das Absenden einer manuellen Chat-Nachricht an einen Partner-Kanal über das Admin-Dashboard schlug sporadisch mit einem 503-Fehler fehl. Logs zeigten: „Dashboard internal API unavailable (degraded mode). First failure in streamers_list: internal server error" — der Endpunkt lieferte immer dann keinen Response, wenn der interne Rust-API-Call auf `GET /streamers` kurzzeitig mit DB-Fehler (500) antwortete.

**Was wurde geändert:** Die Chat-Aktion hat die komplette Streamer-Liste über die interne API geladen, nur um drei Felder eines einzigen Streamers zu prüfen (`twitch_user_id`, `archived_at`, `manual_partner_opt_out`). Das ist durch einen direkten DB-Lookup gegen die `twitch_partners_all_state`-View ersetzt — keine API-Abhängigkeit mehr für diese Validierung.

**Wie es jetzt funktioniert:** Beim Absenden einer Chat-Nachricht liest der Handler direkt aus der DB. Transiente Fehler in der internen API blockieren den Ablauf nicht mehr. Schlägt die DB selbst fehl, erscheint eine klare Fehlermeldung im Dashboard statt eines nackten 503.

## #120 — Bot war blind für Partner mit nicht-deutscher Kanal-Sprache: Auto-Raid & Werbezitate gefixt

**Ausgangslage:** Ein Partner meldete, dass der Auto-Raid bei ihm nie feuert (manuell ging es) und Werbezitate in seinem Chat komplett ausbleiben. Die Spur in Logs und Datenbank: Der Bot hat seinen Stream über Wochen kein einziges Mal live „gesehen" — keine Stream-Session, keine Kategorie, keine Zuschauerzahlen, nie ein Live-Posting. Grund: Die Live-Abfrage bei Twitch lief mit Sprachfilter Deutsch, und der Partner hat seine Twitch-Kanal-Sprache auf Englisch gestellt. Das Push-Event „Kanal ist live" kam zwar an, enthält aber weder Kategorie noch Zuschauer — und alles, was darauf aufbaut, lief ins Leere: Der Auto-Raid wird am Stream-Ende nur ausgelöst, wenn die letzte Kategorie Deadlock war; bei „Kategorie unbekannt" greift die Sicherheitsregel und überspringt (24 Mal in Folge, als einziger Partner). Werbezitate brauchen Zuschauer-Baseline bzw. Chat-Aktivitätsdaten aus genau diesem Tracking — ohne Daten kein Trigger.

**Was wurde geändert:**

- **Partnerliste ohne Sprachfilter:** Die Live-Abfrage der eigenen, kuratierten Partnerliste läuft jetzt ohne Sprachfilter — wer Partner ist, wird getrackt, egal welche Kanal-Sprache bei Twitch eingestellt ist. Der Deutsch-Filter bleibt nur dort aktiv, wo er hingehört: bei der Entdeckung fremder Deadlock-Streams in der Kategorie-Suche.
- **Kategorie sofort beim Live-Gehen:** Zusätzlich holt der Bot beim „Kanal ist live"-Event jetzt sofort Titel und Kategorie per gezieltem Einzel-Lookup, statt auf den nächsten Abfrage-Takt zu warten. Damit ist die Kategorie ab Sekunde 1 bekannt — auch bei Streams, die kürzer sind als ein Abfrage-Intervall, und unabhängig von jedem Sprachfilter.
- **Nebenfix Kanal-Sprache:** Beim Kategorie/Titel-Änderungs-Event wurde die Kanal-Sprache aus einem falschen Feld der Twitch-Nachricht gelesen und blieb deshalb in der Statistik fast immer leer (1997 von 1998 Einträgen). Jetzt wird das richtige Feld gelesen.

**Wie es jetzt funktioniert:** Geht ein Partner live, kennt der Bot binnen Sekunden Titel und Kategorie; der laufende Abfrage-Takt ergänzt Zuschauerzahlen und legt die Stream-Session an — für alle Partner, egal ob ihr Kanal auf Deutsch, Englisch oder sonstwas steht. Damit funktionieren Auto-Raid am Stream-Ende, Werbezitate, Live-Postings und Stream-Statistiken jetzt auch für Partner mit nicht-deutscher Kanal-Sprache.

**Betroffen:** Alle Partner, deren Twitch-Kanal-Sprache nicht auf Deutsch steht — bei ihnen waren Auto-Raid, Werbezitate, Live-Postings und Statistiken bisher komplett tot, ohne dass es eine Fehlermeldung gab.

## #119 — Drei latente Robustheits-Lücken im Live-Bot geschlossen

**Ausgangslage:** Beim Rust-Port des Monitorings (#110–#116) sind drei Schwachstellen im laufenden Python-Bot aufgefallen, die bisher nur unter Race-/Fehlerbedingungen zuschlagen — also genau dann, wenn man es nicht sieht. Sie wurden zunächst nur im Rust-Code gelöst; jetzt sind sie auch im Live-Bot gefixt.

**Was wurde geändert:**

- **Doppel-Abschluss-Schutz für Sessions:** Stream-Ende kann von zwei Seiten gleichzeitig erkannt werden (Abfrage-Takt und Twitch-Offline-Event). Bisher konnte der zweite Abschluss die bereits berechneten Kennzahlen (Retention, Chatter, Dauer) überschreiben. Jetzt greift der Abschluss nur noch, wenn die Session wirklich noch offen ist — der Verlierer des Rennens räumt nur seinen Zwischenspeicher auf und lässt die korrekten Werte stehen.
- **Doppel-Anlage-Schutz für Sessions:** Das Anlegen einer neuen Session war nur durch einen Zwischenspeicher im Prozess geschützt — bei einem Race konnten zwei offene Sessions für denselben Streamer entstehen. Jetzt sichert eine Datenbank-Sperre pro Streamer plus eine Prüfung in derselben Transaktion ab: Existiert schon eine offene Session, wird deren ID übernommen statt eine zweite anzulegen.
- **Event-Warteschlange übersteht Fehler:** Der Hintergrund-Arbeiter der Event-Verarbeitung konnte bei einem Datenbank-Fehler im Abhol-Schritt still sterben — Events wären ab dann liegen geblieben, ohne dass es jemand merkt. Zusätzlich hätte eine kaputte gespeicherte Nachricht beim Aussortieren einen Absturz im Absturz ausgelöst. Beides abgefangen: Der Arbeiter loggt, wartet kurz und macht weiter; kaputte Nachrichten wandern sauber in die Aussortierten-Liste.

**Nebenbei:** Zwei Tests, die der Code-Entwicklung hinterherhingen (veraltete Attrappen nach Raid-Refactoring), wurden auf den aktuellen Stand gebracht — sie waren rot und hätten echte Regressionen verdeckt.

## #118 — Doppel-Abschluss-Schutz für Stream-Sessions (Rust-Monitoring)

**Problem:** Eine Stream-Session kann theoretisch von zwei Seiten gleichzeitig abgeschlossen werden — vom Abfrage-Takt und vom Twitch-Offline-Event. Ohne Schutz überschreibt der zweite Abschluss die bereits berechneten Kennzahlen (Retention, Chatter, Dauer) mit neuen, dann falschen Werten. Im Python-Original existiert diese Lücke bis heute.

**Geändert:** Der Abschluss im Rust-Monitoring greift jetzt nur noch, wenn die Session wirklich noch offen ist (Bedingung direkt im Datenbank-Update). Kommt ein zweiter Abschluss-Versuch, läuft er ins Leere, räumt nur seinen Zwischenspeicher auf und meldet sauber „war schon zu" — die ersten, korrekten Kennzahlen bleiben unangetastet.

## #117 — Chatter-Statistiken pro Stream waren seit Monaten 0 — gefixt

**Was war das Problem:** Beim Abschluss jeder Stream-Session zählt der Bot, wie viele verschiedene Chatter im Stream geschrieben haben, wie viele davon zum ersten Mal da waren und wie viele Wiederkehrer — diese drei Werte landen in den Session-Statistiken fürs Dashboard. Genau diese Zählung war kaputt: Sie benutzte eine SQL-Summen-Funktion auf einem Wahrheitswert-Feld (`SUM` über ein boolean), und die gibt es in Postgres schlicht nicht. Seit der Umstellung dieser Spalte auf echte Wahrheitswerte schlug die Abfrage bei **jedem** Session-Abschluss fehl. Tückisch daran: Der Fehler wurde intern abgefangen und nur als unauffällige Debug-Zeile geschluckt — nach außen sah alles normal aus, aber jede abgeschlossene Session wurde mit 0 einzigartigen, 0 neuen und 0 wiederkehrenden Chattern gespeichert.

**Was wurde geändert:** Die Zählung nutzt jetzt einen Filter-Ausdruck (`COUNT` mit Bedingung) statt der Summe — das ist die korrekte Postgres-Schreibweise für „zähle nur die Zeilen, bei denen das Flag gesetzt ist". Aufgefallen ist der Bug übrigens nicht im Betrieb, sondern bei der systematischen Verifikation der Datenbank-Zugriffe gegen das echte Schema im Rahmen interner Umbauarbeiten.

**Wie es jetzt funktioniert:** Ab sofort bekommt jede neu abgeschlossene Session wieder echte Chatter-Zahlen: Gesamtzahl der Chatter aus der Session-Chatter-Tabelle, Erstschreiber über das First-Time-Flag, Wiederkehrer als Differenz. **Betroffen:** Alle Sessions seit der Boolean-Umstellung der Chatter-Flags stehen mit 0er-Werten in der Datenbank — die Chat-Nachrichten selbst sind vollständig vorhanden, nur die verdichteten Kennzahlen am Session-Datensatz fehlen. Eine Rückrechnung der Altdaten wäre aus den vorhandenen Chatter-Einträgen möglich, ist aber bewusst nicht Teil dieses Fixes.

## #116 — Monitoring-Cutover vorbereitet (Schritt 4f) — Umschalten bleibt manuell

**Ausgangslage:** Alle Monitoring-Bausteine (#110–#115) existierten als Bibliotheks-Code mit Tests, aber nichts davon war im Rust-Bot-Prozess zusammengesteckt. Für den späteren Umstieg muss der komplette Kreislauf — Twitch abfragen, Zustände schreiben, Events verarbeiten, Discord posten — startklar verdrahtet sein, ohne versehentlich loszulaufen.

**Geändert:** Der Rust-Bot setzt jetzt beim Start alles zusammen: Twitch-API-Anbindung für Streams/Kategorien/Follower/VOD-Vorschau, Discord-Versand über die interne Bridge (inklusive Rückfall auf einen einfachen Link-Button, wenn der Tracking-Button beim Broker nicht verfügbar ist), Event-Verarbeitung und Abfrage-Schleife. **Entscheidend ist der Schutzschalter:** Die Schleife startet nur, wenn sie per Umgebungsvariable ausdrücklich eingeschaltet wird — standardmäßig bleibt der Python-Bot der einzige Schreiber, und der Rust-Prozess beantwortet nur den Event-Empfang. Der Umschaltplan steht jetzt als Checkliste in der internen Doku: welche Variablen zu setzen sind, in welcher Reihenfolge Python-Monitoring aus- und Rust eingeschaltet wird, wie man den Erfolg prüft — plus eine ehrliche Liste der sechs Stellen, die beim Umschalten bewusst noch ohne Nachfolger sind (Raid-Auslösung, Statistik-Anmeldungen für Neue, Rollen-Erstellung, Auto-Archivierung, Offline-Folgeaktionen, Partner-Rekrutierung) und in welcher späteren Phase jede davon ankommt.

**Damit ist Schritt 4 (Monitoring) code-komplett:** sechs Teilschritte, alle gegen eine Wegwerf-Datenbank bzw. Mock-Server getestet, das echte Datenbank-Schema read-only verifiziert. Das tatsächliche Umschalten passiert bewusst nicht automatisch, sondern im geplanten Wartungsfenster.

## #115 — Go-Live-Embeds in Rust (Schritt 4e)

**Ausgangslage:** Der letzte fehlende Monitoring-Baustein vor dem Umschalt-Schritt: die Discord-Ankündigungen. Geht ein Partner mit Deadlock live, postet der Bot ein konfigurierbares Embed mit Ping-Rolle und Tracking-Button; endet der Stream, wird dasselbe Posting zum „ist OFFLINE"-Embed mit VOD-Button umgebaut.

**Geändert:** Das komplette Announcement-System ist nachgebaut — in zwei sauber getrennten Teilen:

- **Template-Engine** (reine Logik, ohne Netz/Datenbank): Jeder Streamer kann sein Embed per gespeicherter Konfiguration anpassen — Titel-/Beschreibungs-/Footer-Vorlagen mit Platzhaltern wie `{channel}`, `{game}`, `{viewer_count}` oder `{uptime}`, Farben (auch als Hex), eigene Felder, Bild-Modi (Stream-Vorschau, eigenes Bild, keins) und Erwähnungs-Regeln. Unbekannte Platzhalter bleiben sichtbar stehen, Discord-Längenlimits werden eingehalten, und das Vorschaubild bekommt einen stabilen Cache-Buster, damit Discord beim erneuten Senden kein veraltetes Bild zeigt.
- **Broker-Versand:** Gesendet wird wie bisher über die interne Discord-Bridge mit deterministischem Idempotenz-Schlüssel — derselbe Inhalt kann nicht doppelt gepostet werden, auch wenn der Versand wiederholt wird. Schlägt das Senden fehl, merkt sich der Bot Tracking-Token und Render-Zeitpunkt und versucht es im nächsten Durchlauf mit identischem Token erneut. Das Stream-Ende editiert das Posting zum Offline-Embed (inkl. VOD-Vorschaubild des letzten Streams, sofern abrufbar) und hängt einen einfachen VOD-Link-Button an.

**Bewusste Grenze:** Das automatische *Anlegen* der „ist live"-Ping-Rollen braucht die Discord-Verbindung des Python-Bots — Rust verwendet die bereits hinterlegte Rollen-ID. Auch das ist im Cutover-Plan als offener Punkt notiert.

## #114 — EventSub-Subscription-Verwaltung in Rust (Schritt 4d, Teil 2)

**Ausgangslage:** Empfangen konnte Rust seit #113 — aber jemand muss Twitch auch sagen, welche Events es überhaupt schicken soll. Diese Anmeldungen (Subscriptions) leben bei Twitch und müssen pro Streamer angelegt, beim Start abgeglichen und bei departnerten Streamern wieder abgeräumt werden.

**Geändert:** Rust verwaltet jetzt die Kern-Anmeldungen des Monitorings selbst — live gegangen, offline gegangen, Titel-/Kategoriewechsel. Der Ablauf wie im Original: Geht ein Raid-fähiger Partner live, wird sofort seine Offline-Anmeldung sichergestellt (damit das Stream-Ende nicht verpasst wird); antwortet Twitch „existiert schon", zählt das als Erfolg statt als Fehler; ein Speicher im Prozess merkt sich bestehende Anmeldungen (beim Start aus dem Twitch-Bestand aufgebaut), damit nicht bei jedem Event neu angelegt wird. Das Aufräumen löscht gezielt nur Anmeldungen der eigenen Callback-Adresse, deren Ziel-Streamer nicht mehr aktiv ist — fremde Anmeldungen werden nie angefasst. Jede Offline-Anmeldung schreibt außerdem einen Kapazitäts-Schnappschuss fürs Admin-Dashboard.

**Bewusste Grenze:** Die Statistik-Anmeldungen (Bits, Subs, Bans, …) brauchen die verschlüsselten Streamer-Tokens aus dem Raid-Bereich — der kommt erst in einer späteren Phase. Bestehende Anmeldungen liefern unabhängig davon weiter; nur für *neue* Partner würde im Übergangsfenster niemand Statistik-Anmeldungen anlegen. Das ist als offener Punkt im Cutover-Plan festgehalten statt halbgar mitgebaut.

## #113 — EventSub-Empfang in Rust (Schritt 4d, Teil 1) + Exists-Check-Fix

**Ausgangslage:** Twitch-Events (live gegangen, offline, Titelwechsel, Bits, Subs, Follows, …) erreichen den Bot über die Dashboard-Bridge, die sie per HTTP an die interne API auf Port 8776 zustellt. Beim Cutover übernimmt Rust diesen Port — der Empfangsweg musste also vorher vollständig stehen.

**Geändert:** Die Rust-API hat jetzt denselben Zustell-Endpoint wie Python, mit identischem Vertrag (die Bridge merkt keinen Unterschied). Dahinter steckt die volle Verarbeitungskette:

- **Annahme:** Jede Nachricht wird über den persistenten Dedup-Speicher geprüft (10 Minuten Fenster) — Doppelzustellungen werden sofort als Duplikat beantwortet. Schlägt die Annahme fehl, wird der Dedup-Eintrag wieder freigegeben und die Bridge puffert den Event durable und versucht es erneut — es geht nichts verloren.
- **Kern-Events** (live/offline/Titelwechsel) wandern in die durable Warteschlange aus #110 und werden vom Worker verarbeitet: live → minimaler Live-State sofort (der Poll-Tick füllt den Rest), offline → Session abschließen + Live-State auf offline (mit 120-Sekunden-Drossel gegen Online/Offline-Flattern und Doppel-Trigger durch Polling), Titelwechsel → Protokoll-Eintrag + Live-State-Update. Jeder fachliche Effekt ist pro Nachricht exactly-once absichert (7-Tage-Gedächtnis) — auch wenn dieselbe Nachricht mehrfach verarbeitet wird, feuert z. B. der Go-Live-Trigger nur einmal.
- **Telemetrie-Events** (Bits, Subs/Gifts/Resubs, Follows, Werbepausen, Hype-Trains, Bans, Shoutouts, Erstnachrichten) werden direkt in die Statistik-Tabellen geschrieben, inklusive Zuordnung zur laufenden Session. Hype-Train-Enden aktualisieren das passende offene Begin-Event statt zu doppeln.
- **Raid-Events** gehen an einen Andockpunkt fürs Raid-Subsystem (kommt in einer späteren Phase) statt in der Warteschlange zu verhungern.

**Nebenbei gefunden und gefixt:** Der „Streamer hinzufügen"-Endpoint der Rust-API aus Schritt 3b hatte einen Typ-Fehler im Duplikats-Check — wer einen bereits vorhandenen Streamer anlegte, bekam einen 500er statt der sauberen „existiert schon"-Antwort. Aufgefallen, weil die Tests jetzt konsequent gegen eine echte Wegwerf-Datenbank laufen statt übersprungen zu werden.

## #112 — Poll-Loop in Rust (Schritt 4c)

**Ausgangslage:** Mit Fundament (#110) und Schreibkern (#111) fehlte der Taktgeber des Monitorings: die Schleife, die alle 15 Sekunden bei Twitch abfragt, wer von den getrackten Streamern live ist, und daraus alle Zustandsübergänge ableitet.

**Geändert:** Der komplette Poll-Durchlauf ist jetzt in Rust nachgebaut, in derselben Mechanik wie das Original: getrackte Streamer plus ein Kategorie-Sample (bis 400 Streams, mit Sprachfiltern und Cursor-Pagination) werden von der Twitch-API geholt, daraus entstehen die Übergänge — frisch live (Session anlegen), offline (Session abschließen samt Kennzahlen), Stream-Neustart (alte Session zu, neue auf), Spielwechsel (Transition-Protokoll). Dazu die Pflege-Kadenzen: Statistik-Samples pro Tick, alle 10 Ticks verwaiste Sessions schließen und abgelaufene Dedup-Einträge abräumen, höchstens alle 15 Minuten die Auto-Archiv-Prüfung (Partner ohne Deadlock-Stream seit 10 Tagen). Das Abfrage-Intervall bleibt zur Laufzeit verstellbar (5–3600 Sekunden, aus der Datenbank gelesen, ungültige Werte fallen auf 15 zurück).

**Wie es zusammenhält:** Alles mit Außenwirkung — Discord-Postings, EventSub-Anmeldungen, Raid-Bewertungs-Updates, Archivierungen — läuft über definierte Andockpunkte, die aktuell auf „tue nichts" stehen. Dadurch kann der Rust-Loop heute schon vollständig gegen eine Wegwerf-Datenbank getestet werden (Transitions über mehrere Ticks, Neustart-Erkennung, Kadenzen), ohne dass er irgendetwas nach außen sendet — die Andockpunkte werden in den nächsten Schritten (EventSub, Announcements, Cutover) einzeln scharfgeschaltet. Die Twitch-API-Anbindung selbst (Streams, Kategorie-Suche mit Cache, Follower-Zahl) ist gegen einen Mock-Server getestet.

## #111 — Monitoring-Write-Core in Rust (Schritt 4b) + zwei Prod-Befunde

**Ausgangslage:** Nach dem Idempotenz-Fundament (#110) fehlte der eigentliche Schreibkern des Monitorings: Wer ist live, welche Stream-Session läuft, Viewer-Verläufe, Statistik-Zeitreihen. Das ist der Teil, der pro Poll-Tick in die Datenbank schreibt.

**Geändert:** Der komplette Write-Core ist jetzt in Rust nachgebaut — Live-State (inkl. Drift-Schutz: wechselt ein Login die Twitch-User-ID, wird die alte Zeile vorher entfernt, Konflikt-Schlüssel bleibt die User-ID), Session-Lebenszyklus (Anlegen, Viewer-Samples mit laufendem Durchschnitt/Peak, Abschluss mit Retention-Kennzahlen bei 5/10/20 Minuten, größtem Zuschauer-Einbruch, Chatter-Zählung und Follower-Differenz), Auto-Aufräumen verwaister Sessions (Scout-Reste nach 24 h, eingeschlafene Sessions 1 h nach dem letzten Viewer-Datenpunkt) und die Statistik-Zeitreihen. Die experimentellen Analytics-Tabellen werden dünn mitgeschrieben, weil AI-Reports sie lesen.

**Beim Verifizieren des echten Datenbank-Schemas (read-only) kamen zwei Dinge ans Licht:**

- **Die Tabellen-Definitionen im Python-Code sind veraltet.** Die echte Datenbank führt für Sessions längst echte Zeitstempel-, Boolean- und Bigint-Spalten statt der im Code behaupteten Text-/Integer-Typen. Der Rust-Port bindet die echten Typen direkt; ein neuer Schema-Vertrags-Test schlägt künftig an, wenn Prod davon abweicht.
- **Ein stiller Prod-Bug:** Die Chatter-Zählung beim Session-Abschluss benutzt eine SQL-Funktion, die es für Boolean-Spalten nicht gibt (`SUM` auf einem Wahrheitswert). Seit der Umstellung dieser Spalten schlägt die Abfrage bei jedem Abschluss fehl, der Fehler wird nur als Debug-Zeile geschluckt — **jede Session wurde mit 0 einzigartigen/neuen/wiederkehrenden Chattern gespeichert**. Die Rust-Version zählt korrekt über einen Filter-Ausdruck.

**Außerdem bewusst robuster als das Original:** Das Anlegen einer Session ist jetzt datenbankseitig gegen Doppel-Einträge abgesichert (Sperre pro Login + Prüfung in derselben Transaktion) — Python verlässt sich nur auf einen In-Memory-Cache, was bei einem Race zwei offene Sessions erzeugen kann. Abgesichert mit 16 Tests gegen eine Wegwerf-Datenbank plus Kennzahlen-Unit-Tests.

## #110 — Idempotenz-Fundament fürs Monitoring (Rust Schritt 4a)

**Ausgangslage:** Schritt 4 des Rust-Umbaus portiert das Monitoring — den Teil des Bots, der weiß wer live ist, Sessions und Statistiken schreibt und Go-Live-Posts auslöst. Bevor irgendein Schreibpfad portiert werden kann, braucht es das Fundament, auf dem dort alles ruht: die Mechanik, die garantiert, dass jedes Twitch-Event genau einmal wirkt, auch wenn es doppelt ankommt oder die Verarbeitung mittendrin abstürzt.

**Geändert:** Neues Rust-Modul mit den zwei Bausteinen, die Python und Rust sich während der Migration über dieselben Tabellen teilen:

- **Guard-Store** (`eventsub_guard_state`): Ein „Claim" gewinnt genau dann, wenn für den Schlüssel kein aktiver Eintrag existiert — entschieden durch einen einzigen bedingten Datenbank-Upsert, also ohne Race-Fenster zwischen Prüfen und Schreiben. Damit werden doppelte Event-Zustellungen, Online/Offline-Flapping und doppelte fachliche Effekte (z. B. zwei Go-Live-Posts für denselben Stream-Start) unterdrückt.
- **Processing-Inbox** (`twitch_eventsub_processing_inbox` + Dead-Letter): Eingehende Events landen erst durable in einer Warteschlange; ein Worker leased fällige Aufträge (Lease 30 s, Batch 20), verarbeitet sie und wiederholt Fehlschläge mit wachsendem Abstand (1 s verdoppelnd, Cap 60 s). Nach 5 Versuchen wandert der Auftrag in die Dead-Letter-Tabelle und kann von dort per Knopfdruck zurück in die Queue. Das Leasing läuft über Row-Locks mit Skip — mehrere Worker (auch der Python-Prozess parallel) stehlen sich keine Aufträge.

**Bewusst anders als das Python-Original:** Die Müllabfuhr abgelaufener Guard-Einträge läuft nicht mehr bei jedem einzelnen Claim mit (unnötiger Schreib-Traffic pro Event), sondern als periodischer Sweep. Zwei latente Python-Schwächen sind nicht mitportiert: Ein Datenbankfehler beim Lease konnte den Verarbeitungs-Worker still sterben lassen, und ein kaputtes Payload-JSON hätte beim Dead-Letter-Alarm den Worker gecrasht — der Rust-Worker loggt, wartet und macht weiter. Außerdem entschieden: Der WebSocket-Fallback-Transport wird nicht portiert (Prod läuft nachweislich über Webhook), und die experimentellen Session-Tabellen werden dünn mitgeführt, weil AI-Reports sie lesen. Abgesichert mit 9 Tests gegen eine Wegwerf-Datenbank.

## #109 — EventSub stream.offline sofort nach Auth registrieren

**Problem:** Wenn ein Streamer die Raid-Auth abschloss während sein Stream bereits lief, wartete der Bot bis zu 45 Minuten auf den nächsten Polling-Tick, bevor die `stream.offline` EventSub-Subscription angelegt wurde. In diesem Fenster konnte der Bot ein Offline-Event komplett verpassen — passiert heute bei cheazycrust: Auth um 16:42, Subscription erst 17:27, erster Offline-Event dazwischen verpasst.

**Ursache:** `complete_setup_for_streamer()` erledigte Mod-Hinzufügen und Chat-Join, rief aber nie `_handle_stream_went_live()` auf. Diese Funktion — eigentlich für den Polling-Tick zuständig — ist der einzige Ort, der die Subscription registriert.

**Fix:** `PartnerSetupService` bekommt einen optionalen `stream_went_live_fn`-Callback. Direkt nach dem Auth-Setup (Mod + Chat + Willkommensnachricht) wird dieser aufgerufen und registriert die `stream.offline`-Subscription sofort. Der Callback ist idempotent — läuft der Streamer gerade nicht live, passiert schlimmstenfalls gar nichts.

## #108 — Streamer-CRUD-Endpoints (Rust Schritt 3b)

**Ausgangslage:** Nach Schritt 3a hatte das neue `tb-bot`-Binary die Global-Ban-Endpoints, aber noch kein Äquivalent für die Streamer-Verwaltung — Add, Remove, Verify, Archive/Block und Discord-Profil-Updates fehlten komplett.

**Geändert:** 7 neue Endpoints unter `/internal/twitch/v1/streamers/`:

- **`GET /streamers`** — Liste aller nicht-archivierten Streamer.
- **`POST /streamers`** — Streamer hinzufügen: holt die Twitch-User-ID über die Helix-API, prüft ob der Login schon aktiv ist, führt danach einen Upsert in `twitch_streamers` + `twitch_streamer_identities` durch. Ohne konfigurierten Twitch-Client → 503.
- **`DELETE /streamers/{login}`** — Departnern: setzt `archived_at = NOW()` auf dem aktiven Eintrag und löscht `twitch_live_state`. War der Login noch nie aktiv, wird der Eintrag direkt gelöscht.
- **`POST /streamers/{login}/verify`** — Setzt `manual_verified_permanent = 1` in `twitch_partners` wenn ein aktiver Eintrag vorhanden ist; 404 sonst.
- **`POST /streamers/{login}/archive`** — Archivieren/Blockieren per `mode`-Feld: `archive`/`unarchive` steuert `admin_archived_at`, `block`/`unblock` steuert `technical_pause_reason` + Opt-Out-Flag.
- **`POST /streamers/{login}/discord-flag`** — Setzt `is_on_discord` (Partner-Pfad via `twitch_partners`, Fallback via `twitch_streamers`).
- **`POST /streamers/{login}/discord-profile`** — Setzt `discord_user_id` + `discord_display_name` (gleicher Dual-Pfad).

Discord-Rollen-Sync und EventSub-Callbacks sind bewusst weggelassen (kommen mit Schritt 5/6). `HelixClient` wird beim Start aus den Env-Variablen `TWITCH_CLIENT_ID`/`TWITCH_CLIENT_SECRET` gebaut und als Extension im Router eingehängt.

---

## #107 — Internes API-Binary tb-bot (Rust Schritt 3a)

**Ausgangslage:** Die Python-interne API auf Port 8776 bedient alle CRUD-Operationen des Dashboards. Für den Rust-Umbau braucht es ein neues Binary das diesen Port übernehmen kann — zunächst nur mit den einfachsten, rein datenbankbasierten Endpoints.

**Geändert:** Neues Rust-Binary `tb-bot` auf `127.0.0.1:8776` mit neuem Crate `tb-internal-api`. Implementiert sind 5 Endpoints:

- **`GET /internal/twitch/v1/healthz`** — `{"ok": true, "service": "twitch-internal-api"}` mit DB-Schema-Fingerprint (SHA-256 über `information_schema.tables`) und Connection-Metadaten (Host, Port, DB, User — aus DSN geparst).
- **`POST /internal/twitch/v1/globalban/add`** — Upsert in `twitch_chatter_global_ban` + Einbahn-Spiegel in `twitch_raid_blacklist`, beides in einer Transaktion.
- **`POST /internal/twitch/v1/globalban/remove`** — Löscht aus `twitch_chatter_global_ban` und `twitch_chatter_global_ban_applied` (Sweep-Ledger), damit ein späterer Re-Add sauber über alle Kanäle ausrollt.
- **`GET /internal/twitch/v1/globalban/check?login=...`** — Boolean-Check (Login oder Chatter-ID).
- **`GET /internal/twitch/v1/globalban`** — Alle Einträge, neueste zuerst.

Auth: `X-Internal-Token`-Header + Loopback-Guard (Defense-in-Depth, da 127.0.0.1-Binding allein schon externe Verbindungen blockiert). healthz antwortet auch bei Fingerprint-Fehler immer 200.

**Wie es jetzt funktioniert:** `tb-bot` liegt als eigenes Binary neben `tb-dashboard` im Workspace. Schritt 3b (Streamer-CRUD mit Twitch-API-Lookup) folgt als nächster Schritt.

---

## #106 — Admin-Streamer-Endpoints (Rust Schritt 2b)

**Ausgangslage:** Das Rust-Dashboard konnte nach Schritt 2a zwar den Systemzustand abfragen, hatte aber noch keinen Ersatz für die Python-seitigen Admin-Streamer-Übersichten — keine Möglichkeit, alle Partner nach Status zu filtern oder einen einzelnen Streamer vollständig abzufragen.

**Geändert:** Zwei neue Admin-only-Endpoints unter `/twitch/api/admin/streamers/`:

- **`GET /twitch/api/admin/streamers?view=<view>`** — gibt alle Streamer eines bestimmten Views zurück: `active`, `archived`, `departnered`, `blocked`, `non_partner`, `token_error` oder `all`. Ungültiger View → 400 mit Liste der erlaubten Werte.
- **`GET /twitch/api/admin/streamers/{login}`** — vollständige Detail-Ansicht eines Streamers mit Settings, OAuth-Zustand (inkl. Scope-Diff gegen die 7 Required-Scopes), Stream-Stats und letzten 10 Sessions.

Beide Endpoints berechnen den OAuth-Status (`connected/partial/reauth/missing`) und den logischen Partner-Status (`active/archived/departnered/blocked/non_partner/token_error`) lokal aus den DB-Feldern — exakt dieselbe Logik wie bisher in Python.

**Wie es jetzt funktioniert:** Die Query nutzt 5 CTEs (Billing, Partner-State mit ROW_NUMBER-Dedup, Live-State mit User-ID-bevorzugtem JOIN, OAuth mit Reauth-Flag, letzte Stream-Session). Der WHERE-Zweig für den jeweiligen View ist als statischer String im Code hinterlegt und wird per `format!` eingesetzt — kein dynamischer SQL-Wert, kein Injection-Risiko. Zusätzlich wurde `ApiError::not_found()` und `ApiError::bad_request_with_body()` in `tb-http-core` ergänzt.

---

## #105 — Admin-Session: 5-Minuten-Cap beim Cross-Dashboard-Login entfernt

**Ausgangslage:** Wenn man sich am Discord-Dashboard einloggte und dann das Twitch-Dashboard öffnete, holte der Twitch-Bot die Session per internem API-Call vom Discord-Bot — soweit korrekt. Das Problem: Die lokal gecachte Session wurde dabei mit `expires_at = now + 300` gespeichert (hardcoded 5 Minuten), obwohl die eigentliche Session im Discord-Bot noch 2 Wochen gültig war. Nach 5 Minuten galt die lokale Kopie als abgelaufen; Route-Handler sahen keine gültige Session mehr und verhielten sich inkonsistent, während Caddys Forward-Auth (der parallel re-validiert) noch 200 zurückgab.

**Geändert:** Der 300-Sekunden-Hardcode in `_fetch_discord_dashboard_session` wurde durch `_discord_admin_session_ttl` ersetzt. Die gecachte Synth-Session läuft jetzt genauso lang wie eine lokal erstellte Admin-Session.

**Wie es jetzt funktioniert:** Login am Discord-Dashboard → Cookie wird am Twitch-Dashboard erkannt → beim ersten Zugriff wird die Session vom Discord-Bot geholt und lokal mit voller TTL gespeichert. Route-Handler und Forward-Auth sehen ab dann konsistent dieselbe gültige Session.

## #104 — Admin System-Endpoints (Rust Schritt 2a)

**Ausgangslage:** Das Rust-Dashboard hatte nach Schritt 1b zwar Auth-geschützte Analytics-Routen, aber keine Möglichkeit, den Gesundheitszustand des laufenden Systems abzufragen — kein Uptime, kein Memory, kein Überblick über DB-Größe, EventSub-Zustand oder Error-Log.

**Geändert:** Vier neue Admin-only-Endpoints unter `/twitch/api/admin/system/`:

- **`/health`** — Prozess-Uptime (OnceLock-Timestamp aus `main.rs`), RSS-Memory aus `/proc/self/status`, PID, letzter DB-Tick aus `twitch_live_state`, Raw-Chat-Ingest-Lag aus `twitch_raw_chat_ingest_health`. Lag-Warning (`RAW_CHAT_LAG`) wird nur ausgelöst wenn der herangezogene Streamer gerade live ist — Offline-Fallback läuft stumm durch.
- **`/database`** — DB-Gesamtgröße + Row-Count/Size für 10 definierte Tabellen via `pg_class.reltuples` + `pg_total_relation_size`. Nicht-existierende Tabellen werden einfach weggelassen.
- **`/eventsub`** — Neuester EventSub-Snapshot aus `twitch_eventsub_capacity_snapshot` (nur Zeilen mit `listener_count > 0` und gefülltem JSON, max. 200 Einträge zurück).
- **`/errors`** — Paginierter Error-Log aus `twitch_admin_error_log` (page/page_size Query-Params, Default 1/25). Falls die Tabelle noch nicht existiert: leere Antwort statt 500.

Alle vier Routen verlangen den internen Token (`X-Internal-Token`), 401 sonst. Der Live/Fallback-JOIN in der Health-Query nutzt `LOWER()`-Vergleich — konsistent mit allen anderen JOINs im Codebase. `sqlx`-Workspace-Feature `chrono` ergänzt (war bisher nicht gesetzt).

**Wie es jetzt funktioniert:** `build_admin_system_router` wird in `build_router` per `.merge()` eingebunden. Der Uptime-Timestamp wird einmalig in `main.rs` nach dem DB-Connect gesetzt, damit er die echte Startzeit des Prozesses widerspiegelt — nicht den Zeitpunkt des ersten Health-Requests.

## #103 — Auth-Layer + Admin-Analytics-Routen (Rust Schritt 1b)

**Ausgangslage:** Der Rust-Rewrite hatte nach Schritt 1a drei öffentliche Read-only-Endpoints ohne jede Zugriffskontrolle. Alle Admin-seitigen Analytics-Routen — Streamer-Liste, Session-Übersicht — waren noch ausschließlich im Python-Backend.

**Geändert:** Rust `tb-dashboard`-Binary (Port 8767) bekommt einen vollständigen Auth-Layer und drei neue Admin-Routen:

- **Auth-Level-Extraktion** (`AuthLevel`): Jeder Request wird eingestuft — `Localhost` wenn Loopback-IP + localhost-Host-Header (kein Token nötig), `Admin` bei korrektem `X-Internal-Token`-Header (constant-time-Vergleich), `None` sonst. Der Extractor sitzt als axum-`FromRequestParts`-Impl direkt im Request-Pipeline, kein Middleware-Wrapper nötig. Partner-Session-Auth (Fernet-Cookie) bleibt deferred (ADR 0003 — kein Fernet-Nachbau, Migration erst wenn Login-Flow auf Rust wechselt).
- **IDOR-Guard** (`require_owner`): Blockiert `None`-Requests mit 401, lässt Admin/Localhost durch — vorbereitet für Partner-Level wenn es kommt.
- **Plan-Gating** (`require_extended_plan`): Liest `manual_plan_id` + `manual_plan_expires_at` aus `streamer_plans`, prüft gegen die bekannten Extended-Plan-IDs (`analytics_pro`, `analytics_extended`), respektiert Ablaufdaten. Admin/Localhost überspringen die Prüfung.
- `GET /twitch/api/v2/auth-status` — antwortet immer 200, liefert `auth_level` + `logged_in`; nützlich für Frontend-Conditional-Rendering.
- `GET /twitch/api/v2/streamers` — Admin-only; gibt alle aktiven Partner mit Live-Status + Viewer-Count zurück, sortiert nach Live-Zustand dann Viewer-Zahl.
- `GET /twitch/api/v2/overview?streamer=<login>&days=30` — Admin-only; aggregierte Session-Metriken (avg/peak Viewers, Airtime, Hours-Watched, Follower-Delta, Session-Count) für einen konfigurierbaren Zeitraum (7–365 Tage). Leerer Zeitraum liefert `{empty: true}` statt 500.

**Wie's jetzt funktioniert:** `into_make_service_with_connect_info` injiziert die echte Client-IP in jede Request-Extension — ohne diesen axum-Aufruf würde `ConnectInfo` nie befüllt und `AuthLevel::Localhost` in Produktion nie feuern. Alle SQL-Zeitvergleiche nutzen explizite `::TIMESTAMPTZ`-Casts (PostgreSQL lehnt `TEXT >= TIMESTAMPTZ` sonst ab), Aggregat-Summen explizit `::FLOAT8`/`::BIGINT` um NUMERIC-Rückgaben zu vermeiden. Jeder Test läuft in einem eigenen PostgreSQL-Schema (`SET search_path TO test_xxx`) — keine parallelen Testkonflikte möglich.

## #102 — Werbefrei deckt jetzt auch die Fake-Server-Warnung ab

**Problem:** Streamer mit Werbefrei-Abo (`chat.promos.disable` bzw. gesetztes `promo_disabled`) sollen keine automatischen Bot-Werbe-Announcements im Chat bekommen. Die regulären Promos (Chat-Aktivität, Viewer-Spike) hielten sich daran, weil sie über `_send_promo_message` laufen, das die Werbefrei-Sperre prüft. Die periodische Promo-Schleife rief aber zwei Inhalte — die Fake-Server-/Scam-Warnung und die Targeted-Promo — *direkt* auf, also am gemeinsamen Sende-Pfad und damit an der Sperre vorbei. Ergebnis: Ein Werbefrei-Kanal sah trotzdem die orange Warn-Announcement (die de facto den eigenen offiziellen Discord bewirbt) und ggf. die personalisierte Promo.

**Geändert:** Die Werbefrei-Sperre greift jetzt eine Ebene höher — direkt an der Quelle der Live-Kanal-Liste, über die die periodische Schleife iteriert. Gesperrte Kanäle werden vorab herausgefiltert, bevor irgendein Sende-Pfad sie überhaupt erreicht.

**Wie's funktioniert:** Sobald die Schleife die Liste der aktuell live-Kanäle geholt hat, läuft jeder Kanal einmal durch dieselbe Sperr-Prüfung, die der reguläre Promo-Pfad schon nutzt (DB-Lookup auf `promo_disabled` + Plan-Entitlement `chat.promos.disable`). Kanäle, die gesperrt sind, fallen aus der Liste — die nachfolgende Schleife sieht sie gar nicht mehr, weder für die Scam-Warnung noch für Targeted-, Stats- oder Viewer-Spike-Promos. Die Lurker-Steuer bleibt unberührt: Sie nutzt eine eigene Kanal-Liste und ist ein vom Streamer selbst aktiviertes Feature, kein aufgedrängter Werbeinhalt. Damit gilt „werbefrei heißt wirklich werbefrei" konsistent für alle automatischen Announcements statt nur für einen Teil der Sende-Pfade.

## #101 — Jeder Partner bekommt jetzt seinen eigenen Discord-Invite-Link

**Problem:** Der Bot sollte pro Streamer einen eindeutigen Discord-Invite erstellen, damit die Quelle jedes Joins (welcher Kanal hat jemanden reingebracht) nachvollziehbar ist. In der Praxis fiel der Bot aber regelmäßig auf den allgemeinen Fallback-Link zurück, besonders bei neu eingetragenen Partnern. Die Ursache: Der Twitch-Bot läuft als eigener Prozess mit einem eigenen Discord-Account — und dieser Account ist kein Mitglied im Deadlock-Discord-Server. `bot.guilds` war daher immer leer, `channel.create_invite()` wurde nie aufgerufen, und jeder Invite-Erstellungsversuch schlug lautlos fehl.

**Geändert:** Invite-Erstellung läuft jetzt über den internen Master-Broker (Port 8770). Der Hauptbot (der tatsächlich im Server ist) bekommt einen neuen Endpunkt `/internal/master/v1/discord/create-invite` und erstellt den Invite auf Anfrage. Der Twitch-Bot ruft diesen Endpunkt per HTTP via dem gleichen Token auf, das er ohnehin für Nachrichten-Routing nutzt.

Zusätzlich läuft beim Start ein Backfill-Task (`_ensure_partner_invites`), der alle aktiven Partner ohne Invite-Eintrag in der DB sequenziell nachversorgt — mit 0,5s Pause zwischen den Requests und einem automatischen Retry-Pass (60s Wartezeit) falls transiente Timeouts einzelne Requests beim Startup-Burst treffen.

**Wie's funktioniert:** Beim Bot-Start wartet `_ensure_partner_invites` auf das Discord-Ready-Event, fragt dann die DB nach Partnern ohne Eintrag in `twitch_streamer_invites`, und ruft für jeden `_create_streamer_invite` auf. Diese Methode baut eine HTTP-POST-Anfrage an den Broker mit `channel_id`, `reason` und einem zufälligen Idempotency-Key. Der Broker löst den Channel auf (`get_channel` / `fetch_channel`), ruft `channel.create_invite(max_age=0, max_uses=0, unique=True)` auf und gibt `invite_url`, `code`, `guild_id` und `channel_id` zurück. Der Twitch-Bot speichert das Ergebnis in `twitch_streamer_invites` (UPSERT) und cached es im Memory-Dict.

## #100 — Jahresabo: Bonusmonate automatisch + Admin-Planvergabe vollständig

**Problem:** Drei separate Baustellen. Erstens: Wer ein Jahresabo über den neuen Checkout-Flow buchte, bekam die versprochenen 2 Bonusmonate nicht — der Webhook hat sie nie in die DB geschrieben, weil das `bonus_months`-Feld nur im alten Legacy-Flow in die Stripe-Subscription-Metadata gesetzt wurde, im neuen API-Checkout-Endpunkt aber fehlte. Zweitens: Das Admin-Dashboard hatte im Dropdown zur manuellen Planvergabe nur 4 der 8 verfügbaren Pläne — `chat_quiet` (Werbefrei) und die neueren Bundles fehlten komplett, was das manuelle Setzen für diese Pläne blockierte. Drittens: Das Ablaufdatum-Feld zeigte das ISO-Format `YYYY-MM-DD`, was Tag und Monat verwechselbar macht.

**Geändert:** (1) Der neue API-Checkout-Endpunkt setzt jetzt bei `cycle_months = 12` automatisch `subscription_data.metadata.bonus_months = "2"` in die Stripe-Session — der Webhook liest das nach Zahlungseingang aus und verlängert das Ablaufdatum um 62 Tage über das Abo-Ende hinaus. (2) Das Admin-Dropdown enthält jetzt alle 8 Pläne inkl. Werbefrei, Bundle Werbefrei+Analyse und Bundle Komplett. (3) Das Ablaufdatum-Feld ist jetzt ein nativer Datums-Picker, der im Browser im deutschen Format (TT.MM.JJJJ) anzeigt.

**Wie's funktioniert:** Jahreskäufer erhalten ab sofort ihre Bonusmonate vollautomatisch: Stripe feuert `checkout.session.completed`, der Bot liest `bonus_months: 2` aus der Subscription-Metadata, addiert 62 Tage auf das `current_period_end` und schreibt das Ergebnis als `manual_plan_expires_at` in die DB — kein Admin-Eingriff mehr nötig. Checkout ohne verknüpften Login wird in beiden Flows jetzt hart geblockt: der alte Abbo-Flow leitet zurück zur Abo-Seite, der neue API-Flow gibt 401 zurück — eine Zahlung ohne zugeordnetem Account ist damit nicht mehr möglich.

## #99 — Global-Ban-Sweep nur noch für OAuth-autorisierte Kanäle

**Problem:** Der tägliche Ban-Sweep hat alle aktiven Partner-Kanäle durchgearbeitet — auch solche, die sich nie per OAuth autorisiert haben und bei denen der Bot gar kein Moderator sein kann. Das war verschwendete Arbeit und produzierte unnötige 403-Fehler.

**Geändert:** Der Sweep prüft jetzt vor dem Durchlauf, welche Streamer einen gültigen OAuth-Eintrag haben (`needs_reauth = FALSE`). Kanäle ohne Eintrag oder mit ungültigem Token werden übersprungen — invalide Token werden sowieso departnered.

**Wie's funktioniert:** Vor dem Sweep lädt der Bot einmalig alle `twitch_user_ids` mit gültigem OAuth aus der Datenbank. Nur Kanäle, die in dieser Menge enthalten sind, landen im Durchlauf. Wer kein OAuth hat, kommt gar nicht erst dran — statt mit 403 zu scheitern.

## #98 — Keine doppelte Werbung mehr

**Problem:** Der Bot hat seine Discord-Werbung manchmal doppelt gepostet — zwei Werbungen mit nur einer einzigen Chat-Nachricht dazwischen. Eigentlich gibt es eine Mindestschwelle, wie viele Nachrichten zwischen zwei Werbungen liegen müssen, aber die wurde übergangen. Grund: Über das Werben entscheiden zwei Stellen parallel — eine reagiert auf jede eingehende Chat-Nachricht, eine läuft als Timer im Hintergrund. Beide prüfen die Schwelle, senden dann aber erst nach mehreren Zwischenschritten und setzen den „zuletzt geworben"-Marker erst ganz am Ende. Trifft eine Chat-Nachricht genau in dieses Zeitfenster, sehen beide noch den alten Stand und werben gleichzeitig.

**Geändert:** Beide Werbe-Pfade werden jetzt pro Kanal gegeneinander gesperrt, sodass immer nur einer gleichzeitig entscheiden und senden darf.

**Wie's funktioniert:** Ein Schloss pro Kanal umschließt die komplette Sequenz „Schwelle prüfen → senden → Marker setzen". Während ein Pfad sendet, muss der andere warten; sobald er drankommt, sieht er den frisch gesetzten Marker und die noch nicht zurückgesetzte Nachrichtenschwelle und bricht ab. Doppelte Werbung ist damit ausgeschlossen, und die Mindestnachrichten-Schwelle greift wieder zuverlässig.

## #97 — Netzwerkweite Sperrliste bannt jetzt vorsorglich (nur wenn offline)

**Problem:** Es gibt eine netzwerkweite Sperrliste für unerwünschte Accounts (z.B. Scammer oder Leute, die gegen die Community-Richtlinien verstoßen haben). Bisher wurde so jemand erst gebannt, wenn er in einem der betreuten Kanäle tatsächlich geschrieben hat — der Ban passierte also reaktiv, mitten im laufenden Stream, oft in einem Kanal, in dem die Person vorher nie aufgetaucht war. Das war auffällig und für Zuschauer verwirrend (warum bannt der Bot jemanden, der gerade nichts getan hat?). Und wer einfach nichts schrieb, rutschte komplett durch.

**Geändert:** Zusätzlich zum reaktiven Ban gibt es jetzt einen vorsorglichen Abgleich, der gesperrte Accounts aus allen betreuten Kanälen bannt — aber bewusst nur, wenn der jeweilige Kanal gerade offline ist. Außerdem wurde die Chat-Begründung beim reaktiven Ban ehrlich gemacht.

**Wie's funktioniert:** Der Abgleich läuft zu zwei Zeitpunkten, beide an „Streamer ist offline" gekoppelt: einmal rund eine Stunde nachdem ein Stream endet (gezielt für genau diesen Kanal), und einmal täglich gegen 6 Uhr morgens als Sammeldurchlauf über alle Kanäle, die gerade nicht senden. Ist ein Kanal live, wird er übersprungen und beim nächsten Mal erneut versucht — so wird nie jemand sichtbar mitten im Stream gebannt. Damit nicht bei jedem Durchlauf dieselben Bans erneut an Twitch geschickt werden, merkt sich das System pro Kombination „Account × Kanal", wo der Ban schon gesetzt wurde. Der frühere reaktive Ban bleibt als Sicherheitsnetz — falls jemand schreibt, bevor der Offline-Abgleich den Kanal erreicht hat — sagt im Chat jetzt aber klar, dass die Person netzwerkweit gesperrt ist (Verstoß gegen die Community-Richtlinien), statt sie pauschal als Spam zu deklarieren. Wer neu auf die Sperrliste kommt, landet automatisch auch auf der Raid-Sperrliste.

## #96 — Engagement-AI: Validate+Erklär-Muster gebrochen

**Problem:** 44% aller generierten Nachrichten folgten dem Muster `[kurze Reaktion], [erklärender zweiter Teil]` — z.B. "haha genau, das ist halt typisch für den hero". Dieses Muster ist das sicherste Bot-Tell überhaupt: echte Chatter sagen entweder die Reaktion oder die Meinung — nie beides in einem Satz.

**Geändert:** Zwei Stellen.

1. **System-Prompt:** Neuer Abschnitt mit konkretem Gegenbeispiel (Show-don't-tell statt abstrakter Regel): Die KI soll bei einem Komma in ihrer Antwort alles nach dem Komma streichen und prüfen ob der erste Teil alleine steht. Dazu explizit: entweder Reaktion oder Meinung, nie beides plus Begründung.

2. **Gold-Beispiele in den Few-Shot-Vorlagen:** Die kuratierten Muster-Nachrichten wurden um mehr reine Reaktionen ergänzt ("alter bitte", "echt wild", "ngl stimmt", "no shot den", "ich auch") — kurze Fragmente ohne Erklär-Nachklapp. Diese erscheinen immer als erste im Prompt und konditionieren den Ton.

**Wie's funktioniert:** LLMs imitieren konkrete Beispiele besser als abstrakte Regeln. Wenn die ersten 4 Beispiele alle pure Reaktionsfragmente ohne Erklärung sind, schreibt das Modell automatisch im gleichen Register — statt zu erklären warum etwas "genau" ist.

## #95 — Engagement-AI: weniger Bot-isch, keine Dialekt-Imitation

**Problem:** Die AI hat sich im Chat zu oft verraten — vor allem durch einen einzigen Tell: 40% aller Nachrichten fingen mit "haja" an, weil das Modell den Ausdruck aus den Channel-Stilvorlagen übernahm und ihn auf jede Antwort klebte. Dazu kamen erfundene Compound-Wörter ("brumm-heizkessel", "konsequenz-plan"), broken Schweizerdeutsch in Channels wo Schweizer zuschauen, und gelegentlich `<3` als Antwort auf komplett unpassende Trigger.

**Geändert:** Drei Stellen gleichzeitig angepasst.

1. **Starter-Repeat-Guard in der Pipeline:** Nach dem Modell-Call wird das erste Wort der generierten Antwort mit dem ersten Wort der letzten Bot-Nachricht verglichen. Stimmen sie überein → Antwort verworfen (silent). Das ist ein harter Filter unabhängig vom Prompt — fängt auf, wenn das Modell die Prompt-Regel ignoriert.

2. **System-Prompt-Ergänzungen:** Vier neue Regeln direkt im Baseline-Prompt: (a) nie mit demselben Wort starten wie die vorherige Antwort, (b) "haja", "hmm", "naja", "danke" nicht als Opener, (c) kein Dialekt — auch wenn Schweizerdeutsch im Channel läuft, bleibt die Antwort normales Deutsch mit Chat-Slang, (d) `<3` nie als eigene Antwort senden.

3. **Style-Beispiel-Deduplication:** Die Few-Shot-Stilvorlagen, die dem Modell aus dem Channel-Chat gezogen werden, dürfen jetzt max. 2 Beispiele mit demselben Starter-Wort enthalten. Vorher konnten 6 von 8 Beispielen mit "haja" anfangen — das hat das Modell trainingsartig auf genau das konditioniert.

**Wie's funktioniert:** Prompt und Filter wirken als Doppelnetz: der Prompt verhindert, der Guard fängt auf. Die Stil-Dedup greift schon früher und sorgt dafür, dass das Modell gar nicht erst lernt, dass "haja" der Standard-Opener ist.

## #94 — Fairere Raid-Verteilung im Fallback (kein Partner online)

**Problem:** Wenn kein Partner live war, wurde der Fallback-Kanal immer nur nach wenigsten Viewern ausgewählt — ohne Rücksicht darauf, wie viele Raids jemand schon bekommen hat. Wer klein streamt und oft live ist, bekam dadurch unverhältnismäßig viele Raids.

**Geändert:** Die Fallback-Selektion (`select_fairest_candidate`) berücksichtigt jetzt `received_successful_raids_total` aus der Score-Datenbank als primären Sortierschlüssel. Viewer-Count bleibt Tiebreaker.

**Wie's funktioniert:** Vor der Auswahl wird für alle Kandidaten die bisherige Raid-Gesamtzahl aus der DB geladen. Sortiert wird dann zuerst nach wenigsten erhaltenen Raids — wer noch kaum Raids hat, kommt nach vorne. Kanäle die noch gar nicht im System sind, bekommen automatisch 0 und haben damit höchste Priorität. Viewer-Count und Follower-Count entscheiden bei Gleichstand. Das verhindert, dass ein Kanal mit wenig Viewern dauerhaft alle Fallback-Raids kassiert während andere leer ausgehen.

## #93 — Abo-Preise drastisch gesenkt + Raid-Boost-Gewichtung reduziert

**Ausgangslage:** Die Abo-Preise lagen zwischen 3,99 € und 13,99 € und wurden als zu hoch eingestuft — insbesondere weil das Analyse-Dashboard aktuell MiniMax als KI nutzt, nicht Claude.

**Geändert:** Alle sieben Abo-Stufen neu bepreist, neue Stripe-Preise angelegt und die alten deaktiviert. Zusätzlich wurde die Zusatz-Gewichtung für Raid-Boost-Abonnenten von 25 % auf 15 % gesenkt.

**Neue Preise:**
- Werbefrei: 3,99 € → **1,99 €/Monat**
- Raid Boost: 3,99 € → **1,99 €/Monat**
- Analyse Dashboard: 8,49 € → **1,99 €/Monat**
- Werbefrei + Raids: 6,99 € → **3,49 €/Monat** (spart 49 ¢ gegenüber Einzelkauf)
- Werbefrei + Analyse: 10,49 € → **3,49 €/Monat**
- Analyse + Raids: 11,49 € → **3,49 €/Monat**
- Alles drin (Komplett): 13,99 € → **4,99 €/Monat** (spart 0,98 € gegenüber Einzelkauf)

**Wie Preise in Stripe funktionieren:** Stripe-Preise sind unveränderbar — ein gesenkter Preis bedeutet immer: neuen Price-Eintrag anlegen, alten auf inaktiv setzen. Bestehende Abos laufen weiterhin auf dem alten Preis, bis sie verlängert oder migriert werden. Neue Checkout-Sessions zeigen sofort die neuen Preise. Die Raid-Boost-Gewichtung ist ein interner Score-Multiplikator: Abonnenten werden im Raid-Netzwerk weiterhin bevorzugt, aber weniger stark als vorher (15 % statt 25 % Score-Aufschlag).

## #92 — Live-Ping-Rolle: Umbenennung + Streamer pingen sich nicht mehr selbst

**Problem:** Die automatisch erstellte Discord-Ping-Rolle hieß `KANALNAME LIVE PING` (alles Großbuchstaben, englisch) — unpassend. Dazu hatte der Bot beim Erstellen der Rolle dem Streamer die Rolle direkt selbst zugewiesen: wer live geht, bekam die eigene Ping-Rolle und wurde beim nächsten Go-Live-Event angepingt. Das ist das Gegenteil von sinnvoll.

**Geändert:** Der Rollenname folgt jetzt dem Format `kanalname ist live`. Die Zuweisung der Ping-Rolle an den Streamer selbst wurde in beiden Code-Pfaden (Monitoring-Flow + Dashboard-Live-Flow) entfernt.

**Wie's funktioniert:** Die Rolle wird nach wie vor automatisch erstellt und im Live-Embed als Mention eingebunden — Zuschauer können sie sich selbst in Discord zuweisen, um Benachrichtigungen zu erhalten. Der Streamer bekommt sie nicht mehr automatisch und wird damit nicht über den eigenen Stream angepingt.

## #91 — Live-Ankündigung: Textzeile entfernt, Umlaute gefixt, Offline-Embed aufgeräumt

**Problem:** Drei Kleinigkeiten, die zusammen störten. (1) Vor jedem Live-Embed stand eine redundante Klartextzeile (`X ist live! Schau über den Button unten rein.`), die denselben Inhalt wie das Embed selbst doppelt angezeigt hat. (2) Im Footer (`fuer`) und im Content-Template (`ueber`) standen ASCII-Umlaute statt echter Zeichen — sichtbar für alle Streamer ohne eigene DB-Konfiguration, z. B. daxy. (3) Das Offline-Embed zeigte `OFFLINE` dreifach: im Embed-Titel, im Author-Label (`OFFLINE: Name`) und als eigenes Status-Feld.

**Geändert:** Content-Template auf reinen Rollen-Ping (`{rolle}`) gekürzt — die Textzeile fällt weg, der Mention bleibt. Footer-Text und Template-Fallbacks verwenden jetzt echte Umlaute. Im Offline-Embed ist das redundante Status-Feld (`OFFLINE`) entfernt und das Author-Label zeigt nur noch den Streamer-Namen ohne Präfix.

**Ergebnis:** Live-Embeds sehen kompakter aus (kein doppelter Informationstext über dem Embed). Offline-Embeds zeigen `OFFLINE` nur noch einmal im Titel. Alle Standardtexte verwenden korrekte Umlaute.

## #90 — Admin-Dashboard: Changelog-History, Raid-Historie, DB-Query, Raw-Chat-Lag-Fix, Memory-Fix

**Problem:** Sechs Baustellen im Admin-Dashboard auf einmal. (1) Die Changelog-Seite zeigte nie eine History, weil das Backend das Feld schlicht nicht lieferte. (2) Die Raids-Seite zeigte keine vergangenen Raids — gleicher Grund: Backend lieferte keine History. (3) Die EventSub-Seite zeigte bei inaktivem WebSocket 0 Subscriptions, ohne Hinweis was zuletzt registriert war. (4) Die Memory-KPI zeigte dauerhaft „0 B", weil `psutil` nicht im venv installiert war. (5) Der Raw-Chat-Lag-Warning blieb permanent aktiv für `its_raffi`, weil der Live-State seit dem EventSub-Ausfall nie auf Offline gesetzt wurde — der Bot behandelte einen 80 Tage alten Timestamp als „live". (6) Keine Möglichkeit, direkt Daten aus der DB abzulesen.

**Geändert:**
- **Changelog-History:** Der `/twitch/api/admin/config/overview`-Endpoint gibt jetzt `changelog.entries` mit den letzten 20 Einträgen aus der `internal_home_changelog`-Tabelle zurück — die History-Sektion auf der Changelog-Seite füllt sich damit automatisch.
- **Raid-Historie:** Derselbe Endpoint enthält jetzt `raids.history` mit den letzten 50 Einträgen aus `twitch_raid_history` (Streamer, Ziel, Viewer-Zahl, Zeitstempel, Erfolg). Die Raids-Seite im Dashboard zeigt sie direkt in der Tabelle.
- **EventSub Last-Known-Snapshot:** Wenn der WebSocket inaktiv ist und keine aktiven Subscriptions vorliegen, liest der EventSub-Endpoint den letzten Snapshot mit `listener_count > 0` aus `twitch_eventsub_capacity_snapshot` — inklusive `listeners_json` — und gibt ihn als `lastKnownSubscriptions` zurück. Die EventSub-Seite zeigt diesen Snapshot mit Zeitstempel als eigene Sektion.
- **Raw-Chat-Lag-Fix:** Das Live-Scope-JOIN in `_fetch_raw_chat_health_snapshot` prüft jetzt zusätzlich, ob `last_seen_at`/`last_started_at` des Live-States nicht älter als 4 Stunden ist. Eingefrorene States aus der Zeit vor dem EventSub-Ausfall gelten nicht mehr als „live" — die veraltete Warnung verschwindet beim nächsten Health-Poll.
- **Memory-Fix:** `psutil` ins venv installiert. Die RSS/Process-Memory-KPI im System Overview zeigt jetzt den echten Prozess-Speicherverbrauch.
- **DB Query:** Neuer Admin-Endpoint `GET /twitch/api/admin/system/query?sql=...` — nur SELECT erlaubt, max. 200 Rows, gefährliche Keywords (`INSERT`, `UPDATE`, `DROP` etc.) werden geblockt. Neue Seite „DB Query" in der Sidebar unter Operations: Tabellenliste zum Klicken, SQL-Textarea mit Ctrl+Enter, Ergebnistabelle.

**Wie's funktioniert jetzt:** Admin öffnet `/twitch/admin/operations/query`, klickt eine Tabelle an, der Editor füllt sich mit einem Basis-SELECT, Ctrl+Enter schickt die Abfrage ab, das Ergebnis erscheint als sortierbare Tabelle. Schreiboperationen schlägt der Server mit HTTP 400 zurück bevor eine Verbindung zur DB geht.

## #89 — Live-Ankündigungen: kein Schriftzug über dem Embed, echte Umlaute, Promos nur für aktive Partner

**Problem:** Drei separate Baustellen. Erstens stand über jedem Live-Embed ein redundanter Klartext-Satz („X ist live! Schau über den Button unten rein."), der denselben Inhalt wie das Embed doppelt zeigte. Zweitens verwendeten Embeds und Buttons ASCII-Ausweichreplacement statt echter Umlaute (`ue`, `fuer`, `ueber` statt `ü`, `für`, `über`). Drittens wurden Chat-Promos an Streamer gesendet, auch wenn deren Partner-Status inzwischen deaktiviert oder archiviert war.

**Geändert:** Der Freitext-Content der Go-Live-Nachricht enthält jetzt nur noch die Rollen-Mention (damit der Ping feuert), kein angehängter Beschreibungstext mehr — das Embed trägt alle Infos. Der Offline-Edit schreibt ebenfalls keinen Plaintext mehr über das OFFLINE-Embed. Alle `ue`/`fuer`/`ueber`-Einträge in Embed-Titeln, Feldern und Footer wurden durch echte Umlaute ersetzt. `_promo_channel_allowed` fragt jetzt live gegen `twitch_streamers_partner_state` ab: `is_partner_active = true` und `archived_at IS NULL` — wer nicht mehr aktiver Partner ist, bekommt keine Chat-Promos mehr.

**Wie's funktioniert:** Discord zeigt Mentions nur dann als echten Ping an, wenn die Mention-ID im Content-Text steht — deshalb bleibt der Content nicht leer, sondern enthält nur die `<@&role_id>`. Fehlt eine Rolle (kein Ping konfiguriert), ist Content `None` → keine sichtbare Textzeile über dem Embed. Die Partner-Prüfung passiert bei jedem `_promo_channel_allowed`-Aufruf synchron gegen die DB, da dieselbe Funktion ohnehin vor echten Netzwerk-/API-Operationen steht.

## #88 — Chat-Werbung: kein Spam zum Stream-Start, Fake-Server-Warnung wird endlich sichtbar

**Problem:** Drei zusammenhängende Macken in der automatischen Discord-Werbung im Chat. Erstens warf der Bot direkt zu Stream-Beginn eine Werbenachricht raus, obwohl noch niemand im Chat geschrieben hatte. Zweitens griff die eigentlich vorgesehene Regel „erst nach genug Chat-Aktivität werben" nicht — es wurde geworben, egal wie tot der Chat war. Und drittens tauchte die Warnung vor den gefälschten Discord-Servern („Deadlock Discord Deutschland" / „Deadlock German Competitiv HUB") praktisch nie auf.

Ursache war ein zweiter, neuerer Werbe-Weg (die zielgerichtete Promo mit KI-Vorauswahl), der parallel zum älteren lief, aber nur seinen eigenen 15-Minuten-Kanaltakt prüfte — nicht die Aktivitäts-Schwellen. Bei frischem Stream ist dieser Takt sofort „frei", also feuerte er sofort. Und weil er den Werbe-Slot dauerhaft belegte, kam der ältere Weg, an dem die Fake-Server-Warnung hängt, fast nie zum Zug — die Warnung wurde regelrecht ausgehungert, ihr Timer nicht mal gestartet.

**Geändert:** Beide Werbe-Wege hängen jetzt an derselben Aktivitäts-Schwelle, und die Fake-Server-Warnung wird eine Ebene höher entschieden — dort, wo feststeht, welcher Werbetyp den Slot überhaupt bekommt.

**Wie's funktioniert:** Bevor geworben wird — egal über welchen Weg — muss seit der letzten Werbung genug echte Chat-Aktivität zusammengekommen sein: eine Mindestzahl an Nachrichten seit der letzten Promo, mehrere verschiedene Schreiber im Zeitfenster und (nach der ersten Promo) ein paar neue Gesichter. Erst wenn das erfüllt ist, kommt eine Werbung — am toten Stream-Start also gar nichts. Die Fake-Server-Warnung wird jetzt zentral im fälligen Werbe-Slot geprüft: Ist ihr eigener Takt (Standard 45 Minuten pro Kanal) reif, kommt die Warnung statt einer normalen Promo — unabhängig davon, welcher Werbe-Weg sonst gefeuert hätte. So wechseln sich Werbung und Warnung von selbst ab, statt dass ein Weg den anderen verdrängt. Der Wortlaut bleibt bewusst vorsichtig („könnte/möglicherweise Fake"), nennt die beiden Server aber klar beim Namen.

**Betroffen:** Alle Partner-Kanäle, in denen der Bot Discord-Werbung postet. Zuschauer sehen am Stream-Anfang keine Werbung mehr ins Leere und bekommen die Warnung vor den Fake-Servern jetzt regelmäßig zu Gesicht.

## #87 — Admin-Dashboard: Logout und Direktaufrufe führen nicht mehr ins Leere

**Problem:** Zwei Ärgernisse beim Admin-Zugang. Erstens schickte der Logout-Button einen auf eine „Seite nicht gefunden", statt sauber abzumelden — er rutschte in den Anmelde-Weg der öffentlichen Streamer-Seite, dessen Ziel es auf der Admin-Adresse gar nicht gibt. Zweitens: Wer eine Admin-Unterseite direkt aufrief (z. B. einen gespeicherten Link), ohne angemeldet zu sein, bekam eine nackte Fehlerseite (401) statt zur Anmeldung geleitet zu werden — man musste erst umständlich die Startseite ansteuern und sich dort einloggen.

**Geändert:** Auf der Admin-Adresse führt der Logout jetzt direkt zur Admin-Anmeldung. Und ein Direktaufruf einer Admin-Seite ohne Anmeldung leitet automatisch zur Anmeldung weiter, statt eine Fehlerseite zu zeigen.

**Wie's funktioniert:** Der Abmelde-Vorgang erkennt jetzt, dass er auf der Admin-Adresse läuft, und nimmt den richtigen Discord-Abmelde-Weg — der die Sitzung beendet und auf die Anmeldeseite führt, statt in den Twitch-Login der Streamer-Seite zu rutschen (dessen Weiterleitungsziel auf der Admin-Adresse nicht existiert und deshalb „nicht gefunden" lieferte). Für Direktaufrufe prüft der vorgelagerte Reverse-Proxy, ob überhaupt eine Admin-Sitzung vorliegt: fehlt sie, geht es sofort zur Anmeldung; ist sie da, läuft alles unverändert weiter — Angemeldete merken nichts. Der Proxy musste dafür einmal neu gestartet werden, weil seine Konfigurationsdatei nur als Einzeldatei eingebunden war und Änderungen sonst nicht ankamen.

**Betroffen:** Admin-Login-/Logout-Komfort; für Zuschauer und Streamer nichts sichtbar.

## #86 — Chat-KI (Testphase): vom Cringe-Mitspieler zum stillen Zuschauer

**Problem:** Der KI-Stammgast, der gerade nur intern im Review getestet wird, verhielt sich wie ein aufdringlicher Mitspieler statt wie ein normaler Zuschauer. Er reagierte auf praktisch jede Chatzeile — auch auf einzelne Emotes, Begrüßungen, Reaktionswörter („gg", „easy") und Chat-Commands. Schlimmer: Lob für gute Spielzüge nahm er an, als hätte er selbst gespielt („läuft grad gut"), sprach einzelne Zuschauer wie ein Gastgeber mit Namen an und klinkte sich in Absprachen der Stammcrew ein. Dazu klangen die Antworten nach KI — zu lang, zwei ausformulierte Sätze mit Abschluss-Pointe, wo ein echter Chatter nur ein Fragment hinwirft.

**Geändert:** Eine zweistufige Brems-Logik, ein neues Selbstbild und ein an echten Chatdaten geeichter Schreibton. Außerdem hört die KI im Test nur noch echte Partnerkanäle mit.

**Wie's funktioniert:**
- Vor dem Sprachmodell sortiert ein billiger Vorfilter offensichtliches Rauschen aus, ohne das Modell überhaupt zu fragen: einzelne Emotes und Reaktionswörter, mehrfach wiederholte Emote-Ketten, Chat-Commands und Nachrichten, die mit „@name" direkt an eine bestimmte Person gehen. Das spart Rechenzeit und lässt sich nicht per Prompt „überreden".
- Das Modell weiß jetzt klar, dass es Zuschauer ist und nicht der Streamer: Lob und Zurufe zum Spielgeschehen gehören dem Streamer, nicht ihm; es grüßt, dankt und verabschiedet niemanden wie ein Gastgeber und mischt sich nicht in Pläne ein.
- Melden darf es sich nur noch bei einem echten Deadlock-Anlass — einer konkreten Frage oder Meinung zu Helden, Items, Builds oder Meta. Alles andere bleibt stumm; der Normalfall ist Schweigen.
- Der Schreibton wurde an rund 3000 echten Chatnachrichten gemessen (typisch: drei bis vier Wörter, ein einziger Satz, kaum Satzzeichen, echte Umlaute) und genau darauf geeicht — kurze Fragmente statt ausformulierter Absätze.

**Betroffen:** Intern. Die Chat-KI ist weiter in der Review-Testphase und für Zuschauer wie Streamer noch nicht aktiv.

## #85 — Admin-Dashboard: Affiliate-Abrechnung repariert + Admin-Login hält 2 Wochen

**Problem:** Zwei Dinge im Admin-Bereich. Erstens warf die Affiliate-Abrechnung beim Öffnen einen Server-Fehler (500) und lud gar nicht — die Datenbank-Abfrage fragte eine Spalte („Provisionssatz") ab, die es in der Vertriebler-Konten-Tabelle nie gegeben hat. Zweitens fiel die Admin-Anmeldung schon nach wenigen Stunden wieder raus (die Sitzung galt nur 6 Stunden), was sich anfühlte, als wäre das Dashboard ständig „tot": Oberfläche lädt, aber jede Aktion läuft ins Leere, weil man im Hintergrund längst abgemeldet war.

**Geändert:** Die nicht existierende Spalte fliegt aus der Abrechnungs-Abfrage. Die Gültigkeit der Admin-Sitzung steigt von 6 Stunden auf 2 Wochen.

**Wie's funktioniert:** Provisionen werden ohnehin als fester Betrag pro Buchung gespeichert, nicht als Satz am Konto — die Abfrage liest jetzt nur noch tatsächlich vorhandene Felder, der 500er ist weg und die Abrechnungs-Liste lädt wieder. Bei der Sitzung wurden beide beteiligten Lebensdauern angehoben: die zentrale Login-Sitzung (über die der Admin-Zugang läuft) und die davon abgeleitete Dashboard-Sitzung stehen jetzt auf 14 Tagen und verlängern sich bei jeder Nutzung automatisch — einmal anmelden reicht damit praktisch für zwei Wochen, statt mehrmals täglich neu einloggen zu müssen.

**Betroffen:** Admin-intern (Affiliate-Verwaltung und Login-Komfort), für Zuschauer und Streamer nichts sichtbar.

## #84 — Tracking-Tabelle: vestigiale Alt-Spalten entfernt

**Problem:** Auf der Tracking-Tabelle lagen noch vier Spalten aus einem alten Schema (Beschreibung des letzten Link-Checks, Link-Status, „hinzugefügt von", Zeitpunkt des letzten Checks) — partner-klingende Reste, die seit Langem von keinem Code mehr geschrieben oder gelesen wurden. Tot, aber verwirrend, weil sie auf der „nur Tracking"-Tabelle nach Partner-Daten aussehen.

**Geändert:** Diese vier toten Spalten wurden entfernt. Eine geprüfte Abhängigkeits-Analyse hat vorher bestätigt, dass kein Programmteil sie noch liest.

**Wie's funktioniert:** Ein weiterer versionierter Migrationsschritt entfernt die vier Spalten (idempotent, greift nur solange sie existieren). Ihre Daten waren ohnehin schon im Gesamt-Backup aus #83 enthalten. Die interne Identitäts-Spalte der Tabelle bleibt unangetastet. Damit enthält die Tracking-Tabelle wirklich nur noch Tracking-/Identitäts-Felder — keine partner-klingenden Altlasten mehr.

**Betroffen:** Für Zuschauer und Streamer nichts sichtbar — interne Datenhygiene.

## #83 — Partner-DB-Konsolidierung: doppelte Partner-Spalten endgültig entfernt

**Problem:** Partner-Eigenschaften (Verifizierungs-Status, Raid-Schalter, Stumm-Flags, Live-Ping-Einstellungen, Discord-Link-Pflicht) lagen jahrelang in **zwei** Tabellen gleichzeitig — einmal in der schmalen Partner-Tabelle (die eigentliche Wahrheit) und einmal als gespiegelte Kopie in der breiten Tracking-Tabelle. Zwei Pflegeorte für denselben Wert heißt: sie können auseinanderlaufen, und genau dieser Drift war die Wurzel der wiederkehrenden „mal stimmt der Partner-Status, mal nicht"-Verwirrung (siehe #80–#82).

**Geändert:** Die elf gespiegelten Spalten wurden aus der Tracking-Tabelle entfernt. Der Partner-Status lebt jetzt an genau einer Stelle.

**Wie's funktioniert:** Voraussetzung war die Vorarbeit aus #80–#82 — erst nachdem nachweislich kein Programmteil diese Spalten mehr aus der Tracking-Tabelle liest und die zentrale Schreib-Stelle sie nicht mehr dorthin spiegelt, sind sie „tot" und können weg. Ein versionierter Migrationsschritt legt beim Start einmalig ein vollständiges Backup der Tracking-Tabelle an und entfernt dann die elf Duplikat-Spalten; er greift nur solange die Alt-Spalten existieren und läuft sonst als No-op. Reine Tracking-Felder (Beobachtungs-Archivierung, „nur beobachtet"-Markierung, Identität) bleiben unangetastet. Ergebnis: eine einzige Quelle für den Partner-Status, kein Drift mehr möglich, und die ohnehin kanonische Lese-Sicht zieht ihre Werte unverändert aus der Partner-Tabelle.

**Betroffen:** Für Zuschauer und Streamer nichts sichtbar — interne Datenhygiene, Abschluss der Partner-DB-Konsolidierung.

## #82 — Partner-DB-Konsolidierung: Monitoring-Loop liest keine Partner-Config mehr aus der Tracking-Tabelle

**Problem:** Die Überwachungs-Schleife lud auch für reine Beobachtungs-Kanäle (keine Partner) Partner-Einstellungen wie „Discord-Link nötig" oder die Live-Ping-Rolle aus der breiten Tracking-Tabelle mit — Werte, die für einen Nicht-Partner nichts bedeuten. Das war der letzte offene Leser aus der Aufräumarbeit von #80/#81.

**Geändert:** Für Nicht-Partner setzt die Schleife diese Partner-Felder jetzt auf ihre Standardwerte, statt sie aus der Tracking-Tabelle zu lesen. Das echte Beobachtungs-Feld „archiviert?" bleibt unangetastet.

**Wie's funktioniert:** Aktive Partner kommen ohnehin schon aus der zentralen Partner-Sicht; nur der Zweig für reine Beobachtungs-Kanäle griff noch auf die gespiegelten Partner-Spalten zu. Für solche Kanäle sind diese Felder jetzt fest auf den Standard gesetzt (kein Link-Zwang, keine Live-Ping-Rolle) — exakt das, was dort heute ohnehin drinstand. Damit liest kein Programmteil mehr Partner-Status aus der Tracking-Tabelle; die doppelten Spalten lassen sich in einem späteren Schritt gefahrlos entfernen.

**Betroffen:** Nichts sichtbar — interne Datenhygiene, Abschluss der Leser-Umstellung.

## #81 — Partner-Status-Konsolidierung: zwei weitere Leser auf die zentrale Wahrheit

**Problem:** Mehrere Programmteile lasen Partner-Eigenschaften (z. B. „verifiziert seit", Raid-/Live-Ping-Einstellungen) direkt aus der breiten Tracking-Tabelle statt aus der zentralen Partner-Sicht. Dort liegen diese Eigenschaften nur als gespiegelte Kopie — pflegt man denselben Wert an zwei Orten, driften die beiden Stände früher oder später auseinander. Das ist die Fortsetzung der Aufräumarbeit aus #80.

**Geändert:** Zwei dieser Stellen beziehen den Partner-Status jetzt aus der zentralen Wahrheit bzw. fragen ihn gar nicht mehr aus der Tracking-Tabelle ab.

**Wie's funktioniert:** Der Post-Stream-Report lud zwei Partner-Konfig-Werte mit, die er nirgends verwendet hat — die werden schlicht nicht mehr abgefragt. Der Admin-Verifizierungs-Dialog entscheidet „schon verifiziert → keine erneute Benachrichtigung" jetzt anhand des echten Partner-Datensatzes statt anhand der gespiegelten Kopie in der Tracking-Tabelle. Für bestehende Partner ist das Ergebnis heute identisch (beide Kopien sind gleich); der Unterschied greift erst, wenn die doppelten Spalten später ganz aus der Tracking-Tabelle entfernt werden — dann gibt es nur noch eine Quelle, die nicht mehr veralten kann.

**Betroffen:** Für Zuschauer und Streamer nichts sichtbar — reine interne Datenhygiene als Zwischenschritt der Partner-DB-Konsolidierung.

## #80 — Partner-Status-Check: kein veralteter Nachbau mehr

**Problem:** Der Bot beantwortet an mehreren Stellen die Frage „ist das ein operativ aktiver Partner?" — und zwei dieser Stellen waren sich uneinig. Die eine las den zentral gepflegten Partner-Status; die andere, eigentlich die strengere (sie steuert u. a. den Bot-Ban-/Blacklist-Schutz und seit der letzten Änderung auch das Engagement-AI-Gate), rechnete den Status von Hand aus dem rohen Partner-Datensatz nach. Dieser Nachbau war veraltet: Er prüfte nur, ob die Partnerschaft formal aktiv, nicht per Opt-out abgeschaltet und nicht admin-archiviert war — den Zustand „technisch pausiert" bzw. „Bot wurde in diesem Kanal gebannt" hat er übersehen. Folge: Kanäle, in denen der Bot pausiert oder gebannt war, zählten trotzdem als operative Partner. Genau das war der Auslöser dafür, dass die Engagement-AI in solchen Kanälen anschlug.

**Geändert:** Die strenge „operativ aktiver Partner"-Prüfung rechnet nichts mehr selbst nach, sondern liest dieselbe zentrale Wahrheit wie alle übrigen Partner-Checks.

**Wie's funktioniert:** Es gibt eine zentrale, laufend gepflegte Sicht auf den Partner-Status, die alle Ausschlussgründe zu einem einzigen „aktiv: ja/nein" zusammenfasst — formal aktiv, kein Opt-out, nicht archiviert, nicht pausiert, nicht gebannt. Die strenge Prüfung schaut jetzt genau dort nach, statt die Bedingungen einzeln und unvollständig selbst zusammenzusetzen. Dadurch ist sie immer auf demselben Stand wie diese Sicht: Wird ein Partner pausiert oder der Bot dort gebannt, fällt der Kanal sofort aus allen Pfaden, die diese Prüfung nutzen. Vorher konnte der handgestrickte Nachbau der zentralen Sicht „hinterherhinken", weil er bei jeder Erweiterung der Ausschlussgründe mit angepasst werden musste — und das zuletzt nicht geschehen war.

**Betroffen:** Bot-Ban-/Blacklist-Schutz und die (noch nicht scharfgeschaltete) Engagement-AI — jeweils nur für Kanäle, in denen der Bot pausiert oder gebannt ist.

## #79 — Engagement-AI: redet nur noch in echten Partner-Kanälen

**Problem:** Die KI prüfte vor dem Antworten nur zwei Dinge: ob sie für den Kanal eingeschaltet ist und ob dort gerade Deadlock live läuft. Den Partner-Status hat sie dabei komplett übersprungen. Der Bot ist aber auch in beobachteten Nicht-Partner-Kanälen präsent (rein fürs Statistik-Tracking), und genau dort war die Engagement-AI die einzige Chat-Funktion, die trotzdem mitgeredet hat — Moderation, Promos und die frechen Auto-Antworten schweigen in solchen Kanälen längst, weil sie alle denselben Partner-Check davorhängen. Zusätzlich blieb der Engagement-Schalter eines Kanals auf „an", selbst wenn die Partnerschaft später beendet wurde.

**Geändert:** Ein Partner-Gate vor jeder Engagement-Antwort, plus ein automatischer Aufräum-Schritt beim Beenden einer Partnerschaft.

**Wie's funktioniert:** Vor jeder möglichen Antwort prüft die KI jetzt zusätzlich denselben Partner-Status, den Moderation, Promos und das Kanal-Beitreten ohnehin schon verwenden — also über eine zentrale, gemeinsame Stelle, nicht über eine eigene Sonderlogik. Durch kommen nur operativ aktive Partner: reines Beobachten, ein Admin-Opt-out oder eine pausierte bzw. beendete Partnerschaft zählen bewusst nicht. Trifft das nicht zu, bleibt sie still — selbst wenn ihr Schalter (noch) auf „an" steht. Als zweite, unabhängige Sicherung wird beim Beenden einer Partnerschaft der Engagement-Schalter des Kanals automatisch ausgeschaltet, genau wie es dort schon mit der Raid-Berechtigung passiert; so bleibt kein „verwaister An-Zustand" in der Datenbank zurück. Damit greifen zwei Schichten: Das Gate fängt jede einzelne Nachricht in Echtzeit ab, der Aufräum-Schritt hält den gespeicherten Zustand sauber — fällt eine Schicht aus, schützt die andere weiter.

**Betroffen:** Nur die (noch nicht scharfgeschaltete) Engagement-AI.

## #78 — Engagement-AI: lockerer Stammgast statt Deadlock-Roboter

**Problem:** Nach der Themen-Eingrenzung war die KI zu steif — sie redete fast nur noch in ausformulierten Deadlock-Takes und ging auf lockeren Chat-Banter gar nicht mehr ein. Das wirkte wieder nach Bot, nur andersrum.

**Geändert:** Die KI ist wieder ein lockerer Stammgast, der mit dem Chat vibet — und sie schreibt kürzer und trockener, an einem echten Vorbild ausgerichtet.

**Wie's funktioniert:** Deadlock bleibt ihre Stärke und sie ist nur in Deadlock-Streams aktiv, aber sie zwingt das Thema nicht mehr in jede Nachricht: Banter, Reaktionen und Mitlachen sind wieder erlaubt, solange es zur Runde passt. Nachrichten, die klar an den Streamer gerichtet sind, lässt sie weiter aus, und bei fremden Themen spielt sie sich nicht als Experte auf. Für den Schreibstil dient jetzt der echte Chat-Ton eines Stamm-Streamers als Gold-Vorlage (kurz, trocken, viel Banter, oft nur ein paar Wörter) — diese Beispiele stehen im Stil-Vorbild immer ganz vorne, sodass die KI in diesem Register schreibt statt in langen Absätzen.

**Betroffen:** Nur die (noch nicht scharfgeschaltete) Engagement-AI.

## #77 — Engagement-AI: schweigt, wenn kein Deadlock läuft

**Problem:** Die KI hat auch in Streams geantwortet, die gerade gar kein Deadlock zeigten — bei „Just Chatting", anderen Spielen oder wenn der Kanal offline war. Sie hing nur am Chat-Inhalt, nicht daran, was im Stream tatsächlich lief.

**Geändert:** Ein Stream-Gate vor allem anderen: Die KI wird pro Kanal nur aktiv, wenn der gerade live ist UND als Kategorie Deadlock läuft.

**Wie's funktioniert:** Der Bot pflegt ohnehin für jeden Streamer einen Live-Status samt aktueller Spiel-Kategorie. Bevor die KI überhaupt über eine Antwort nachdenkt, prüft sie diesen Status (kurz zwischengespeichert, damit das nicht jede Nachricht die Datenbank trifft): Ist der Kanal offline oder läuft etwas anderes als Deadlock, bleibt sie komplett still — egal wie deadlock-lastig eine einzelne Chat-Nachricht klingt. So redet sie nur dort, wo wirklich gerade Deadlock gestreamt wird.

**Betroffen:** Nur die (noch nicht scharfgeschaltete) Engagement-AI.

## #76 — Engagement-AI: nur noch Deadlock, kürzer, mit Streamer-Gespür

**Problem:** Im Mehr-Kanal-Test fiel auf, dass die KI sich überall einmischte — bei Begrüßungen, Resubs, Smalltalk, sogar bei Nachrichten, die klar an den Streamer gerichtet waren. Das wirkte aufdringlich und nach Bot. Dazu zwei technische Sachen: In belebten Chats (mehrere Leute gleichzeitig) brach jede Antwort mit einem Schnittstellen-Fehler ab, und die KI hatte keinerlei Hintergrundwissen über den einzelnen Streamer.

**Geändert:** Klare Eingrenzung aufs Thema, knappere Antworten, ein Fix für den Mehr-Personen-Fehler und ein wachsendes Profil pro Streamer.

**Wie's funktioniert:** Die KI antwortet jetzt nur noch, wenn es im Chat tatsächlich um Deadlock geht (Helden, Matches, Plays, Meta, Patches) — bei reinem Smalltalk, Subs, Begrüßungen oder Off-Topic bleibt sie still. Ist eine Nachricht erkennbar an den Streamer oder eine bestimmte Person gerichtet, hält sie sich raus, weil das nicht ihre Nachricht ist. Antworten sind kürzer und haben mehr Kante statt abwägender Absätze, und auf reine Emotes reagiert sie gar nicht. Der Mehr-Personen-Fehler lag daran, dass der Sprechername in einem separaten Feld mitgeschickt wurde, das die KI-Schnittstelle bei wechselnden Namen ablehnte — jetzt steht der Name direkt im Nachrichtentext, was den Verlauf bei vielen Chattern sogar klarer macht. Zusätzlich destilliert ein Hintergrund-Durchlauf alle paar Stunden pro Kanal ein kurzes Profil aus dem Chat (welche Helden der Streamer spielt, sein Hintergrund, der Community-Vibe, Running-Gags) — reines Kontextwissen, das der KI hilft, sich natürlich einzufügen, ohne es je vorzulesen.

**Betroffen:** Nur die (noch nicht scharfgeschaltete) Engagement-AI.

## #75 — Engagement-AI: Stats-Gespür + die Soul wächst mit

**Problem:** Nach dem großen Umbau fehlten dem KI-Stammgast noch zwei Sachen. Erstens hatte er kein Gefühl dafür, welcher Held gerade wirklich stark oder beliebt ist — er konnte über die Meta-Stimmung reden, aber nicht über die nackte Stärke einzelner Helden. Zweitens war seine Persönlichkeit statisch: Er konnte sich nichts aus laufenden Gesprächen merken.

**Geändert:** Zwei neue Bausteine — echte Spielstatistiken als Stärke-Anhaltspunkt pro Held, und ein „Gedächtnis", das sich aus den Gesprächen selbst füllt.

**Wie's funktioniert:** Wird ein Held erwähnt, zieht die KI dessen aggregierte Win- und Pick-Rate aus der Statistik-Schnittstelle — aber bewusst nur als grobes Gefühl („Winrate über 50%, wird oft gespielt") statt als Zahlen-Tabelle zum Vorlesen. So weiß sie, ob ein Held gerade meta ist. Parallel läuft alle paar Stunden ein Reflexions-Durchgang: Die KI schaut sich die letzten Chats an, in denen sie mitgemischt hat, und merkt sich — wenn etwas hängen blieb (ein gutes Gespräch, ein Running Gag, ein cooler Move, ein lustiges Wort) — eine kurze Notiz. Diese Notizen hängen unter ihrer Grund-Persönlichkeit und werden später nur beiläufig aufgegriffen, wenn es gerade passt — nicht ausgepackt. So fühlt sich der Charakter mit der Zeit lebendiger an, statt jeden Tag bei null zu starten.

**Betroffen:** Nur die (noch nicht scharfgeschaltete) Engagement-AI.

## #74 — Engagement-AI: großer Qualitäts-Umbau gegen „klingt wie ein Bot"

**Problem:** Der KI-Stammgast, der sich in den Twitch-Chat einklinken soll, hatte zwei Kernprobleme. Erstens halluzinierte er Deadlock-Fakten — im Test erfand er ein Item samt frei erfundener Mechanik. Zweitens klang er unverkennbar nach KI: Floskeln wie „gute Frage, das kann ich gerade nicht belegen", reflexhaftes Zustimmen zu jeder Meinung, steife ganze Sätze statt Chat-Sprache.

**Geändert:** Der Antwort-Aufbau wurde von Grund auf umgebaut. Die KI bekommt jetzt echte Fakten vorgesetzt und bildet sich darauf eine Meinung, statt aus dem Gedächtnis zu raten — und sie hat einen festen Charakter mit eigenem Geschmack.

**Wie's funktioniert:** Vor jeder Antwort werden mehrere Faktenquellen zusammengezogen und der KI als Beleg mitgegeben: (1) Helden-/Item-Beschreibungen aus dem Deadlock-Wiki, (2) ein Stimmungsbild, das laufend aus den echten Chat-Nachrichten *aller* mitgelesenen Streams destilliert wird („wie fühlt sich die Meta gerade an"), und (3) die echten Änderungen des aktuellen Patches direkt aus den offiziellen Notes. Die KI darf nur über das reden, was in diesen Belegen steht; fehlt ein Beleg, trifft sie keine faktische Aussage, sondern weicht locker aus oder schweigt — statt einen Disclaimer abzulassen. Dazu kam ein fester Charakter („Soul"), den das Modell sich selbst geschrieben hat, plus eigene, aus allen 38 Helden samt Fähigkeiten gebildete Lieblings- und Hass-Helden — die liefern aber nur die Meinung, nicht den Ton: im Chat bleibt die KI knapp und trocken. Eiserne Regeln unterbinden das KI-Gehabe: nie zugeben, eine KI zu sein, nie über die eigene Funktionsweise reden, nicht jeder Meinung hinterherlaufen.

**Betroffen:** Nur die (noch nicht scharfgeschaltete) Engagement-AI; am restlichen Bot ändert sich nichts.

## #73 — Highlight-Clips landen jetzt im dedizierten Thread

**Problem:** Der Highlight-Clipper hat fertige Gameplay-Clips in den allgemeinen Bot-Log-Kanal gepostet, wo sie zwischen anderen Bot-Meldungen untergingen.

**Geändert:** Ziel-Kanal auf den dedizierten Highlight-Thread umgestellt.

**Wie's funktioniert:** Der Clipper schickt fertige Clips per interner HTTP-API an den Deadlock-Bots-Prozess, der den Discord-Post übernimmt. Der API-Payload enthält eine `channel_id` — die zeigt jetzt auf den Thread. Discord-Threads verhalten sich aus Bot-Sicht identisch zu normalen Kanälen (gleiches `send()`-Interface), daher war keine weitere Code-Änderung nötig.

## #72 — Interne Blacklist-API komplett: Check, List und Remove

**Problem:** Nach dem ersten Blacklist-Endpunkt (`/add`) fehlten noch die restlichen Operationen — prüfen ob ein Kanal gebannt ist, alle gesperrten Kanäle auflisten und einen Bann wieder aufheben.

**Geändert:** Drei neue Endpunkte auf demselben internen API-Port (8776, nur localhost):

- `GET /raid/blacklist/check?login=kanalname` — antwortet mit `blacklisted: true/false` plus Grund und Zeitstempel falls vorhanden
- `GET /raid/blacklist` — gibt alle aktuell gesperrten Kanäle sortiert nach Eintragsdatum zurück
- `POST /raid/blacklist/remove` — entfernt einen Kanal aus der Blacklist, `removed: true` wenn er tatsächlich drin war

**Wie's funktioniert:** Alle drei Operationen laufen über direktes SQL auf `twitch_raid_blacklist`, in `asyncio.to_thread` gekapselt damit der Event-Loop nicht blockiert. Auth und Loopback-Schutz identisch zu `/add`. Die Endpunkte sind rein additiv — kein bestehender Code-Pfad wurde geändert.

---

## #71 — Interner API-Endpunkt: Twitch-Kanäle manuell in die Raid-Blacklist eintragen

**Problem:** Es gab keinen direkten Weg, einen Twitch-Kanal per API in die Raid-Blacklist einzutragen — der einzige Pfad war ein manuelles SQL-Insert in die Datenbank, was jedes Mal erforderte, den DSN-Secret in eine Shell zu laden.

**Geändert:** Neuer interner HTTP-Endpunkt `POST /internal/twitch/v1/raid/blacklist/add`. Akzeptiert `login` und optional `reason` im JSON-Body. Trägt den Kanal direkt per `ON CONFLICT`-Insert in `twitch_raid_blacklist` ein — idempotent, d.h. ein wiederholter Aufruf überschreibt nur Grund und Zeitstempel, legt keinen Duplikat-Eintrag an.

**Wie's funktioniert:** Der Endpunkt hängt in der bestehenden internen API auf Port 8776, die ausschließlich auf `127.0.0.1` lauscht und durch zwei Middleware-Schichten abgesichert ist: eine Loopback-Prüfung (Verbindungen nur von localhost) und eine Token-Auth via `X-Internal-Token`-Header. Port 8776 ist zusätzlich nicht in der UFW-Allowlist — von extern ist der Port komplett unerreichbar. Die DB-Operation läuft in einem Thread-Pool (`asyncio.to_thread`), damit der async Event-Loop nicht blockiert.

---

## #70 — Viewbot-Spam-Filter: trennscharf gegen Verschleierung, sicherer für echte Zuschauer

**Problem:** Die Viewbot-/SMM-Werbung im Chat (Marke „Ai viewers streamboo. com" und Verwandte) rutschte zuletzt durch, obwohl der Filter die Marke kannte. Der Trick der Spammer: Sie streuen Leerzeichen und Sonderzeichen in die Domain (`streamboo. com`, `s t r e a m b o o . c o m`), wodurch die wörtliche Mustererkennung ins Leere lief — der Treffer reichte nur für „verdächtig", nicht für eine Aktion, und derselbe Spam kam tagelang immer wieder. Gleichzeitig steckte ein latentes Fehlban-Risiko im Filter: generische Wendungen wie „best viewers" zählten so stark, dass ein normales Kompliment an den Streamer („you have the best viewers today") rechnerisch die Ban-Schwelle erreichen konnte.

**Geändert:**
- **Verschleierungs-robuste Erkennung:** Bekannte Spam-Domains werden jetzt erkannt, egal wie viele Leerzeichen oder Trenner dazwischenstehen.
- **Hart/Weich-Trennung:** Signale werden unterteilt in „hart" (echte Spam-Domain/Markenname — kommt in normalem Chat praktisch nie vor) und „weich" (Alltagswörter wie „viewers", „best viewers"). Nur harte Signale lösen automatische Maßnahmen aus.
- **Selbstlernend in beide Richtungen:** Eine KI prüft Grenzfälle und pflegt zwei Listen — eine Spam-Liste für neue/abgewandelte Schreibweisen und eine Schutz-Liste für Fehlalarme (harmlose Wendungen, die fälschlich angeschlagen haben).
- **Generische Floskeln entschärft:** Reine Komplimente wie „best viewers" können allein keinen Bann mehr auslösen.
- **Fremdschrift-Tarnung erkannt:** Buchstaben aus anderen Alphabeten, die wie lateinische aussehen (z. B. kyrillisches „о/е/а" in „strеаmbоо"), werden vor der Prüfung auf normale Buchstaben zurückgeführt.
- **Kontext als Verstärker:** Frisch erstelltes Konto und allererste Nachricht im Kanal erhöhen den Verdacht — aber nur, wenn ohnehin schon ein echtes Spam-Signal vorliegt.

**Wie's funktioniert:** Jede Chat-Nachricht bekommt einen Spam-Score. Ab 3 Punkten wird automatisch gelöscht und gebannt; darunter passiert je nach Signalstärke nichts oder die Nachricht wird (nur bei einem harten Treffer) entfernt, ohne den Schreiber zu bannen. Punkte gibt es u. a. für eine bekannte Spam-Domain (+2) und für das Muster „viewers <Wort>" (+1) — Alltagswörter allein erreichen die Ban-Schwelle also nie. Vor der Bewertung wird der Text „geglättet": eingestreute Leerzeichen und Sonderzeichen werden ignoriert, sodass `streamboo. com` und `s t r e a m b o o` denselben Treffer ergeben wie die saubere Domain. Damit das nicht überschießt, ist die Domain-Erkennung an eine echte Endung gekoppelt (`.com`, `.ru`, …) — harmlose Wortpaare wie „laptop smm" oder „stream boo" lösen daher nichts aus.

Auch Tarnung über fremde Alphabete läuft ins Leere: Zeichen wie das kyrillische „о", das optisch identisch zum lateinischen „o" ist, werden beim Glätten auf den lateinischen Buchstaben zurückgeführt — `strеаmbоо` wird also wie `streamboo` behandelt.

Zwei zusätzliche Kontext-Punkte verschärfen den Verdacht gezielt: ein erst kürzlich erstelltes Twitch-Konto (+1) und die allererste Nachricht eines Accounts in genau diesem Kanal (+1). Beide zählen aber **nur**, wenn bereits ein hartes Spam-Signal vorliegt — ein bekannter Spam-Bot, der mit frischem Konto seine erste Nachricht absetzt, kippt damit über die Ban-Schwelle, während ein neuer echter Zuschauer mit einer harmlosen Begrüßung garantiert unberührt bleibt.

Die KI-Prüfung läuft im Hintergrund: Schlägt der Filter an, ohne sicher zu sein, bewertet die KI nach, ob es echte Viewbot-Werbung ist. Bestätigt sie es, lernt das System die neue Schreibweise und erkennt sie beim nächsten Mal sofort. Sagt die KI „kein Spam", wandert das auslösende Alltagswort auf die Schutz-Liste und senkt den Score solcher Nachrichten künftig wieder — **außer** es kommt zusätzlich ein hartes Spam-Signal hinzu. Diese Vorrang-Regel verhindert, dass jemand ein harmloses Wort an eine echte Spam-Domain anhängt, um den Filter auszuhebeln.

**Für Zuschauer wichtig:** Automatische Banns treffen ausschließlich klar erkennbare Viewbot-Werbung mit echter Spam-Domain. Ein normales Gespräch über „viewers" oder ein Kompliment an den Streamer löst keine Maßnahme aus. Sollte trotzdem jemand fälschlich getimeoutet oder gebannt werden: Das ist sehr unwahrscheinlich, aber kein Bann ist endgültig — meldet euch beim Streamer oder einem Mod (Rückgängigmachen per `!unban`), wir schauen es uns an. Genau für solche Fälle gibt es die lernende Schutz-Liste.

---

## #69 — Chat-Actions im Admin-Dashboard vollständig repariert

**Problem 1 (Senden-Button tut nichts):** Die Streamer-Suchbox hat einen internen Debounce (220 ms). Wenn der Anzeigename vom Login-Slug abweicht (z. B. `"Earl Salty"` vs. `"earlsalty"`), hat der Debounce 220 ms nach dem Klick die Auswahl gecleart — `canSubmit` wurde false, der Button disabled. Fix: Vergleich jetzt gegen Login **und** Anzeigenamen.

**Problem 2 (Fehlermeldung „Twitch Chat Bot ist aktuell nicht verfügbar"):** Bot und Dashboard laufen als getrennte Prozesse. Alle Bot-Operationen (Streamer hinzufügen, entfernen, …) gehen über eine interne REST-API. Chat-Actions hatten diese Brücke nie bekommen — das Dashboard versuchte den Chat-Bot direkt anzusprechen und schlug deshalb immer fehl.

**Geändert:** Die interne API-Kette wurde um eine `partner_chat_action`-Callback-Route erweitert. Der Bot-Prozess führt die eigentliche Chat-Aktion aus (er hat Zugriff auf den Chat-Bot); der Dashboard-Prozess leitet die Anfrage über HTTP weiter. Wenn kein lokaler Chat-Bot vorhanden ist, greift das Dashboard jetzt auf diesen Callback zurück statt mit einer Fehlermeldung abzubrechen.

---

## #69 — Chat-Actions im Admin-Dashboard funktionstüchtig

**Problem:** Auf der Seite `/twitch/admin/community/chat` passierte beim Drücken von "Nachricht senden" scheinbar nichts — der Button blieb inaktiv oder die Aktion wurde nie abgeschickt.

**Ursache:** Die Streamer-Suchbox hat einen internen Debounce (220 ms). Wenn nach einem Klick auf einen Streamer-Vorschlag der Anzeigename (`displayName`) vom Login-Slug abweicht (z. B. `"Earl Salty"` vs. `"earlsalty"`), hat der Debounce die Auswahl 220 ms später wieder gecleart. Danach war kein Streamer mehr ausgewählt, `canSubmit` war false und der Button disabled.

**Geändert:** Die Vergleich-Logik prüft jetzt, ob der Eingabewert mit dem Login **oder** dem Anzeigenamen des ausgewählten Streamers übereinstimmt. Stimmt beides nicht, wird die Auswahl gecleart — stimmt eines, bleibt sie erhalten.

---

## #68 — Zielgerichtete Discord-Promos mit MiniMax-Preset-Auswahl

**Problem:** Alle Promo-Nachrichten waren bisher kanal-weite Announcements — gleicher Text für alle, egal ob jemand seit Monaten dabei ist oder gerade zum ersten Mal reinschaut.

**Geändert:**
- **Preset-System** (neu): 10 definierte Templates — 5 globale Announcements (competitive, community, chill etc.) und 5 user-spezifische Nachrichten mit @mention (welcome, lurker, mates, ranked, new player).
- **MiniMax-Auswahl**: MiniMax bekommt die letzten paar Chat-Messages des Ziel-Users + die Preset-Tags und entscheidet welches Template am besten passt. Kein Freitext — nur Auswahl aus vordefinierten Texten. Timeout 5s, Fallback auf Zufalls-Preset.
- **Stammgast-Ausschluss**: Wer ≥ 10 Chat-Messages in den letzten 30 Tagen hat (Stammgast), wird beim User-Targeting übersprungen. Zielgruppe: neue Chatter und stille Lurker.
- **1x pro User pro Tag**: Jeder User bekommt höchstens einmal täglich einen personalisierten Pitch.
- **Abwechslung Global/User**: Das System wechselt zwischen kanal-weiter Announcement und direktem @mention-Pitch — kein Double-Ping.
- **Vorrang + Fallback**: Targeted Promo hat Vorrang (eigener 15-Min-Cooldown). Feuert er nicht (Cooldown läuft noch), übernimmt das bestehende Activity/Spike-System.

**Wie's funktioniert:** Jeder Promo-Loop-Tick prüft pro Channel ob der targeted 15-Min-Cooldown abgelaufen ist. Wenn ja: aktive Chatter aus dem 8-Min-Aktivitätsfenster filtern, Kandidaten-Check (nicht Stammgast, nicht heute gepitcht, user_id via DB), MiniMax-Aufruf für Preset-Wahl, Message rendern + senden. @mention-Pitches gehen als normale Chat-Message, globale als farbige Announcement.

## #67 — Timeout-Erkennung über EventSub + Stream-Start-Pitch

**Problem:** Ein 10-Minuten-Timeout des Bots wurde über den bisherigen Drop-Code-Ansatz nicht erkannt, weil der Bot in dieser Zeit nichts schreibt. Außerdem kam der Werbefrei-Pitch beim nächsten Promo-Slot statt am nächsten Stream-Start. Pitch-Text war zu generisch.

**Geändert:**
- **Echtzeit-Erkennung via EventSub**: Die bestehende `channel.ban`-Subscription (feuert bei Timeout und permanentem Ban) prüft jetzt ob der Bot selbst das Ziel ist. Bei `is_permanent: false` → Timeout erkannt → TimeoutGuard wird aktualisiert.
- **Pitch zum Stream-Start**: Der Werbefrei-Pitch wird nicht mehr beim nächsten freien Promo-Slot gesendet, sondern 90 Sekunden nach dem nächsten Stream-Start – so ist der Streamer gerade frisch live und liest tatsächlich mit.
- **Klarerer Pitch-Text**: "Beim letzten Stream wurde der Bot in diesem Chat getimed outed 🙈 Falls die automatischen Promo-Nachrichten stören – es gibt ein Werbefrei-Abo…" Klar formuliert, kein Rätselraten.

**Wie's funktioniert:** Der `channel.ban`-Callback im Monitoring-Bot vergleicht die `user_id` des Gebannten mit der Bot-ID (aus `_twitch_chat_bot.bot_id`). Passt sie, trägt er den Timeout im `TimeoutGuard` (Singleton, gleicher Prozess) ein. Wenn der Streamer das nächste Mal live geht, überprüft der Go-Live-Handler ob ein Pitch aussteht, und plant ihn mit 90s Delay als asyncio-Task.

## #66 — Timeout-Schutz, Werbefrei-Pitch und schärfere Promo-Logik

**Problem:** Der Bot hat in Channels, die ihn regelmäßig timeout'en, trotzdem weitergemacht — ohne Konsequenz. Außerdem hat die Deadlock-Beta-Zugangserkennung zu viele Fehlzündungen produziert (z.B. "brauchst mitspieler?" löste fälschlicherweise einen Invite-Hinweis aus). Die Promo-Nachrichten wiederholten sich zu schnell und kamen zu oft.

**Geändert:**
- **Timeout-Tracking** (neu): Wenn der Bot in einem Channel getimed outed wird (Twitch meldet `sender_banned` beim nächsten Send-Versuch), wird das gezählt. Bei 2x an einem Tag oder 5x in einer Woche werden **alle Bot-Funktionen in diesem Channel für 7 Tage deaktiviert**.
- **Werbefrei-Pitch**: Nach einem erkannten Timeout schickt der Bot (sobald er wieder darf) einmalig einen Hinweis auf das Werbefrei-Abo, das Promo-Nachrichten abschaltet.
- **Beta-Zugang-Fix**: "Mitspiel\*"-Signale lösen nur noch einen Invite-Hinweis aus, wenn gleichzeitig ein starkes Zugangs-Signal ("beta", "zugang", "key" etc.) oder expliziter Deadlock-Bezug vorliegt. Damit fällt "moin, brauchst mitspieler?" nicht mehr in die Kategorie.
- **Mehr Nachrichten-Vielfalt**: Der Promo-Pool wuchs von 14 auf 22 Texte (aufgeteilt in 5 Kategorien). Zusätzlich wird die zuletzt gesendete Nachricht beim nächsten Mal ausgeschlossen.
- **Höhere Cooldowns**: Minimaler Promo-Cooldown von 30 auf 45 Min, maximaler von 120 auf 180 Min, globaler Cooldown von 60 auf 90 Min, Attempt-Cooldown von 5 auf 10 Min.

**Wie's funktioniert:** Der `TimeoutGuard` hält eine In-Memory-Liste der Timeout-Zeitpunkte pro Channel (rollendes 7-Tage-Fenster). Überschreitet die Tageszahl ≥ 2 oder die Wochenzahl ≥ 5, wird der Channel für 604.800 Sekunden stumm geschaltet — kein Discord-Post, kein Raid, kein Bot-Ban, keine Promos, nichts. Der Werbefrei-Pitch erscheint maximal einmal pro 24 Stunden pro Channel.

## #65 — Globale Chatter-Bannliste + Discord-Invite-Erkennung

- Bestimmte Nutzer können jetzt global gesperrt werden und werden in jedem Partner-Kanal sofort gebannt, sobald sie dort schreiben
- Discord-Invite-Links (discord.gg/...) werden automatisch im Log als verdächtig markiert
- Normales Reden über Discord oder "mitspielen" wird davon nicht berührt

## #64 — Highlight-Clips zeigen jetzt echte Combos aus dem Replay

- Clips werden ab jetzt nur noch für echte Combo-Kills erstellt — wenn der Spieler kurz vor dem Kill mindestens 2 Abilities eingesetzt hat (z. B. Hook → Sticky Bomb → Uppercut)
- Solo-Kills ohne Combo-Bewegung werden automatisch herausgefiltert
- Combo-Label erscheint im Discord-Clip-Embed (z. B. "Hook → Bomb → Uppercut")
- Für jedes Match wird das Valve-Replay (227 MB) automatisch geladen und mit dem Open-Source-Tool "boon" analysiert — kein manueller Eingriff nötig

## #63 — Highlight-Erkennung deutlich smarter

- Close Situations werden jetzt erkannt: wenn ein Kill und ein eigener Tod nah beieinander liegen (z. B. Kill dann 3 Sekunden später gestorben), wird das als Highlight gewertet
- Teamfights werden jetzt auch geclippt wenn der Spieler nur einen Kill gemacht hat (nicht mehr min. 2 nötig)
- Einzelne, isolierte Kills werden weiterhin nicht geclippt

## #62 — Highlight-Clips zeigen jetzt echte Highlights

- Einzelne Kills werden nicht mehr als Clips verschickt — das war uninteressantes 0815-Gameplay
- Nur noch Double Kills, Triple Kills und Team Fights werden geclippt
- Neue Bezeichnungen: Double Kill, Triple Kill, Quadra Kill statt generischem "Multi Kill"

## #61 — Highlight-Clips landen jetzt wirklich im Discord-Channel

- Clips wurden erstellt aber konnten nicht gesendet werden — Fehler beim Discord-Zugriff behoben
- Clips werden jetzt über den Deadlock-Bot in den Highlight-Channel gepostet

## #60 — Highlight-Clipper: Clip-Download funktioniert jetzt zuverlässig

- Clips konnten bisher wegen eines ffmpeg-Crashes nicht erstellt werden — der Fehler ist behoben
- yt-dlp nutzt jetzt das stabile System-ffmpeg statt einer statischen Binary die abstürzte
- VODs mit eingeschränkten Qualitätsstufen (z. B. Sub-Only-Streams) werden trotzdem verarbeitet

## #59 — Highlight-Clipper funktioniert jetzt für alle Partner-Streamer

- Der automatische Clip-Ersteller lief bisher gar nicht — Twitch-Zugangsdaten wurden falsch gesucht und nie gefunden
- Clips werden jetzt automatisch für alle 18 aktiven Partner-Streamer erstellt, die ihren Steam-Account bereits im Discord verknüpft haben
- Steam-Accounts werden direkt aus der Discord-Verknüpfung übernommen — kein separates Einrichten nötig
- Fertige Clips landen in einem Discord-Channel zur Durchsicht

## #58 — Streamer-Seite lädt jetzt blitzschnell ohne weißen Flash

- Die Streamer-Seite wird beim Build jetzt vollständig als fertiges HTML vorgerendert — Besucher sehen die Seite sofort, ohne kurzen weißen Ladeblitz
- Kein sichtbarer Unterschied für normale User, aber Suchmaschinen und KI-Crawler sehen exakt dasselbe wie echte Besucher — keine versteckten Tricks mehr

## #57 — Streamer-Landingpage komplett SEO-fähig gemacht

- Die Streamer-Seite hatte vorher nur einen leeren Titel ("Twitch-Bot") und keinen lesbaren Inhalt für Google — deshalb hat sie Google bisher gar nicht gecrawlt
- Jetzt mit klarem Titel ("Deadlock Auto-Raid Bot für Twitch — Streamer-Netzwerk"), Hero-Text, Feature-Liste, So-funktioniert-es-Schritten und FAQ-Block, die alle Suchmaschinen direkt lesen können
- Strukturierte Daten (Schema.org) für die Web-App, Breadcrumbs und FAQ ergänzt — damit Google AI Overviews, Bing Copilot und Perplexity die Seite zitieren können
- Vollständige Social-Media-Vorschau (Open Graph + Twitter Card) für Discord, Twitter und WhatsApp ergänzt — Links zeigen jetzt Vorschaubild und Beschreibung
- Statischer Inhalt bleibt für Crawler ohne JavaScript (Bing, Brave, DuckDuckGo) dauerhaft sichtbar, normale Besucher sehen die interaktive Version wie gewohnt

## #56 — Twitch Admin Dashboard komplett neu strukturiert

- Neues Admin-Cockpit auf `/twitch/admin` mit Live-Status auf einen Blick: aktive EventSub-Verbindung, ausstehende OAuth-Reauths, Bot-Uptime und Datenbank-Status
- Sidebar in fünf klare Bereiche gegliedert: Cockpit, Operations, Community, Content & Comms, Money & Compliance — keine endlose flache Linkliste mehr
- Globale Streamer-Suche oben in der Top-Bar — Login eintippen, Enter, direkt in der Detail-Ansicht
- Neue Bereiche im Dashboard: OAuth-Scope-Diff pro Streamer, Bot-Control (Reload, Promo-Mode), Engagement-AI-Steuerung, Chat-Aktionen senden, Roadmap- und Legal-Editor mit Vorschau, Audit-Log über alle Admin-Aktionen
- Das alte Legacy-Dashboard leitet jetzt automatisch auf die neue Oberfläche um — Bookmarks bleiben funktionieren

## #55 — Lade-Bildschirm für Rechtstexte vereinfacht

- Die Bot-Prüfseite zeigt jetzt nur noch einen Spinner mit „Einen Moment bitte …" und einem dezenten Hinweis
- Kein sichtbares Captcha, keine Erklärungstexte mehr — für normale Besucher wirkt es wie ein kurzer Ladevorgang

## #54 — Impressum & Datenschutz jetzt Bot-geschützt, Dashboard erweitert

- Impressum, Datenschutz und AGB sind jetzt durch eine unsichtbare KI-Bot-Sperre geschützt — normale Besucher werden automatisch weitergeleitet, ohne ein Captcha zu sehen
- Admin-Dashboard: Inhalte der Rechtstexte (Impressum, AGB, Datenschutz) und der Roadmap können direkt im Dashboard bearbeitet werden
- Admin-Dashboard: Neue Analytics-Übersicht und überarbeitete Navigation mit erweitertem Sidebar-Menü
- Zugriffs-Log des Dashboards wird jetzt in eine eigene Datei geschrieben (rotierend, max. 5 MB)

## #53 — Spam-Filter präziser und resistenter gegen Unicode-Tricks

- Spam-Schreibweisen mit verschlüsselten Buchstaben (z. B. `sᴛʀᴇᴀᴍbᴏᴏ`, `𝗩𝗶𝗲𝘄𝗲𝗿𝘀`) werden jetzt korrekt erkannt und gebannt
- Die KI-Überprüfung startet nur noch bei echten Spam-Signalen (Phrase, Fragment oder Spam-Domain) — harmlose Viewer-Erwähnungen werden nicht mehr unnötig geprüft
- Von der KI bestätigte Muster fließen sofort als vollwertige Filter-Bedingung in die Spam-Bewertung ein — gleicher Score-Mechanismus wie für manuell gepflegte Einträge

## #52 — Auto-Ban lernt jetzt selbst neue Spam-Muster dazu

- Nachrichten die verdächtig wirken aber noch nicht gebannt werden, prüft MiniMax M2.7 automatisch im Hintergrund
- Wird Spam bestätigt, merkt sich der Bot das Kernmuster (Domain, Phrase, Keyword) dauerhaft in der Datenbank
- Zukünftige Nachrichten mit demselben Muster werden direkt erkannt und gebannt — kein manuelles Nachtragen mehr nötig
- Auch Score-0-Nachrichten mit URLs oder Viewer-Keywords werden geprüft (wie der Miracle-Ghost-Fall)

## #51 — AI-Engagement pro Kanal im Admin-Dashboard steuerbar

- Im Streamer-Detail des Admin-Dashboards gibt es jetzt einen Toggle für den AI-Engagement-Chatter
- Der Schalter zeigt den aktuellen Status (Aktiv/Inaktiv) und wer ihn zuletzt geändert hat
- Änderungen greifen sofort — kein Reload nötig

## #50 — Streamer-Dashboard und Clip-Workflow zentral dokumentiert

- Tarife (Free, Raid Boost, Werbefrei, Analyse Dashboard, Bundles), Free-vs-Paid-Cutoffs, Testphase und Kündigung sind jetzt klar erklärt
- Auch der Clip-Workflow (Approval, Upload, Retention) und das Affiliate-Portal sind dokumentiert — Streamer- und Viewer-Fragen dazu werden vom FAQ-Bot im Discord direkt beantwortet

## #49 — KI-Analyse besser auffindbar im "Was tun?"-Bereich

- Die KI-Analyse ist jetzt ein eigener Reiter neben "Pro Session" und "Empfehlungen" — vorher lag sie ganz unten am Seitenende und war kaum zu finden

## #48 — Analyse-Dashboard aufgeräumt: 14 Tabs werden zu 7

- Statt 14 Tabs gibt es jetzt 7 klar getrennte Bereiche — kein langes Suchen mehr in einer überfüllten Leiste
- Coaching, KI-Analyse und Stream-Reports sind im neuen Bereich "Was tun?" gebündelt — alle Empfehlungen an einem Ort
- Audience, Viewer und Chat liegen jetzt zusammen unter "Publikum"
- Wachstum, Vergleich und die Kategorie-Leaderboards sind unter "Wachstum" zusammengefasst
- Alte gespeicherte Links zu einzelnen Tabs funktionieren weiterhin

## #47 — Analytics-Tour komplett überarbeitet: 5 Schritte durch echte Dashboard-Features

- Tour zeigt jetzt die KPI-Kacheln direkt (Viewer, Follower, Retention) statt nur Tab-Buttons
- Neuer Schritt erklärt das KI-Insights-Panel und was der Bot analysiert
- Wachstum, Chat und Coaching als eigene Schritte mit besseren Erklärungen
- Tour startet jetzt zuverlässig — alter "gesehen"-Status wird beim Seitenwechsel automatisch zurückgesetzt

## #46 — Onboarding-Tour: Übergabe zur Abo-Seite zuverlässiger + Button-Label korrigiert

- Letzter Button im Dashboard-Onboarding heißt jetzt "Zur Abo-Seite" statt "Fertig"
- Beim Wechsel zur Abo-Seite wird die alte "Tour gesehen"-Markierung automatisch gelöscht — Tour startet immer korrekt
- Kein manuelles Löschen von Browserdaten mehr nötig

## #45 — Tour-Robustheit: fehlende Anker überspringen ohne permanente Deaktivierung

- Wenn eine Tour beim Start keine Elemente im DOM findet, wird sie nicht mehr dauerhaft als "gesehen" markiert
- Bisheriges Verhalten: Tour hat sich selbst deaktiviert, obwohl der Nutzer sie nie gesehen hat
- Tour wird beim nächsten Seitenaufruf korrekt nochmal angezeigt, sobald die Seite vollständig geladen ist

## #44 — Pricing-Tour Bugfix: richtige Anker im FeaturePicker

- Pricing-Tour wurde sofort übersprungen weil die Anker auf dem falschen Komponent lagen (PlanCardRedesign statt FeaturePicker)
- Tour-Kacheln zeigen jetzt die drei echten Features: Werbefrei, Raid Boost und Analyse
- Preview-Version: Tour startet erst wenn Pläne vom Server geladen sind (kein Frühstart mit leerem DOM)

## #43 — Automatische Onboarding-Tour durch Dashboard, Pläne und Analytics

- Neue Nutzer werden nach der Dashboard-Tour automatisch zur Abo-Seite weitergeleitet
- Auf der Abo-Seite erklärt eine Spotlight-Tour die drei Plan-Stufen (Free, Basic, Extended) mit je einer kurzen Erklärung
- Nach der Pricing-Tour geht es direkt weiter zum Analyse-Dashboard mit einer eigenen Tour
- Die Analytics-Tour zeigt die wichtigsten Tabs: Übersicht, Streams, Wachstum und Chat
- Alle drei Touren können einzeln übersprungen werden und zeigen sich nur beim ersten Besuch

## #42 — Pricing-Seite überarbeitet: Vergleich korrigiert & Auswahl klarer

- Feature-Vergleichstabelle zeigt jetzt echte Plan-Spalten: Free / Werbefrei / Raid Boost / Analyse / Alles drin — statt ungenauer Tier-Labels
- „Bot-Werbung deaktivieren" steht jetzt korrekt beim Werbefrei-Plan, nicht erst beim Bundle
- Trial-Plan zeigt keine Chat-Werbung-Deaktivierung mehr (war schon im Backend korrekt, ist jetzt auch im Vergleich sichtbar)
- Feature-Kacheln auf der Pricing-Seite haben jetzt einen klaren Hinweis „+ Auswählen" und einen erklärenden Titel darüber

## #41 — Bundle-Preise angepasst

- Werbefrei + Raid Boost: 5,99 → 6,99 €/Mo. (geringerer Rabatt, da Raid Boost von anderen Streamern abhängt)
- Werbefrei + Analyse: 11,49 → 10,49 €/Mo. (2 € Rabatt statt bisher 99 ct)
- Alles drin und Analyse + Raid Boost bleiben unverändert

## #40 — AGB: Name und Links korrigiert

- Name „EarlySalty / Deadlock Partner Network" → „Deutsche Deadlock Community" in AGB und Seitentitel
- Zurück-Button und Footer-Link auf der AGB-Seite zeigen jetzt korrekt auf `/twitch/pricing`
- Kündigung in §5 verlinkt jetzt auf das Dashboard statt auf die veraltete Abo-Seite

## #39 — Jahresplan: Sofortabbuchung, 14 Monate Zugang und Rechtssicherheit

- Jahrestarif wird jetzt direkt beim Kauf abgebucht (kein Trial-Zeitraum mehr)
- Käufer eines Jahresabos erhalten automatisch 2 Bonusmonate on top — insgesamt 14 Monate Zugang
- Widerrufsbelehrung nach § 356 Abs. 5 BGB direkt im Checkout sichtbar und bestätigungspflichtig
- AGB aktualisiert: neuer Abschnitt zu sofortiger Leistungserbringung, Jahresbonus klar beschrieben, „6-Monats"-Option entfernt
- „30 Tage kostenlos testen"-Button auf der Pricing-Seite ist jetzt vollständig funktional (einmalig pro Account)
- Feature-Picker auf der Pricing-Seite ersetzt die alten Plan-Karten

## #38 — Neue Abo-Pläne, Jahrestarif und überarbeitete Pricing-Seite

- Zwei neue Bundle-Pläne: „Werbefrei + Analyse" (11,49 €/Mo.) und „Alles drin" (13,99 €/Mo.)
- Jahrestarif (12 Monate) mit 20 % Rabatt auf allen bezahlten Plänen wählbar
- Pricing-Seite vollständig überarbeitet: übersichtliches Toggle Monatlich/Jährlich, keine Informations-Überflutung
- Alte `/twitch/abbo`-Seite leitet dauerhaft auf `/twitch/pricing` weiter
- Stripe-Produkte und -Preise für die neuen Bundles automatisch angelegt

## #37 — Security-Scanner-Alerts bereinigt

- 271 offene Code-Scanning-Alerts (Semgrep + CodeQL) vollständig bearbeitet
- Echte Bugs behoben: undefinierte Variable in Tests, ungenutzte Variable, Lambda-Zuweisung
- Alle False-Positive-Alerts mit korrekten Suppression-Kommentaren versehen (`# nosemgrep`, `# lgtm[...]`)
- 4 Discord-Snowflake-ID-Alerts als False Positive über GitHub API dismissed

## #36 — Werbefrei-Plan für 3,99 € + besseres Onboarding und FAQ

- Neuer Plan „Werbefrei" für 3,99 €/Monat: Die Bot-Discord-Einladung in deinem Chat ist dauerhaft aus — auch wenn ein Admin gerade einen globalen Aktions-Text aktiv hat
- Combo „Werbefrei + Raid Boost" für 5,99 €/Monat (spart 2 € gegenüber Einzelkauf)
- Onboarding-Tour im Dashboard erweitert: erklärt jetzt auch was der Bot grundsätzlich macht, deinen aktuellen Plan, Werbung-Einstellungen und wo du Hilfe findest
- Neue Sidebar-Sektion „Hilfe" mit Direktlink zur FAQ und Knopf zum Neu-Starten der Tour
- FAQ um drei neue Sektionen ergänzt: „Was macht der Bot eigentlich?", „Pläne & Preise" und „Chat-Werbung"

## #35 — Clips ohne Alterswarnung und Twitch-Embeds funktionsfähig

- Clips von xradoo_ und miracleghost9 ersetzt — beide hatten eine Twitch-Alterswarnung, die das Einbetten verhinderte
- Ersetzt durch Clips von friduzockt und einsbezi aus der Community
- Caddy-Konfiguration repariert: Twitch-Embeds auf der /streamer/-Seite wurden durch eine zu restriktive Content-Security-Policy geblockt — jetzt erlaubt
- Demo-Dashboard kann wieder in die Streamer-Website eingebettet werden (gleiches CSP-Problem behoben)

## #34 — Branding der öffentlichen Website vereinheitlicht

- Titel und Link-Vorschau zeigen jetzt „Deutsche Deadlock Community" statt „EarlySalty"
- Kaputte Clips (gelöschte Medal-Links, tote EarlySalty-Clips) durch echte Community-Clips ersetzt
- Favicon auf allen Seiten einheitlich — EarlySalty-„E" im Bot-Favicon entfernt
- Logos über alle Projekte auf denselben Stand gebracht
- „DDC"-Abkürzung im gesamten öffentlichen Website-Text durch ausgeschriebenen Namen ersetzt

## #33 — Dashboard zeigt Twitch-Auth nach Erstanmeldung korrekt als aktiv

- AutoRaid-Status wird nach der ersten Twitch-Autorisierung sofort als aktiv angezeigt
- Bisher blieb das Dashboard auf „inaktiv", obwohl die Auth korrekt gespeichert war und Raids funktionierten
- Betrifft nur neue Streamer beim allerersten Auth-Vorgang, nicht Re-Auth

## #32 — Bot-Absturz beim Start behoben

- Twitch-Bot und Dashboard starten wieder fehlerfrei
- Fehler trat auf, weil eine neue Storage-Funktion intern vergessen wurde zu verknüpfen

## #31 — GitHub Actions Minutenverbrauch deutlich reduziert

- Fünf tägliche Security-Workflows auf reine Event-Trigger umgestellt (kein Schedule mehr)
- Security-Scans und Secret-Scanning laufen jetzt nur noch bei Push und Pull Request
- Semgrep bricht den Build nicht mehr bei jedem Fund ab — Ergebnisse werden als Artifact gespeichert

## #30 — Sicherheitslücke in Test-Abhängigkeit geschlossen

- pytest in der CI-Test-Pipeline auf Version 9.0.3 angehoben — schließt eine Privilege-Escalation-Lücke über das /tmp-Verzeichnis

## #29 — Security-Scan: 390 Alerts bereinigt

- Sensible Werte (User-IDs, Streamer-Logins, Dateipfade) werden jetzt überall vor dem Logging bereinigt — verhindert Log-Injection
- Discord-Nutzer-IDs im Code sind als öffentliche IDs markiert, nicht als Secrets
- Rund 350 False-Positive-Alerts aus dem Semgrep-Scanner (SQL-Queries, Logger-Credential, HTML-Format, Dynamic-Imports) wurden mit präzisen Suppression-Kommentaren ausgestattet, damit echte neue Probleme künftig auffallen
- 63 unbenutzte Imports, 41 unbenutzte Variablen und weitere kleine Stil-Verstöße automatisch bereinigt

## #28 — Hintergrund-Konsolidierungen (Audit-Cleanup Phase 2)

- Streamer-Plan- und Billing-Lookups laufen jetzt über eine gemeinsame Quelle — ein Test, der wegen Schema-Drift rot war, ist wieder grün
- Schnellere Streamer-Suche im Dashboard durch zwei neue Datenbank-Indexe für case-insensitive Logins
- Datenbank-Pool gehärtet: Default-Verbindungen 4 → 10, Connect-Timeouts gesetzt, automatischer Retry bei seltenen Postgres-Deadlocks
- Internal-API-Server -319 Code-Zeilen schlanker (doppelte Helper-Logik in policy.py konsolidiert)
- Doku auf Stand gebracht: korrekte Routen-Übersicht in INDEX/API.md, Stream-Report-Sektion ergänzt, veraltete „geplant"-Marker entfernt

## #27 — Stabilität verbessert (Audit-Cleanup Phase 1)

- Social-Media-Uploads laufen nicht mehr in endlose Wartezeiten, wenn TikTok-, Instagram- oder Login-Provider hängen — alle externen Calls haben jetzt feste Timeouts
- Bot-Reload entfernt nicht mehr versehentlich noch laufende Hintergrund-Module aus dem Speicher — weniger sporadische Crashes nach Cog-Reloads
- Doppelte HTTP- und KI-Client-Initialisierungen zusammengezogen, künftige Wartung einfacher
- Test- und Linter-Konfiguration vereinheitlicht, fehlende Dependency `cryptography` korrekt deklariert

## #26 — KI Chat-Analyse (MiniMax Deep) funktioniert jetzt

- "Analyse starten"-Button im Chat-Analytics-Dashboard war kaputt — der Backend-Endpoint crashte sofort
- Ursache: fehlendes `import json` im Backend-Modul
- Zusätzlich: TypeScript-Buildfehler behoben (fehlende `streamer`-Prop und Typcast im Donut-Chart)
- Dashboard neu gebaut und Bot neu gestartet

## #25 — Security-Deep-Scan verschlankt, kein Sicherheitsverlust

- Python-Security-, JavaScript-Security- und Semgrep-Scans aus dem Deep-Scan entfernt — diese laufen bereits täglich in der Security-Fortress
- Deep-Scan fokussiert sich jetzt auf Trivy-Filesystem-Scan und OSSF-Scorecard — beides hat keinen Doppelläufer
- Weniger doppelte CI-Minuten, gleiche Abdeckung

## #24 — CI-Laufzeiten optimiert, kein Sicherheitsverlust

- Security-Scans (Container, IaC, Supply-Chain) laufen jetzt wöchentlich statt täglich — Schutz bleibt vollständig durch Push-/PR-Trigger
- Security-Incident-Automation läuft jetzt täglich statt alle 6 Stunden — 75 % weniger Runs
- Dependency-Review hat keinen sinnlosen Tages-Schedule mehr

## #23 — CI-Artifacts werden nach 30 Tagen automatisch gelöscht

- Alle automatisch erzeugten CI-Berichte (Security-Scans, Dependency-Reports, Logs) werden ab jetzt nach 30 Tagen automatisch von GitHub entfernt
- Verhindert, dass sich der GitHub-Actions-Speicher dauerhaft volläuft

# Changelog

## #22 — Werbung sensibler für neue Zuschauer + Viewer-Trigger auch bei Normalzahlen

- Discord-Einladung wird jetzt schon bei 3 Chat-Nachrichten im Fenster ausgelöst statt 5
- Viewer-basierter Trigger greift ab sofort auch wenn die Zuschauerzahl einfach auf normalem Niveau liegt — kein "Spike" mehr nötig
- Cooldown für den Viewer-Trigger auf 60 Minuten gesenkt (war 90)
- Neue Bedingung: Promo wird nur gesendet, wenn mindestens 2 Chatter im aktuellen Fenster die letzte Werbung noch nicht gesehen haben — verhindert, dass dieselbe Zuschauerschaft mehrfach dieselbe Meldung bekommt; nach 2 Stunden gilt ein Zuschauer wieder als "neu"
- Im Admin-Dashboard gibt es jetzt Felder für Start- und Endzeit der globalen Promo-Überschreibung — die zeitbefristete Steuerung funktioniert damit korrekt

## #20 — Stream-Reports: Rating-System, neues Report-Layout + Auto-Retry

- Jeder Report hat jetzt Bewertungs-Buttons (Gut / Neutral / Schlecht) mit optionalem Kommentar — direkt unter dem jeweiligen Report sichtbar
- Reports zeigen jetzt alle Analyse-Abschnitte aus dem neuen Minimax-Schema: Snapshot, Kritische Momente, Audience, Chat-Diagnose, Wachstum, Vergleich und Maßnahmen
- Keine chinesischen Zeichen mehr in Reports — Minimax bekommt jetzt eine explizite Sprachanweisung
- Fehlgeschlagene Reports werden jetzt automatisch alle 30 Minuten bis zu 3x erneut versucht
- Minimax-Anfragen brechen nach 3 Minuten automatisch ab statt ewig zu hängen

## #19 — Stream-Reports für alle Partner freigegeben

- Stream-Reports sind jetzt für alle aktiven Partner sichtbar — kein kostenpflichtiger KI-Plan mehr nötig
- Fehlerbehebung: Dashboard-Service wurde nach Code-Änderungen nicht neu gestartet, Reports haben deshalb nicht geladen

## #18 — Stream-Reports: Neues Analyse-Schema + Minimax komplett aufgedreht

- Report-Prompt komplett neu geschrieben: 5 konkrete Analyse-Aufgaben (Kritische Momente, Audience-Qualität, Chat-Diagnose, Wachstums-Signale, Ehrlicher Vergleich)
- Minimax darf jetzt deutlich mehr schreiben: Token-Limit von 6.000 auf 16.000 erhöht
- Report-Ausgabe folgt jetzt einem klaren deutschen Schema (snapshot, momente, audience, chat_diagnose, wachstum, vergleich, massnahmen)
- Fehler-Fallback passt sich dem neuen Schema an — Dashboard bricht nicht mehr bei Parse-Fehlern

## #17 — Stream-Reports: Backfill beim Start + weitere SQL-Bugfixes

- Beim Bot-Start werden automatisch die letzten 3 Sessions pro Streamer mit einem Minimax-Report nachgefüllt, falls noch keiner existiert
- Wöchentliche Titel-Insights: zweiter SQL-Bug behoben (Session-Lookup ging an falscher Spalte, Sessions wurden nie geladen)
- Bisherige Stream-Reports ohne Fehler werden nicht doppelt generiert

## #16 — Stream-Reports mit Minimax funktionieren jetzt für alle Streamer

- Nach jedem Stream-Ende erstellt Minimax automatisch einen detaillierten Report mit Viewer-Kurve, Chat-Analyse und Vergleich zu früheren Sessions
- Reports werden für alle Streamer generiert — kein kostenpflichtiger Plan mehr nötig, um die Funktion zu nutzen
- Admins können alle Reports im Dashboard einsehen, unabhängig vom Streamer-Plan
- Wöchentliche Titel-Insights waren wegen eines Datenbankfehlers kaputt — dieser ist jetzt behoben
- Tabellen für KI-Reports werden beim ersten Start automatisch angelegt, falls die Migration noch nicht gelaufen ist

## #15 — Voice-Reaction: Bot führt jetzt echte Gespräche nach Outreach & Raids

- Nach jedem Outreach und jedem Outreach-Boost-Raid kann der Bot eigenständig im Streamer-Chat antworten — wie ein echter Community-Mensch, nicht wie ein Sales-Bot
- Der Bot hört kurz in den Stream rein und reagiert sinnvoll auf das, was der Streamer gerade sagt oder im eigenen Chat schreibt
- Bei klar interessierten Streamern landet automatisch eine Discord-Notification beim Team, damit ein Mensch persönlich übernimmt
- Standardmäßig deaktiviert — wird im Staging mit Trockenlauf aktiviert, bevor live geantwortet wird
- Komplettes Audit-Log pro Konversation, damit das Verhalten manuell durchgesehen und nachjustiert werden kann

## #14 — Beta: Auto-Highlight-Clips per Discord DM (EarlySalty)

- Nach jedem Deadlock-Match werden automatisch Highlights erkannt (Triple Kill, Multi Kill, Team Fights)
- Clips werden direkt aus dem Twitch-VOD ausgeschnitten und per Discord DM gesendet
- Erkennt Multi-Kills (≥3 Kills in 10 Sek) und Team-Fights (≥4 Kills in 15 Sek)
- Prüft alle 10 Minuten auf neue Matches der letzten 24 Stunden

## #13 — Bot taucht nicht mehr in Analyse-Daten auf

- Der Bot selbst (`deutschedeadlockcommunity`) wird jetzt überall aus Chat-Statistiken und Analyse-Dashboards herausgefiltert
- Viewer-Rankings, Publikumsauswertungen und Chat-Tiefenanalysen zeigen keine Bot-Einträge mehr

## #12 — Dashboard-Login funktioniert wieder + sauberes Partner-Status-Gating

- Dashboard-Login war komplett kaputt: SQL-Query referenzierte nicht-existierende Spalten (`is_partner`, `archived_at`, `created_at`) und brach mit 503 ab. Login funktioniert jetzt wieder zuverlässig.
- Wer sich erfolgreich einloggt und nicht permanent gesperrt ist, wird automatisch wieder als aktiver Partner geführt — kein manuelles Reset mehr nötig nach Re-Auth.
- Departnered/archivierte Streamer können sich jetzt einloggen und kommen ins Dashboard, sehen aber nur Verwaltung, Pläne und Affiliate-Bereich. Analyse, Social Media und Title-Generator bleiben gesperrt bis ein gültiger Twitch-OAuth durchläuft (außer Bot-Bann oder permanenter Block).
- Verwaltung-Seite zeigt einen klaren Hinweis-Banner mit „Jetzt neu autorisieren"-Button, wenn der Partner-Status nicht aktiv ist.

## #11 — Plan-Preise um 50% gesenkt

- Raid Boost: 7,99 € → **3,99 €** pro Monat
- Analyse Dashboard: 16,99 € → **8,49 €** pro Monat
- Bundle (Analyse + Raid Boost): 22,99 € → **11,49 €** pro Monat
- 6-Monats- und 12-Monats-Tarife folgen automatisch (mit den bestehenden 10% / 20% Mehrjahresrabatten)
- Stripe-Preise synchronisiert; bestehende Abos rechnet Stripe weiter zum bisherigen Betrag ab, neue Buchungen laufen automatisch auf die halbierten Preise

## #10 — Social-Media-Dashboard ist jetzt eine eigene Seite

- Im Analyse-Dashboard gibt es keinen „Social Media"-Tab mehr; das Tooling sitzt unter der eigenen URL `https://deutsche-deadlock-community.de/social-media-admin`
- Die neue Seite hat einen schlanken eigenen Header (Admin-Badge + Rück-Link auf `/analyse`) und zeigt direkt die Clip-Pipeline ohne Tab-Navigation drumherum
- Partner sehen die Seite weiterhin nicht; ohne Admin-Recht kommt eine klare „Admin-Zugriff erforderlich"-Meldung
- Caddy ist um die neue Route erweitert, der Login-Redirect kehrt nach erfolgreicher Twitch-Auth direkt auf das Social-Media-Dashboard zurück

## #9 — Discord-Freigabe für Clips + Auto-Approve pro Plattform

- Fertig angereicherte Clips landen jetzt zuerst in einer Freigabe-Schleife, statt sofort in die Upload-Pipeline zu rutschen
- Ein Admin bekommt pro Clip eine Discord-DM mit Vorschau, plattformspezifischen Hashtags und den Aktionen „Posten", „Bearbeiten" oder „Skip"
- Beim Freigeben lassen sich YouTube Shorts, TikTok und Instagram Reels einzeln auswählen
- Zusätzlich gibt es im Social-Media-Dashboard neue Auto-Approve-Schalter pro Plattform, damit bestimmte Ziele nach einer Freigabe immer automatisch mit in die Queue gelegt werden
- Cross-Posting startet erst nach Freigabe oder Auto-Approve und nicht mehr schon vor dem Approval-Schritt

## #8 — Social-Media-Phase 3: Performance-Tracking, LLM-Reports und Analytics-Tab

- Veröffentlichten Clips werden jetzt pro Plattform in 24h-, 7d- und 30d-Buckets nachgezogen, inklusive Views, Likes, Comments, Shares, Watch-Time, CTR und Engagement-Rate
- Jede Woche kann automatisch ein deutscher LLM-Report für einzelne Streamer entstehen; zusätzlich gibt es einen monatlichen Cross-Streamer-Report sowie einen wöchentlichen Admin-Report per Discord-DM
- Im Admin-Dashboard gibt es jetzt einen eigenen Analytics-Bereich mit Charts pro Clip und einer Report-Liste für gespeicherte Streamer-, Cross- und Admin-Reports
- Migration weiter separat: vor dem ersten Einsatz einmal `python bot/migrations/social_media_phase3_analytics.py` ausführen, damit die Analytics-Spalten und die neue Tabelle `social_media_reports` angelegt werden

## #7 — Social-Media-Dashboard 2.0: Phase 0–2 (Layout-Editor, Auto-Aufbereitung)

- Bestehender Tab „Streams" wurde in „Social Media" umbenannt und ist vorerst nur für Admins sichtbar
- Clips bekommen jetzt automatisch ein vertikales 9:16-Layout mit Game- und Cam-Box, das pro Streamer als Default speicherbar ist und pro Clip übersteuert werden kann
- Eigene MP4s lassen sich direkt im Dashboard hochladen und werden 14 Tage aufbewahrt, bevor sie automatisch aufgeräumt werden
- Neue Auto-Aufbereitung: Clips werden lokal transkribiert, Deadlock-Begriffe (Helden, Items, Abilities, Slang) werden korrigiert und ein lokales LLM (Ollama auf dem Server, kein Datenabfluss) schlägt Title, Description und Hashtags je YouTube/TikTok/Instagram vor
- Externe LLMs (z. B. MiniMax oder Claude Haiku) bleiben standardmäßig aus und werden nur genutzt, wenn ein Admin den Schalter „External-LLM-Consent" ausdrücklich aktiviert
- Migration nicht automatisch — vor dem ersten Lauf einmal `python bot/migrations/social_media_phase2_enrichment.py` ausführen, damit die neuen Tabellen `deadlock_vocab` und `social_media_clip_enrichment` angelegt sind

## #6 — Changelog im Dashboard zeigt jetzt die letzten Updates

- Die Sektion „Was gibt's Neues" auf dem Streamer-Dashboard ist jetzt befüllt
- Alle bisherigen Verbesserungen (#1–#5) sind dort als Einträge sichtbar
- Künftige Updates erscheinen automatisch dort, sobald sie veröffentlicht werden

## #5 — Aktive Tab-Buttons im Analyse-Dashboard ohne kaputten 1px-Halo

- Aktiver Tab (z. B. „Übersicht") hatte einen harten cyan 1px-Strich am Rand, der mit dem Card-Highlight kollidierte und broken aussah
- Border ersetzt durch weichen Inset-Highlight + sanfteren Außen-Glow

## #4 — Glow-Tuning und feines Hintergrund-Grid

- Mini-KPI-Karten (Ø Viewer, Follower, Chat-Aktivität, Stream-Stunden) leuchten jetzt dauerhaft in ihrer Trendfarbe und nicht erst beim Hover
- Health-Score-Karte hat ein deutlich dezenteres Glow, damit es nicht mehr in den Bereich daneben überstrahlt
- Subtiles Gitternetz-Pattern im Dashboard-Hintergrund — wirkt weniger leblos, aber bleibt im Hintergrund

## #3 — Build-Toolchain auf aktuelle Node-LTS aktualisiert

- Build-System läuft jetzt auf Node 22 LTS statt der alten Node-18-Version
- Frontend-Build ist etwas schneller und ohne Versionswarnungen
- Keine Auswirkungen auf die Bot-Funktionalität, rein interne Aufräumung

## #2 — Streamer-Dashboard mit deutlich mehr Vibe

- Karten haben jetzt einen weichen farbigen Glow am Rand und heben sich beim Hover sichtbar an
- Header bekommt eine subtile rotierende Aura im Hintergrund
- Health-Score-Ring leuchtet farblich passend (grün/gelb/rot) mit Drop-Shadow
- Wochen-KPIs bekommen pro Karte eine farbige Trend-Aura (grün bei +, rot bei -)
- Sparkline-Linien glühen leicht in ihrer Trendfarbe
- Last-Stream-Mini-Stats (Ø Viewer, Peak, Follower, Chat) bekommen Hover-Spotlight und Text-Glow
- Activity-Items haben jetzt einen vertikalen Akzent-Streifen, farblich nach Typ (Raid grün, Ban rot, Warnung gelb)
- Letzte Streams Liste hat einen blau-violetten Akzent-Streifen pro Eintrag
- Live-Indikator pulsiert mit zusätzlichem roten Außenglow

## #1 — Streamer-Dashboard schneller, schöner und mit funktionierender Navigation

- Dashboard lädt deutlich schneller (Doppelter API-Request entfernt, Backend in mehrere parallele Aggregationen aufgeteilt)
- Sidebar-Links zu Overview, Streams und Chat funktionieren wieder und springen direkt auf den richtigen Tab
- Beim Laden erscheint sofort eine animierte Vorschau (Skeleton) statt eines leeren Spinners
- Neuer Live-Indikator im Header zeigt, ob du gerade live bist, mit aktueller Viewer-Zahl und Stream-Titel
- Wochen-KPIs (Ø Viewer, Follower, Chat, Stream-Stunden) haben jetzt eine Mini-Trendlinie der letzten 7 Tage
- Neue Sektion „Letzte Streams" listet die letzten 5 Streams mit Datum, Dauer, Ø Viewer, Peak und Follower-Zuwachs
- Aktivitäts-Feed lässt sich nach Raids, Bans und Warnungen filtern und mit „Mehr laden" ausklappen
- Sanftere Hintergrund-Animation, dezentere Optik und mehr Mikro-Animationen in Sidebar, Cards und Listen
