-- Korrektur zu 20260802010000: die Lookup-Funktion war fest auf `public`
-- verdrahtet.
--
-- Auf Prod fällt das nicht auf, dort ist `public` der einzige Suchpfad. Die
-- hermetischen Fixtures der Crates legen aber pro Test ein eigenes Schema an
-- und setzen `search_path` allein darauf (z. B.
-- `tb-monitoring/tests/support/mod.rs`). Dort fand die Funktion ihre
-- Quelltabellen nicht und gab für jeden Login `NULL` zurück — jede auf sie
-- umgestellte Query hätte im Test still leer geliefert, ohne rot zu werden.
--
-- Deshalb löst die Funktion ihre Quellen jetzt über den `search_path` des
-- Aufrufers auf statt über ein festes Schema. In Prod ist das Verhalten
-- unverändert (`search_path` = `public`); im Test greift das Testschema.
-- Die Funktion ist SECURITY INVOKER (Vorgabe), der Aufrufer sieht also
-- ohnehin nur Tabellen, für die er selbst Rechte hat.
--
-- Unbekannter Login ergibt weiterhin NULL, damit ein Vergleich schlicht nicht
-- trifft, statt falsch zu treffen.
CREATE OR REPLACE FUNCTION tb_twitch_user_id(login TEXT)
RETURNS TEXT
LANGUAGE plpgsql
STABLE
AS $$
DECLARE
    quelle record;
    treffer TEXT;
BEGIN
    IF COALESCE(TRIM(login), '') = '' THEN
        RETURN NULL;
    END IF;

    FOR quelle IN
        SELECT * FROM (VALUES
            ('twitch_streamer_identities', 'twitch_login', 'twitch_user_id'),
            ('twitch_streamers', 'twitch_login', 'twitch_user_id'),
            ('twitch_live_state', 'streamer_login', 'twitch_user_id')
        ) AS q(tabelle, login_spalte, id_spalte)
    LOOP
        -- Ohne Schema-Präfix: greift den `search_path`. Fehlt die Tabelle im
        -- schlanken Test-Schema, wird die Quelle übersprungen statt zu werfen.
        IF to_regclass(quote_ident(quelle.tabelle)) IS NULL THEN
            CONTINUE;
        END IF;
        EXECUTE format(
            'SELECT %I FROM %I
              WHERE LOWER(%I) = LOWER($1)
                AND COALESCE(TRIM(%I), '''') <> ''''
              LIMIT 1',
            quelle.id_spalte, quelle.tabelle, quelle.login_spalte, quelle.id_spalte
        ) INTO treffer USING login;
        IF treffer IS NOT NULL THEN
            RETURN treffer;
        END IF;
    END LOOP;

    -- Frühere Namen zuletzt und nur, wenn sie eindeutig sind: Twitch gibt
    -- aufgegebene Namen wieder frei.
    IF to_regclass('twitch_login_aliases') IS NOT NULL THEN
        SELECT MIN(twitch_user_id) INTO treffer
          FROM twitch_login_aliases
         WHERE LOWER(twitch_login_aliases.login) = LOWER($1)
        HAVING COUNT(DISTINCT twitch_user_id) = 1;
    END IF;
    RETURN treffer;
END
$$;

COMMENT ON FUNCTION tb_twitch_user_id(TEXT) IS
    'Löst einen Twitch-Login zur stabilen twitch_user_id auf; NULL wenn unbekannt. Quellen über search_path.';
