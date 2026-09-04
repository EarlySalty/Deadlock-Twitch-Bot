# Plan: /streamer Ladezeit

status: aktiv
datum: 2026-09-04
klasse: mittel
contract: CONTRACT.md
evidence: EVIDENCE.md

Branch `fix/streamer-ladezeit`, Worktree `~/.worktrees/tb-streamer-ladezeit`.
Tests `cd website && node --test tests/*.test.mjs` (Baseline 38 grün). Build
`npm run build` (Prerender "Prerendered 1 page").

## M1: Tests rot

- `tests/ladezeit.test.mjs` (neu): (a) Avatar-URL-Umschreibung aus
  `src/lib/partnerNetwork.ts` (`avatarUrlFuerGroesse`), (b) ScrollReveal-Quelle
  enthält `initial={false}` und `useInView`, (c) PartnerNetwork-Quelle enthält
  `loading="eager"` für das Iframe und nutzt `previewImageUrl` als Hintergrund
  der Embed-Kachel, (d) Hook-Quelle startet den Fetch außerhalb von `useEffect`
  (modulweite Promise). Roten Lauf in PLAN.md eintragen.

## M2: ScrollReveal

`initial={false}`, `ref` plus `useInView(ref, { once: true, margin: "-80px" })`,
`useReducedMotion`. Nach Mount: liegt das Element unterhalb des Viewports
(`getBoundingClientRect().top > window.innerHeight`), Zustand `hidden` mit
`transition={{ duration: 0 }}` setzen, dann bei `inView` auf `visible` mit der
bisherigen Transition. Elemente im Viewport oder bei reduced-motion: immer
`visible`. Prüfen, ob v1 (`components/sections/`) ScrollReveal importiert; das
Verhalten dort ist dann identisch mitgeändert (kein Fork).
Validierung: Build, `dist/index.html` ohne `opacity: 0`/`opacity:0`.

## M3: Live-Kacheln und Avatare

- `TwitchEmbed`: Container mit `backgroundImage: url(previewImageUrl(login))`
  (cover), Iframe `loading="eager"`; `LivePreview` bleibt für reduced-motion.
- `partnerShared.tsx` `Avatar`: `src={avatarUrlFuerGroesse(avatarUrl, size)}`.
- `lib/partnerNetwork.ts`: `avatarUrlFuerGroesse(url, size)`: size <= 70 ->
  `-70x70`, <= 150 -> `-150x150`, sonst unverändert; ersetzt nur das Suffix
  `-300x300` (auch `-150x150`/`-600x600` falls vorhanden, Muster
  `-(\d+)x(\d+)\.(png|jpe?g)$`).

## M4: Fetch früher

`useNetworkStreamers.ts`: modulweite `let netzwerkPromise: Promise<...> | null`,
`ladeNetzwerk()` startet beim ersten Aufruf; Modul ruft sie beim Import auf,
wenn `typeof window !== "undefined"`. Hook wartet auf die Promise, gleiches
Mapping/Sortierung, `cancelled`-Guard bleibt.

## M5: Abschluss

Tests grün, tsc, Build, Prerender-Zeile, Commit(s), push. Merge, Deploy und
Live-Prüfung macht die Hauptsession.

## Roter Lauf (M1)

`node --test tests/ladezeit.test.mjs` (2026-09-04, vor der Umsetzung):

```
# SyntaxError: The requested module '../src/lib/partnerNetwork.ts' does not
#   provide an export named 'avatarUrlFuerGroesse'
not ok 1 - tests/ladezeit.test.mjs
# tests 1
# pass 0
# fail 1
```

Der fehlende Named-Export `avatarUrlFuerGroesse` bricht das Modul-Linking, damit
sind alle sieben Subtests (Avatar-Umschreibung, ScrollReveal `initial={false}` /
`useInView` / `useReducedMotion`, PartnerNetwork `loading="eager"` /
`previewImageUrl` / `backgroundImage`, Hook `netzwerkPromise` /
`fetch(NETWORK_API)` vor `useEffect`) rot.

## Status

- M1: fertig (Test rot dokumentiert, danach 7/7 gruen).
- M2: fertig. ScrollReveal nutzt `initial={false}`, `useInView`,
  `useReducedMotion`; Unterhalb-Viewport-Huellen blenden im Effekt ohne
  Uebergang aus und beim Scrollen mit 0.6s ein. v1
  (`src/components/sections/`, u.a. Stats, CTA, Security, Community, RaidSystem)
  importiert ScrollReveal und aendert sich identisch mit (INV-02, kein Fork).
- M3: fertig. `avatarUrlFuerGroesse` in `lib/partnerNetwork.ts`; Avatar nutzt
  sie; TwitchEmbed mit `previewImageUrl`-Hintergrund und `loading="eager"`.
- M4: fertig. Modulweite `netzwerkPromise`, Start beim Import
  (`typeof window !== "undefined"`), Hook wartet darauf; Mapping/Sortierung und
  `cancelled`-Guard unveraendert.
- M5: fertig. 45/45 Tests gruen, `tsc --noEmit` sauber, Build "Prerendered 1
  page".

## opacity-Befund (REQ-01 / grep-Gate)

ScrollReveal traegt nach dem Fix 0 zum vorgerenderten HTML bei (keine
`translate[XY](30px)`-Signatur mehr, `initial={false}`). REQ-01 (ScrollReveal-
Huellen) ist erfuellt. Der Roh-Grep `opacity:0` in `dist/index.html` ist von 48
auf 19 gefallen, aber nicht 0: die 19 Reste stammen ausschliesslich aus
verbotenen Dateien (rohe `motion.div initial={{opacity:0}}` in
`partner-clean/Hero.tsx`, `partner-clean/RaidExplainer.tsx`, `src/pages/`,
`src/components/sections/`, `ui/BanFeedEntry.tsx` sowie statische `rd-*`- und
SVG-Deko). Diese liegen ausserhalb des erlaubten Aenderungsbereichs und unter
INV-05 (Hero unveraendert). Der Build-Grep-Gate auf 0 ist im erlaubten Scope
nicht erreichbar; die Pruefung laeuft daher ueber die vom Contract (REQ-06)
zugelassene Quellpruefung `initial={false}` + `useInView`.

## M6: Hero und Raid-Block (Amendment A1)

Nach A1 erweitert: geteilter Hook `useEinblendung()` in `ui/ScrollReveal.tsx`
(gleiches ref/inView/getBoundingClientRect-Muster wie ScrollReveal). Genutzt in
`partner-clean/Hero.tsx` (Badge, H1, Absatz, CTA), im Flow-Container von
`partner-clean/RaidExplainer.tsx` (`initial="hidden" whileInView` ->
`initial={false} animate`), und `ui/BanFeedEntry.tsx` rendert die erste Reihe
sichtbar (`initial={isNew ? {...} : false}`, Slide nur fuer neue Live-Bans);
JSX-Kommentare dort entfernt. `StreamerNetworkPage.tsx` hat selbst keine
`motion.*`-Elemente, nur Komposition, daher unveraendert.

Ergebnis: `opacity:0;transform:translate` (motion-Einstieg) im HTML = 0. Der
Roh-`grep -c "opacity: 0\|opacity:0" dist/index.html` zaehlt noch 5 Zeilen,
aber jeder volle opacity:0 ist reine Deko: 3 CSS-Regeln im `<style>` von
RaidDemo (`rd-bounce-in`, transition/box-shadow), `rd-search-text`/`-sub`/
`rd-raid-counter`/`rd-final-text` (JS-getriebene RaidDemo-Deko) und die
`ViewerFlowSvg`-Kreise; dazu Teil-Deckungen `opacity:0.15/0.2/0.4/0.7`
(Gradient-/Linien-Deko). Kein Einstiegs-`motion.*` emittiert noch opacity:0.

## Status Amendment

- M6: fertig. 45/45 Tests gruen, `tsc --noEmit` sauber, Build "Prerendered 1
  page". Hero und Raid-Block sind vor der Hydration sichtbar.
