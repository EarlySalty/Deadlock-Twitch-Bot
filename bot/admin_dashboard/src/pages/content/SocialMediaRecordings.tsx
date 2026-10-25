import { useEffect, useRef, useState } from 'react';
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { MousePointerClick, RefreshCw, Trash2 } from 'lucide-react';
import { PageHeader } from '@/components/layout/PageHeader';
import { Section } from '@/components/layout/Section';
import { ConfirmDialog } from '@/components/shared/ConfirmDialog';
import { EmptyState } from '@/components/shared/EmptyState';
import {
  deleteSocialMediaRecording,
  fetchSocialMediaRecordingEvents,
  fetchSocialMediaRecordings,
  type SocialMediaRecordingSession,
} from '@/api/client';
import { formatBytes, formatDateTime } from '@/utils/formatters';

const LIST_KEY = ['social-media-recordings'];
const SPEEDS = [1, 2, 4, 8];

type PlayerInstance = { $destroy: () => void };

function asDestroyable(instance: object): PlayerInstance {
  const destroy: unknown = Reflect.get(instance, '$destroy');
  return { $destroy: typeof destroy === 'function' ? () => destroy.call(instance) : () => undefined };
}

function formatClock(totalSeconds: number): string {
  const seconds = Math.max(0, Math.round(totalSeconds));
  const minutes = Math.floor(seconds / 60);
  return `${minutes}:${String(seconds % 60).padStart(2, '0')}`;
}

function viewportLabel(session: SocialMediaRecordingSession): string {
  return session.viewport_width && session.viewport_height
    ? `${session.viewport_width}×${session.viewport_height}`
    : '–';
}

async function mountPlayer(target: HTMLElement, events: unknown[]): Promise<PlayerInstance> {
  const [{ default: Player }] = await Promise.all([
    import('rrweb-player'),
    import('rrweb-player/dist/style.css'),
  ]);
  const width = Math.max(320, target.clientWidth - 16);
  const player = new Player({
    target,
    props: {
      events: events as never,
      width,
      height: Math.round(width * 0.56),
      autoPlay: false,
      skipInactive: true,
      speedOption: SPEEDS,
      showController: true,
      mouseTail: { duration: 800, lineCap: 'round', lineWidth: 3, strokeStyle: '#c5a059' },
    },
  });
  return asDestroyable(player);
}

function ReplayPlayer({ sessionId }: { sessionId: string }) {
  const containerRef = useRef<HTMLDivElement | null>(null);
  const [mountError, setMountError] = useState<string | null>(null);
  const events = useQuery({
    queryKey: ['social-media-recording-events', sessionId],
    queryFn: () => fetchSocialMediaRecordingEvents(sessionId),
    retry: false,
    refetchOnWindowFocus: false,
    staleTime: Infinity,
  });

  useEffect(() => {
    const target = containerRef.current;
    const data = events.data;
    if (!target || !data) return;
    if (data.length < 2) {
      setMountError('Diese Aufnahme enthält zu wenige Ereignisse für eine Wiedergabe.');
      return;
    }
    let player: PlayerInstance | null = null;
    let cancelled = false;
    setMountError(null);
    mountPlayer(target, data)
      .then((instance) => {
        if (cancelled) instance.$destroy();
        else player = instance;
      })
      .catch((error: unknown) => setMountError(error instanceof Error ? error.message : 'Player konnte nicht geladen werden.'));
    return () => {
      cancelled = true;
      player?.$destroy();
      target.replaceChildren();
    };
  }, [events.data]);

  if (events.isLoading) return <p className="text-sm text-text-secondary">Aufnahme wird geladen …</p>;
  if (events.isError) {
    return <p role="alert" className="text-sm text-danger">{events.error instanceof Error ? events.error.message : 'Aufnahme konnte nicht geladen werden.'}</p>;
  }
  return (
    <div className="space-y-3">
      {mountError ? <p role="alert" className="text-sm text-danger">{mountError}</p> : null}
      <div ref={containerRef} className="overflow-hidden rounded-2xl bg-white/[0.03] p-2" />
      <p className="text-xs text-text-secondary">{events.data?.length ?? 0} Ereignisse. Eingaben sind in der Aufnahme maskiert.</p>
    </div>
  );
}

function SessionRow({ session, selected, onSelect, onDelete }: {
  session: SocialMediaRecordingSession;
  selected: boolean;
  onSelect: () => void;
  onDelete: () => void;
}) {
  return (
    <tr className={selected ? 'bg-white/[0.06]' : 'hover:bg-white/[0.03]'}>
      <td className="px-3 py-2">
        <button type="button" onClick={onSelect} className="font-medium text-white underline-offset-2 hover:underline">
          {session.twitch_login}
        </button>
      </td>
      <td className="px-3 py-2 text-text-secondary">{formatDateTime(session.started_at)}</td>
      <td className="px-3 py-2 text-text-secondary">{formatClock(session.duration_seconds)}</td>
      <td className="px-3 py-2 text-text-secondary">{formatBytes(session.total_bytes)}</td>
      <td className="px-3 py-2 text-text-secondary">{viewportLabel(session)}</td>
      <td className="px-3 py-2 text-right">
        <button type="button" onClick={onDelete} aria-label={`Aufnahme von ${session.twitch_login} löschen`} className="rounded-lg p-1.5 text-text-secondary hover:bg-danger/10 hover:text-danger">
          <Trash2 className="h-4 w-4" />
        </button>
      </td>
    </tr>
  );
}

function SessionTable({ sessions, selectedId, onSelect, onDelete }: {
  sessions: SocialMediaRecordingSession[];
  selectedId: string | null;
  onSelect: (id: string) => void;
  onDelete: (session: SocialMediaRecordingSession) => void;
}) {
  return (
    <div className="overflow-x-auto">
      <table className="w-full text-left text-sm">
        <thead className="text-xs uppercase tracking-wide text-text-secondary">
          <tr>
            <th className="px-3 py-2">Streamer</th>
            <th className="px-3 py-2">Beginn</th>
            <th className="px-3 py-2">Dauer</th>
            <th className="px-3 py-2">Größe</th>
            <th className="px-3 py-2">Fenster</th>
            <th className="px-3 py-2" />
          </tr>
        </thead>
        <tbody className="divide-y divide-white/5">
          {sessions.map((session) => (
            <SessionRow
              key={session.id}
              session={session}
              selected={session.id === selectedId}
              onSelect={() => onSelect(session.id)}
              onDelete={() => onDelete(session)}
            />
          ))}
        </tbody>
      </table>
    </div>
  );
}

export default function SocialMediaRecordingsPage() {
  const queryClient = useQueryClient();
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [pendingDelete, setPendingDelete] = useState<SocialMediaRecordingSession | null>(null);
  const sessions = useQuery({ queryKey: LIST_KEY, queryFn: fetchSocialMediaRecordings, retry: false, refetchOnWindowFocus: false });
  const removal = useMutation({
    mutationFn: (id: string) => deleteSocialMediaRecording(id),
    onSuccess: (_result, id) => {
      if (selectedId === id) setSelectedId(null);
      setPendingDelete(null);
      void queryClient.invalidateQueries({ queryKey: LIST_KEY });
    },
  });
  const items = sessions.data ?? [];

  return (
    <section className="space-y-6">
      <PageHeader
        title="Nutzungsaufnahmen"
        description="Aufzeichnungen, wie Streamer mit ihrer Einwilligung den Social-Media-Manager benutzen. Eingaben sind maskiert, Aufnahmen werden nach 30 Tagen automatisch gelöscht."
        primaryAction={
          <button type="button" onClick={() => void sessions.refetch()} className="inline-flex items-center gap-2 rounded-xl border border-white/10 px-3 py-2 text-sm text-white hover:bg-white/[0.06]">
            <RefreshCw className="h-4 w-4" /> Neu laden
          </button>
        }
      />
      {selectedId ? (
        <Section title="Wiedergabe" hint="Abspielen, Pause und Tempo unten im Player. Die Spur zeigt die Mausbewegung, Klicks blinken am Zeiger auf.">
          <ReplayPlayer key={selectedId} sessionId={selectedId} />
        </Section>
      ) : null}
      <Section title="Sitzungen" hint="Neueste zuerst, höchstens 500.">
        {sessions.isLoading ? <p className="text-sm text-text-secondary">Wird geladen …</p> : null}
        {sessions.isError ? (
          <p role="alert" className="text-sm text-danger">{sessions.error instanceof Error ? sessions.error.message : 'Liste konnte nicht geladen werden.'}</p>
        ) : null}
        {!sessions.isLoading && !sessions.isError && items.length === 0 ? (
          <EmptyState icon={MousePointerClick} title="Noch keine Aufnahmen" description="Sobald ein Streamer die Aufzeichnung erlaubt und den Social-Media-Manager nutzt, erscheint die Sitzung hier." />
        ) : null}
        {items.length > 0 ? (
          <SessionTable sessions={items} selectedId={selectedId} onSelect={setSelectedId} onDelete={setPendingDelete} />
        ) : null}
        {removal.isError ? <p role="alert" className="mt-3 text-sm text-danger">Löschen hat nicht geklappt.</p> : null}
      </Section>
      <ConfirmDialog
        open={pendingDelete !== null}
        title="Aufnahme löschen?"
        description={pendingDelete ? `Die Aufnahme von ${pendingDelete.twitch_login} vom ${formatDateTime(pendingDelete.started_at)} wird endgültig gelöscht.` : ''}
        confirmLabel="Löschen"
        tone="danger"
        busy={removal.isPending}
        onConfirm={() => pendingDelete && removal.mutate(pendingDelete.id)}
        onCancel={() => setPendingDelete(null)}
      />
    </section>
  );
}
