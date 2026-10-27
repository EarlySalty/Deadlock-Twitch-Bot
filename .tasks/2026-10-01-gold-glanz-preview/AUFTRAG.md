# Auftrag: Gold-Glanz als Vorschau testen (nichts live umstellen)

Intent-Thread: 37ae96e4-49e9-46b7-a77f-e148784352c1
Stufe: klein. Du bist der einzige Thread für dieses Paket. Keine Unter-Threads oder Unter-Agenten spawnen.

## Hintergrund

Der Nutzer empfindet die Akzentflächen im Partner-Dashboard als "gelb". Die
Markentokens sind seit Wochen unverändert (`bot/dashboard_v2/src/index.css`:
`--color-primary #C5A059`, `--color-accent #D6B676`, `--color-accent-hover
#E8CFA0`). Der gelbe Eindruck kommt vor allem vom flachen Verlauf Gold in helles
Messing, z. B. aktiver Tab in `components/layout/TabNavigation.tsx`
(`from-primary/80 via-primary/75 to-accent/80`), Zeitraum- und Sprach-Chips im
`Header.tsx`, der Knopf "Hilfe bekommen" (`.assistent-knopf`) und die
Trophäen-Kachel "Platz N".

## Ziel

Eine Vorschau, wie das Dashboard mit **edlem, glänzendem Gold** aussieht: kein
mattes Flachgold, sondern metallischer Glanz (mehrstufiger Verlauf mit hellem
Lichtkamm und dunklerem Grund, feine helle Innenkante, dezenter Glanz), dabei
weiter Schwarz-Gold und warm, kein Gelbstich, kein Glow im Hintergrund
(Nutzerregel: kein Gold-Schein im Hintergrund, kein Glow an Avatar oder Icons).

- Gern zwei bis drei Varianten (z. B. "poliertes Gold", "Champagner-Gold",
  "Altgold mit Glanzkante"), umschaltbar per URL-Parameter oder Klasse am Root,
  damit der Nutzer vergleichen kann.
- Nur Akzentflächen und aktive Zustände anfassen (aktiver Tab, Chips, Primärknöpfe,
  Hilfe-Knopf, Rang-Kachel, aktiver Sidebar-Eintrag). Statusfarben und
  Diagrammfarben bleiben.

## Grenzen

- **Nichts mergen, nichts deployen, nichts an der Live-Seite ändern.** Kein Push
  nach main.
- Eigener Worktree `~/.worktrees/tb-gold-glanz-preview` von `origin/main`, Branch
  `preview/gold-glanz-20261001`; den Branch nach origin pushen, damit nichts
  verloren geht.
- Keine Code-Kommentare.
- Vorschau als statischer Build mit Demo-Daten (das Dashboard hat einen
  Demo-Modus), per `vite preview` auf einem freien Port, über Tailscale
  erreichbar (Host `v50671-kde`), oder als Screenshot-Satz alt gegen neu je
  Variante für `/analyse` (Übersicht) und die Startseite. Am besten beides.
  Screenshots ablegen unter `.tasks/2026-10-01-gold-glanz-preview/` und im
  Bericht die absoluten Pfade nennen.
- Headless-Chrome kann in der Sandbox hängen; dann den Preview-Link liefern und
  das im Bericht sagen, nicht an Flags drehen.

## Bericht

Preview-URL, Variantenschalter, Screenshot-Pfade, Branch und Commit, kurze
Empfehlung, welche Variante am edelsten wirkt. Danach im Thread stehen bleiben,
bis der Nutzer entschieden hat (kein settle).
