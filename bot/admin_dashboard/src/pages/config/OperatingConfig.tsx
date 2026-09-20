import { useState } from 'react';
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { RefreshCw, Save } from 'lucide-react';
import { fetchOperatingConfig, saveOperatingConfig, type OperatingOptions } from '@/api/client';
import { PageHeader } from '@/components/layout/PageHeader';

const fields: Array<{ key: keyof OperatingOptions; label: string; hint: string; min: number; max: number }> = [
  { key: 'pool_max', label: 'Verbindungen je Dienst', hint: 'Maximale gleichzeitige Datenbankverbindungen für Bot und Dashboard.', min: 1, max: 1000 },
  { key: 'acquire_timeout_ms', label: 'Warten auf eine freie Verbindung (Millisekunden)', hint: 'Gesamtbudget für das Warten auf eine Datenbankverbindung.', min: 100, max: 300000 },
  { key: 'connect_timeout_seconds', label: 'Verbindungsaufbau (Sekunden)', hint: 'Bei neuen Verbindungen gilt der kleinere Wert aus diesem Limit und dem Wartebudget.', min: 1, max: 300 },
];

export function OperatingConfigPage() {
  const queryClient = useQueryClient();
  const query = useQuery({ queryKey: ['operating-config'], queryFn: fetchOperatingConfig });
  const [draft, setDraft] = useState<OperatingOptions | null>(null);
  const [base, setBase] = useState<string | null>(null);
  const [notice, setNotice] = useState('');
  const save = useMutation({
    mutationFn: ({ fingerprint, options }: { fingerprint: string; options: OperatingOptions }) => saveOperatingConfig(fingerprint, options),
    onSuccess: data => {
      queryClient.setQueryData(['operating-config'], data);
      setDraft(null);
      setBase(null);
      setNotice('Gespeichert. Die geänderten Werte werden erst nach einem Dienstneustart übernommen.');
    },
  });
  const options = draft ?? query.data?.options;

  async function reload() {
    const result = await query.refetch();
    if (result.isSuccess) {
      setDraft(null);
      setBase(null);
      setNotice('Gespeicherten Stand neu geladen.');
      save.reset();
    }
  }

  return <section className="space-y-5">
    <PageHeader title="Betriebseinstellungen" description="Datenbankverbindungen von Twitch-Bot und Dashboard einstellen. Änderungen werden nach einem Neustart wirksam." />
    <div className="flex flex-wrap items-center justify-between gap-3">
      <p className="text-sm text-text-secondary">Der aktive Stand wird für jeden Dienst einzeln geprüft.</p>
      <button type="button" className="admin-button admin-button-secondary" disabled={query.isFetching || save.isPending} onClick={() => void reload()}>
        <RefreshCw size={16} aria-hidden="true" /> {draft ? 'Entwurf verwerfen und neu laden' : 'Stand neu laden'}
      </button>
    </div>
    {query.isPending && <p role="status">Betriebseinstellungen werden geladen …</p>}
    {query.error && <p role="alert" className="panel-card rounded-2xl p-5">{query.error.message}</p>}
    {query.data && options && <>
      <div className="grid gap-4 md:grid-cols-2">
        {query.data.services.map(service => <article key={service.name} className="panel-card rounded-2xl p-5">
          <h2 className="font-semibold text-white">{service.name}</h2>
          <p className="mt-2 text-sm text-text-secondary">
            {service.restart_required === null ? 'Aktiver Stand nicht bestätigt. Dienststatus erneut prüfen.' : service.restart_required ? 'Gespeicherte Änderungen warten auf einen Neustart.' : 'Gespeicherter Stand ist aktiv.'}
          </p>
        </article>)}
      </div>
      <form className="panel-card space-y-5 rounded-2xl p-6" onSubmit={event => {
        event.preventDefault();
        if (!query.data || !draft || !base) return;
        setNotice('');
        save.mutate({ fingerprint: base, options: draft });
      }}>
        <h2 className="text-lg font-semibold text-white">Datenbankverbindungen</h2>
        <div className="grid gap-5 lg:grid-cols-3">
          {fields.map(field => <div key={field.key}>
            <label htmlFor={`operating-${field.key}`} className="mb-2 block text-sm font-medium text-white">{field.label}</label>
            <input id={`operating-${field.key}`} type="number" min={field.min} max={field.max} step={1} required
              className="admin-input w-full" aria-describedby={`hint-${field.key}`} disabled={save.isPending}
              value={Number.isFinite(options[field.key]) ? options[field.key] : ''}
              onChange={event => {
                setDraft({ ...options, [field.key]: event.target.value === '' ? Number.NaN : Number(event.target.value) });
                if (!base && query.data) setBase(query.data.saved_revision);
                setNotice('');
                save.reset();
              }} />
            <p id={`hint-${field.key}`} className="mt-2 text-sm text-text-secondary">{field.hint}</p>
          </div>)}
        </div>
        <p className="text-sm text-text-secondary">Speichern startet keinen Dienst neu. Bis zum Neustart arbeiten laufende Dienste mit ihrem bisherigen Stand.</p>
        {save.error && <p role="alert" className="text-sm text-red-300">{save.error.message}</p>}
        <button type="submit" className="admin-button admin-button-primary" disabled={!draft || save.isPending}>
          <Save size={16} aria-hidden="true" /> {save.isPending ? 'Wird gespeichert …' : 'Änderungen speichern'}
        </button>
      </form>
    </>}
    <p role="status" aria-live="polite" className="text-sm text-text-secondary">{notice}</p>
  </section>;
}
