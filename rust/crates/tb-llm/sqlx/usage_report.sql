-- Bekanntes Verbrauchsvolumen und ausdrücklich ungeklärte Aufrufe.
SELECT substring(ts,1,10) AS day, COALESCE(project,'historisch unbekannt') AS project,
       COALESCE(service,source) AS service, COALESCE(purpose,'unbekannt') AS purpose,
       model, provider, count(*) AS attempts,
       count(*) FILTER (WHERE attempt_state='started') AS open_attempts,
       count(*) FILTER (WHERE total IS NULL) AS unknown_usage,
       count(*) FILTER (WHERE tokens_in IS NULL) AS unknown_input_usage,
       count(*) FILTER (WHERE tokens_out IS NULL) AS unknown_output_usage,
       COALESCE(sum(tokens_in),0) AS known_tokens_in,
       COALESCE(sum(tokens_out),0) AS known_tokens_out,
       COALESCE(sum(total),0) AS known_total
FROM public.llm_usage
GROUP BY 1,2,3,4,5,6 ORDER BY day DESC, known_total DESC;

-- Auch Journalverluste oder Prozessabbrüche bleiben als offene Starts auffindbar.
SELECT id, ts, project, service, purpose, model, provider, request_id, attempt_state,
       tokens_in, tokens_out, total, error_code, http_status
FROM public.llm_usage WHERE attempt_state='started' OR total IS NULL
ORDER BY ts DESC;
