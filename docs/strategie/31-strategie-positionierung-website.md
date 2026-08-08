# Teil 4 — Strategie: Positionierung & Website-Umbau

> Anwendung der Synthese (Kapitel 30) auf Positionierung, Botschaft und die konkrete
> Website. Basiert auf Dunford (Kategorie), Miller (BrandScript, Kapitel 18 H.2),
> Marktanalyse (Kapitel 02) und dem Ist-Stand der Website (docs/funktionsweise/website-und-onboarding.md).

## 1. Die Positionierungs-Entscheidung

**Alt (implizit):** „Twitch-Bot für die Deutsche Deadlock Community" → erzwingt den Vergleich mit Nightbot/StreamElements, wo alles gratis ist und wir die kürzere Feature-Liste haben.

**Neu (Beschluss-Vorlage):**

> **Kategorie:** Das Wachstums-Netzwerk für deutschsprachige Deadlock-Streamer.
> **Best-Fit-Kunde:** Streamer mit 0–100 gleichzeitigen Zuschauern, 3–5 Streams/Woche, Affiliate-/Partner-Ambition, kein Mod-/Editor-Team.
> **Hauptgegner:** „Nichts tun" (versickernde Zuschauer), nicht andere Bots.
> **Unique Attribute mit Burggraben:** das automatische, kuratierte, spielspezifische Raid-Netzwerk — laut Marktanalyse ohne direktes Konkurrenzprodukt.
> **Beweisführung:** Open Metrics (Streamer im Netzwerk, vermittelte Raid-Zuschauer, Bans, Clips) statt Superlativen.

Konsequenzen dieser einen Entscheidung:
- „Bot" wird sprachlich zum Liefermechanismus degradiert („…läuft als Bot, in 2 Minuten verbunden").
- Der Preis wird erklärbar: Ein Bot darf nichts kosten; ein Netzwerk mit bevorzugter Platzierung, Coaching und Content-Produktion darf 5–10 € kosten (Kuchen/Muffin-Logik).
- Nightbot/StreamElements werden zu **Komplementen**, nicht Konkurrenten: „Behalte deinen Bot — wir sind das Netzwerk dahinter." Das entschärft den größten Wechsel-Einwand komplett.
- Das Raid-Netzwerk (nicht die Moderation) steht in jeder Kommunikation an erster Stelle.

## 2. Sprachregelung (aus dem BrandScript, Kapitel 18 H.2)

- **One-Liner** (Standardantwort auf „Was ist das?", überall identisch): „Die meisten kleinen Deadlock-Streamer streamen ins Leere — ihre Zuschauer verschwinden nach jedem Stream. Unser kostenloses Wachstums-Netzwerk übergibt deine Zuschauer beim Stream-Ende per Auto-Raid an andere deutsche Deadlock-Streamer, schützt deinen Chat und schneidet deine Highlights automatisch."
- **Villain-Vokabular:** „die Leere", „ins Leere streamen", „versickern" — konsistent in Hero, FAQ, Clips, Outreach.
- **Verbotsliste** (aus der Gegenpol-Analyse): keine Superlative („#1", „bester"), keine Countdown-Verknappung, keine Angst-Rhetorik, keine unbelegten Zahlen.
- **Zahlenregel (Levels):** Jede Behauptung trägt eine live gemessene Zahl oder fliegt raus.

## 3. Website-Umbau — Seite für Seite

### 3.1 Startseite (Hero nach Grunt-Test)

Ist: „Kein Stream endet im Leeren." — stark als Emotion, fällt allein durch den Grunt Test (sagt nicht was/für wen/wie). Soll:

```
H1:  Kein Stream endet im Leeren.
Sub: Das kostenlose Wachstums-Netzwerk für deutschsprachige Deadlock-Streamer:
     Auto-Raids beim Stream-Ende, Spam-Schutz, KI-Coaching und automatische
     TikTok-Clips.
CTA (direkt, oben rechts + wiederholt):  [ Jetzt kostenlos verbinden ]
CTA (transitional):                      [ Kostenlosen Kanal-Report holen ]
Beweiszeile (live aus dem System):       {N} Streamer im Netzwerk ·
     {N} Raid-Zuschauer vermittelt · {N} Spam-Bots entfernt (30 Tage)
```

Sektionen danach (Miller-Wireframe): Stakes (die Leere, dosiert) → Plan in 3 Schritten (Verbinden → Netzwerk aktivieren → Wachsen lassen) → differenzierter Wert in 4 Karten (Raids · Schutz · Coaching · Clips, in dieser Reihenfolge) → Guide-Sektion („gebaut von Deadlock-Streamern", Gesichter/Namen) → Testimonials kleiner Streamer mit Vorher/Nachher-Zahlen → Preisvorschau (3 Optionen) → FAQ-Auszug mit den Top-Einwänden → Footer.

Bereits vorhandene Stärken, die bleiben und aufgewertet werden: **Live-Ban-Feed** (einziger Bot mit öffentlichem Schutz-Beweis — als „Open Metrics"-Block ausbauen: zusätzlich Raid-Zahlen und Clip-Zahlen), **Demo-Dashboard**, **„Frag den Bot"** (um die Einwand-Bibliothek aus Kapitel 16 erweitern, damit er auf „Ist das Scam?" nicht nur ehrlich, sondern Voss-artig antwortet: erst Label, dann Beleg).

### 3.2 Neuer Lead-Magnet: der Deadlock-Streamer-Report (Priestley-Scorecard × Hormozi-Diagnose)

Öffentliche Seite: Twitch-Kanalname eingeben → automatischer Report aus ohnehin vorhandenen Analytics-Fähigkeiten: Streamzeiten vs. Kategorie-Peaks, Raid-Bilanz (rein/raus), Zuschauer-Drop-Punkte, Clip-Potenzial der letzten Streams + **Netzwerk-Benchmark** („Dein Wachstums-Score: 62/100 — Streamer im Netzwerk mit ähnlicher Größe wachsen im Schnitt X %"). Der Report löst ein Teilproblem komplett (Transparenz) und legt das nächste frei (Coaching/Netzwerk) — Hormozis Lead-Magnet-Bauart. Er ist zugleich der legitime Gesprächsöffner des Outreach (Kapitel 33) und der beste Discord-Share-Inhalt.

### 3.3 Preisseite

Umbau nach Kapitel 32 (drei Optionen statt acht Pläne, Anker sichtbar, Jahresplan als Default, „Warum kostet das was?"-Absatz mit ehrlicher Begründung: Serverkosten, Clip-Rendering, Weiterentwicklung — Limbecks Preiswürde statt Entschuldigung).

### 3.4 Vertrauens-Seite „Was der Bot darf und warum"

Sery_Bot-Vorbild (Marktanalyse §6): jede OAuth-Berechtigung einzeln erklärt, Scope-Staffelung (Raid-Scope erst bei Feature-Aktivierung), Ein-Klick-Entfernung, Datenlöschung, Impressum/Personen. Verlinkt aus jeder Stelle, an der Berechtigungen angefragt werden, und aus der Scam-Einwand-Antwort. Zusätzlich prüfen: Twitch-Chat-Bot-Badge aktivieren (kostenlos, sofort, seriös).

### 3.5 Affiliate-Seite

Neu rahmen als „Werde Netzwerk-Partner": Headline-Vergleich fair und belegt („30 % Lifetime — Branchenüblich sind 5–10 %", Beleg Eklipse), plus fertiges Material-Paket (Panel-Grafiken, Clip-Vorlagen, Textbausteine, persönlicher Link) — Kreuters Affiliate-Ausstattung ohne Kreuters Ton.

## 4. Content-Strategie (der Sog-Kanal)

Formate, priorisiert nach Aufwand/Wirkung, alle aus dem BrandScript gespeist:

1. **Einwand-Shorts (Friedrichs-Muster, ruhig ausgeführt):** „‚IST DAS SCAM?' — fair. Schauen wir rein." 30–60 s, Bildschirmaufnahme statt Talking-Head-Hype. Jeder Top-Einwand ein Video; identischer Text in FAQ und Outreach.
2. **Netzwerk-Beweis-Clips (Hook-Story-Offer, Brunson-Struktur):** Hook = echte Zahl („Dieser 14-Viewer-Streamer bekam letzte Woche 3 Raids"), Story = der Streamer, Offer = „kostenlos beitreten". Nur mit Einverständnis des Streamers — dann ist es zugleich dessen Promo (Win-Win, teilt sich selbst).
3. **Ship-Posts (Levels):** Jedes Feature-Update als kurzer Discord-/X-Post mit Screenshot und Zahl. Wochen-Takt.
4. **Monatlicher Transparenz-Post:** Netzwerk-Metriken, Pricing-Experimente, was schiefging. Baut genau die Vertrauensschicht, die größere Streamer vor dem Beitritt prüfen.
5. **Highlight-Clips der Partner mit dezentem Watermark** — der StreamLadder-Loop: jeder exportierte Clip trägt die Adresse; Discovery ohne Ad-Budget.

Kanal-Priorität (Walling: ein Kanal bis zur Sättigung): **1. die Deadlock-DACH-Discords + eigener Discord, 2. Twitch selbst (Raids/Panels/Badge), 3. TikTok/Shorts.** Kein Paid vor nachgewiesener organischer Sättigung.

## 5. Messgrößen für diesen Teil

- Besucher → „Verbinden"-Klick → abgeschlossener OAuth-Flow (Funnel-Konversion je Schritt).
- Grunt-Test-Proxy: Absprungrate/Verweildauer Hero; Anteil Besucher, die den Report ziehen.
- Report-Konversion: Reports erstellt → Anmeldungen ≤ 7 Tage.
- Anteil Anmeldungen mit Herkunft „Clip-Watermark", „Raid", „Discord", „Affiliate" (UTM/Referral sauber trennen).
