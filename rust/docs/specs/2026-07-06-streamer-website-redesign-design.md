# Streamer-Website Redesign — Design-Spec

**Datum:** 2026-07-06
**Status:** Design abgestimmt (Brainstorm-Session), bereit für Implementierungsplan
**Repo-Bereich:** `website/` (React/Vite-SPA), FAQ-Quelle `rust/knowledge/bot/`

## Ziel

Die Website soll Streamer dazu bringen, **freiwillig den Bot in ihren Kanal zu holen** — weil Programm und Bot so viel Mehrwert bieten, dass man Teil davon sein will. Das ist die eine Conversion; alles auf der Seite zahlt darauf ein.

## Nicht-Ziele

- Kein technischer Neubau: Die bestehende React-SPA wird umgebaut, nicht ersetzt. Vorhandene Komponenten (Hero, Stats, RaidDemo, Features, …) werden wiederverwendet, wo sie passen.
- Das Demo-Dashboard (`/twitch/demo`) wird **nicht** als USP verkauft. Es wird separat ausgebaut; bis dahin taucht es im Pitch nicht prominent auf.
- Affiliate-Programm und Vertriebler-Seite sind nicht Teil des Funnels. Die Seiten bleiben erreichbar (Direktlinks), fliegen aber aus der Hauptnavigation.

## Zielgruppe

1. **Primär:** Kleine deutschsprachige Deadlock-Streamer. Kommen warm an (Raids, Discord, Mundpropaganda) — mit Grundvertrauen, aber wenig Zeit. Die Seite muss in ~10 Sekunden „Was hab ICH davon?" beantworten.
2. **Strategisch:** Ambitionierte Aufsteiger und Umsteiger von anderen Games, die mit Deadlock größer werden wollen. Etablierte große Streamer werden nicht über die Website gewonnen, sondern über persönliche Ansprache — die Website ist für sie der Glaubwürdigkeits-Check (30-Sekunden-Sniff-Test: professionell oder Hobby-Bot?).

Erkenntnis aus dem Brainstorm: **Nicht etablierte Große jagen, sondern die zukünftigen Großen früh binden.** Die deutsche Deadlock-Kategorie hat oben noch viel Platz (Stand 2026-07: bestätigt) — wer jetzt einsteigt, kann noch ein Gründer der Szene werden.

## Pitch-Narrativ (drei Schichten, in dieser Reihenfolge)

1. **Gelegenheit — „warum jetzt":** Die deutsche Deadlock-Kategorie wird gerade verteilt, die Plätze oben sind frei. Kern ist Kategorie-Arbitrage als echte Twitch-Mechanik: 50 Viewer in Fortnite = unsichtbar auf Platz 4.000; 50 Viewer in Deadlock = oben in der deutschen Kategorie, sichtbar für jeden, der das Spiel anklickt.
2. **Sicherheit — „warum mit uns":** Der weiche Wechsel. Ein Game-Wechsel kostet normalerweise Publikum und beginnt bei null — das Netzwerk fängt das ab: Raids vom ersten Stream an, eine Szene die begrüßt, Coaching dazu. Kein Stream endet mehr im Nichts.
3. **Beweis — „warum glaubwürdig":** Volle Transparenz mit echten Live-Zahlen aus der DB plus sichtbare Gesichter der Szene.

**Ehrlichkeits-Prinzip:** Das Wachstumsversprechen wird bewusst begrenzt formuliert — „Wir machen dich nicht groß. Wir sorgen dafür, dass nichts von dem, was du reinsteckst, verpufft." Kleine Streamer riechen leere Versprechen; Bescheidenheit + Beweisbarkeit ist die Differenzierung.

**Alignment statt Altruismus:** Es wird kein Community-Herz verlangt. Das Programm ist so gebaut, dass Community-Verhalten (raiden, Szene pushen) eigennützig rational ist — wer mitmacht, wächst selbst schneller. Wer darüber hinaus echtes Szene-Engagement zeigt, wird erkannt und zum Gesicht der Szene gemacht.

**Der Bot ist in dieser Story nicht das Produkt, sondern der Mitgliedsausweis.**

## Seitenarchitektur — vier Flächen

### 1. Landing (Pitch-Seite)

Eine Seite, eine Story, ein Ziel. Sektionen von oben nach unten:

1. **Hero:** Gelegenheits-Pitch („Die deutsche Deadlock-Kategorie wird gerade verteilt") + primärer CTA „Bot reinholen".
2. **Live-Beweisblock:** Echte Zahlen direkt aus der DB — aktive Partner, vermittelte Raids (kumulativ gesamt + rollierend „diese Woche"), weitergereichte Viewer. Kumulativ + rollierend kombiniert, damit die Zahlen echt sind, aber nie peinlich leer wirken.
3. **„Wer streamt gerade"-Wall:** Live-Status der Partner. Doppelfunktion: macht die Szene sichtbar UND ist Gratis-Sichtbarkeit für jeden Partner — die Website selbst wird Teil der Programm-Belohnung.
4. **Mechanismus:** Der Raid-Kreislauf visualisiert — „kein Stream endet im Nichts".
5. **Umsteiger-Sektion:** Spricht wörtlich den Wechselmoment an („Du überlegst, das Spiel zu wechseln?") — weicher Wechsel, Kategorie-Arbitrage. Gibt DMs an Wechselkandidaten ein Linkziel.
6. **Gesichter der Szene:** Featured Partner. Zeigt, dass zentrale Rollen existieren und verdient werden — ambitionierte Streamer selektieren sich selbst.
7. **Ehrliches Versprechen + finaler CTA.**

### 2. Features (Katalog-Seite)

Thematisch gegliedert: Raids & Netzwerk, Stats & Overlay, Chat & Moderation, Analytics, Clips, Community. Pro Block: was es tut, ein Screenshot, ein konkretes Beispiel aus dem echten Betrieb. Kein Doku-Vollständigkeitsanspruch — „guck, was du alles kriegst".

### 3. FAQ (SSOT-Architektur)

Die FAQ-Seite wird **aus `rust/knowledge/bot/faq-*.md` gerendert** — derselben Quelle, aus der der Bot antwortet. Eine Wahrheit: Wer das Wissen pflegt, pflegt Bot UND Website. Kein handgepflegtes FAQ-HTML mehr.

### 4. Onboarding

Existiert bereits; wird nahtlos ans Ende des Pitches angedockt, damit zwischen „ich will das" und „Bot läuft" keine Reibung liegt.

## Navigation

Hauptnavigation enthält nur, was auf die Conversion einzahlt: Landing, Features, FAQ, CTA/Onboarding. Affiliate, Vertriebler, Roadmap fliegen aus der Hauptnav (bleiben per Direktlink erreichbar). Jeder zusätzliche Nav-Punkt verwässert den Pitch.

## Technik & Umsetzung

- **V2-Ansatz (Nachtrag 2026-07-06):** Das Redesign wird als paralleles V2 gebaut, nicht als Ersatz der Live-Seite. Neuer Vite-Entry `website/v2/index.html` + `website/src/v2/` → landet in `dist/v2/` → ist über das bestehende statische Serving (`GET /streamer/{path}` aus `website/dist`, `handlers/website.rs`) automatisch unter `/streamer/v2/` erreichbar. Keine Rust-Routing-Änderung nötig; `/streamer/` bleibt unverändert die alte Seite, bis der User den Cutover freigibt. Vorbild: `deutsche-deadlock-community.de/new/`.
- **Design-Sprache:** Gold-Teal-System der Community-Website (`Website/dl-brand/tokens.css` + `deco-elevator-new/`): Ink-Hintergründe (`#0b0907`), Bone-Text (`#ece0c8`), Gold (`#c8a86b`/`#efd49d`/`#806534`), Teal (`#55978f`), Rust-Akzent (`#ad4932`), Sora (Display) + Manrope (Body), Grain+Vignette-Textur, Uppercase-Letterspaced-Nav, Gold-CTA mit Ink-Text. Tokens werden ins V2 kopiert (Twitch-Bot-Repo bleibt eigenständig deploybar).
- Bestehende Vite/React-SPA in `website/` (React 19, Tailwind 4, framer-motion), ausgeliefert wie bisher über tb-dashboard (:8769). Kein Greenfield.
- Live-Zahlen: neuer/erweiterter öffentlicher Read-only-Endpoint in `tb-dashboard-api`, der aggregierte Netzwerk-Metriken liefert (keine sensiblen Einzeldaten).
- FAQ-Rendering: Build- oder Serverzeit-Rendering der `faq-*.md` aus `rust/knowledge/bot/`.
- **User-sichtbare Texte (gesamte Copy): Deutsch, Dev-Ton, locker-nüchtern, konkret. Finale Copy schreibt Claude, nicht Codex** (Codex liefert nur Platzhalter).

## Offene Punkte (für den Implementierungsplan)

- Auswahl der konkreten Kennzahlen + deren DB-Quellen (welche Metriken sind belastbar und schwankungsarm?).
- „Gesichter der Szene": welche Partner, Einverständnis einholen.
- Screenshots/Assets für den Feature-Katalog erstellen.
- Genaue Hero-Copy und Sektions-Texte (Claude, finale Redaktion mit User).
