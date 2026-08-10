-- Pricing-Umbau 2026-08-09: Der Trial laeuft 14 Tage und ist danach genau
-- einmal erneut einloesbar. Aus dem Einmal-Boolean `trial_ever_granted` wird
-- der Zaehler `trials_granted`.
--
-- Der Boolean bleibt bewusst stehen: `tb_db::rows::StreamerPlanRow` und der
-- Python-Altbestand lesen ihn weiter. Er wird ab jetzt zusaetzlich zum Zaehler
-- gesetzt, damit beide Wahrheiten nicht auseinanderlaufen.
ALTER TABLE public.streamer_plans
    ADD COLUMN IF NOT EXISTS trials_granted integer DEFAULT 0 NOT NULL;

-- Backfill: wer den einen alten Trial hatte, hat genau eine Einloesung uebrig.
UPDATE public.streamer_plans
   SET trials_granted = 1
 WHERE COALESCE(trial_ever_granted, 0) = 1
   AND trials_granted = 0;
