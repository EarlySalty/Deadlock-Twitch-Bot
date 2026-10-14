import { useCallback, useEffect, useMemo, useState } from 'react';
import { useQuery, useQueryClient } from '@tanstack/react-query';
import { RefreshCw } from 'lucide-react';
import { fetchSettingsCatalog } from '@/api/client';
import {
  isDraftChanged,
  matchesSearch,
  uniqueFields,
  type SettingsCatalog,
  type SettingsDraft,
  type SettingsField,
} from '@/api/settingsCatalog';
import { SearchInput } from '@/components/shared/SearchInput';
import { SettingsGroupCard } from './SettingsGroupCard';

const QUERY_KEY = ['operating-settings-catalog'];

function hasChanges(catalog: SettingsCatalog | undefined, fields: SettingsField[], drafts: Record<string, SettingsDraft>) {
  if (!catalog) return false;
  return fields.some(field => field.path in drafts && isDraftChanged(field, drafts[field.path], catalog.values[field.path]));
}

export function SettingsCatalogSection() {
  const queryClient = useQueryClient();
  const query = useQuery({ queryKey: QUERY_KEY, queryFn: fetchSettingsCatalog, refetchOnWindowFocus: false });
  const [drafts, setDrafts] = useState<Record<string, SettingsDraft>>({});
  const [search, setSearch] = useState('');
  const [openGroups, setOpenGroups] = useState<Set<string>>(new Set());
  const [restartPending, setRestartPending] = useState<Set<string>>(new Set());
  const catalog = query.data;
  const fields = useMemo(() => uniqueFields(catalog?.fields ?? []), [catalog]);
  const dirty = hasChanges(catalog, fields, drafts);

  useEffect(() => {
    if (!dirty) return undefined;
    const warn = (event: BeforeUnloadEvent) => event.preventDefault();
    window.addEventListener('beforeunload', warn);
    return () => window.removeEventListener('beforeunload', warn);
  }, [dirty]);

  const setDraft = useCallback((path: string, value: SettingsDraft | undefined) => {
    setDrafts(current => {
      const next = { ...current };
      if (value === undefined) delete next[path];
      else next[path] = value;
      return next;
    });
  }, []);

  const onSaved = useCallback((fresh: SettingsCatalog, saved: SettingsField[]) => {
    queryClient.setQueryData(QUERY_KEY, fresh);
    setDrafts(current => {
      const next = { ...current };
      for (const field of saved) delete next[field.path];
      return next;
    });
    setRestartPending(current => new Set([...current, ...saved.filter(field => field.restart_required).map(field => field.path)]));
  }, [queryClient]);

  function toggle(groupId: string) {
    setOpenGroups(current => {
      const next = new Set(current);
      if (next.has(groupId)) next.delete(groupId);
      else next.add(groupId);
      return next;
    });
  }

  const cards = (catalog?.groups ?? []).map(group => {
    const groupFields = fields.filter(field => field.group === group.id);
    return { group, fields: groupFields, visible: groupFields.filter(field => matchesSearch(field, search)) };
  }).filter(card => card.fields.length > 0);
  const shown = search ? cards.filter(card => card.visible.length > 0) : cards;

  return <section className="space-y-4" aria-labelledby="settings-catalog-title">
    <div className="flex flex-wrap items-end justify-between gap-3">
      <div className="min-w-0">
        <h2 id="settings-catalog-title" className="text-lg font-semibold text-white">Bot-Konfiguration</h2>
        <p className="mt-1 text-sm text-text-secondary">Angezeigt wird der gespeicherte Stand. Speichern startet keinen Dienst neu.</p>
      </div>
      <button type="button" className="admin-button admin-button-secondary" disabled={query.isFetching} onClick={() => void query.refetch()}>
        <RefreshCw size={16} aria-hidden="true" /> Stand neu laden
      </button>
    </div>
    {query.isPending && <p role="status" className="text-sm text-text-secondary">Einstellungen werden geladen …</p>}
    {query.error && <p role="alert" className="panel-card rounded-2xl p-5 text-sm">{query.error.message}</p>}
    {catalog && <>
      <div className="flex flex-wrap items-center gap-3">
        <div className="min-w-0 flex-1 basis-64">
          <SearchInput placeholder="Einstellung suchen: Name, Pfad oder Beschreibung …" onDebouncedChange={setSearch} />
        </div>
        <p className="text-sm text-text-secondary">{fields.length} Einstellungen in {cards.length} Bereichen</p>
      </div>
      {shown.length === 0 && <p className="panel-card rounded-2xl p-5 text-sm text-text-secondary">Keine passenden Einstellungen.</p>}
      <div className="space-y-3">
        {shown.map(card => <SettingsGroupCard key={card.group.id} group={card.group} fields={card.fields} visible={card.visible}
          catalog={catalog} drafts={drafts} open={Boolean(search) || openGroups.has(card.group.id)} restartPending={restartPending}
          reloading={query.isFetching} onToggle={() => toggle(card.group.id)} onDraft={setDraft} onSaved={onSaved}
          onReload={() => void query.refetch()} />)}
      </div>
    </>}
  </section>;
}
