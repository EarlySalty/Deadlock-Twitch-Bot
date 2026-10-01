# Auftrag: partner-signup-blocks-kein-doppelt

status: aktiv (2026-09-15)

## Ziel

Auf der Admin-Seite Partneraufnahme (`/twitch/admin/community/partner-signup-blocks`)
passiert Ausschließen, Tag sperren und Aufheben beim ersten Klick. Kein Dialog,
kein Abtippen, kein zweiter Bestätigen-Knopf. Die Hinweisboxen auf der Seite
bleiben. Der Nutzer muss nirgends auf dieser Seite doppelt bestätigen.

## Arbeitsschritte

1. In `bot/admin_dashboard/src/pages/community/PartnerSignupBlocks.tsx` die vier
   `ConfirmTypedDialog`-Instanzen entfernen und den Import löschen.
2. `pendingAdd`, `pendingRemove`, `pendingTagAdd`, `pendingTagRemove` entfernen.
3. Die bestehenden Mutationen direkt vom jeweiligen Knopf auslösen:
   - `Ausschließen` ruft nach der bestehenden Validierung (Login und Grund
     Pflicht, sonst Toast) sofort `addMutation` auf, Inhalt von `confirmAdd`.
   - `Tag sperren` ruft nach der bestehenden Validierung (Tag Pflicht, sonst
     Toast) sofort `tagAddMutation` auf, Inhalt von `confirmTagAdd`.
   - Kanal-`Aufheben` ruft sofort `removeMutation` auf, Inhalt von
     `confirmRemove`, mit dem Zeilen-Eintrag als Argument.
   - Tag-`Aufheben` ruft sofort `tagRemoveMutation` auf, Inhalt von
     `confirmTagRemove`, mit dem Zeilen-Eintrag als Argument.
4. Knöpfe bleiben während der laufenden Mutation disabled, Toasts bleiben.
5. Hinweisboxen mit `ADD_STEPS` / `TAG_ADD_STEPS` und die Listen-Texte bleiben.
6. `ConfirmTypedDialog.tsx` und `StreamerDetail.tsx` nicht anfassen.

## Fundstellen (aus dem Vorcheck)

- `bot/admin_dashboard/src/pages/community/PartnerSignupBlocks.tsx:7`: Import
  `ConfirmTypedDialog`.
- `bot/admin_dashboard/src/pages/community/PartnerSignupBlocks.tsx:85-88`:
  State `pendingAdd`, `pendingRemove`, `pendingTagAdd`, `pendingTagRemove`.
- `bot/admin_dashboard/src/pages/community/PartnerSignupBlocks.tsx:90-100`:
  `openAddConfirm` validiert Login und Grund, setzt dann nur `pendingAdd`.
- `bot/admin_dashboard/src/pages/community/PartnerSignupBlocks.tsx:102-135`:
  `confirmAdd` führt die Mutation aus.
- `bot/admin_dashboard/src/pages/community/PartnerSignupBlocks.tsx:137-161`:
  `confirmRemove` führt die Aufhebung aus.
- `bot/admin_dashboard/src/pages/community/PartnerSignupBlocks.tsx:163-171`:
  `openTagAddConfirm` validiert Tag, setzt dann nur `pendingTagAdd`.
- `bot/admin_dashboard/src/pages/community/PartnerSignupBlocks.tsx:173-199`:
  `confirmTagAdd` führt die Tag-Sperre aus.
- `bot/admin_dashboard/src/pages/community/PartnerSignupBlocks.tsx:201-228`:
  `confirmTagRemove` hebt die Tag-Sperre auf.
- `bot/admin_dashboard/src/pages/community/PartnerSignupBlocks.tsx:267`:
  Kanal-Aufheben setzt `setPendingRemove(entry)`.
- `bot/admin_dashboard/src/pages/community/PartnerSignupBlocks.tsx:328`:
  Tag-Aufheben setzt `setPendingTagRemove(entry)`.
- `bot/admin_dashboard/src/pages/community/PartnerSignupBlocks.tsx:414`:
  Knopf `Ausschließen` ruft `openAddConfirm`.
- `bot/admin_dashboard/src/pages/community/PartnerSignupBlocks.tsx:468`:
  Knopf `Tag sperren` ruft `openTagAddConfirm`.
- `bot/admin_dashboard/src/pages/community/PartnerSignupBlocks.tsx:509-556`:
  vier `ConfirmTypedDialog` (Kanal add/remove, Tag add/remove), expected =
  Login bzw. Tag, zweiter Klick plus Abtippen.
- `bot/admin_dashboard/src/components/shared/ConfirmTypedDialog.tsx:25`:
  geteilte Komponente, auch von StreamerDetail genutzt. Nicht löschen.
- `bot/admin_dashboard/src/pages/streamers/StreamerDetail.tsx:856`: andere
  Seite, nicht anfassen.

## Was nicht angefasst wird

- `ConfirmTypedDialog.tsx`, `ConfirmDialog.tsx`, `StreamerDetail.tsx`.
- Backend, API, Mutationen, Hooks, Toasts, Validierung, Hinweisboxen.
- Texte der Knöpfe und Felder.
- Andere Admin-Seiten.

## Fertig-Kriterium

Auf `/twitch/admin/community/partner-signup-blocks` gilt:

- Klick auf `Ausschließen` speichert den Kanal, ohne Dialog.
- Klick auf `Tag sperren` speichert den Tag, ohne Dialog.
- Klick auf `Aufheben` in einer der beiden Listen hebt sofort auf, ohne Dialog.
- Kein `ConfirmTypedDialog` und kein `ConfirmDialog` mehr in dieser Datei.
- Leere Pflichtfelder bleiben Toast, kein speichern.
- `git grep ConfirmTypedDialog bot/admin_dashboard/src/pages/community/PartnerSignupBlocks.tsx`
  liefert nichts.

## Deploy-Weg

Nicht deployen. Branch pushen, Fertigmeldung, dann stoppen. Merge und Live macht
der Orchestrator.

## Rahmen

- Worktree: `/home/nathanael/.worktrees/tb-partner-signup-blocks-kein-doppelt`
- Branch: `feat/partner-signup-blocks-kein-doppelt`
- Basis: `origin/main` (`58fb24477fa454a4ddae5d9d61b3b3addea59dfa`)
- Repo: nur Deadlock-Twitch-Bot
- Du bist der einzige Thread für dieses Paket. Keine Unter-Threads oder
  Unter-Agenten spawnen.
- Keine Code-Kommentare schreiben, Code erklärt sich selbst.
- Nur den eigenen Branch pushen, nie main.
- Nutzersichtbare Texte auf Deutsch mit echten Umlauten, keine Gedankenstriche
  der Form Em-Dash.
- Auftrag größer als beschrieben: Bump-up, dann stoppen.
