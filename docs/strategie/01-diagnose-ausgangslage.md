# Teil 1 — Diagnose der Ausgangslage

> Teil der Vergleichsrecherche „Vertriebs- und Marketingstrategie Deadlock-Twitch-Bot".
> Übersicht und Navigation: [00-ueberblick.md](00-ueberblick.md)

## 1. Unternehmenskontext (ausgefüllt aus dem Ist-Stand des Produkts)

Die Platzhalter aus dem Rechercheauftrag, gefüllt mit dem, was Produkt-Doku, Website
und Billing heute tatsächlich hergeben:

| Feld | Ist-Stand |
|------|-----------|
| **Produkt** | Twitch-Bot + Partner-Netzwerk für deutschsprachige Deadlock-Streamer: Auto-Raid-Netzwerk bei Stream-Ende, Auto-Moderation/Spam-Bot-Schutz, netzwerkweite Bannliste, Go-Live-Discord-Posts, Analytics-Dashboard mit KI-Coaching, automatische Highlight-Clips (TikTok/Instagram/YouTube), KI-Titelgenerator, Chat-KI (Shadow-Modus) |
| **Produktkategorie** | Heute kommuniziert als „Twitch-Bot" — faktisch eher „Wachstums- und Schutz-Netzwerk für eine Spiele-Nische" (Positionierungsfrage, siehe Teil 3) |
| **Zielkunden** | Kleine und mittlere deutschsprachige Deadlock-Streamer; künftig zusätzlich größere Streamer |
| **B2B oder B2C** | Hybrid: formal B2C-nah (Einzelpersonen), verhaltensmäßig B2B (Streamer entscheiden wie Solo-Unternehmer über ein Werkzeug für ihr „Geschäft") |
| **Region und Sprache** | DACH, Deutsch |
| **Preis** | Freemium; Einzelpläne je 1,99 € netto/Monat (Werbefrei, Raid Boost, Analytics), Zweier-Bundles 3,49 €, „Alles drin" 4,99 €; monatlich/jährlich; kostenlose Testphase |
| **Verkaufsprozess** | Vollständig Self-Serve: Twitch-Login < 1 Minute → Dashboard → gesperrte Vorschau-Karten als Upgrade-Hinweis. Kein menschlicher Vertrieb. Zusätzlich Affiliate-/„Vertriebler"-Programm mit 30 % Lifetime-Provision |
| **Marketingkanäle** | Eigene Website (Hero: „Kein Stream endet im Leeren.", Live-Ban-Feed, Demo-Dashboard, „Frag den Bot"-Box, FAQ), Discord der Deutschen Deadlock Community, Bot-eigene Chat-Promos in Partner-Kanälen, Social-Media-Uploads der Clips |
| **Verkaufsdauer** | Anmeldung Minuten; Free→Paid-Dauer unbekannt (keine Zahlen vorliegend) |
| **Erklärungsbedürftigkeit** | Mittel: „Bot" versteht jeder Streamer, das Raid-Netzwerk-Prinzip und der Unterschied zu Nightbot/StreamElements müssen erklärt werden |
| **Wettbewerber/Alternativen** | Generische Bots (Nightbot, StreamElements, Moobot, Fossabot), Clip-Tools (z. B. Eklipse), Analytics-Seiten (SullyGnome u. a.), manuelle Raids/Discord-Absprachen, **Nichtstun** |
| **Bisherige Probleme** | Kleine Nische (Deadlock-Szene DACH), Vertrauenshürde („Ist das Scam?" steht wörtlich als Beispiel-Frage in der eigenen Onboarding-Doku), Micro-Preise mit unklarer Konversion, kein aktiver Outbound |
| **Stärken/Beweise** | Live-Ban-Feed mit echten Kennzahlen, öffentliches Demo-Dashboard, „Frag den Bot" (radikale Transparenz), kostenlose Vollmoderation, Community-Anbindung (DDC), Leitprinzip „ruhig statt Hype, keine Viewer-Versprechen" |
| **Gewünschte Markenwirkung** | Kompetent, ehrlich, transparent, community-nah — bewusst nicht marktschreierisch |

## 2. Bekannte Fakten vs. Annahmen

**Bekannte Fakten** (aus Repo/Doku belegbar):

1. Die kostenlose Basis ist vollwertig (Raids + komplette Moderation) und bleibt es laut Produktprinzip dauerhaft.
2. Bezahlt werden heute genau vier Feature-Hebel: Werbefreiheit, Raid-Boost, Analytics, Lurker-Steuer-Erinnerung — Maximalerlös 4,99 € netto/Monat pro Streamer.
3. Der Verkaufsprozess ist zu 100 % Self-Serve; das einzige „Vertriebsteam" ist das Affiliate-Programm (30 %).
4. Die Marke ist auf Ehrlichkeit/Transparenz gebaut (Ban-Feed, Frag-den-Bot, „keine Viewer-Versprechen"). Das ist ein echtes, schwer kopierbares Asset.
5. Die eigene Doku benennt Deadlock als „kleine Szene" — der adressierbare Markt ist eng.
6. Es existieren bereits Vertriebs-Infrastrukturstücke: Affiliate-Portal, Partner-Recruiting-Feature, Rechnungswesen, Trial-Mechanik.

**Annahmen** (plausibel, aber nicht belegt — müssen mit Daten geprüft werden):

- A1: Die Free→Paid-Konversion ist niedrig (typisch für Freemium-Creator-Tools: niedriger einstelliger Prozentbereich).
- A2: Der Engpass ist nicht der Abschluss, sondern dass zu wenige relevante Streamer überhaupt vom Bot wissen (Reichweite/Leadgen).
- A3: Größere Streamer kommen nicht von selbst, weil der soziale Beweis fehlt („Wer von den Großen nutzt das?").
- A4: 1,99 € ist unter der Schmerzgrenze der Zielgruppe — der Preis ist wahrscheinlich nicht der Blocker, wohl aber die wahrgenommene Notwendigkeit („nice to have").
- A5: Das stärkste unerschlossene Monetarisierungs-Asset ist der Highlight-Clipper (bei Wettbewerbern ein eigenständiges bezahltes Produkt im zweistelligen Dollar-Bereich), der heute gar nicht als bezahlter Plan auftaucht.

## 3. Engpassanalyse entlang der Kette

Bewertung jeder Stufe: **Engpass?** (hoch/mittel/niedrig) + Begründung.

| Stufe | Engpass | Begründung |
|-------|---------|------------|
| Produkt | niedrig | Funktionsumfang ist für die Nische stark und differenziert (spielspezifisches Raid-Netzwerk hat kein generischer Bot). |
| Zielgruppenauswahl | **hoch (strukturell)** | Deadlock-DACH ist klein; die Nische ist zugleich der Burggraben. Risiko und Chance hängen an Valves Spiel. |
| Positionierung | **hoch** | „Twitch-Bot" ruft den Vergleich mit Nightbot & Co. ab (dort ist alles gratis). Als „Wachstums-Netzwerk für Deadlock-Streamer" gäbe es keine direkte Vergleichsgröße. |
| Nutzenversprechen | mittel | „Kein Stream endet im Leeren" ist stark für Raids, trägt aber die anderen Säulen (Clips, Analytics, Schutz) nicht mit. |
| Angebot | **hoch** | Nur 3 Zahl-Hebel, Maximal-ARPU 4,99 €; Clipper/Titelgenerator/Chat-KI sind unbepreist. Die Angebots­architektur schöpft den geschaffenen Wert nicht ab. |
| Vertrauen | mittel–hoch | OAuth-Zugriff + unbekannte Marke = „Scam?"-Reflex. Gute Gegenmittel existieren schon (Ban-Feed, Demo, Frag-den-Bot), aber es fehlen Gesichter/Testimonials bekannter Streamer. |
| Reichweite | **hoch** | Kein systematischer Kanal außerhalb der eigenen Community sichtbar; kein Outbound. |
| Leadgenerierung | **hoch** | Kein Lead-Magnet außer dem Produkt selbst; Affiliate-Programm vorhanden, aber Wirkung unbelegt. |
| Gesprächseröffnung | mittel | Wird erst relevant, wenn Outbound (AI-Agent) startet — dann entscheidend, s. Teil 5. |
| Bedarfsermittlung | niedrig | Self-Serve; das Dashboard zeigt Bedarf (gesperrte Karten) automatisch. |
| Angebotspräsentation | mittel | Gesperrte Vorschau-Karten sind gut; es fehlt die wertbasierte Erzählung (was ist ein Raid wert? was ist ein Clip wert?). |
| Einwandbehandlung | mittel | „Frag den Bot" beantwortet ehrlich, aber verkauft bewusst nicht — Einwände wie „hab schon StreamElements" brauchen eine bessere Antwortarchitektur. |
| Follow-up | mittel | Trial-Ende ohne dokumentierte Reaktivierungs-Sequenz. |
| Abschluss | niedrig | Bei 1,99–4,99 € ist der Abschluss kein rhetorisches Problem, sondern eine Relevanz- und Timing-Frage. |

## 4. Priorisierte Engpasshypothese

1. **Reichweite & Leadgenerierung** — Es wissen schlicht zu wenige der (wenigen) relevanten Streamer vom Bot. Alles andere skaliert erst danach. *(Annahme A2 — mit Traffic-/Anmeldezahlen prüfen.)*
2. **Angebotsarchitektur & Pricing** — Der wertvollste Output (Clips, Coaching, Netzwerk-Boost) ist nicht oder zu billig bepreist; ARPU-Deckel 4,99 €. Hier liegt der schnellste Umsatz-Hebel bei bestehenden Nutzern.
3. **Positionierung** — Kategorie „Bot" erzwingt den Gratis-Vergleich; Kategorie „Netzwerk/Wachstumssystem" rechtfertigt Geld. Muss vor jeder Outbound-Welle sauber stehen.
4. **Vertrauen bei größeren Streamern** — Ohne sichtbare Referenz-Streamer keine Großen; ohne Große schwacher Social Proof für die Kleinen (Henne-Ei). Ein „Leuchtturm-Programm" ist Teil der Lösung.
5. **Strukturelles Marktrisiko Deadlock** — nicht behebbar, nur managebar: Early-Mover-Wette halten, aber Architektur spielagnostisch denken (das Raid-Netzwerk-Prinzip funktioniert für jede Nischen-Game-Community).

**Ausdrücklich kein Engpass:** Produktqualität, Onboarding-Reibung, Abschlusstechnik.

## 5. Fehlende Informationen für eine sichere Diagnose

Diese Zahlen sollten erhoben werden, bevor größere Wetten platziert werden:

1. Anzahl registrierter Partner gesamt; davon aktiv (letzte 30 Tage live).
2. Free→Trial→Paid-Konversion und Trial-Endverhalten; Churn der Bezahlpläne.
3. Verteilung der gewählten Pläne (wird „Alles drin" gewählt? Welcher Einzelplan führt?).
4. Website-Funnel: Besucher → „Kanal verbinden"-Klicks → abgeschlossene OAuth-Flows.
5. Wirkung des Affiliate-Programms: aktive Affiliates, geworbene Streamer, Provisionsvolumen.
6. Größe der Grundgesamtheit: Wie viele deutschsprachige Kanäle streamen Deadlock ≥ 1×/Woche? (extern messbar über Twitch-Tracker-Dienste)
7. Meistgestellte Fragen in „Frag den Bot" (kostenlose Einwand-Forschung, liegt bereits im System).
8. Raid-Wirkungsdaten: durchschnittlich übergebene Zuschauer pro Auto-Raid — das ist der wichtigste beweisbare Wert des Produkts und heute nirgends als Verkaufsargument beziffert.
