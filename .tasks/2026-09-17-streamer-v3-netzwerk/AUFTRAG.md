# Auftrag: streamer-v3-netzwerk (Paket A, Seite)

status: aktiv (2026-09-17)

## Ziel

Unter `/streamer/v3/` entsteht eine dritte Fassung der Streamer-Landing. Sie verkauft nicht mehr einen Bot mit vielen Funktionen, sondern den Anschluss ans Partner-Netz der deutschen Deadlock-Streamer. Weniger Sektionen, Vertrauen in Klartext direkt am Partner-Knopf, ein Block für die Community. `/streamer/` (v2) und `/streamer/v1/` bleiben unverändert. Hintergrund und Wortwahl stehen in `KONZEPT.md` im selben Ordner, vollständig lesen.

## Arbeitsschritte

1. Einstieg anlegen: `website/v3/index.html` nach dem Muster von `website/v1/index.html`, mit `noindex, nofollow`, Canonical auf `https://deutsche-deadlock-community.de/streamer/`, eigener Titel und Beschreibung in der neuen Wortwahl. Entry `website/src/streamer-v3.tsx` nach dem Muster von `website/src/streamer-v2.tsx`. Input `streamerV3: v3/index.html` in `website/vite.config.ts` ergänzen.
2. Seite `website/src/pages/StreamerNetworkV3Page.tsx` mit neuen Komponenten unter `website/src/components/partner-v3/`. Bestehende Bausteine wiederverwenden (Navbar, Footer, SiteChatbot, GlowOrb, `useNetworkStreamers`, `SectionHeading`, `ScrollReveal`, `GradientText`, `panel-card`, `soft-elevate`, `gradient-accent`). `PartnerNetwork` aus `partner-clean` wird unverändert importiert. Look ist der dichte v1-Look der bestehenden Seite, kein Neu-Design, keine luftigen Layouts mit nummerierten Sektionen.
3. Reihenfolge der Seite:
   1. Hero: Leitsatz "Kein zweiter Bot. Dein Anschluss ans deutsche Deadlock-Netz.", die zwei anderen Leitsätze aus dem Konzept liegen als Konstante im selben Modul zum Tausch bereit. Großes Live-Embed wie im bestehenden Hero, echte Partnerzahl aus `useNetworkCount`, Knopf "Partner werden" mit `buildTwitchBotAuthUrl()`. Direkt unter dem Knopf drei kurze Klartext-Zeilen zum Vertrauen (Inhalt siehe Schritt 5).
   2. Das Netz live: `PartnerNetwork`.
   3. So wandern Zuschauer: gekürzte Fassung des Raid-Erklärers (eine Grafik oder Animation aus `RaidExplainer.tsx` übernehmen, Text auf Überschrift plus einen Satz kürzen). Darunter echte Netz-Zahlen aus `GET /twitch/api/v2/public/network-stats`: `active_partners`, `raids_total`, `raids_7d`, `viewers_forwarded_total`. Neuer Hook `website/src/hooks/useNetworkStats.ts` nach dem Muster von `useNetworkCount.ts`. Schlägt der Abruf fehl oder ist ein Wert 0, verschwindet die jeweilige Kachel; keine Fallback-Zahlen, keine erfundenen Werte. "Gesamt" und "letzte 7 Tage" ehrlich beschriften, ein 30-Tage-Wert existiert nicht.
   4. "Läuft neben deinen Bots": ein Satz, dass nichts ersetzt wird und kein bestehender Bot weichen muss; ein Satz zur Deadlock-Frage (der Anschluss gilt immer, weitergereicht wird nach Deadlock-Streams, ab und zu Deadlock reicht). Die sechs Vorteile aus `partner-clean/Features.tsx` als kompakte Chips (Icon plus Titel), die Beschreibung erscheint erst hinter "Mehr anzeigen". BanFeed, ClipManager, Community und PartnerPitch kommen auf v3 nicht vor.
   5. Vertrauen in Klartext, vier kurze Karten ohne Fachvokabular:
      - Was der Bot mit dem Partner-Zugang darf: raiden, moderieren, im Chat schreiben, Clips erstellen, Kanalpunkte-Einlösungen, Bits und Werbezeiten lesen. Quelle ist die Scope-Liste `rust/crates/tb-raid/src/scope_profiles.rs:37-45`, die Aussagen müssen dazu passen.
      - Was er mit diesem Zugang nicht kann: Streamtitel, Kategorie und Kanal-Einstellungen ändern. Wichtig: das gilt nur für den Partner-Zugang. Zusatzrechte (Titel, Uplink) holt sich niemand automatisch, sie kommen nur über einen eigenen bewussten Knopf im Dashboard. Genau so formulieren, keine absolute Aussage "kann er technisch gar nicht".
      - Was wir nicht haben: keine E-Mail-Adresse, kein Passwort, keine Zahlungsdaten. (Beleg: Partner-Login fragt `user:read:email` nicht an, `twitch_partners` und `twitch_streamers` haben keine E-Mail-Spalte.)
      - Wie du rauskommst: ein Klick in den Twitch-Einstellungen unter Verbindungen, der Bot merkt das und schaltet sich für den Kanal ab; dazu der Selbstbedienungsweg `https://deutsche-deadlock-community.de/twitch/verwaltung`.
      Der Hex-Blob und "AES-256-GCM" kommen auf v3 nicht vor. Der Link "Ganzes Sicherheitskonzept lesen" (`TWITCH_SECURITY_URL`) bleibt als Textlink.
      Die drei Zeilen am Hero-Knopf sind die Kurzform: "Keine E-Mail, kein Passwort, keine Zahlungsdaten." / "Titel und Kanal-Einstellungen bleiben bei dir." / "Ein Klick bei Twitch und der Bot ist raus."
   6. Community-Block "Du streamst nicht selbst?": Zuschauer sollen ihren Streamer ins Netz holen. Zwei Wege, beide existieren wirklich: Knopf "Link kopieren" (kopiert `https://deutsche-deadlock-community.de/streamer/` in die Zwischenablage, mit sichtbarer Bestätigung) und Link zum Community-Discord (bestehende Konstante in `website/src/data/externalLinks.ts` verwenden). Kein Formular, kein Versprechen einer Belohnung, kein Vorschlagsweg, den es nicht gibt.
   7. Abschluss: bestehende `CTA`-Komponente oder eine v3-Fassung mit gleichem Knopf.
4. Wortwahl laut Konzept: "Bot" nie als Hauptsache, keine Wörter wie Features, Funktionen, Tarif, Preis, Tool, Produkt, verbinden, Dashboard, Analytics. Keine Preise, keine Upgrade-Hinweise. Echte Umlaute, keine Em-Dashes, kein Füllsatz. Pro Sektion höchstens Überschrift, ein Nutzen-Satz und ein visueller Anker.
5. Bewegung dezent und mit Rücksicht auf `prefers-reduced-motion`, wie auf der bestehenden Seite.
6. Prüfen: `npm run build` in `website/` läuft durch, `npx tsc --noEmit` (oder das im Repo übliche Lint-Skript) ohne neue Fehler, `dist/v3/index.html` existiert und enthält den vorgerenderten Hero-Text.

## Fundstellen (aus dem Vorcheck, Stand origin/main bc7363aa)

- `website/vite.config.ts:21`: `base: '/streamer/'`; Zeilen 26-35: Rollup-Inputs (`main`, `streamerV1` und weitere).
- `website/index.html:131`: Einstieg lädt `/src/streamer-v2.tsx` mit `prender`-Attribut.
- `website/src/streamer-v2.tsx:6,10-22`: rendert `StreamerNetworkPage`, hydrate und prerender.
- `website/v1/index.html:9-11,18,159`: Muster für `noindex` und Canonical.
- `website/src/pages/StreamerNetworkPage.tsx:1-40`: aktuelle Komposition.
- `website/src/hooks/useNetworkCount.ts:15-33`: Partnerzahl.
- `rust/crates/tb-dashboard-api/src/handlers/network_stats.rs:19-26`: Felder von `/twitch/api/v2/public/network-stats`.
- `website/src/data/externalLinks.ts:20-38`: `buildTwitchBotAuthUrl()`, `TWITCH_SECURITY_URL`.
- `rust/crates/tb-raid/src/scope_profiles.rs:37-45`: Rechte des Partner-Zugangs; Zeilen 48-107: Zusatzprofile.
- Caddy: Block `handle /streamer*` fällt per `try_files` auf `dist/v3/index.html`, im Repo ist dafür nichts zu tun.

## Was nicht angefasst wird

- `website/index.html`, `website/v1/`, `StreamerNetworkPage.tsx` und alle Dateien unter `components/partner-clean/` bleiben unverändert (nur importieren).
- Kein Rust-Code, keine Migration, keine API-Änderung, kein Caddyfile.
- Kein Vorschlags-Formular und kein neuer Endpunkt (eigenes Paket B, noch nicht beauftragt).
- Der geteilte Checkout `/home/nathanael/repos/Deadlock-Twitch-Bot` wird nicht berührt, gearbeitet wird nur im Worktree.

## Fertig-Kriterium

`npm run build` erzeugt `dist/v3/index.html`; die Seite zeigt die sieben Abschnitte in der genannten Reihenfolge, die Netz-Zahlen kommen live aus der API und verschwinden bei Fehlern, keine der verbotenen Vokabeln steht im v3-Code, v2 und v1 bauen unverändert.

## Deploy-Weg

Nach Review und Merge: Build im Worktree, rsync von `website/dist` in das von Caddy gelesene `dist` (Memory `streamer-website-deploy-weg`), Live-Prüfung unter `https://deutsche-deadlock-community.de/streamer/v3/`. Deploy macht der Orchestrator, nicht der Worker.

## Rahmen

- Du bist der einzige Thread für dieses Paket. Keine Unter-Threads oder Unter-Agenten spawnen.
- Keine Code-Kommentare schreiben, Code erklärt sich selbst.
- Nur den eigenen Branch pushen, nie main.
- Codebase-Fragen zuerst über `graphify query`, grep nur zum Nachlesen.
- Auftrag größer als beschrieben: Bump-up-Nachricht an den Intent-Thread, dann stoppen.
