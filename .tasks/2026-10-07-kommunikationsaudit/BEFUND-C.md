# Paket C: Einstieg und allgemeines Dashboard

Rechercheagent: aece84227e7f4f288. Hauptsession hat die Agentenübergabe hier festgehalten. Snapshot `9315b3cf7e4a6feeffc32b603db26eca78b03a2c`; Pfade relativ zum Repository, gelesen unter `/tmp/twitch-kommunikationsaudit-20261007/`. Keine produktiven Aktionen oder Tests. Belegstärke der folgenden Funde: hoch für den statischen Codepfad, kein Nachweis tatsächlicher Fehlerfälle im Betrieb.

## C-01: Falsche technische Unmöglichkeitsbehauptung

P1, `/streamer/`. Sicherheitstext: „Keine Broadcaster-Rechte“, „Streamtitel, Kategorie oder Kanal-Einstellungen kann er technisch gar nicht anfassen.“ Der Titelgenerator kann nach zusätzlicher Freigabe Titel manuell und automatisch setzen. Das Titelprofil fordert das entsprechende Schreibrecht an.

Belege: `website/src/components/partner-clean/Security.tsx:12`; `rust/crates/tb-raid/src/scope_profiles.rs:75`; `rust/crates/tb-dashboard-api/src/handlers/title.rs:423`.

Korrektur: Basisrechte von zusätzlich freigegebenen Funktionen unterscheiden, absolute Unmöglichkeit streichen. Textproblem. Keine Aussage zu den Rechten konkreter Konten.

## C-02: Unsichtbare Vorabmoderation versprochen

P1, Landingpage. „Scam- und Spam-Konten fliegen aus deinem Chat, bevor jemand sie sieht.“ Tatsächlich verarbeitet die Pipeline eingegangene Nachrichten. Der Scam-Pitch-Pfad kann warnen, ohne zu löschen, und später einen Timeout auslösen.

Belege: `website/src/components/partner-clean/Features.tsx:60`; `rust/crates/tb-chat/src/pipeline.rs:1007,1046`.

Korrektur: Reaktion auf erkannte Nachrichten beschreiben, keine garantierte Unsichtbarkeit. Textproblem. Keine Reaktionszeitmessung.

## C-03: „Vollständig“ bedeutet lediglich vorhandenes Raid-Recht

P2, `/twitch/verwaltung#konto`. „Twitch-OAuth ist aktiv und vollständig.“ beziehungsweise „1 Scope fehlt. Bitte neu autorisieren.“ Der Status vergleicht lediglich `channel:manage:raids`. Bei einem Datenbankfehler kommt `missing` mit leerer Fehlrechte-Liste zurück, die Oberfläche behauptet trotzdem ein fehlendes Recht.

Belege: `bot/dashboard_v2/src/pages/Verwaltung.tsx:146`; `rust/crates/tb-dashboard-api/src/handlers/internal_home.rs:44,1191`.

Korrektur: Verbindung pro Funktion bewerten und „gerade nicht prüfbar“ von fehlender Freigabe unterscheiden. Text und Statusverhalten.

## C-04: Warnschalter verschweigt Zuschauer-Timeouts

P1, `/twitch/verwaltung#bot`. „Vor Abzock-Maschen warnen“, „warnt frühzeitig davor“. Derselbe Schalter aktiviert im Scam-Pitch-Pfad auch `StrongTimeout`.

Belege: `bot/dashboard_v2/src/components/verwaltung/ModerationSection.tsx:19`; `rust/crates/tb-chat/src/pipeline.rs:1007,1017`.

Korrektur: Warnung und mögliche Eskalation nennen. Eine reine Warnfunktion wäre eine gesonderte Verhaltensentscheidung. Textproblem und mögliche Produktentscheidung.

## C-05: „Scam-Schutz aus“ wird als Gesamtschalter verstanden

P2, `/twitch/verwaltung#bot`. „Scam-Schutz aktiv“, Modus „Nur melden“ mit „Der Bot führt keine automatische Aktion aus.“ Gemeint ist der zusätzliche Gesprächsprüfer. Global-Ban, Scam-Pitch, Spam-Auto-Ban und verdächtige Einladungen besitzen separate Schalter und können weiterhin eingreifen.

Belege: `bot/dashboard_v2/src/components/verwaltung/ScamGuardSection.tsx:23,112`; `rust/crates/tb-chat/src/pipeline.rs:901,1067`.

Korrektur: Den Geltungsbereich der zusätzlichen Gesprächsprüfung benennen und verbleibende Schutzaufgaben erkennbar machen. Text/Bedienung.

## C-06: Sichtbar ausgeschaltete Titelautomatik kann weiter veröffentlichen

P1, `/twitch/titel`. Schalter „Experimentell: automatisch auf Twitch setzen“ und Knopf „Titel bauen“. Der Schalter ändert den lokalen Entwurf, erst „Speichern“ schreibt ihn. Die Generierung übermittelt die neue Schalterstellung nicht; Rust verwendet die gespeicherte Einstellung. Wer ausschaltet und sofort einen Titel baut, kann deshalb entgegen der sichtbaren Stellung veröffentlichen.

Belege: `bot/dashboard_v2/src/pages/TitleGenerator.tsx:119,168,301`; `rust/crates/tb-dashboard-api/src/handlers/title.rs:423`.

Korrektur: Entwurf und gespeicherten Automatikzustand trennen, ungespeicherte Änderung vor Generierung klären. Bedienung/Verhalten.

## C-07: Schreibrecht ohne geladenen Status als verbunden angezeigt

P1, Titelgenerator. „Schreibrecht verbunden“ erscheint auch dann, wenn die Einstellungen fehlen: Die Bedingung fällt in den grünen Zweig. Ladefehler sowie bestimmte Speicherfehler zu Stil, Ausschlüssen und Automatik werden nicht angezeigt.

Belege: `bot/dashboard_v2/src/pages/TitleGenerator.tsx:94,134,294`; `rust/crates/tb-dashboard-api/src/handlers/title.rs:706`.

Korrektur: Laden, unbekannt, verbunden und fehlende Freigabe unterscheiden; Speicherfehler sichtbar machen. Statusverhalten.

## C-08: Fehlgeschlagenes automatisches Titelsetzen unsichtbar

P1, Titelgenerator bei eingeschalteter Automatik. Rust kann `auto_set_status: "error"` zurückgeben. Die Oberfläche behandelt `set` und `scope_missing`, macht den sonstigen Fehler aber zu `idle`. Dadurch erscheint die vorhandene Fehlermeldung nicht.

Belege: `rust/crates/tb-dashboard-api/src/handlers/title.rs:437`; `bot/dashboard_v2/src/pages/TitleGenerator.tsx:124,394`.

Korrektur: „Vorschlag erstellt, auf Twitch nicht übernommen“ samt Rückweg anzeigen. Statusverhalten, Fehler nicht live ausgelöst.

## C-09: Erfolg bleibt nach Titelbearbeitung stehen

P1, Titelgenerator. „Auf Twitch gesetzt“ und „Dieser Titel wurde automatisch auf Twitch übernommen.“ Bearbeitung oder Alternativauswahl ändern den Entwurf, setzen den Veröffentlichungsstatus aber nicht zurück. Damit sieht ein noch nicht übernommener Text bereits veröffentlicht aus.

Belege: `bot/dashboard_v2/src/pages/TitleGenerator.tsx:189,362,384,395`.

Korrektur: Erfolg an den tatsächlich gesetzten Text binden und bei Änderung „Noch nicht übernommen“ anzeigen. Statusverhalten.

## C-10: Fehlende Analysegrundlage erscheint als gemessene 50 Punkte

P2, freigeschaltete Analyseübersicht. „Channel Health“, „Ret.“ und „Eng.“ zeigen Zahlen. Unter drei verwertbaren Messungen setzt Rust Teilwerte pauschal auf 50; die Karten kennzeichnen diesen Ersatz nicht. Er fließt auch in den Gesamtwert ein.

Belege: `rust/crates/tb-dashboard-api/src/handlers/overview.rs:103,128`; `bot/dashboard_v2/src/components/cards/HealthScoreCard.tsx:11`; `bot/dashboard_v2/src/App.tsx:151`.

Korrektur: Zu wenig Daten anzeigen statt scheinbar gemessener Punkte; Kennzahlen verständlich benennen. Text/Verhalten. Betrifft die freigeschaltete Übersicht, nicht den kostenlosen Tagesform-Einstieg.

## C-11: Hilfe führt vor allem durch Einrichtung

P2, `/twitch/hilfe`. „Hilfe & Einrichtung“ zeigt Rundgang und Rückmeldebox. Die separate öffentliche Bot-Hilfe unter `/streamer/help` wird von der Einrichtungskarte nicht verlinkt. Wer eine unbekannte Bot-Aktion oder einen Befehl verstehen will, landet nicht unmittelbar bei der passenden Erklärung.

Belege: `bot/dashboard_v2/src/App.tsx:438`; `bot/dashboard_v2/src/components/onboarding/EinrichtungCard.tsx:19`; `rust/crates/tb-dashboard-api/src/handlers/help_page.rs:37`; `rust/crates/tb-dashboard-api/src/lib.rs:1949`.

Korrektur: Einrichtung, Befehle, automatische Aktionen und Problemlösung verbinden. Navigationsproblem. Vollständigkeit des produktiv geladenen Wissenskorpus nicht geprüft.

## C-12: Trennbericht liefert interne Zustände statt ausführbarer Hilfe

P2, `/twitch/verwaltung#bot`. „Opt-out: gesetzt“, `no_token` und „Bitte im Twitch-Chat mit /unmod entfernen.“ Der Botname fehlt im genannten Befehl. Außerdem kann der Ablauf nach erfolgreichem Moderatorentzug noch beim Beenden der Partnerschaft scheitern.

Belege: `bot/dashboard_v2/src/components/verwaltung/DisconnectBotSection.tsx:85`; `bot/dashboard_v2/src/api/disconnectBot.ts:38`; `rust/crates/tb-internal-api/src/handlers/streamers.rs:1023,1041`.

Korrektur: Erledigte und offene Schritte verständlich trennen, vollständigen kopierbaren Befehl liefern, unklaren Ausgang erneut prüfen. Text/Bedienung. Kein echter Trennvorgang ausgeführt.

## Abdeckung und positive Gegenbeispiele

| Oberfläche | Ergebnis |
| --- | --- |
| Aktive Landingpage | Einstieg über `website/index.html:131` und `streamer-v2.tsx` nachvollzogen. |
| Anmeldung/Onboarding | Vier Schritte und Basisfreigabe geprüft; neue Teilnahmeentscheidung nicht als vorhanden behandelt. |
| Verwaltung | Konto-, Verbindungs-, Partnerstatus- und Fehlerkommunikation geprüft. |
| Einrichtungstour | Positiv: Speichern versus sofortige Wirkung, stille Hinweise versus Aktion und freiwillige Schritte werden erklärt. |
| Hilfe | Einrichtung und Rust-Hilfe getrennt untersucht; Live-Korpus offen. |
| Analyse | Kostenloser Einstieg von freigeschalteter Übersicht unterschieden. |
| Community | Verknüpfung, Zugriff, Aktualität und Leerzustände nachvollziehbar. Freier Sprachkanalplatz wird nicht als Spielplatzgarantie dargestellt. |
| Titelgenerator | Einstellungs-, Vorschlags- und Veröffentlichungswege gegen Rust geprüft. |
| Bot & Schutz | Vier Schutzaufgaben, zusätzlicher Gesprächsprüfer, stille Hinweise und Trennung geprüft. |
| Historische oder ungeklärte Komponenten | `AuthScopes.tsx` und `AIEngagementSection.tsx` ohne gefundenen Einbau nicht als ausgelieferte Hauptbefunde gewertet. |

Andere Pakete prüfen Raids, Werbung, Social Media, Uplink und Clips. Kein Browser-, Login-, Produktions- oder Caddy-Nachweis durch diesen Agenten.
