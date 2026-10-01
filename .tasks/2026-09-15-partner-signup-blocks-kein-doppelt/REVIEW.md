# Review: partner-signup-blocks-kein-doppelt

status: aktiv (2026-09-15)

Runde 1. Commit `aa16a6c5` gegen `origin/main` `58fb2447`.
Datei: `bot/admin_dashboard/src/pages/community/PartnerSignupBlocks.tsx`.
Scope-Zaun: Diff `origin/main...HEAD` ändert fachlich nur diese TSX-Datei plus `.tasks/`-Artefakte. `ConfirmTypedDialog.tsx`, `ConfirmDialog.tsx` und `StreamerDetail.tsx` sind unangetastet.

ORCHESTRIERUNG[OR-1]: Stufe klein | Schritt review | Artefakt: .tasks/2026-09-15-partner-signup-blocks-kein-doppelt
WIRKUNGSPRUEFUNG[WP-1]: 0 Befunde | Zwillingssuche: grep-belegt | Fremddienst-Pfade: 4/4 geprüft

## Pflichtfragen

1. Alle vier `ConfirmTypedDialog` und der Import sind weg. `git grep ConfirmTypedDialog|ConfirmDialog|window.confirm|setPending` in dieser Datei: keine Treffer. Import-Zeile entfernt, JSX-Block ab alter Zeile 509 entfernt.

2. Die vier Aktionen laufen beim ersten Klick. Kein `ConfirmDialog` als Ersatz.
   - Ausschließen: `357:360` ruft `submitAdd` (`73`).
   - Tag sperren: `411:414` ruft `submitTagAdd` (`138`).
   - Kanal-Aufheben: `228:232` ruft `submitRemove(entry)` (`113`).
   - Tag-Aufheben: `281:285` ruft `submitTagRemove(entry)` (`167`).
   `pendingAdd` / `pendingRemove` / `pendingTagAdd` / `pendingTagRemove` sind entfernt.

3. Validierung bleibt, leere Pflichtfelder speichern nicht.
   - Login leer: `75:77` Toast, return.
   - Grund leer: `79:81` Toast, return.
   - Tag leer: `140:142` Toast, return.
   Tag-Grund bleibt optional (`147`), wie zuvor.

4. Hinweisboxen und Toasts bleiben.
   - `ADD_STEPS` `22:26`, gerendert `368:377`.
   - `TAG_ADD_STEPS` `28:33`, gerendert `422:431`.
   - Toast `463:468`. Erfolgs- und Fehler-Toasts in den vier `submit*`-Funktionen unverändert in der Substanz.
   `REMOVE_STEPS` / `TAG_REMOVE_STEPS` lebten nur im Dialog; Auftrag verlangt sie nicht auf der Seite.

5. Knöpfe sind während `isPending` disabled: `231`, `284`, `359`, `413`. Jeweils die passende Mutation.

6. Kein Zwilling auf dieser Seite. Kein zweiter Bestätigen-Knopf, kein `window.confirm`, kein hängender Dialog-State. `ConfirmTypedDialog` bleibt in `StreamerDetail.tsx:8` und `:856` (Auftrag: nicht anfassen). `ConfirmDialog.tsx` existiert weiter als geteilte Komponente, wird hier nicht importiert.

7. Doppelklick vor dem Re-Render: `submit*` prüft `isPending` nicht selbst, nur `disabled={…isPending}`. Dasselbe Muster wie `GlobalBans.tsx:176`. TanStack Query meldet pending über den Notify-Manager, daher ist ein zweites `mutateAsync` in dem kurzen Fenster theoretisch möglich. Backend: Kanal-`add` mit Advisory-Lock und `ON CONFLICT` (`partner_signup_block.rs:124`, `:178`); Tag-`add` ebenfalls `ON CONFLICT` (`partner_signup_tag_block.rs:191`), Backfill läuft über denselben Upsert. Kein Fix, Hausmuster, Wirkung bleibt eine Sperre.

Fremddienst-Pfade (alle mit try/catch und Toast, nach Fehler wieder klickbar):
- `addMutation` POST `/partner-signup-blocks` (`84:109`)
- `removeMutation` POST `/partner-signup-blocks/remove` (`115:135`)
- `tagAddMutation` Tag-Sperre anlegen (`145:163`)
- `tagRemoveMutation` Tag-Sperre aufheben (`169:186`)

## Mängel: keine
