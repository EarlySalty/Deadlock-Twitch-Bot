# Teil 5 — Strategie: Pricing-Neuaufbau, Upsell-Features, Angebotsarchitektur

> Anwendung von Hormozi (Value Equation, Offer-Stack), Brunson (Value Ladder), Walling
> (Churn/Jahrespläne), Priestley (ehrliche Knappheit) und den Pricing-Benchmarks der
> Marktanalyse (Kapitel 02) auf unser Billing. Ist-Stand: 8 Pläne, 1,99–4,99 € netto,
> ARPU-Deckel 4,99 €, Clipper/Titelgenerator unbepreist.

## 1. Diagnose des heutigen Pricings

1. **Zu viele Pläne, zu wenig Unterschied:** 8 Auswahlmöglichkeiten zwischen 1,99 und 4,99 € erzeugen Entscheidungsarbeit (Millers „confuse → lose") für maximal 3 € Spanne.
2. **Der wertvollste Output ist gratis:** Clip-Automation kostet am Markt allein 15–25 $/Monat (Eklipse 24,99 $, Opus 15–29 $) — bei uns ist sie in keinem Plan bepreist.
3. **Kein Preisanker nach oben:** Die teuerste Option (4,99 €) ist zugleich der Deckel; nichts lässt sie günstig wirken (Brunson: die oberste Stufe fehlt).
4. **Kein Jahresplan:** Bei Micro-Abos entscheidet Churn über alles (Walling); 35–50 % Jahresrabatt ist Marktstandard.
5. **Richtig ist heute schon:** Freemium ist gerechtfertigt (jeder Gratis-Streamer vergrößert das Raid-Netzwerk = Netzwerkeffekt, Wallings Freemium-Test bestanden); Moderation bleibt für immer gratis (Nullpreis-Markt, Kapitel 02); 4,99 € liegt exakt auf dem Twitch-Sub-Anker.

## 2. Neue Plan-Architektur (Vorschlag)

Drei sichtbare Stufen statt acht Pläne; die alten Einzelpläne bleiben als Grandfathering/Deep-Link bestehen, verschwinden aber aus der Standard-Preisseite.

| | **Netzwerk Free** | **Netzwerk Plus** ⭐ empfohlen | **Creator Pro** (neu) |
|---|---|---|---|
| Preis | 0 € | **4,99 €/Monat** · 39,99 €/Jahr (−33 %) | **9,99 €/Monat** · 79,99 €/Jahr |
| Anker-Framing | „vollwertig, für immer" | „der Preis eines Twitch-Subs" | „günstiger als jedes Clip-Tool allein" |
| Auto-Raid-Netzwerk + volle Moderation + Go-Live-Posts | ✔ | ✔ | ✔ |
| Werbefreier Chat | – | ✔ | ✔ |
| Raid Boost (bevorzugte Platzierung) | – | ✔ | ✔ |
| Volles Analytics + KI-Coaching-Wochenreport | – | ✔ | ✔ |
| Lurker-Erinnerung, Custom-Bot-Name | – | ✔ | ✔ |
| **Auto-Clips** | 3 Clips/Monat mit Watermark (= Produkt-Demo + Wachstums-Loop) | 10 Clips/Monat, ohne Watermark | **Unbegrenzt + Auto-Posting-Scheduler (TikTok/IG/YT) + Formate/Untertitel** |
| Priorität bei Support/Features | – | – | ✔ |

Begründungen:
- **4,99 € wird vom Deckel zum Mittelpreis.** Der 9,99-€-Tier existiert primär, damit „Plus" günstig wirkt (Anker) — und sekundär, weil Clip-Automation nachweislich der einzige Baustein mit zweistelliger Zahlungsbereitschaft ist. Selbst 9,99 € bleiben weit unter Eklipse/Opus: der Vergleich steht auf der Preisseite.
- **Free behält 3 Watermark-Clips:** Hormozis „Probe"-Lead-Magnet und der StreamLadder-Watermark-Loop in einem; Kostentreiber (Rendering) bleibt durch das Limit kontrolliert (Walling).
- **Wert-Stack-Darstellung (Hormozi):** Auf der Preisseite jede Plus-Komponente einzeln mit Marktreferenz beziffern („Clip-Tool: 15 €+ · Analytics-Coaching: 10 €+ · Priority-Raids: unbezahlbar, gibt's nur hier") → Summe vs. 4,99 €. Kein „feel stupid saying no"-Wording, aber dessen Mechanik.
- **Jahresplan als Default-Vorauswahl** im Checkout (Walling); Framing „2 Monate geschenkt", nie „Rabatt".

## 3. Preis-Regeln (aus Limbeck/Taxis/Hormozi destilliert)

1. **Nie rabattieren.** Vergünstigung nur gegen Gegenleistung: Gratismonat für eine erfolgreiche Empfehlung, für ein Testimonial mit Zahlen, für einen Case-Study-Dreh.
2. **Preis früh und selbstbewusst nennen** (Heinrich): Auf Website und im Outreach steht das Modell offen — „kostenlos; optionale Pläne 4,99/9,99 €" — das entwaffnet den „Was kostet es wirklich?"-Verdacht.
3. **Preisänderungen öffentlich begründen** (Levels): „Wir testen X — hier sind die Ergebnisse." Hormozis +20-%-Testschritte auf das Bundle anwenden, aber transparent kommuniziert.
4. **Ehrliche Knappheit statt Countdown** (Priestley): Zwei legitime Knappheiten existieren wirklich — (a) **Founding-Lifetime**: die ersten ~50 zahlenden Partner erhalten „Alles drin lebenslang" für einmalig 59–79 € (bindet die Kern-Streamer vor Deadlock 1.0, finanziert vor, erzeugt Testimonials); (b) **Netzwerk-Slots**: Raid-Boost-Plätze pro Zeitzone/Slot begrenzen, damit der Boost wirksam bleibt — die Begrenzung ist produktlogisch wahr und darf deshalb kommuniziert werden.
5. **Raid Boost fair halten:** Der bezahlte Vorteil darf das Netzwerk-Fairness-Versprechen nicht brechen (kleine Free-Streamer müssen weiter regelmäßig Raids bekommen), sonst kollabiert der Netzwerkeffekt, der das Freemium rechtfertigt.

## 4. Upsell-Roadmap (aus Marktanalyse §5, priorisiert)

| Prio | Feature | Monetarisierung | Begründung |
|---|---|---|---|
| P1 (sofort) | Jahrespläne + neue 3-Stufen-Preisseite | Struktur | reine Konfigurations-/UI-Arbeit, größter Churn-Hebel |
| P1 (sofort) | Founding-Lifetime (limitiert, echt) | Einmalerlös + Bindung | Netzwerk-Aufbauphase, Testimonial-Maschine |
| P1 | **Creator Pro:** Clip-Unlimited + Auto-Posting-Scheduler | 9,99 € | einziger Baustein mit belegter zweistelliger Zahlungsbereitschaft; Clipper existiert bereits, Scheduler ist der Neubau |
| P1 | KI-Coaching-Wochenreport (proaktiv per Discord-DM statt nur Dashboard) | in Plus | verwandelt vorhandene Analytics in gefühlten Dauerwert → senkt Churn |
| P2 | Custom-Bot-Name / Mini-White-Label | in Plus | Moobot-belegtes Feature, geringer Aufwand |
| P2 | Multi-Plattform-Erweiterungen (YouTube-Restream-Hooks, Kick) | in Pro | Markt-Differenzierung von Opus/Eklipse |
| P3 (ab ~50 aktiven Partnern) | **Netzwerk-Sponsoring:** gebündelte Reichweite aller Partner an Sponsoren vermarkten, Erlösanteil an Streamer | Provision | StreamElements' Kern-Monetarisierung; macht das Netzwerk für größere Streamer wirtschaftlich interessant („bei uns verdienst du, statt zu zahlen") |
| P3 | Grounded Deadlock-Wissens-Chat („beantwortet Ranked-Fragen im Chat") | in Pro | Engagement-Feature; erst nach Knowledge-Grundlage (siehe docs/TODO-OFFEN.md) |

**Nicht bauen:** Overlays/Alerts, generische Analytics, Emote-Tools (gesättigte Nullpreis-Märkte).

## 5. Erwartungsrahmen (ehrlich, aus Kapitel 02)

Bei realistisch 200–600 aktiven DACH-Deadlock-Streamern und guter Community-Konversion (10–15 %) liegt das kurzfristige Umsatzpotenzial im niedrigen dreistelligen Euro-Bereich pro Monat — plus Founding-Lifetime-Einmalerlöse. **Das Pricing-Redesign ist deshalb primär Vorbereitung, nicht Ernte:** Es maximiert ARPU und Churn-Resistenz einer kleinen Basis und steht bereit, wenn Deadlock 1.0 die Kategorie vergrößert. Die eigentliche Wette bleibt Netzwerk-Dominanz vor dem Release; die Architektur sollte spielagnostisch gehalten werden, um sie auf weitere Nischen-Games klonen zu können.

## 6. Messgrößen für diesen Teil

- ARPU, Plan-Mix (Free/Plus/Pro), Monats- vs. Jahresanteil.
- Trial→Paid-Konversion; Churn je Plan und Zahlungsintervall.
- Clip-Nutzung Free (Watermark-Loop: Anmeldungen mit Herkunft „Clip").
- Founding-Lifetime: Absatz, daraus gewonnene Testimonials/Case Studies.
- Preisexperimente: Conversion-Delta je Testschritt, öffentlich dokumentiert.
