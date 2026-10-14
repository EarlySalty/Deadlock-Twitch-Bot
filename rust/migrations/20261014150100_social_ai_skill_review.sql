ALTER TABLE public.social_ai_skill_activations
    DROP CONSTRAINT social_ai_skill_activations_kind_check,
    ADD CONSTRAINT social_ai_skill_activations_kind_check CHECK (kind IN ('learned', 'rollback', 'auto_rollback'));

ALTER TABLE public.social_ai_change_proposals
    ADD COLUMN reviewed_at TIMESTAMPTZ,
    ADD COLUMN reviewed_by TEXT CHECK (char_length(reviewed_by) BETWEEN 1 AND 200);
