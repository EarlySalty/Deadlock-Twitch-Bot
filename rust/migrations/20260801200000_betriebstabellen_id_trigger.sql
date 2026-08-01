-- Übergangsbrücke für Schritt 1 des ID-Umbaus.
--
-- Die Betriebstabellen tragen jetzt eine `*_user_id`, aber die Schreibpfade
-- kennen an vielen Stellen nur den Login — die Spalte bliebe für jede neue
-- Zeile leer und damit nutzlos. Dieser Trigger löst den Login beim Schreiben
-- einmalig zur ID auf.
--
-- Bewusst nur, wenn die ID leer ist: Der Rename setzt sie selbst und darf nicht
-- von einer Namensauflösung überstimmt werden. Ist der Login unbekannt, bleibt
-- die Spalte NULL — eine geratene ID wäre schlimmer als eine offene.
--
-- Der Trigger verschwindet wieder, sobald die Schreibpfade die ID selbst
-- mitgeben; bis dahin hält er die Spalte vollständig.
CREATE OR REPLACE FUNCTION tb_fill_twitch_user_id_from_login()
RETURNS trigger
LANGUAGE plpgsql
AS $$
DECLARE
    login_spalte CONSTANT text := TG_ARGV[0];
    id_spalte CONSTANT text := TG_ARGV[1];
    login_wert text;
    id_wert text;
    aufgeloest text;
BEGIN
    EXECUTE format('SELECT ($1).%I, ($1).%I', login_spalte, id_spalte)
       INTO login_wert, id_wert
      USING NEW;

    IF COALESCE(TRIM(id_wert), '') <> '' OR COALESCE(TRIM(login_wert), '') = '' THEN
        RETURN NEW;
    END IF;

    SELECT twitch_user_id INTO aufgeloest
      FROM twitch_streamer_identities
     WHERE LOWER(twitch_login) = LOWER(login_wert)
       AND COALESCE(TRIM(twitch_user_id), '') <> ''
     LIMIT 1;

    IF aufgeloest IS NULL THEN
        SELECT twitch_user_id INTO aufgeloest
          FROM twitch_streamers
         WHERE LOWER(twitch_login) = LOWER(login_wert)
           AND COALESCE(TRIM(twitch_user_id), '') <> ''
         LIMIT 1;
    END IF;

    IF aufgeloest IS NULL THEN
        RETURN NEW;
    END IF;

    NEW := jsonb_populate_record(NEW, jsonb_build_object(id_spalte, aufgeloest));
    RETURN NEW;
END
$$;

DO $trigger$
DECLARE
    ziel record;
BEGIN
    FOR ziel IN
        SELECT * FROM (VALUES
            ('twitch_engagement_settings', 'channel_login', 'channel_user_id'),
            ('twitch_engagement_channel_profile', 'channel_login', 'channel_user_id'),
            ('twitch_engagement_log', 'channel_login', 'channel_user_id'),
            ('twitch_engagement_stream_transcripts', 'channel_login', 'channel_user_id'),
            ('twitch_outreach_shadow_events', 'channel_login', 'channel_user_id'),
            ('twitch_scam_guard_settings', 'channel_login', 'channel_user_id'),
            ('twitch_smalltalk_messages', 'channel_login', 'channel_user_id'),
            ('twitch_channel_match_state', 'channel_login', 'channel_user_id'),
            ('twitch_chat_word_groups', 'streamer_login', 'twitch_user_id'),
            ('twitch_live_announcement_configs', 'streamer_login', 'twitch_user_id'),
            ('twitch_scout_pitch_blacklist', 'streamer_login', 'twitch_user_id'),
            ('twitch_scout_pitch_ledger', 'streamer_login', 'twitch_user_id'),
            ('twitch_promo_cooldowns', 'login', 'twitch_user_id')
        ) AS t(tabelle, login_spalte, id_spalte)
    LOOP
        EXECUTE format(
            'DROP TRIGGER IF EXISTS trg_%s_fill_user_id ON public.%I',
            ziel.tabelle, ziel.tabelle
        );
        EXECUTE format(
            'CREATE TRIGGER trg_%s_fill_user_id
             BEFORE INSERT OR UPDATE OF %I ON public.%I
             FOR EACH ROW EXECUTE FUNCTION tb_fill_twitch_user_id_from_login(%L, %L)',
            ziel.tabelle, ziel.login_spalte, ziel.tabelle,
            ziel.login_spalte, ziel.id_spalte
        );
    END LOOP;
END
$trigger$;
