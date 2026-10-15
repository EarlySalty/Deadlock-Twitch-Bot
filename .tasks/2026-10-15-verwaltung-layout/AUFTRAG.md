# Verwaltung layout rework (/twitch/verwaltung)

Source: user contract 2026-10-15. Frontend only, `bot/dashboard_v2`.

## Constraints
- Colour scheme unchanged (dark, gold/bronze accents, green/red status). Left sidebar (MAIN/TOOLS/ADMIN/HILFE) untouched.
- All API endpoints, auto-save, form actions and OAuth flows keep working identically.

## Global
1. Slim page header: remove outer card around KONTO / "Dein Bot, deine Einstellungen"; title + subtitle directly above the tab bar. Tab bar stays compact horizontal (sticky).
2. No box-in-box: inner bordered containers with the action button outside them (Twitch-Verbindung, Discord verbinden, each chat command) become flat rows with subtle border-bottom or a clean grid inside one section card.
3. One toggle switch everywhere: replace all red/green Aktivieren/Deaktivieren buttons (Begrüßung, !lurk, !clip, !vodclip, !title, !rank, !wins, ...) with the existing toggle switch from tab "Bot & Schutz". Remove "Befehl ist aktiv" / "Aktiv" status lines.
4. Auto-save everywhere with a subtle "Gespeichert ✓" indicator (text/number/slider debounced or onBlur, selects onChange). Remove "Zeitzone speichern", "Dauer speichern" and duplicate "Einstellungen speichern" buttons.

## Tab "Konto & Verbindungen"
- Twitch / Discord / Steam cards in grid `repeat(auto-fit, minmax(300px, 1fr))`, equal height. Header: icon + title left, compact status badge "● Verbunden" right. Body: short status sentence, no inner frame. Footer: secondary action button (Jetzt neu autorisieren / Erneut verbinden) bottom aligned.
- Twitch scopes collapsed behind disclosure "17 Berechtigungen aktiv ▾" (count dynamic); pills only when expanded.
- Zeitzone: slim one-row card above or below the grid. Left: title + description. Right in one line: select + "Browser-Zeitzone übernehmen". Saves on change.

## Tab "Chat-Befehle"
- Remove the per-input hint "Namen ändern · speichert automatisch · nur Buchstaben und Zahlen"; show it once as a subtle banner/subtext at the top of the tab.
- Remove the duplicate !vodclip tile from "Weitere Chat-Befehle" (it has its own section with downloads).
- New shared CommandRow/CommandCard: left command input (width 140px), middle title + one-line description, right toggle.
- Three section cards:
  1. CHAT-INTERAKTION & CLIPS: rows for Begrüßung (no input), !lurk, !title, !clip (sub-panel indented when active: "Clip-Dauer in Sekunden" + hint, flat row, autosave), !vodclip (category label, main row, indented hints 1-10 min / max 84 MiB / 24h, "Deine Downloads" with "Neu laden" right aligned).
  2. SPIELSTATISTIK: all stat commands (!rank, !wins, ...) as 2-column grid of CommandCards.
  3. COMMUNITY, RAIDS & SYSTEM: 2-column grid for !connect, !unconnect, !watchtime, !invite, !dldc, !commands, !trustpilot, !dashboard, !help, !ping, !engagement_i, !engagement_r, !raid, !raid_status, !raid_history, slim layout without the repeated hint.

## Tab "Bot & Schutz"
- MODERATION "Was der Bot übernimmt": keep 4 toggle rows; remove "Einstellungen speichern", autosave on toggle.
- MODERATION "Scam-Schutz": header row "Scam-Schutz aktiv" + description left, toggle right. Below a 2-column grid: left "Verhalten bei hoher Sicherheit" + description + segmented control (Auto-Bann | Auto-Timeout | Nur melden); right both threshold sliders stacked. Remove save button, debounce slider autosave.
- CHAT-BENACHRICHTIGUNGEN "Stille Hinweise": !silentban and !silentraid as one-line rows: left icon + title + description, right input + toggle side by side. No repeated hint.
- KANAL "Bot vom Kanal trennen": one-line danger-zone bar, text left, red button right vertically centred.

## Acceptance
1. No box-in-box at connections and commands.
2. Twitch/Discord/Steam side by side at desktop width; scopes collapsed by default.
3. Hint text not under every input.
4. !vodclip appears once in Chat-Befehle.
5. All toggleable chat commands use the same toggle switch as Bot & Schutz.
6. Scam-Schutz sliders share a 2-column layout with the mode selector.
7. Sidebar and palette unchanged.
