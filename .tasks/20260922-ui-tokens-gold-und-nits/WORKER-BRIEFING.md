# Worker-Briefing: ui-Akzent-Tokens global auf Markenmetalle plus zwei Nits

## Rolle

Du bist der einzige Thread für dieses Paket. Keine Unter-Threads oder Unter-Agenten spawnen.

## Hintergrund

Ein Gate-Durchlauf auf main (Stand 72151dc4) fand Befunde, die aus dem älteren Paket 29dd9c2d („Social Media Dashboard neu strukturieren") stammen und jetzt auf main gelten.

## Auftrag 1 (Pflicht): ui-Akzent-Tokens global auf Gold

- Datei: `bot/dashboard_v2/src/index.css`, Zeilen 31 bis 40.
- Die globalen ui-Tokens stehen auf Indigo/Violett (rgb(165 180 252) und Folgewerte) und werden nur innerhalb `.social-studio`/`.studio-shell` auf Gold abgebildet.
- Die gemeinsame Seitenleiste nutzt `text-ui-accent` und `bg-ui-accent-strong` auf allen Routen und zeigt außerhalb des Studios Indigo. Das widerspricht der Marke (Gold).
- Mechanischer Fix laut Gate: die ui-Akzent-Tokens global auf die Markenmetalle mappen, `var(--color-primary)` für den Akzent und `var(--color-accent-hover)` für die starke Variante. Prüfen, ob die studio-internen Abweichungen danach redundant sind und sich entfernen lassen, ohne das Studio-Design zu ändern.
- Sichtprüfung: Seitenleiste und eine Nicht-Studio-Route müssen danach Gold zeigen, das Studio unverändert. Desktop- und Mobilbreite ansehen.

## Auftrag 2 (Nit, trotzdem beheben): Pipeline-Abbruch bei Paginierungsverschiebung

- `queuePresentation.ts` bricht die Pipeline-Ansicht ab, wenn mehr als 100 Clips vorliegen und zwischen zwei Seitenabrufen ein Clip dazukommt (Verschiebung der Offset-Seiten). Robust machen: Überlappung tolerieren oder anhand eindeutiger Clip-IDs deduplizieren, sodass die Ansicht nicht abbricht.
- Regressionstest für den Fall ergänzen: 101 Clips, neuer Clip zwischen zwei Abrufen, Ansicht rendert ohne Abbruch.

## Auftrag 3 (Nit, trotzdem beheben): Browsertest-Pfade ent-harten

- Der Browsertest hardcoded Chromium- und Font-Pfade. Pfade über Umgebungsvariablen mit sinnvollen Fallbacks lesbar machen (z. B. `CHROMIUM_PATH`, Font-Verzeichnis aus dem Repo auflösen) und im Test eine klare Fehlermeldung ausgeben, wenn etwas fehlt.

## Tests

- Fokussierte Vertragstests des Dashboards und der berührten Bereiche laufen lassen (node --import tsx --test tests/... im dashboard_v2-Verzeichnis), ESLint für berührte Dateien, `npm run build`.
- Die volle Suite hat 5 bekannte Basisfehler (Stand 384/389); diese nicht jagen.

## Abschluss

- Auf dem eigenen Feature-Branch committen und pushen (nie direkt main).
- Bump-up an Intent-Thread `0d56f126-eb40-4b0c-b18d-2b14446d69dd`: Grund, Erledigt (Commit-SHA, Testkommandos, Ergebnisse), Worktree, Offen. Danach bist du fertig.
