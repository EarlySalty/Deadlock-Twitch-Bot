import { useEffect, useMemo, useRef, useState } from 'react';
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import {
  ArrowLeft,
  ArrowLeftRight,
  Camera,
  Copy,
  Eye,
  EyeOff,
  Loader2,
  Plus,
  Radio,
  RotateCw,
  Trash2,
  Video,
} from 'lucide-react';
import {
  createUplinkCastScene,
  createUplinkCastSource,
  deleteUplinkCastScene,
  deleteUplinkCastSource,
  fetchUplinkCastStudio,
  rotateUplinkCastSource,
  selectUplinkCastPreview,
  selectUplinkCastProgram,
} from '@/api/uplinkCast';
import type {
  UplinkCastScene,
  UplinkCastSource,
  UplinkCastSourceKind,
  UplinkCastStudio,
} from '@/api/uplinkCast';
import { PREVIEW_UPLINK_ROUTE } from '@/preview/routes';
import { Rise } from '@/motion/Rise';

const STUDIO_QUERY = ['uplink-cast-studio'] as const;

function formatBitrate(value: number | null | undefined) {
  if (!value || !Number.isFinite(value)) return null;
  return value >= 1000 ? `${(value / 1000).toFixed(1)} Mbit/s` : `${Math.round(value)} kbit/s`;
}

function sourceState(source: UplinkCastSource) {
  if (!source.enabled) return { label: 'Deaktiviert', className: 'text-text-secondary' };
  if (!source.session?.active) return { label: 'Offline', className: 'text-text-secondary' };
  if (source.session.error) return { label: 'Fehler', className: 'text-warning' };
  if (source.session.cast_role === 'program') return { label: 'PROGRAM', className: 'text-warning' };
  if (source.session.cast_role === 'preview') return { label: 'PREVIEW', className: 'text-success' };
  return { label: 'Standby', className: 'text-primary' };
}

function SourceSignal({ source }: { source: UplinkCastSource }) {
  const status = sourceState(source);
  const bitrate = formatBitrate(source.session?.input_bitrate_kbps);
  return (
    <div className="flex min-w-0 items-center gap-2 text-[11px]">
      <span className={`font-semibold ${status.className}`}>{status.label}</span>
      {source.session?.input_codec ? (
        <span className="rounded-md border border-border/70 bg-black/20 px-1.5 py-0.5 uppercase text-text-secondary">
          {source.session.input_codec}
        </span>
      ) : null}
      {bitrate ? <span className="truncate text-text-secondary">{bitrate}</span> : null}
    </div>
  );
}

type CastPreviewState = 'connecting' | 'live' | 'offline' | 'unsupported' | 'error';

type CastPreviewFeed = {
  sourceId: number;
  socket: WebSocket | null;
  decoder: VideoDecoder | null;
  canvases: Set<HTMLCanvasElement>;
  listeners: Set<(state: CastPreviewState) => void>;
  state: CastPreviewState;
  waitingForKeyframe: boolean;
  closed: boolean;
  retryTimer: ReturnType<typeof setTimeout> | null;
};

const castPreviewFeeds = new Map<number, CastPreviewFeed>();

function setCastPreviewState(feed: CastPreviewFeed, state: CastPreviewState) {
  if (feed.state === state) return;
  feed.state = state;
  for (const listener of feed.listeners) listener(state);
}

function closeCastPreviewFeed(feed: CastPreviewFeed) {
  feed.closed = true;
  if (feed.retryTimer) clearTimeout(feed.retryTimer);
  feed.retryTimer = null;
  feed.socket?.close();
  feed.socket = null;
  if (feed.decoder) {
    try {
      feed.decoder.close();
    } catch {
      // Der Decoder kann nach einem asynchronen WebCodecs-Fehler bereits geschlossen sein.
    }
    feed.decoder = null;
  }
  castPreviewFeeds.delete(feed.sourceId);
}

function drawCastPreview(feed: CastPreviewFeed, frame: VideoFrame) {
  try {
    const width = frame.displayWidth || frame.codedWidth;
    const height = frame.displayHeight || frame.codedHeight;
    for (const canvas of feed.canvases) {
      if (canvas.width !== width) canvas.width = width;
      if (canvas.height !== height) canvas.height = height;
      const context = canvas.getContext('2d', { alpha: false });
      context?.drawImage(frame, 0, 0, width, height);
    }
    setCastPreviewState(feed, 'live');
  } finally {
    frame.close();
  }
}

function h264CodecFromAvcConfig(description: Uint8Array) {
  if (description.length < 4 || description[0] !== 1) return null;
  const hex = (value: number) => value.toString(16).padStart(2, '0');
  return `avc1.${hex(description[1])}${hex(description[2])}${hex(description[3])}`;
}

function configureCastPreviewDecoder(feed: CastPreviewFeed, payload: Uint8Array) {
  if (typeof VideoDecoder === 'undefined' || typeof EncodedVideoChunk === 'undefined') {
    setCastPreviewState(feed, 'unsupported');
    return;
  }
  // Der Server entfernt den RTMP-/E-RTMP-Rahmen. Für H.264 kommt hier direkt
  // AVCDecoderConfigurationRecord bzw. bei Frames das AVCC-Sample an.
  const description = payload.slice();
  const codec = h264CodecFromAvcConfig(description);
  if (!codec) {
    setCastPreviewState(feed, 'unsupported');
    return;
  }
  if (feed.decoder) {
    try {
      feed.decoder.close();
    } catch {
      // Siehe closeCastPreviewFeed.
    }
  }
  try {
    feed.decoder = new VideoDecoder({
      output: (frame) => drawCastPreview(feed, frame),
      error: () => setCastPreviewState(feed, 'error'),
    });
    feed.decoder.configure({
      codec,
      description,
      hardwareAcceleration: 'prefer-hardware',
      optimizeForLatency: true,
    });
    feed.waitingForKeyframe = true;
    setCastPreviewState(feed, 'connecting');
  } catch {
    feed.decoder = null;
    setCastPreviewState(feed, 'unsupported');
  }
}

function consumeCastPreviewPacket(feed: CastPreviewFeed, buffer: ArrayBuffer) {
  if (buffer.byteLength < 16) {
    setCastPreviewState(feed, 'error');
    return;
  }
  const view = new DataView(buffer);
  if (view.getUint8(0) !== 1) {
    setCastPreviewState(feed, 'error');
    return;
  }
  const kind = view.getUint8(1);
  const codec = view.getUint8(2);
  const keyframe = view.getUint8(3) === 1;
  const ptsMs = Number(view.getBigInt64(8, false));
  const payload = new Uint8Array(buffer, 16);

  if (codec !== 1) {
    setCastPreviewState(feed, 'unsupported');
    return;
  }
  if (kind === 0) {
    configureCastPreviewDecoder(feed, payload);
    return;
  }
  if (kind === 2) {
    feed.waitingForKeyframe = true;
    return;
  }
  if (kind !== 1 || !feed.decoder || feed.decoder.state !== 'configured') return;
  if (feed.waitingForKeyframe && !keyframe) return;
  if (keyframe) feed.waitingForKeyframe = false;
  if (payload.length === 0) return;

  try {
    feed.decoder.decode(new EncodedVideoChunk({
      type: keyframe ? 'key' : 'delta',
      timestamp: Math.max(0, ptsMs) * 1000,
      data: payload,
    }));
  } catch {
    setCastPreviewState(feed, 'error');
  }
}

function openCastPreviewSocket(feed: CastPreviewFeed) {
  if (feed.closed || feed.socket || typeof WebSocket === 'undefined') return;
  const protocol = window.location.protocol === 'https:' ? 'wss:' : 'ws:';
  setCastPreviewState(feed, 'connecting');
  const socket = new WebSocket(
    `${protocol}//${window.location.host}/twitch/api/v2/uplink/cast/sources/${feed.sourceId}/preview`,
  );
  socket.binaryType = 'arraybuffer';
  feed.socket = socket;
  socket.onmessage = (event) => {
    if (!feed.closed && event.data instanceof ArrayBuffer) consumeCastPreviewPacket(feed, event.data);
  };
  socket.onerror = () => {
    if (!feed.closed) setCastPreviewState(feed, 'error');
  };
  socket.onclose = () => {
    if (feed.socket === socket) feed.socket = null;
    if (feed.closed || feed.state === 'unsupported') return;
    setCastPreviewState(feed, 'offline');
    if (feed.retryTimer || feed.canvases.size === 0) return;
    feed.retryTimer = setTimeout(() => {
      feed.retryTimer = null;
      openCastPreviewSocket(feed);
    }, 1_500);
  };
}

function startCastPreviewFeed(sourceId: number) {
  const feed: CastPreviewFeed = {
    sourceId,
    socket: null,
    decoder: null,
    canvases: new Set(),
    listeners: new Set(),
    state: 'connecting',
    waitingForKeyframe: true,
    closed: false,
    retryTimer: null,
  };
  castPreviewFeeds.set(sourceId, feed);
  if (typeof WebSocket === 'undefined') {
    feed.state = 'unsupported';
    return feed;
  }
  openCastPreviewSocket(feed);
  return feed;
}

function useCastPreview(source: UplinkCastSource | undefined, enabled: boolean) {
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const active = Boolean(source?.session?.active);
  const sessionId = source?.session?.id ?? null;
  const codec = source?.session?.input_codec ?? null;
  const [state, setState] = useState<CastPreviewState>(active ? 'connecting' : 'offline');

  useEffect(() => {
    const canvas = canvasRef.current;
    if (!source || !canvas || !enabled || !active) {
      setState('offline');
      return;
    }
    if (codec && codec !== 'h264') {
      setState('unsupported');
      return;
    }
    const feed = castPreviewFeeds.get(source.source_id) ?? startCastPreviewFeed(source.source_id);
    feed.canvases.add(canvas);
    feed.listeners.add(setState);
    setState(feed.state);
    return () => {
      feed.canvases.delete(canvas);
      feed.listeners.delete(setState);
      if (feed.canvases.size === 0 && feed.listeners.size === 0) closeCastPreviewFeed(feed);
    };
  }, [active, codec, enabled, sessionId, source?.source_id]);

  return { canvasRef, state };
}

function CastPreviewCanvas({ source, enabled, className = '' }: {
  source?: UplinkCastSource;
  enabled: boolean;
  className?: string;
}) {
  const { canvasRef, state } = useCastPreview(source, enabled);
  const message = !source?.session?.active
    ? 'Offline'
    : state === 'unsupported'
      ? source.session.input_codec && source.session.input_codec !== 'h264'
        ? `${source.session.input_codec.toUpperCase()}-Preview folgt später`
        : 'Browser kann diese H.264-Vorschau nicht dekodieren'
      : state === 'error'
        ? 'Vorschau unterbrochen'
        : state === 'connecting'
          ? 'Warte auf Keyframe'
          : null;
  return (
    <div className={`relative overflow-hidden bg-black ${className}`}>
      <canvas ref={canvasRef} className="h-full w-full object-contain" aria-label={source ? `Vorschau ${source.label}` : 'Vorschau'} />
      {message ? (
        <div className="absolute inset-0 flex items-center justify-center bg-black/55 px-3 text-center text-[11px] font-medium text-white/65">
          {message}
        </div>
      ) : null}
    </div>
  );
}

function PreviewSurface({
  title,
  scene,
  source,
  live,
  previewAvailable,
}: {
  title: string;
  scene?: UplinkCastScene;
  source?: UplinkCastSource;
  live?: boolean;
  previewAvailable: boolean;
}) {
  return (
    <section
      className={`overflow-hidden rounded-2xl border bg-black/45 ${
        live ? 'border-warning/55 shadow-sm' : 'border-border'
      }`}
    >
      <div className="flex items-center justify-between gap-3 border-b border-border/70 px-4 py-2.5">
        <div className="flex min-w-0 items-center gap-2">
          <span className={`h-2 w-2 rounded-full ${live ? 'bg-warning' : 'bg-success'}`} />
          <span className="text-xs font-bold uppercase tracking-[0.16em] text-white">{title}</span>
          {live ? (
            <span className="rounded-md bg-warning/15 px-1.5 py-0.5 text-[10px] font-bold text-warning">
              LIVE
            </span>
          ) : null}
        </div>
        {source ? <SourceSignal source={source} /> : null}
      </div>
      <div className="relative aspect-video bg-[radial-gradient(circle_at_center,rgba(255,255,255,0.06),transparent_55%)]">
        {source ? (
          <CastPreviewCanvas source={source} enabled={previewAvailable} className="absolute inset-0 h-full w-full" />
        ) : (
          <div className="absolute inset-0 flex flex-col items-center justify-center gap-2 px-6 text-center">
            <Radio className="h-7 w-7 text-white/25" />
            <div className="text-sm font-semibold text-white/65">Keine Szene gewählt</div>
          </div>
        )}
      </div>
      <div className="flex items-center justify-between gap-3 px-4 py-3">
        <div className="min-w-0">
          <div className="truncate text-sm font-semibold text-white">{scene?.name ?? 'Keine Szene'}</div>
          <div className="truncate text-xs text-text-secondary">{source?.label ?? 'Keine Quelle zugeordnet'}</div>
        </div>
        {source?.session?.active ? (
          <span className="shrink-0 rounded-full border border-success/30 bg-success/10 px-2.5 py-1 text-[11px] font-semibold text-success">
            Eingang verbunden
          </span>
        ) : null}
      </div>
    </section>
  );
}

function SecretField({ label, value, hidden = false }: { label: string; value: string; hidden?: boolean }) {
  const [visible, setVisible] = useState(false);
  const masked = hidden && !visible;
  return (
    <div className="space-y-1.5">
      <div className="text-[10px] font-semibold uppercase tracking-[0.14em] text-text-secondary">{label}</div>
      <div className="flex min-w-0 items-center gap-2 rounded-xl border border-border bg-background/60 px-3 py-2">
        <code className="min-w-0 flex-1 truncate text-xs text-white">
          {masked ? '••••••••••••••••••••••••••••••••' : value}
        </code>
        {hidden ? (
          <button
            type="button"
            onClick={() => setVisible((old) => !old)}
            className="rounded-md p-1.5 text-text-secondary hover:bg-white/5 hover:text-white"
            aria-label={visible ? 'Verbergen' : 'Anzeigen'}
          >
            {visible ? <EyeOff className="h-3.5 w-3.5" /> : <Eye className="h-3.5 w-3.5" />}
          </button>
        ) : null}
        <button
          type="button"
          onClick={() => navigator.clipboard?.writeText(value)}
          className="rounded-md p-1.5 text-text-secondary hover:bg-white/5 hover:text-white"
          aria-label={`${label} kopieren`}
        >
          <Copy className="h-3.5 w-3.5" />
        </button>
      </div>
    </div>
  );
}

function SourceManager({ studio }: { studio: UplinkCastStudio }) {
  const queryClient = useQueryClient();
  const [label, setLabel] = useState('');
  const [kind, setKind] = useState<UplinkCastSourceKind>('pov');
  const createSource = useMutation({
    mutationFn: createUplinkCastSource,
    onSuccess: () => {
      setLabel('');
      queryClient.invalidateQueries({ queryKey: STUDIO_QUERY });
    },
  });
  const rotateSource = useMutation({
    mutationFn: rotateUplinkCastSource,
    onSuccess: () => queryClient.invalidateQueries({ queryKey: STUDIO_QUERY }),
  });
  const removeSource = useMutation({
    mutationFn: deleteUplinkCastSource,
    onSuccess: () => queryClient.invalidateQueries({ queryKey: STUDIO_QUERY }),
  });

  return (
    <Rise className="panel-card space-y-4 rounded-2xl p-4 md:p-5">
      <div className="flex flex-wrap items-start justify-between gap-3">
        <div>
          <div className="text-[10px] font-semibold uppercase tracking-[0.16em] text-primary">Eingänge</div>
          <h2 className="mt-0.5 text-base font-bold text-white">POVs und Kameras</h2>
          <p className="mt-1 text-xs text-text-secondary">
            Jede Person bekommt einen eigenen Schlüssel. Standby bleibt verbunden, startet aber keinen Plattform-Encode.
          </p>
        </div>
        <span className="rounded-full border border-border bg-background/60 px-2.5 py-1 text-[11px] font-semibold text-text-secondary">
          {studio.sources.length}/{studio.limits.sources}
        </span>
      </div>

      <div className="grid gap-2 sm:grid-cols-[minmax(0,1fr)_8rem_auto]">
        <input
          value={label}
          onChange={(event) => setLabel(event.target.value)}
          placeholder="z. B. Team A POV"
          maxLength={80}
          className="min-h-10 rounded-xl border border-border bg-background/65 px-3 text-sm text-white outline-none focus:border-primary/60"
        />
        <select
          value={kind}
          onChange={(event) => setKind(event.target.value as UplinkCastSourceKind)}
          className="min-h-10 rounded-xl border border-border bg-background/65 px-3 text-sm text-white outline-none"
        >
          <option value="pov">POV / OBS</option>
          <option value="camera">Kamera</option>
        </select>
        <button
          type="button"
          disabled={!label.trim() || createSource.isPending || studio.sources.length >= studio.limits.sources}
          onClick={() => createSource.mutate({ label: label.trim(), kind })}
          className="inline-flex min-h-10 items-center justify-center gap-2 rounded-xl bg-primary px-3 text-sm font-semibold text-[#0D0806] disabled:opacity-50"
        >
          {createSource.isPending ? <Loader2 className="h-4 w-4 animate-spin" /> : <Plus className="h-4 w-4" />}
          Quelle
        </button>
      </div>

      {createSource.isError ? (
        <p role="alert" className="text-xs text-warning">Quelle konnte nicht angelegt werden.</p>
      ) : null}

      <div className="space-y-3">
        {studio.sources.map((source) => (
          <div key={source.source_id} className="rounded-xl border border-border bg-background/45 p-3">
            <div className="flex flex-wrap items-start justify-between gap-3">
              <div className="min-w-0">
                <div className="flex items-center gap-2">
                  {source.kind === 'camera' ? <Camera className="h-4 w-4 text-primary" /> : <Video className="h-4 w-4 text-primary" />}
                  <span className="truncate text-sm font-semibold text-white">{source.label}</span>
                </div>
                <div className="mt-1"><SourceSignal source={source} /></div>
              </div>
              <div className="flex items-center gap-1">
                <button
                  type="button"
                  disabled={Boolean(source.session?.active) || rotateSource.isPending}
                  onClick={() => rotateSource.mutate(source.source_id)}
                  title={source.session?.active ? 'Quelle zuerst beenden' : 'Schlüssel neu erzeugen'}
                  className="rounded-lg border border-border p-2 text-text-secondary hover:text-white disabled:opacity-35"
                >
                  <RotateCw className="h-3.5 w-3.5" />
                </button>
                <button
                  type="button"
                  disabled={Boolean(source.session?.active) || removeSource.isPending}
                  onClick={() => removeSource.mutate(source.source_id)}
                  title={source.session?.active ? 'Quelle zuerst beenden' : 'Quelle löschen'}
                  className="rounded-lg border border-border p-2 text-text-secondary hover:border-warning/40 hover:text-warning disabled:opacity-35"
                >
                  <Trash2 className="h-3.5 w-3.5" />
                </button>
              </div>
            </div>
            <div className="mt-3 grid gap-2 lg:grid-cols-2">
              <SecretField label="Server" value={source.ingest_url} />
              <SecretField label="Streamschlüssel" value={source.ingest_key} hidden />
            </div>
          </div>
        ))}
        {studio.sources.length === 0 ? (
          <div className="rounded-xl border border-dashed border-border px-4 py-5 text-center text-xs text-text-secondary">
            Noch keine Casting-Quelle. Die erste Quelle legt automatisch eine gleichnamige Szene an.
          </div>
        ) : null}
      </div>

      <div className="rounded-xl border border-border bg-black/15 px-3 py-2.5 text-xs text-text-secondary">
        Browser-Direktkamera ohne Room ist als eigener WHIP/WebRTC-Eingang vorgesehen. Der Knopf wird erst freigeschaltet, wenn der Browser-Publish-Pfad authentifiziert und lastbegrenzt nachgewiesen ist.
      </div>
    </Rise>
  );
}

function SceneList({
  studio,
  onPreview,
  previewPending,
}: {
  studio: UplinkCastStudio;
  onPreview: (sceneId: number) => void;
  previewPending: boolean;
}) {
  const queryClient = useQueryClient();
  const [name, setName] = useState('');
  const [sourceId, setSourceId] = useState<number | ''>('');
  const createScene = useMutation({
    mutationFn: createUplinkCastScene,
    onSuccess: () => {
      setName('');
      queryClient.invalidateQueries({ queryKey: STUDIO_QUERY });
    },
  });
  const removeScene = useMutation({
    mutationFn: deleteUplinkCastScene,
    onSuccess: () => queryClient.invalidateQueries({ queryKey: STUDIO_QUERY }),
  });

  return (
    <section className="panel-card flex min-h-0 flex-col overflow-hidden rounded-2xl">
      <div className="border-b border-border px-4 py-3">
        <div className="flex items-center justify-between gap-3">
          <div>
            <div className="text-[10px] font-semibold uppercase tracking-[0.16em] text-primary">OBS-Logik</div>
            <h2 className="text-base font-bold text-white">Szenen</h2>
          </div>
          <span className="text-xs text-text-secondary">{studio.scenes.length}/{studio.limits.scenes}</span>
        </div>
      </div>
      <div className="space-y-1.5 p-2">
        {studio.scenes.map((scene) => {
          const source = studio.sources.find((item) => item.source_id === scene.source_id);
          const isProgram = studio.program_scene_id === scene.scene_id;
          const isPreview = studio.preview_scene_id === scene.scene_id;
          return (
            <div
              key={scene.scene_id}
              className={`group flex items-center gap-2 rounded-xl border px-2.5 py-2 ${
                isProgram
                  ? 'border-warning/45 bg-warning/10'
                  : isPreview
                    ? 'border-success/40 bg-success/10'
                    : 'border-transparent bg-background/45 hover:border-border'
              }`}
            >
              <button
                type="button"
                disabled={previewPending || isPreview}
                onClick={() => onPreview(scene.scene_id)}
                className="min-w-0 flex-1 text-left disabled:cursor-default"
              >
                <div className="truncate text-sm font-semibold text-white">{scene.name}</div>
                <div className="mt-0.5 truncate text-[11px] text-text-secondary">
                  {source?.label ?? 'Quelle fehlt'}
                </div>
              </button>
              <div className="flex shrink-0 items-center gap-1">
                {isProgram ? <span className="text-[9px] font-bold text-warning">PGM</span> : null}
                {isPreview ? <span className="text-[9px] font-bold text-success">PVW</span> : null}
                {!isProgram && !isPreview ? (
                  <button
                    type="button"
                    onClick={() => removeScene.mutate(scene.scene_id)}
                    className="rounded-md p-1.5 text-text-secondary opacity-0 transition-opacity hover:text-warning group-hover:opacity-100"
                    aria-label={`${scene.name} löschen`}
                  >
                    <Trash2 className="h-3.5 w-3.5" />
                  </button>
                ) : null}
              </div>
            </div>
          );
        })}
      </div>
      <div className="mt-auto space-y-2 border-t border-border p-3">
        <input
          value={name}
          onChange={(event) => setName(event.target.value)}
          placeholder="Neue Szene"
          maxLength={80}
          className="min-h-9 w-full rounded-lg border border-border bg-background/65 px-2.5 text-xs text-white outline-none focus:border-primary/60"
        />
        <div className="flex gap-2">
          <select
            value={sourceId}
            onChange={(event) => setSourceId(event.target.value ? Number(event.target.value) : '')}
            className="min-h-9 min-w-0 flex-1 rounded-lg border border-border bg-background/65 px-2 text-xs text-white"
          >
            <option value="">Quelle wählen</option>
            {studio.sources.map((source) => (
              <option key={source.source_id} value={source.source_id}>{source.label}</option>
            ))}
          </select>
          <button
            type="button"
            disabled={!name.trim() || sourceId === '' || createScene.isPending || studio.scenes.length >= studio.limits.scenes}
            onClick={() => createScene.mutate({ name: name.trim(), source_id: Number(sourceId) })}
            className="rounded-lg border border-border px-2.5 text-white disabled:opacity-40"
            aria-label="Szene anlegen"
          >
            <Plus className="h-4 w-4" />
          </button>
        </div>
      </div>
    </section>
  );
}

function SourceGrid({ studio, onPreview }: { studio: UplinkCastStudio; onPreview: (sceneId: number) => void }) {
  return (
    <section className="panel-card rounded-2xl p-4">
      <div className="mb-3 flex flex-wrap items-center justify-between gap-3">
        <div>
          <div className="text-[10px] font-semibold uppercase tracking-[0.16em] text-primary">Multiview</div>
          <h2 className="text-base font-bold text-white">Quellen</h2>
        </div>
        <span className="text-xs text-text-secondary">
          Nicht ausgewählt: Pakete werden nach Status/Preview-Bedarf verworfen.
        </span>
      </div>
      <div className="grid gap-3 sm:grid-cols-2 xl:grid-cols-3">
        {studio.sources.map((source) => {
          const scene = studio.scenes.find((item) => item.source_id === source.source_id);
          const role = source.session?.cast_role;
          return (
            <button
              key={source.source_id}
              type="button"
              disabled={!scene}
              onClick={() => scene && onPreview(scene.scene_id)}
              className={`overflow-hidden rounded-xl border text-left transition-colors ${
                role === 'program'
                  ? 'border-warning/55 bg-warning/10'
                  : role === 'preview'
                    ? 'border-success/45 bg-success/10'
                    : 'border-border bg-background/45 hover:border-primary/40'
              }`}
            >
              <div className="relative aspect-video bg-black/45">
                <CastPreviewCanvas
                  source={source}
                  enabled={studio.preview.available}
                  className="absolute inset-0 h-full w-full"
                />
                <span className="absolute left-2 top-2 rounded-md bg-black/65 px-1.5 py-0.5 text-[9px] font-bold text-white">
                  {role === 'program' ? 'PROGRAM' : role === 'preview' ? 'PREVIEW' : source.session?.active ? 'STANDBY' : 'OFFLINE'}
                </span>
              </div>
              <div className="space-y-1 px-3 py-2.5">
                <div className="truncate text-sm font-semibold text-white">{source.label}</div>
                <SourceSignal source={source} />
              </div>
            </button>
          );
        })}
      </div>
    </section>
  );
}

export function UplinkCastStudioPage() {
  const queryClient = useQueryClient();
  const { data: studio, isLoading, isError, error } = useQuery({
    queryKey: STUDIO_QUERY,
    queryFn: fetchUplinkCastStudio,
    retry: false,
    refetchInterval: 2_000,
    refetchOnWindowFocus: true,
  });

  const selectPreview = useMutation({
    mutationFn: ({ sceneId, generation }: { sceneId: number; generation: number }) =>
      selectUplinkCastPreview(sceneId, generation),
    onSuccess: (ack) => {
      queryClient.setQueryData<UplinkCastStudio>(STUDIO_QUERY, (old) => old ? {
        ...old,
        preview_scene_id: ack.preview_scene_id,
        program_scene_id: ack.program_scene_id,
        switch_generation: ack.switch_generation,
      } : old);
    },
    onError: () => queryClient.invalidateQueries({ queryKey: STUDIO_QUERY }),
  });
  const takeProgram = useMutation({
    mutationFn: ({ sceneId, generation }: { sceneId: number; generation: number }) =>
      selectUplinkCastProgram(sceneId, generation),
    onSuccess: (ack) => {
      queryClient.setQueryData<UplinkCastStudio>(STUDIO_QUERY, (old) => old ? {
        ...old,
        preview_scene_id: ack.preview_scene_id,
        program_scene_id: ack.program_scene_id,
        switch_generation: ack.switch_generation,
      } : old);
      queryClient.invalidateQueries({ queryKey: STUDIO_QUERY });
    },
    onError: () => queryClient.invalidateQueries({ queryKey: STUDIO_QUERY }),
  });

  const mapping = useMemo(() => {
    if (!studio) return { programScene: undefined, previewScene: undefined, programSource: undefined, previewSource: undefined };
    const programScene = studio.scenes.find((scene) => scene.scene_id === studio.program_scene_id);
    const previewScene = studio.scenes.find((scene) => scene.scene_id === studio.preview_scene_id);
    return {
      programScene,
      previewScene,
      programSource: studio.sources.find((source) => source.source_id === programScene?.source_id),
      previewSource: studio.sources.find((source) => source.source_id === previewScene?.source_id),
    };
  }, [studio]);

  const choosePreview = (sceneId: number) => {
    if (!studio || selectPreview.isPending) return;
    selectPreview.mutate({ sceneId, generation: studio.switch_generation });
  };
  const canTake = Boolean(
    studio?.program_switch.available &&
    studio?.program_switch.persistent_connection &&
    mapping.previewScene &&
    mapping.previewScene.scene_id !== studio?.program_scene_id &&
    mapping.previewSource?.session?.active &&
    !takeProgram.isPending,
  );

  return (
    <div className="space-y-4 md:space-y-5">
      <Rise className="panel-card rounded-2xl p-4 md:p-5">
        <div className="flex flex-wrap items-start justify-between gap-4">
          <div>
            <a href={PREVIEW_UPLINK_ROUTE} className="mb-2 inline-flex items-center gap-1.5 text-xs font-semibold text-text-secondary no-underline hover:text-white">
              <ArrowLeft className="h-3.5 w-3.5" /> Uplink
            </a>
            <div className="text-[10px] font-bold uppercase tracking-[0.18em] text-primary">Casting-Regie</div>
            <h1 className="mt-0.5 text-2xl font-extrabold text-white">Uplink Studio</h1>
            <p className="mt-1 max-w-3xl text-sm text-text-secondary">
              Mehrere POVs kommen gleichzeitig am Server an. Nur Program geht in den Plattformpfad; Standby bleibt verbunden und wird nach dem nötigen Status verworfen.
            </p>
          </div>
          <div className="flex flex-wrap gap-2">
            <span className="rounded-full border border-primary/30 bg-primary/10 px-3 py-1 text-xs font-semibold text-primary">
              {studio?.sources.filter((source) => source.session?.active).length ?? 0}/{studio?.limits.online_sources ?? 0} Eingänge online
            </span>
            <span className={`rounded-full border px-3 py-1 text-xs font-semibold ${
              studio?.program_switch.persistent_connection
                ? 'border-success/30 bg-success/10 text-success'
                : 'border-warning/30 bg-warning/10 text-warning'
            }`}>
              {studio?.program_switch.persistent_connection ? 'Keyframe-Schnitt bereit' : 'Program-Mux gesperrt'}
            </span>
          </div>
        </div>
      </Rise>

      {isLoading ? (
        <div className="panel-card flex items-center gap-2 rounded-2xl p-5 text-sm text-text-secondary">
          <Loader2 className="h-4 w-4 animate-spin" /> Studio wird geladen
        </div>
      ) : null}
      {isError ? (
        <div role="alert" className="panel-card rounded-2xl p-5 text-sm text-warning">
          {error instanceof Error ? error.message : 'Uplink Studio ist gerade nicht erreichbar.'}
        </div>
      ) : null}

      {studio ? (
        <>
          <div className="grid items-start gap-4 xl:grid-cols-[15rem_minmax(0,1fr)]">
            <SceneList studio={studio} onPreview={choosePreview} previewPending={selectPreview.isPending} />
            <div className="space-y-4">
              <div className="grid gap-4 lg:grid-cols-2">
                <PreviewSurface
                  title="Preview"
                  scene={mapping.previewScene}
                  source={mapping.previewSource}
                  previewAvailable={studio.preview.available}
                />
                <PreviewSurface
                  title="Program"
                  scene={mapping.programScene}
                  source={mapping.programSource}
                  live
                  previewAvailable={studio.preview.available}
                />
              </div>

              <div className="flex flex-col items-center gap-2 rounded-2xl border border-border bg-background/35 px-4 py-3 sm:flex-row sm:justify-center">
                <button
                  type="button"
                  disabled={!canTake}
                  onClick={() => mapping.previewScene && takeProgram.mutate({
                    sceneId: mapping.previewScene.scene_id,
                    generation: studio.switch_generation,
                  })}
                  className="inline-flex min-h-11 items-center justify-center gap-2 rounded-xl bg-primary px-5 text-sm font-bold text-[#0D0806] disabled:cursor-not-allowed disabled:opacity-35"
                >
                  {takeProgram.isPending ? <Loader2 className="h-4 w-4 animate-spin" /> : <ArrowLeftRight className="h-4 w-4" />}
                  TAKE / SWAP
                </button>
                <p className={`max-w-2xl text-center text-xs sm:text-left ${
                  studio.program_switch.persistent_connection ? 'text-text-secondary' : 'text-warning'
                }`}>
                  {studio.program_switch.persistent_connection
                    ? 'TAKE schaltet auf dem nächsten Video-Keyframe. Die Plattformverbindung bleibt bestehen; die Quellen müssen dieselben Audio-/Video-Track-Codecs liefern.'
                    : 'Program-Mux ist noch nicht freigegeben.'}
                </p>
              </div>

              <SourceGrid studio={studio} onPreview={choosePreview} />
            </div>
          </div>

          <SourceManager studio={studio} />
        </>
      ) : null}
    </div>
  );
}
