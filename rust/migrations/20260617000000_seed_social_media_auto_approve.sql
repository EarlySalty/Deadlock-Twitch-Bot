-- M12-3 (social-media-phase0-4-schema-3): Auto-Approve-Settings-Keys seeden.
--
-- Python-Orakel `bot/social_media/storage.py:_ensure_auto_approve_settings`
-- legt beim Start drei Settings-Zeilen an, falls noch nicht vorhanden:
--   auto_approve_youtube / auto_approve_tiktok / auto_approve_instagram
-- jeweils JSONB `false`, updated_by = 'phase4_migration', ON CONFLICT DO NOTHING.
--
-- Ohne diesen Seed liefert get_auto_approve_settings() auf einer frischen DB zwar
-- via Default-Fallback ebenfalls `false`, aber die Zeilen fehlen — das gewachsene
-- Prod-Schema hat sie. Dieser additive Seed stellt die Schema-/Daten-Parität her.
--
-- Default-AUS-Garantie: value = 'false' — Auto-Approve bleibt deaktiviert, bis ein
-- Admin im Dashboard explizit umschaltet. Idempotent über ON CONFLICT DO NOTHING:
-- bestehende (ggf. bereits true gesetzte) Zeilen werden nicht überschrieben.
INSERT INTO public.social_media_settings (key, value, updated_at, updated_by)
VALUES
    ('auto_approve_youtube',   'false'::jsonb, CURRENT_TIMESTAMP, 'phase4_migration'),
    ('auto_approve_tiktok',    'false'::jsonb, CURRENT_TIMESTAMP, 'phase4_migration'),
    ('auto_approve_instagram', 'false'::jsonb, CURRENT_TIMESTAMP, 'phase4_migration')
ON CONFLICT (key) DO NOTHING;
