-- Signup-Block: eigenständiger Zustand für Streamer, die nicht ins
-- Partnerprogramm aufgenommen werden sollen.
--
-- Bewusst KEINE Wiederverwendung von twitch_raid_blacklist,
-- twitch_partners.manual_partner_opt_out oder technical_pause_reason:
-- die decken andere Zwecke ab (Raid-Ziel-Auswahl, Streamer-eigenes Opt-out,
-- technische Zwangspause). Vermischung würde jeden dieser Zustände
-- mehrdeutig machen.
--
-- Richtungsregel: Signup-Block impliziert Raid-Blacklist (die API schreibt
-- dort zusätzlich einen Eintrag mit reason-Präfix 'signup_block:').
-- Raid-Blacklist impliziert KEINEN Signup-Block.

CREATE TABLE IF NOT EXISTS public.twitch_partner_signup_denylist (
    twitch_user_id          text PRIMARY KEY,
    twitch_login            text NOT NULL,
    reason                  text NOT NULL,
    public_message          text,
    added_by                text NOT NULL,
    added_at                timestamptz NOT NULL DEFAULT now(),
    partner_paused_by_block boolean NOT NULL DEFAULT false
);

COMMENT ON TABLE public.twitch_partner_signup_denylist IS
    'Streamer, die nicht ins Partnerprogramm aufgenommen werden. Anker ist twitch_user_id, twitch_login ist nur Anzeige/Fallback.';
COMMENT ON COLUMN public.twitch_partner_signup_denylist.reason IS
    'Interner Grund. Wird niemals an den Streamer ausgeliefert.';
COMMENT ON COLUMN public.twitch_partner_signup_denylist.public_message IS
    'Optionaler individueller Absagetext. NULL bedeutet: Default-Text aus dem Code.';
COMMENT ON COLUMN public.twitch_partner_signup_denylist.partner_paused_by_block IS
    'true = die technical_pause_reason=blocked auf twitch_partners stammt von diesem Signup-Block. Nur dann darf das Aufheben sie zuruecknehmen; ein Admin-Block aus der Streamer-Verwaltung setzt dasselbe Wort und bleibt sonst stehen.';

CREATE UNIQUE INDEX IF NOT EXISTS idx_partner_signup_denylist_login
    ON public.twitch_partner_signup_denylist (lower(twitch_login));

INSERT INTO public.twitch_partner_signup_denylist
    (twitch_user_id, twitch_login, reason, public_message, added_by)
VALUES
    ('173926844', 'temmiee985',      'owner_decision:repraesentation', NULL, 'seed'),
    ('166907981', 'ludi7',           'owner_decision:repraesentation', NULL, 'seed'),
    ('839304219', 'taiju_redestein', 'owner_decision:repraesentation', NULL, 'seed')
ON CONFLICT (twitch_user_id) DO NOTHING;

-- Richtungsregel direkt für den Seed mitziehen: die drei sind auch keine
-- Raid-Ziele mehr. ON CONFLICT schützt bereits bestehende Blacklist-Gründe.
INSERT INTO public.twitch_raid_blacklist (target_id, target_login, reason, added_at)
SELECT d.twitch_user_id,
       lower(d.twitch_login),
       'signup_block:' || d.reason,
       to_char(now() AT TIME ZONE 'UTC', 'YYYY-MM-DD"T"HH24:MI:SS+00:00')
FROM public.twitch_partner_signup_denylist d
WHERE d.added_by = 'seed'
ON CONFLICT (target_login) DO NOTHING;
