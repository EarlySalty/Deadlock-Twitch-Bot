-- Schritt 2 des ID-Umbaus: die Lesepfade sollen einen Kanal über seine stabile
-- ID finden, nicht über den Namen.
--
-- Die Aufrufketten reichen bis heute nur den Login durch — ihn überall bis in
-- den Chat-Handler hinein durch eine ID zu ersetzen, wäre ein Umbau quer durch
-- sechs Crates. Diese Funktion schließt die Lücke an einer Stelle: sie löst
-- einen Login zur ID auf, sodass eine Abfrage die Zeile auch dann findet, wenn
-- der Login *in der Zeile* veraltet ist.
--
-- Absichtlich plpgsql mit `to_regclass`-Prüfung statt einer SQL-Funktion: so
-- lässt sie sich auch in den schlanken Test-Schemata anlegen, die nur einen
-- Ausschnitt der Tabellen kennen. Fehlt eine Quelle, wird sie übersprungen.
--
-- Unbekannter Login ergibt NULL, damit ein Vergleich schlicht nicht trifft,
-- statt falsch zu treffen.
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
        IF to_regclass('public.' || quelle.tabelle) IS NULL THEN
            CONTINUE;
        END IF;
        EXECUTE format(
            'SELECT %I FROM public.%I
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
    IF to_regclass('public.twitch_login_aliases') IS NOT NULL THEN
        SELECT MIN(twitch_user_id) INTO treffer
          FROM twitch_login_aliases
         WHERE LOWER(twitch_login_aliases.login) = LOWER($1)
        HAVING COUNT(DISTINCT twitch_user_id) = 1;
    END IF;
    RETURN treffer;
END
$$;

COMMENT ON FUNCTION tb_twitch_user_id(TEXT) IS
    'Löst einen Twitch-Login zur stabilen twitch_user_id auf; NULL wenn unbekannt.';
