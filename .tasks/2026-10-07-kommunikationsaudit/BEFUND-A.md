# Paket A: Auto-Raid und Partnerschaft

Rechercheagent: a69256172e2af6cc4. Hauptsession hat die Agentenübergabe festgehalten. Grundlage: Snapshot `9315b3cf7e4a6feeffc32b603db26eca78b03a2c`, gelesen unter `/tmp/twitch-kommunikationsaudit-20261007/`. Pfade unten relativ zum Repository. Graphify vor der Codeprüfung. Keine produktiven Aktionen, Kontoänderungen oder Tests.

## Ergebnis

Die gewünschte Partnerschaft ohne Raid-Netzwerk-Teilnahme ist im geprüften Stand nicht gebaut. Senden und Empfangen hängen an verschiedenen Bedingungen. Außerdem sind Ziele außerhalb des Netzwerks und eine kurze Just-Chatting-Ausnahme vorhanden. Eine Formulierung „nur innerhalb unseres Netzwerks, nur wenn du gerade Deadlock spielst“ wäre deshalb ohne Verhaltensänderung nicht richtig.

## A-01: „Volle Kontrolle“ ohne getrennten Raid-Schalter

P1, Landingpage, Anmeldung und Verwaltung. „Volle Kontrolle“, „Automatisierung, wenn sie hilft“ wecken die Erwartung, automatische Raids abschalten und trotzdem Partner bleiben zu können. Die öffentliche Hilfe erklärt dagegen ausdrücklich, dass weder separater Schalter noch Chatbefehl existieren. Der angebotene Ausstieg trennt den Bot und beendet die Partnerschaft.

Belege: `website/src/components/partner-clean/RaidExplainer.tsx:24-26`; `rust/knowledge/bot/auto-raid.md:19`; `rust/knowledge/bot/faq-raids.md:23-25`; `bot/dashboard_v2/src/pages/Verwaltung.tsx:394-400`.

Korrektur: Gegenwärtige Kontrolle ehrlich erklären; künftig Partnerschaft und Netzwerk-Teilnahme getrennt darstellen. Text/Bedienung plus gewünschte Verhaltensänderung. Weitere verbleibende Netzwerkleistungen gemeinsam klären. Belegstärke hoch.

## A-02: Ausgehende und eingehende Raids haben keinen gemeinsamen Teilnahmezustand

P1. „Das Netzwerk arbeitet in beide Richtungen“ legt eine gekoppelte Teilnahme nahe. Die Quelle benötigt aktive Partnerschaft, `raid_bot_enabled` und `raid_enabled`. Der reguläre Empfängerpool prüft dagegen aktive Partnerschaft und setzt für aufgenommene Kandidaten `raid_enabled: true`, unabhängig vom Sendeschalter. Ein Kanal außerhalb des Partnerpools ist zudem nicht grundsätzlich aus dem Kategorie-Fallback ausgeschlossen.

Belege: `rust/knowledge/bot/faq-raids.md:43-45`; `rust/crates/tb-raid/src/offline_eligibility.rs:70-91`; `rust/crates/tb-raid/src/partner_roster.rs:61-89`; `rust/crates/tb-raid/src/target_resolution.rs:260-278`; `rust/crates/tb-raid/src/auto_raid_pipeline.rs:516-519`.

Korrektur: Für den neuen Nutzerwunsch einen ausdrücklich beidseitigen Teilnahmezustand vorsehen. Er muss auch Förder- und Kategoriepfade berücksichtigen, wenn diese Netzwerkunterstützung verteilen. Behandlung manueller Raids gesondert entscheiden. Verhaltensänderung mit passenden Statusanzeigen. Belegstärke hoch für Auswahlregeln, keine betroffenen Konten ermittelt.

## A-03: Ziele außerhalb des Netzwerks werden verschwiegen

P1, Landingpage und Raid-Hilfe. „Der Bot sucht einen geeigneten Live-Partner im Deadlock-Netzwerk. Wenn kein Ziel die Voraussetzungen erfüllt, findet kein Raid statt.“ Im eingebundenen Auswahlpfad kann dagegen zuerst ein Förderziel aus angesprochenen Streamern gewählt werden, danach ein Netzwerkpartner, schließlich ein deutschsprachiger Stream aus der Deadlock-Kategorie. Der Förderpfad kann also vor vorhandenen Partnerzielen greifen. Auch fehlende Partner-Scores können zum Kategoriepfad führen.

Belege: `rust/knowledge/bot/faq-raids.md:29`; `rust/crates/tb-raid/src/auto_raid_pipeline.rs:781-848,931-940`; `rust/crates/tb-raid/src/target_resolution.rs:189-195`; `rust/bin/tb-bot/src/main.rs:1079-1088`.

Korrektur: Außenpfade ehrlich benennen. Für die gewünschte ausschließliche Verteilung im Netzwerk Förderpriorität und Kategorie-Fallback ändern oder entfernen. Textproblem und Produkt-/Verhaltensentscheidung. Belegstärke hoch für eingebundenen Code, aktuelle Förderkandidaten nicht geprüft.

## A-04: Deadlock-Grenzen werden widersprüchlich vereinfacht

P2. „Nur bei Deadlock aktiv“ und Empfang „solange du live bist“ vermitteln unterschiedliche Grenzen. Bei Zielspiel Deadlock sind aktuell Deadlock spielende Quellen und Partnerziele geeignet. Just Chatting ist ebenfalls geeignet, wenn in derselben Session Deadlock lief und der letzte Deadlock-Zeitpunkt höchstens sechs Minuten zurückliegt. Andere Kategorien sind ungeeignet; Deadlock-Ziele werden vor der Just-Chatting-Ausnahme bevorzugt.

Belege: `website/src/data/twitchKnowledgeBase.ts:70-73`; `rust/knowledge/bot/faq-raids.md:45`; `rust/crates/tb-raid/src/eligibility.rs:14,43-62`; `rust/bin/tb-bot/src/auto_raid.rs:325-338,555-570`.

Korrektur: Quellen und Empfänger getrennt erklären, kurze Verabschiedung in Just Chatting berücksichtigen. Strengere oder großzügigere Grenzen gemeinsam entscheiden. Textproblem, bei geänderter Grenze Produktentscheidung. Belegstärke hoch für Implementierung, produktive Zielspielkonfiguration nicht geprüft.

## A-05: Freigabenachricht behauptet fälschlich „nur die Raid-Berechtigung“

P1, Discord-Nachricht zur Twitch-Verbindung. „Du gibst nur die Raid-Berechtigung frei, sonst nichts“. Schon das Basisprofil verlangt neben Raid-Verwaltung unter anderem Moderatorenverwaltung, Kanalzugang, Clip-Erstellung sowie Leserechte für Werbung, Bits und Kanalpunkte. Der Link begrenzt das Profil nicht ausdrücklich auf Basisrechte. Bei bestehendem Streamerkontext kann das automatische Profil zusätzliche Dashboard-Rechte für Titel- und Werbesteuerung verlangen.

Belege: `rust/bin/tb-bot/src/raid_oauth_impl.rs:69,980-982,1079-1081`; `rust/crates/tb-raid/src/scope_profiles.rs:37-70`; `rust/crates/tb-raid/src/oauth_flow.rs:119-124`.

Korrektur: Falsche Ausschließlichkeitsbehauptung entfernen, Zwecke der angefragten Rechte verständlich nennen. Rechtebedarf für Partner ohne Raid-Teilnahme gesondert prüfen. Textproblem mit Vertrauenswirkung; mögliche Rechteaufteilung ist Verhaltens-/Produktentscheidung. Belegstärke hoch.

## A-06: `!raid_status` zeigt keinen verlässlichen Betriebsstatus

P1, Chatbefehl. „Aktiv | Auto-Raids sind aktiviert.“ liest lediglich `twitch_raid_auth.raid_enabled`, nicht den zusätzlichen Sendeschalter oder die zeitweilige Unterdrückung nach Verbindungsabbrüchen. Der tatsächliche Offline-Pfad prüft diese zusätzlich. Bei inaktiver Partnerschaft sagt derselbe Befehl „Dieser Kanal ist nicht als Partner registriert“, obwohl die Partnerschaft vorhanden, aber inaktiv sein kann.

Belege: `rust/crates/tb-chat/src/commands.rs:169-174,817,1537-1541`; `rust/bin/tb-bot/src/chat_wiring.rs:2542-2551`; `rust/bin/tb-bot/src/auto_raid.rs:283-307`.

Korrektur: Betriebsbereitschaft samt Grund anzeigen und Senden von Empfang unterscheiden. Statusermittlung und Textproblem. Belegstärke hoch. Ein zusätzlich fest verdrahteter Home-API-Status in `internal_home.rs:764` wurde mangels belegter sichtbarer Verwendung nicht als eigener UI-Fund aufgenommen.

## A-07: Rückkehr nach Trennung wird falsch mit Anmeldung gleichgesetzt

P2, `/twitch/verwaltung#bot`. „er kommt nicht von selbst zurück“ und „meldest du dich später wieder an, moddet er sich erneut selbst“. Gewöhnlicher Dashboard-Login respektiert die bewusste Trennung. Der ausdrückliche Raid-/Partner-Verbindungsweg kann dagegen reaktivieren.

Belege: `bot/dashboard_v2/src/components/verwaltung/DisconnectBotSection.tsx:62-64`; `rust/crates/tb-internal-api/src/handlers/streamers.rs:1051-1053`; `rust/crates/tb-dashboard-api/src/auth/session.rs:1489-1496`; `rust/bin/tb-bot/src/raid_oauth_impl.rs:1425-1432,1462-1470`.

Korrektur: Dashboard-Anmeldung und erneutes Verbinden klar trennen; passenden Rückweg verlinken. Vorhandenen Schutz nicht aufweichen. Text/Bedienung, Belegstärke hoch.

## A-08: Automatische Raid-Pause ohne Endzeit und Empfangsfolgen

P2, Chat nach wiederholten Streamabbrüchen. „Die automatischen Raids halte ich hier fürs Erste an“, mit Hinweis auf `!raid`. Wiederholtes Zurückkommen setzt eine zeitlich begrenzte Unterdrückung für die Quelle. Default: zwölf Stunden. Empfang bleibt möglich.

Belege: `rust/bin/tb-bot/src/flip_unraid.rs:24-28,179-196`; `rust/crates/tb-config/src/operations.rs:121-124`; `rust/crates/tb-raid/src/partner_roster.rs:61-89`.

Korrektur: Tatsächlichen Endzeitpunkt und den auf ausgehende automatische Raids begrenzten Umfang nennen; Zustand in Dashboard und Statusbefehl konsistent zeigen. Text und fehlende Statusanzeige. Belegstärke hoch für Mechanik und Defaults, produktive Zeitwerte offen.

## Zustände im geprüften Stand

| Zustand | Automatisch senden | Im regulären Partnerpool empfangen |
| --- | --- | --- |
| Aktiver Partner mit funktionierendem Raid-Zugang | Bei weiteren passenden Voraussetzungen | Bei passender Kategorie und Auswahl |
| Sendeschalter aus, Partner aktiv | Nein | Möglich |
| Zeitweilige Unterdrückung nach Abbrüchen | Während der Unterdrückung nein | Möglich |
| Inaktiv oder bewusst getrennt | Durch entsprechende Zustandsprüfung blockiert | Nicht im regulären Partnerpool |
| Deadlock-Pause nach längerer Spielabwesenheit | Pausenmarker allein schaltet Raids nicht ab | Partnerstatus bleibt, Kategorieprüfung gilt |

Ausschluss aus dem Partnerpool ist kein vollständiger Ausschluss aus Kategorie- oder Förderpfaden.

Die Deadlock-Pause ist keine freiwillige Teilnahmeentscheidung. Nach standardmäßig 60 Tagen ohne Deadlock gibt der Bot Moderatorrechte ab. Der Text „Du bleibst Partner und musst nichts machen“ samt automatischer Rückkehr erklärt diesen Fall bereits passend. Der Marker ändert weder Partnerstatus noch Raid-Schalter. Belege: `rust/crates/tb-raid/src/deadlock_pause.rs:29,80-91,459-479`; `rust/migrations/20260623150000_drop_manual_verified_columns.sql:44-48`.

## Abdeckung und Grenzen

Geprüft: eingebundene Partner-Landingpage, Onboarding, öffentliche Raid-Hilfe, Verwaltung und Trennung, Freigabenachricht, Rechteprofile, Statusbefehl, Quellenprüfung, Empfängerpool, Kategorie-/Förderauswahl und beide Pausenmechanismen. Positiv: Hilfe benennt fehlenden Raid-Schalter ehrlich; Rundgang erklärt, dass „stille Raids“ Nachrichten unterdrücken, nicht Raids (`OnboardingGuide.tsx:11`).

Keine Anmeldung, Browserprüfung, Datenbankabfrage oder Prüfung der Produktionskonfiguration. Keine Aussage zu ausgeführten Förderraids, betroffenen Konten oder momentan laufendem Build. Historische Komponenten und Tests sind keine Belege aktueller Nutzeroberflächen.
