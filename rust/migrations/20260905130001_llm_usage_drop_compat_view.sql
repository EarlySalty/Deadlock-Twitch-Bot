-- Kompat-View minimax_usage entfernen, nachdem das neue Binary auf llm_usage
-- umgestellt ist. Von Hand als postgres NACH dem Restart anzuwenden.
DROP VIEW public.minimax_usage;
