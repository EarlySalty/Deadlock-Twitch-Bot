# Bauqualität W03

Feste Basis `0ecae1370f1a80d1a101249b5c932663d69be8af`, Workflow `wf_461b0370-4c3`. Neun Bewertungen und neun frische Kritiken sind abgeschlossen und auf Sol geprüft; Einzelbelege in MODELLE-W03-QUALITAET.md. Die folgenden Fassungen übernehmen Korrekturen und Einschränkungen der Kritiker.

Reine statische Bewertung. Keine Tests, Builds, Browser-, Datenbank- oder Netzwerkprüfungen ausgeführt. Die Kritiken beruhen auf gezielter Gegenlektüre, nicht auf einer erneuten Volllektüre des gesamten Pakets. Vorhandener Testcode ist kein grüner Testlauf; negative Testsuchen schließen externe oder anders benannte Tests nicht aus. Empfehlungen sind C, keine Umsetzung und keine Sicherheitsfreigabe.

## DA03: Twitch-, Discord- und Steam-Anmeldung

**3/5, BESTÄTIGT.** Injizierbare Anbieter-Schnittstellen, gemeinsame verschlüsselte Sitzungsspeicherung und atomarer State-Verbrauch sind vorhandene Stärken. Der gemeinsame Callback bündelt trotzdem zahlreiche Anmeldeziele; Discord-Brokertransport, HTTP und eingebettete Darstellung liegen eng zusammen. Dateilängen enthalten erhebliche Testanteile und sind für sich kein Qualitätsmangel.

Belege: `rust/crates/tb-dashboard-api/src/auth/oauth_login.rs:105`, `auth/discord_admin_login.rs:114-196,480,1174`, `handlers/auth_login.rs:199-453`, `auth/session.rs:968-1036`, jeweils unter derselben Crate. Broker- und Cookie-Helfer überschneiden sich mechanisch, haben aber unterschiedliche Timeout-, Mehrfachcookie- und Berechtigungsverträge. Bestehende Konfigurationsübergabe und typisierte Zustände nicht als fehlend darstellen.

Testlage: Substanzielle Browserkontext-, Replay-, Sitzungs- und Berechtigungstests existieren. Einige Callback-Fixtures machen Setupfehler selbst bei gewünschter Test-DB zu None. Gemeinsame Testkonfiguration liegt bereits in `rust/test-support/database.rs:17-35`; vorhandene isolierte PostgreSQL-Unterstützung wiederverwenden. Keine vollständige Aussage zur externen Discord-Brokersemantik.

C-Empfehlungen: Verlässliche Callback-/Discord-Link-Testausführung zuerst verbessern. Bei einem getrennten Umbau Orchestrierung, Transport und Darstellung gezielt trennen und Konfiguration injizieren. Nutzen: kleinere Änderungskontexte und prüfbare Fehlerpfade. Wesentliche Grenzen: verschiedene Anmeldezwecke, bestehende Laufzeiten, generische öffentliche Fehlermeldungen und absichtlicher Ausfallfallback erhalten. Keine vollständige Login-Neuarchitektur.

## DA04: Admin-Konfiguration und Aktionen

**3/5, BESTÄTIGT.** JSON- und Formularadapter nutzen gemeinsame Fachmutationen; der Betriebseditor besitzt typisierte Eingaben und Konfliktbehandlung. Die belegten Schwächen betreffen kopierte Dienstadressauflösung, unterschiedlich präzise Dateilesezustände und veraltete Schutzbeschreibungen.

Belege: `rust/crates/tb-dashboard-api/src/handlers/admin_streamers.rs:25,525,562,708,780`, `admin_form_aliases.rs:18,58,138`, `admin_chat_action.rs:268,317`, `admin_operating_config.rs:43,63,132`, jeweils im Handlerverzeichnis. `admin_config.rs:8`, `admin_promo_mode.rs:9` und `admin_roadmap.rs:10` widersprechen dem tatsächlich vorgeschalteten CSRF-Layer. Die gemeinsame Form- und JSON-Schutzlogik hat unterschiedliche Aufgaben; keine unnötige Sicherheitsdopplung daraus ableiten.

Testlage: Echte Streamer-Mutations- und Bestätigungstests sind vorhanden. Ergänzende Lücken betreffen einzelne Dashboard-Form-/HTTP-Adapter, nicht pauschal die Fachfunktionen oder internen Dienstaktionen. Legal und Roadmap besitzen bereits pfadinjizierte Funktionen, Uplink bereits Transportbausteine.

C-Empfehlungen: Diensttransport auf vorhandenen Bausteinen injizierbarer machen, gezielte Adaptertests ergänzen und Schutztexte nachführen. Dokumentablage und Audit-Quellen nur bei gesondertem Umbau klarer abgrenzen. Nutzen: weniger doppelte Konfigurationslogik und bessere Diagnose. Defaults, Fehlerverträglichkeit, Konfliktverhalten und unterschiedliche Schutzschichten sind dabei bestehende Verträge.

## DA05: System-, Datenbank- und Diagnose-Endpunkte

**3/5, KORRIGIERT bei gleicher Note.** Kleine Diagnosehandler nutzen gemeinsame Adminprüfung und Analytics-Lader. Die SQL-Konsole hat ausdrückliche Ausführungsgrenzen. Dreifache DSN-Fingerprint-Logik und verteilte interne Transportkonfiguration erhöhen dagegen den Pflegeaufwand.

Belege: `rust/crates/tb-dashboard-api/src/handlers/system/database.rs:39-64`, `system/query.rs:113-120,257-320`, `system/health.rs:57-155`, `health_probe.rs:449-496` und `rust/crates/tb-internal-api/src/handlers/healthz.rs:45-86`. Der Self-Explainer bündelt HTTP-Orchestrierung, globale Zustände und Protokollierung, verwendet aber bereits injizierte Knowledge-/Brain- und Generierungsbausteine: `handlers/self_explainer.rs:354-405,514-620,810-920`.

Korrekturen: `tb-config` besitzt bereits InternalApiConfig und Settings::load. Nicht sämtliche DB-Skips sind still; mehrere Diagnosemodule haben einen Pflichtschalter und melden fehlende Voraussetzungen. Readiness besitzt vier Verhaltenstests, davon drei mit Mock-Upstream. Die 59 Inline-Testfälle sind statischer Bestand. Die engeren Lücken für Self-Explainer-HTTP und URL-Regeln bleiben bestehen.

C-Empfehlungen: Den reinen gemeinsamen Fingerprint-Vertrag zentral pflegen und vorhandene Konfiguration verwenden. Nur die verbleibende Handler-Orchestrierung besser isolieren; bestehende Test-DB-Helfer und Transportregeln erhalten. Nutzen: weniger Synchronisierung und gezieltere Tests. DSN-Normalisierung, Hashwerte und der vorhandene 400-Fehlervertrag dürfen dabei nicht beiläufig verändert werden. Exportierte Legacy-Helfer erst nach Verbraucherprüfung entfernen.

## DA06: Abrechnung und Stripe-Webhook

**3/5, BESTÄTIGT.** Gemeinsamer Stripe-Client, reine Preisberechnung, transaktionaler Webhook-Kern und substanzielle Tests sind vorhanden. Einzelne HTTP-Module tragen dennoch viel Fachlogik und SQL. Katalog, Checkout und Synchronisierung vertreten teilweise widersprüchliche Konfigurationsannahmen.

Belege: `rust/crates/tb-analytics/src/stripe/client.rs:19-87,122-175`, `billing/catalog.rs:166-176,248-290,536-574`; `rust/crates/tb-dashboard-api/src/handlers/billing_page.rs:180-311,879-901`, `billing_profile.rs:25,225-260`, `billing_stripe_sync.rs:58-318,335-359`, `billing_webhook.rs:210-258`. Leere Default-ID-Listen stehen Aussagen über vollständige Defaults und konstantem price_map_ready gegenüber. Kein tatsächlicher Live-Konfigurationsfehler wird daraus behauptet.

Testlage: 72 explizite Testfälle statisch erfasst, darunter aussagekräftige Checkout-, Trial-, Kündigungs- und Webhook-Prüfungen. Drei Sync-Fehlertests erreichen ihre beschriebenen Retrieve-Fehlerzweige nach dem Katalogumbau nicht. Das ist hier eine C-Testempfehlung, keine zusätzliche A/B-Freigabe.

C-Empfehlungen: Konfigurationssicht und Readiness auf vorhandener ID-Auflösung abstimmen; konkrete Testzweige mit Request-Erwartungen belegen. Kleine Servicegrenzen und identische Customer-Reference-Helfer prüfen. Nutzen: nachvollziehbare Konfiguration und aussagekräftigere Tests. Verschiedene Cookie-Auswahlregeln, Header-CSRF und Webhook-Signaturen nicht zusammenlegen; keine neue Rechnungsengine oder Secret-Store-Schreibfunktion.

## DA07: Affiliate und öffentliche Partnerprofile

**3/5, KORRIGIERT bei gleicher Note.** Öffentliche Profile besitzen gemeinsame Aktivitätsprüfung, atomare Revisionskontrolle und erneute Sichtbarkeitsprüfung vor dem Rendern. Provisionsbuchung ist dauerhaft und wiederholbar angelegt. Affiliate-HTTP bündelt dagegen verbliebene Konto-SQL-Abfragen, Claim-Transaktionen und Orchestrierung; einzelne Fehlerursachen und Portalzustände werden stark reduziert.

Belege: `rust/crates/tb-dashboard-api/src/handlers/partner_profiles.rs:525-540,659-675,834-842`, `affiliate.rs:768-831,903-1043,1359-1371`, `affiliate_portal.rs:73-165`; `rust/crates/tb-analytics/src/affiliate_commission.rs:232-426,539-566`. PII-Speicherung und Klartextmigration sind bereits in Analytics umgesetzt und werden nicht im Handler neu gebaut.

Korrekturen: Affiliate nutzt bereits database_url und für 14 Tests TestPostgres, einschließlich nicht still übersprungener Claim-Race-Tests. Helix besitzt austauschbare Test-Endpunkte, der Profilcache erreichbare Testfelder und snapshot einen expliziten fachlichen Zeitpunkt. Zusätzliche Client-/Clock-Injection ist deshalb keine belegte Voraussetzung für Cachetests. Die lange HTML-Funktion hat bereits Rahmen-, Escaping- und Terminhelfer.

C-Empfehlungen: Cache-Komposition mit vorhandenen Testzugängen für Ablauf, erzwungene Aktualisierung, partielle Fehler und Wrapper-Timeout prüfen. Verbliebene Konto-/Claim-Persistenz nur bei separatem Auftrag enger kapseln, interne Fehlerursachen erhalten und Rendering schrittweise lesbarer machen. Nutzen: kleinere Änderungskontexte und bessere Fehlerzuordnung. Hohe Sorgfalt gilt Identitäten, Sperren, Transaktionen, persistierten Provisionszuständen und festem Profilursprung. Keine neue Eigentums- oder Auszahlungspolitik aus dieser Qualitätsbewertung.

## DA15: öffentliche Formulare und Clip-Wettbewerb

**4/5, BESTÄTIGT.** Monatssperre, Datenbankzeitprüfung und gemeinsame Transaktion für Einreichung und Punkte-Outbox sind ausdrücklich umgesetzt. Feedback besitzt personenbezogene Idempotenz, Revisionskontrolle und konsistente Detailsnapshots. Das trägt die höhere Note; vollständige Sicherheits- oder Laufzeitabnahme folgt daraus nicht.

Belege: `rust/crates/tb-dashboard-api/src/handlers/clip_contest/contest.rs:17-79,109-175,421-477,605-663`, `handlers/feedback/mod.rs:62-68,259-288,352-383,453-496`, `handlers/onboarding.rs:86-143,207-225`. Grenzen bleiben die gemischte Wettbewerbs-HTTP-/SQL-Orchestrierung, verlorene Fehlerursachen und globale Integrationsabhängigkeiten.

Testlage: Vorhandene PostgreSQL-Fälle prüfen konkurrierende Stimmen, wiederholten Monatsabschluss, Outbox und Sitzungstrennung. Feedback hat bereits direkte konkurrierende Handler- und Fremdzugriffstests. Fehlende Vollkettennachweise betreffen eng den Monatswettbewerb und dessen Integrationen. Bestehende Helix-Testendpunkte nutzen; das Admin-OAuth-Muster hat andere Rechteverträge als Wettbewerbsteilnehmer.

C-Empfehlungen: Gezielte Wettbewerbs-Handler-/Routertests und datensparsame Fehlerdiagnose ergänzen. Transaktionslogik nur im gesonderten Umbau von HTTP lösen. Nutzen: gut belegte Datenregeln auch über die echte Handlerkette absichern. Monatssperre, Outbox, Antwortverträge und die Trennung zum Wochenwettbewerb erhalten. Feste Communitywerte sind ohne zweites Deployment kein Konfigurationsdefekt; die beiden Sitzungslaufzeiten stimmen derzeit überein.

## DA17: Plattform-OAuth und Overlays

**3/5, BESTÄTIGT.** Anbieteradapter, verschlüsselter Speicher, Verbindungslifecycle und Ausgabe sind sinnvoll getrennt. Anbieter-, Relay- und Zeitparameter bilden echte Testnähte. Twitch verwendet bestehende RaidAuth-/Refresh-Bausteine; andere Anbieter ihren PlatformConnectionStore, was unterschiedliche Verträge abbildet.

Belege: `rust/crates/tb-dashboard-api/src/handlers/plattform_oauth.rs:55,69`, `plattform_connect.rs:392-451,1511-1595`, `platform_token.rs:274-305,459-460`, `platform_store.rs:128`. Caster bündelt dagegen Szenen, Kamera-Freigaben, globalen Hub, WebSocket-Lifecycle und externe Kontextanreicherung: `caster_overlay.rs:225,399,523,610,880,968`. Verdichtete Clientlogik auf langen Einzelzeilen erschwert gezielte Zustandsänderungen.

Testlage: 86 lokale Testfunktionen, darunter substanzielle Refresh-, Verschlüsselungs- und Disconnect-Regressionen. Die sieben lokalen Caster-Tests erreichen die kritischen Lifecycle- und Integrationspfade nicht. Zusätzliche gezielte Suchen fanden keine entsprechenden Verhaltenstests; das ist keine gemessene Coverage und kein Nachweis völliger Testlosigkeit.

C-Empfehlungen: Caster-Vertragstests für Revision, Freigabe/Widerruf, Publisher-Ersatz und Signaling priorisieren. Zustandsverantwortung und Konfiguration bei separatem Umbau instanzbezogener gestalten, vorhandene Testnähte behalten. Nutzen: isolierbare Lifecycle-Prüfungen. Relay-Regeln nicht auf externe OAuth-/Asset-Dienste übertragen; Asset-Auslagerung, Sprachwechsel und Frontend-Buildumbau sind gesonderte Entscheidungen.

## MO01: EventSub-Empfang, Inbox und Abonnements

**3/5, BESTÄTIGT.** Empfang, Dispatcher, dauerhafte Inbox und Adapter sind getrennt. Signatur- und Zeitprüfung erfolgen vor Verarbeitung; fehlende Bereitschaft wird als 503 ausgedrückt. Konditionale Guards, SKIP-LOCKED-Leasing und transaktionale Dead-Letter-Übergänge begründen ein tragfähiges At-least-once-Fundament, keine pauschale Exactly-once-Garantie.

Belege: `rust/crates/tb-monitoring/src/webhook_receiver.rs:225,239,297`, `guard.rs:122`, `inbox_store/mod.rs:119,214,300`, `subscriptions.rs:523,1034,1332,1669,2042`. Der Subscription-Manager bündelt viele Zuständigkeiten; mehrere Mutexes allein sind kein Defekt. Chat-Fallback-Konverter in `dispatch.rs:417,508,524` und `rust/bin/tb-bot/src/chat_wiring.rs:2162,2263,2282` überlappen, unterscheiden sich aber in Konvertierungsregeln. Doppelte produktive Ausführung ist nicht belegt.

Testlage: Umfangreicher Helper-, Dispatcher-, Signatur- und Inbox-Testbestand. Gezielte Suchen fanden keine zusammengesetzten nativen Receiver-Vertragstests, gleichzeitigen Lease-Worker-Tests oder Abbruchtests zwischen Guard-Claim und Fachwirkung. Einzelne Hooks mit Rückgabe () begrenzen die Aussage über Fachfehler und Retryfähigkeit.

C-Empfehlungen: Diese zusammengesetzten Tests ergänzen und Garantiegrenzen dokumentieren. Vorhandene Katalog-, Capacity- und Transportports beim gezielten Zuschnitt der Subscription-Verantwortung nutzen. Nutzen: klarere Wiederholungs- und Abbruchverträge. Neue Retryentscheidungen, ein kanonischer Chat-Konverter und zusätzliche operative Konfiguration benötigen jeweils einen getrennten Verhaltensauftrag.

## IA01: interner Router, Schutz und Lebenszyklus

**3/5, KORRIGIERT bei gleicher Note.** Injizierte Ports und zentraler interner Schutz ermöglichen aussagekräftige Tests. Die internen Origin-/Peer-/Schlüsselregeln sind bewusst strenger als generische Middleware. Zwei ausgeführte Wege pflegen dennoch ähnliche Persistenz für das Trennen von Partnerschaften und verlangen laut Kommentaren synchronisierte Änderungen.

Belege: `rust/crates/tb-internal-api/src/lib.rs:57,389`, `security.rs:137,172`, `streamer_lifecycle.rs:201,256,407,515,826`; `rust/crates/tb-analytics/src/streamers_crud.rs:398,488`; `rust/crates/tb-dashboard-api/src/handlers/admin_streamers.rs:526`. HTTP und MCP nutzen bereits disconnect_bot_handler_inner gemeinsam. Die Empfehlung betrifft die zwei Persistenzimplementierungen, nicht eine erneute gemeinsame Orchestrierung.

Korrekturen: `tb-config/src/runtime.rs:10,27` stellt bereits einen validierten unveränderlichen Startsnapshot bereit. Das Problem ist impliziter Zugriff neben separaten ENV-Abfragen, keine fehlende Konfigurationsarchitektur. Produktive Routertests existieren auch für Community-Punkte; Cliptests verwenden einen konkreten Submitter mit austauschbaren Lookup-/Brokerports. Best-effort-Verhalten ist teilweise ausdrücklich dokumentiert, rohe SQLx-Fehler sind nicht pauschal ein Strukturmangel.

C-Empfehlungen: Gemeinsame Partner-Persistenz auf bestehender Fach-/Speichergrenze zusammenführen, benannte Routerabhängigkeiten einführen und den vorhandenen Snapshot expliziter übergeben. Idempotenztests für TTL, Verdrängung, wartende Requests und Owner-Abbruch gezielt erweitern. Nutzen: weniger doppelte Zustandslogik und klarere Komposition. Berechtigungen, externe Wirkungen, Abmelde-/Pausenmarker und Kompatibilitätsantworten erhalten; optionale Proxys und öffentliche Helfer erst nach Verbraucher- und Betriebsprüfung entfernen.

## Einordnung

Mit R09, DA01 und DA02 liegen zwölf gegengeprüfte Bereichsbewertungen vor. Für die übrigen 96 fehlen Bewertungen und Kritik. Eine Gesamtnote und die fünf bereichsübergreifend wichtigsten Umbauten werden erst nach weiterer Abdeckung begründet. Keiner dieser C-Vorschläge gehört allein wegen seiner Qualitätsbewertung in eine laufende Fixrunde.
