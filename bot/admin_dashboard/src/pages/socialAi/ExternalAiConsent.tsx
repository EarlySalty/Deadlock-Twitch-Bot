import { useId } from 'react';
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { fetchExternalConsent, saveExternalConsent } from '@/api/socialAi';
import { actionError } from './SocialAiLayout';

const key = ['social-ai', 'external-consent'] as const;

export function ExternalAiConsent() {
  const id = useId();
  const client = useQueryClient();
  const query = useQuery({ queryKey: key, queryFn: ({ signal }) => fetchExternalConsent(signal), retry: false, refetchOnWindowFocus: false });
  const save = useMutation({ mutationFn: saveExternalConsent, retry: false, onSuccess: data => { client.setQueryData(key, data); } });
  const blocked = query.isFetching || query.isError || save.isPending || save.isError;
  return <section className="panel-card rounded-2xl p-5 space-y-4" aria-labelledby={`${id}-heading`}>
    <h3 id={`${id}-heading`} className="text-lg font-semibold">Externe KI für Social Media</h3>
    <p id={`${id}-description`} className="text-sm text-text-secondary">Erlaubt die Verarbeitung von Social-Media-Inhalten durch externe KI-Anbieter. Zusätzlich muss jeder Streamer für seinen Kanal zustimmen. Ohne diese Freigabe bleibt die externe KI für alle Kanäle gesperrt. Änderungen gelten sofort.</p>
    {query.isPending && <p role="status">Freigabe wird geladen …</p>}
    {query.data && !query.isError && <label className="flex items-center gap-3">
      <input type="checkbox" role="switch" aria-describedby={`${id}-description`} checked={query.data.enabled} disabled={blocked} onChange={event => save.mutate(event.target.checked)} />
      Externe KI global freigeben
    </label>}
    {(query.error || save.error) && <p role="alert" className="social-ai-notice">{actionError(query.error ?? save.error)} Bitte lade die Freigabe neu, bevor du sie erneut änderst.</p>}
    {save.isPending && <p role="status">Freigabe wird gespeichert …</p>}
    {save.isSuccess && <p role="status">Freigabe gespeichert: {query.data?.enabled ? 'Externe KI ist global freigegeben.' : 'Externe KI ist global gesperrt.'}</p>}
    <button type="button" className="admin-button admin-button-secondary" disabled={query.isFetching || save.isPending} onClick={async () => { const result = await query.refetch(); if (result.isSuccess) save.reset(); }}>Freigabe neu laden</button>
  </section>;
}
