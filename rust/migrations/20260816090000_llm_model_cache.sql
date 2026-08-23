-- Aufgeloeste LLM-Modellnamen, damit der Bot nach einem Neustart nicht blind
-- auf dem hartcodierten Default sitzt, wenn die Anbieter-API gerade klemmt.
--
-- Hintergrund: Fireworks hat `deepseek-v4-flash` am 15.08.2026 ersatzlos
-- abgeschaltet. Der Endpunkt antwortete 404, der Conversation-Scam-Judge fiel
-- fail-safe auf `unsure` und hat einen ganzen Tag lang niemanden mehr gebannt.
-- Der Resolver fragt die Modellliste ab und merkt sich hier, welche Fassung
-- einer Familie (z. B. `deepseek-v4-flash`) aktuell die neueste ist.
--
-- Eine Zeile je (Provider, Familie). Der Resolver schreibt per UPSERT, liest
-- beim Start und faellt auf diesen Stand zurueck, wenn die API nicht antwortet.
CREATE TABLE IF NOT EXISTS public.llm_model_cache (
    provider      TEXT        NOT NULL,
    family        TEXT        NOT NULL,
    model         TEXT        NOT NULL,
    -- `created` ist der Zeitstempel des Anbieters (Unix-Sekunden), nach dem
    -- die neueste Fassung bestimmt wird. Fehlt er, bleibt die Spalte NULL und
    -- der Resolver sortiert nur nach Name.
    model_created BIGINT,
    resolved_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (provider, family)
);

COMMENT ON TABLE public.llm_model_cache IS
    'Letzter erfolgreich aufgeloester Modellname je Anbieter und Modellfamilie. Notnagel fuer Neustarts ohne API-Zugriff.';
