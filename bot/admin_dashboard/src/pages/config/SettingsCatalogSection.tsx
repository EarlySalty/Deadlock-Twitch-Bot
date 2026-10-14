import { useCallback, useEffect, useMemo, useState } from 'react';
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { RefreshCw } from 'lucide-react';
import { fetchSettingsCatalog, saveSettingsCatalog } from '@/api/client';
import {
  isDraftChanged,
  matchesSearch,
  uniqueFields,
  withoutSubmitted,
  type SettingsCatalog,
  type SettingsDraft,
  type SettingsField,
} from '@/api/settingsCatalog';
import { SearchInput } from '@/components/shared/SearchInput';
import { SettingsGroupCard } from './SettingsGroupCard';
import { collectChanges, failureFeedback, savedFeedback, type CardFeedback, type GroupSave } from './settingsSave';

const QUERY_KEY = ['operating-settings-catalog'];

function hasChanges(catalog: SettingsCatalog | undefined, fields: SettingsField[], drafts: Record<string, SettingsDraft>) {
  if (!catalog) return false;
  return fields.some(field => field.path in drafts && isDraftChanged(field, drafts[field.path], catalog.values[field.path]));
}

function toggled(current: Set<string>, id: string) {
  const next = new Set(current);
  if (next.has(id)) next.delete(id);
  else next.add(id);
  return next;
}

export function SettingsCatalogSection() {
  const queryClient = useQueryClient();
  const query = useQuery({ queryKey: QUERY_KEY, queryFn: fetchSettingsCatalog, refetchOnWindowFocus: false, refetchOnReconnect: false });
  const [drafts, setDrafts] = useState<Record<string, SettingsDraft>>({});
  const [search, setSearch] = useState('');
  const [openGroups, setOpenGroups] = useState<Set<string>>(new Set());
  const [restartPending, setRestartPending] = useState<Set<string>>(new Set());
  const [feedback, setFeedback] = useState<Record<string, CardFeedback>>({});
  const catalog = query.data;
  const fields = useMemo(() => uniqueFields(catalog?.fields ?? []), [catalog]);
  const dirty = hasChanges(catalog, fields, drafts);

  const save = useMutation({
    mutationFn: async (request: GroupSave) => {
      await queryClient.cancelQueries({ queryKey: QUERY_KEY });
      return saveSettingsCatalog({ revision: request.revision, changes: request.changes });
    },
    onSuccess: (fresh, request) => {
      queryClient.setQueryData(QUERY_KEY, fresh);
      setDrafts(current => withoutSubmitted(current, request.submitted));
      const restartPaths = fields.filter(field => field.restart_required && field.path in request.submitted).map(field => field.path);
      setRestartPending(current => new Set([...current, ...restartPaths]));
      setFeedback(current => ({ ...current, [request.groupId]: savedFeedback(request.changes.length) }));
    },
    onError: (error, request) => {
      const paths = new Set(fields.filter(field => field.group === request.groupId).map(field => field.path));
      setFeedback(current => ({ ...current, [request.groupId]: failureFeedback(error, paths) }));
    },
  });
  const busy = save.isPending || query.isFetching;

  useEffect(() => {
    if (!dirty) return undefined;
    const warn = (event: BeforeUnloadEvent) => event.preventDefault();
    window.addEventListener('beforeunload', warn);
    return () => window.removeEventListener('beforeunload', warn);
  }, [dirty]);

  const setDraft = useCallback((groupId: string, path: string, value: SettingsDraft | undefined) => {
    setDrafts(current => {
      const next = { ...current };
      if (value === undefined) delete next[path];
      else next[path] = value;
      return next;
    });
    setFeedback(current => {
      const entry = current[groupId];
      if (!entry || entry.status?.kind === 'conflict') return current;
      const serverErrors = { ...entry.serverErrors };
      delete serverErrors[path];
      return { ...current, [groupId]: { status: null, serverErrors } };
    });
  }, []);

  function reload() {
    if (save.isPending) return;
    void query.refetch();
  }

  function saveGroup(groupId: string, groupFields: SettingsField[]) {
    if (!catalog || busy) return;
    const { changes, errors, submitted } = collectChanges(groupFields, catalog, drafts);
    if (changes.length === 0 || Object.keys(errors).length > 0) return;
    setFeedback(current => ({ ...current, [groupId]: { status: null, serverErrors: {} } }));
    save.mutate({ groupId, revision: catalog.revision, changes, submitted });
  }

  function discardGroup(groupId: string, groupFields: SettingsField[]) {
    setDrafts(current => {
      const next = { ...current };
      for (const field of groupFields) delete next[field.path];
      return next;
    });
    setFeedback(current => ({ ...current, [groupId]: { status: null, serverErrors: {} } }));
  }

  function reloadAfterConflict(groupId: string) {
    setFeedback(current => ({ ...current, [groupId]: { status: null, serverErrors: {} } }));
    reload();
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
      <button type="button" className="admin-button admin-button-secondary" disabled={busy} onClick={reload}>
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
          feedback={feedback[card.group.id]} saving={save.isPending && save.variables?.groupId === card.group.id} busy={busy}
          onToggle={() => setOpenGroups(current => toggled(current, card.group.id))}
          onDraft={(path, value) => setDraft(card.group.id, path, value)}
          onDiscard={() => discardGroup(card.group.id, card.fields)}
          onSave={() => saveGroup(card.group.id, card.fields)}
          onReload={() => reloadAfterConflict(card.group.id)} />)}
      </div>
    </>}
  </section>;
}
