-- Kurzlebige Vorwarnung für den Werbemanager des Raid-Empfängers.
CREATE TABLE twitch_raid_ad_vorlauf (
    attempt_id TEXT PRIMARY KEY CHECK (attempt_id <> ''),
    from_broadcaster_id TEXT NOT NULL CHECK (from_broadcaster_id <> ''),
    to_broadcaster_id TEXT NOT NULL CHECK (to_broadcaster_id <> ''),
    status TEXT NOT NULL CHECK (status IN ('pending', 'started', 'failed', 'arrived')),
    protected_until TIMESTAMPTZ NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
REVOKE ALL ON TABLE twitch_raid_ad_vorlauf FROM PUBLIC;
CREATE INDEX twitch_raid_ad_vorlauf_target_active
    ON twitch_raid_ad_vorlauf (to_broadcaster_id, protected_until);
CREATE INDEX twitch_raid_ad_vorlauf_expiry ON twitch_raid_ad_vorlauf (protected_until);
CREATE INDEX twitch_raid_ad_vorlauf_source_active
    ON twitch_raid_ad_vorlauf (from_broadcaster_id, created_at)
    WHERE status IN ('pending', 'started');

DO $$
BEGIN
    IF EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'twitchbot') THEN
        GRANT SELECT, INSERT, UPDATE, DELETE ON twitch_raid_ad_vorlauf TO twitchbot;
    END IF;
END;
$$;

-- Der kurze Datenbanklock serialisiert Arrival und abgekoppelte Vorlaufwrites.
-- Bestehende Arrival-Schreiber erhalten keine Rechte auf die Vorlauftabelle.
CREATE FUNCTION public.finish_raid_ad_vorlauf_on_arrival()
RETURNS trigger
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog
AS $$
BEGIN
    PERFORM pg_catalog.pg_advisory_xact_lock(
        pg_catalog.hashtextextended('raid-ad-vorlauf:' || NEW.to_broadcaster_id, 0)
    );
    UPDATE public.twitch_raid_ad_vorlauf
    SET status = 'arrived'
    WHERE from_broadcaster_id = NEW.from_broadcaster_id
      AND to_broadcaster_id = NEW.to_broadcaster_id
      AND status IN ('pending', 'started')
      AND NEW.detected_at >= created_at
      AND NEW.detected_at <= protected_until;
    RETURN NEW;
END;
$$;
REVOKE ALL ON FUNCTION public.finish_raid_ad_vorlauf_on_arrival() FROM PUBLIC;
CREATE TRIGGER finish_raid_ad_vorlauf_on_arrival
AFTER INSERT ON public.twitch_raid_arrival_tracking
FOR EACH ROW EXECUTE FUNCTION public.finish_raid_ad_vorlauf_on_arrival();
