# Teil 6 — Der AI-Akquise-Agent: Architektur, Regeln, Sequenzen, Einwandbibliothek

> Konzept für den produktspezifischen AI-Agenten, der größere deutschsprachige
> Deadlock-Streamer gewinnt. Gesprächsführung nach Braun/Voss/Taxis/Heinrich (Kapitel 16,
> 10, 14), Systematik nach Blount/Kreuter (Kadenz, KPIs), Plattform-Regeln nach
> Marktanalyse (Kapitel 02 §4).

## 0. Die wichtigste Design-Entscheidung: Mensch sendet, Agent arbeitet

Die Marktanalyse ist hier eindeutig: **Automatisierte, unaufgeforderte Werbe-Whispers/Chat-Nachrichten verstoßen gegen die Twitch-ToS** („unsolicited advertising … solicitation") und riskieren einen unbefristeten Bann — im schlimmsten Fall des Produktions-Bot-Accounts, auf dem Raids und Moderation aller Partner laufen. Dazu kommt das UWG-Risiko elektronischer Direktwerbung ohne Einwilligung (keine Rechtsberatung — vor Start juristisch prüfen lassen).

Daraus folgt die Architektur:

> **Der Agent ist ein Vertriebs-Copilot, kein Spam-Kanon.** Er übernimmt 90 % der Arbeit — Zielauswahl, Recherche, Clip-Produktion, Nachrichtenentwurf, Timing, Wiedervorlage, Antwort-Vorschläge, Messung — aber **jede ausgehende Erstnachricht wird von einem Menschen freigegeben und über konforme Kanäle gesendet** (Discord dort, wo Server-Regeln es erlauben oder Kontakt besteht; E-Mail an öffentliche Business-Adressen; X-DMs; niemals Twitch-Whisper-Massenversand). Volumen ist bewusst klein: Der DACH-Markt hat nur wenige hundert Accounts — Qualität pro Kontakt schlägt Menge (Anti-Blount-Entscheidung, siehe Kapitel 30 §4).

## 1. Pipeline des Agenten (7 Stufen)

```
1 SCOUT      Kandidaten finden: Twitch-API/Tracker — wer streamt Deadlock auf
             Deutsch, Frequenz, Viewer-Band, bereits im Netzwerk? → Dream-100-
             Liste (Brunson): kuratiert, priorisiert, klein.
2 RESEARCH   Pro Kandidat ein Dossier: letzte Streams, Raid-Verhalten (rein/raus),
             Chat-Moderations-Situation, Clip-Präsenz auf TikTok/YT, Discord-
             Mitgliedschaften, gemeinsame Bekannte im Netzwerk (Warm-Path!).
3 GIFT       Das Geschenk produzieren (Hormozi „Big Fast Value"): 2–3 fertige
             Highlight-Clips aus dem letzten VOD des Streamers + sein
             Kanal-Report (Priestley-Scorecard). Kein Pitch im Geschenk.
4 DRAFT      Erstnachricht nach Braun-Architektur entwerfen (Trigger → eine
             Illumination-Frage ODER Geschenk-Angebot → weicher CTA mit
             Autonomie-Klausel), personalisiert mit echten Daten aus Stufe 2.
             Accusation-Audit-Baustein, wenn Kanal Scam-sensibel wirkt.
5 SEND       Mensch prüft, passt an, sendet über den konformen Kanal. Bei
             vorhandenem Warm-Path: stattdessen Intro über den gemeinsamen
             Partner erbitten (Hormozi Warm Outreach — immer erste Wahl).
6 CONVERSE   Antworten: Agent schlägt Antwort im Voss-Modus vor (Label →
             No-Frage/kalibrierte Frage), Mensch sendet. Einwand-Bibliothek §4.
7 TRACK      Jede Interaktion geloggt: Touch → Antwort → Report/Clip angenommen →
             verbunden → aktiviert → zahlend. Wochenreport der Ratios (Blount),
             Kanal-Vergleich, Reaktivierungs-Wiedervorlage nach 60–90 Tagen
             nur mit neuem Anlass (Patch, Feature, Meilenstein).
```

## 2. Die Sequenz (max. 4 Touches, Priestley 7-11-4 light)

Bewusst kürzer als Blounts 7er-Kadenz — kleiner Markt, hohe Sichtbarkeit untereinander:

1. **Touch 1 — Präsenz ohne Bitte:** Follow, echtes Clip-Like, ggf. beiläufige, ehrliche Reaktion im gemeinsamen Discord. Kein DM. (Familiarity/7-11-4.)
2. **Touch 2 — Geschenk-Nachricht** (Kanal je nach Zugang): Braun-Struktur, Kern: „Wir haben testweise 3 Highlights aus deinem Mittwoch-Stream geschnitten — willst du sie haben? Gehören dir, egal ob du mit uns was machst." Optional Ehrlichkeits-Opener nach Friedrichs, wenn der Kontext nach Vorlage aussieht: „Ganz transparent: vorbereitete Anfrage, in 10 Sekunden wegklickbar."
3. **Touch 3 — Wert-Follow-up (nach 7–10 Tagen, nur einmal):** ein neuer, konkreter Insight aus dem Kanal-Report („Dein Zuschauer-Drop liegt fast immer in Minute X — im Netzwerk-Schnitt ist das der Raid-Moment"). Keine Wiederholung der Bitte.
4. **Touch 4 — Magic Email (Voss):** eine Zeile: „Hast du die Idee mit dem Deadlock-Raid-Netzwerk verworfen?" Danach Stopp; Opt-out ist endgültig, Wiedervorlage nur ereignisgetrieben.

**Beispiel-Erstnachrichten (Deutsch, Braun-Stil) und die komplette Voss-Einwandbibliothek („Kenne ich nicht" / „Klingt nach Scam" / „Hab schon StreamElements" / „Keine Zeit" / Ghosting): siehe Kapitel 16, Teil H — dort ausformuliert und direkt übernehmbar.**

## 3. Gesprächsregeln (hart codiert in den Agenten)

1. **Trigger-Pflicht:** Keine Nachricht ohne mindestens einen echten, überprüfbaren Datenpunkt aus dem Kanal des Empfängers. Pseudo-Personalisierung ist schlimmer als keine (Braun).
2. **Eine Frage pro Nachricht, maximal.** Keine Annahme über den Bedarf des Empfängers formulieren (Zone of Resistance).
3. **Autonomie-Klausel immer:** Jede Bitte trägt einen echten Ausstieg („Falls nein — auch gut, weiter viel Erfolg"). Detach from the outcome als Policy.
4. **Transparenz über Preis und Modell in Nachricht 1–2:** „kostenlos; optionale Pläne ab 4,99 €" (Heinrich: frühe Preisnennung entwaffnet Verdacht).
5. **Kein Druck-Vokabular:** keine Dringlichkeit, kein „nur noch heute", keine Follow-up-Vorwürfe („Ich hatte schon zweimal geschrieben…").
6. **Große Streamer = anderes Motiv (Limbeck Motivanalyse):** Ihnen bringt das Raid-Netzwerk als Empfänger wenig — sie sind Geber. Ihr Pitch ist ein anderer: (a) Clips/Analytics als persönlicher Nutzen, (b) Community-Leadership („dein Raid entscheidet, welcher kleine Deutsche als Nächstes wächst"), (c) perspektivisch Netzwerk-Sponsoring-Erlöse (Kapitel 32 P3), (d) ggf. bezahlte/beteiligte Leuchtturm-Partnerschaft. Niemals denselben Text wie für 10-Viewer-Streamer.
7. **Jede Absage freundlich quittieren und taggen** — „Nicht gekauft hat er schon" (Limbeck): der Ist-Zustand ist kein Verlust, und die Beziehung bleibt intakt für den ereignisgetriebenen zweiten Anlauf.
8. **Opt-out ist heilig.** Einmal „nein danke" = keine weitere Ansprache, nur noch passive Sichtbarkeit (Raids, Content).

## 4. Einwand-Bibliothek (Betriebslogik)

Die Bibliothek (Formulierungen in Kapitel 16 H) wird als lebendes System geführt: Jede echte Einwand-Variante aus Outreach, „Frag den Bot" und Discord wird gesammelt, beantwortet und dreifach verwertet (FAQ, Kurzvideo, Skript) — Friedrichs' Content-Maschine mit Voss' Antwortarchitektur. Der Agent lernt daraus: neue Einwände eskaliert er an den Menschen, statt zu improvisieren.

## 5. Der zweite Motor: produktinhärente Akquise (läuft parallel, vollautomatisch UND konform)

Der Outreach-Agent adressiert die Dream 100. Für den Long Tail arbeiten die produktinhärenten Loops, die keine Direktnachricht brauchen:

1. **Raid-Loop:** Hinweis im *eigenen* Kanal des raidenden Partners („Dieser Raid kam aus dem DACH-Deadlock-Netzwerk — kostenlos beitreten"), gelegentliche kuratierte Raids an vielversprechende Nicht-Partner. ToS-konform, da im eigenen Kanal.
2. **Clip-Watermark-Loop:** Free-Clips tragen dezentes Branding (Kapitel 32).
3. **Empfehlungs-Loop (Friedrichs/Kreuter):** Nach messbaren Erfolgsmomenten (erster großer eingehender Raid, Analytics-Meilenstein) fragt der Bot den Partner dezent nach 2–3 befreundeten Streamern — verzahnt mit dem 30-%-Affiliate („Empfehlung in 45 Sekunden, Provision dauerhaft").
4. **Discord-Partnerschaft:** offizielle Verankerung in der Deutschen Deadlock Community (Bot-Kanal, Go-Live-Posts als Community-Service) — die Bürgschaft, die den Scam-Einwand strukturell entkräftet.

## 6. KPIs des Agenten

| Stufe | Kennzahl | Warnsignal |
|---|---|---|
| Scout/Research | Dossiers/Woche, Abdeckung der Dream 100 | Liste veraltet |
| Gift | angenommene Clip-Geschenke / versendete Angebote | < 30 % → Geschenk-Qualität prüfen |
| Erstkontakt | Antwortquote je Kanal (Discord/E-Mail/X) | < 15 % → Trigger-Qualität prüfen |
| Konversion | Antworten → verbunden; verbunden → nach 14 Tagen aktiv | Aktivierung < 50 % → Onboarding-Problem, nicht Outreach-Problem |
| Hygiene | Beschwerden/Spam-Meldungen | > 0 ist ein Incident: Sequenz stoppen, Ursache klären |
| Gesamt | Kosten pro aktiviertem Partner; Anteil Warm-Intros | Outreach teurer als produktinhärente Loops → Budget umschichten |

## 7. Ausbaustufen

- **Stufe 1 (sofort):** Scout/Research/Gift/Draft als interne Tools; Versand und Konversation vollständig menschlich.
- **Stufe 2:** Antwortvorschläge im Voss/Braun-Modus, Einwand-Erkennung, automatische Wiedervorlagen; Versand weiter menschlich.
- **Stufe 3 (nur nach Rechtsprüfung und nur auf Kanälen mit klarer Erlaubnis):** teilautomatisierter Versand an Empfänger mit bestehender Beziehung/Einwilligung (z. B. Discord-Mitglieder des eigenen Servers, Newsletter-Abonnenten). Kalt-Erstkontakt bleibt dauerhaft menschlich freigegeben.
