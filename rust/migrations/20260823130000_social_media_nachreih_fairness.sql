-- Fairness im Nachreih-Lauf des Social-Media-Approvals.
--
-- `iter_approved_clips_pending_queue` liefert hoechstens `BATCH_SIZE` (10)
-- Freigaben je Lauf und sortierte bisher allein nach `decided_at ASC`. Es gibt
-- aber Freigaben, die im Fenster stehen und trotzdem nie eine Queue-Zeile
-- bekommen koennen: eine Plattform steht bereits auf "hochgeladen", der
-- Planungshorizont ist voll (ausdruecklich ein Dauerzustand), oder das
-- Einreihen scheitert wiederholt. Solche Dauergaeste standen fuer immer vorn.
-- Zehn davon fuellen das Fenster, und keine neue Freigabe wird je wieder
-- eingereiht, ohne Log und ohne Fehler.
--
-- Der Stempel macht aus der Rangliste eine Rotation: der Worker vermerkt nach
-- jedem Versuch den Zeitpunkt, sortiert wird "noch nie versucht" zuerst und
-- danach der aelteste Versuch zuerst.
--
-- Additiv und ohne Backfill: der Bestand startet mit NULL und wird damit
-- einmal bevorzugt, was genau richtig ist, denn versucht wurde er unter der
-- neuen Regel noch nicht.
ALTER TABLE public.social_media_clip_approval
    ADD COLUMN IF NOT EXISTS letzter_nachreih_versuch TIMESTAMPTZ;

COMMENT ON COLUMN public.social_media_clip_approval.letzter_nachreih_versuch IS
    'Letzter Nachreih-Versuch des Approval-Workers. NULL heisst: noch nie versucht, kommt zuerst dran.';

-- Deckt die Sortierung des Nachreih-Laufs ab; nur freigegebene Zeilen sind
-- dafuer ueberhaupt Kandidaten.
CREATE INDEX IF NOT EXISTS idx_social_media_clip_approval_nachreihen
    ON public.social_media_clip_approval
       (letzter_nachreih_versuch ASC NULLS FIRST, decided_at ASC NULLS FIRST, clip_db_id)
    WHERE state = 'approved';
