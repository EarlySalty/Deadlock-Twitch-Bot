import { eingangStatus } from '@/uplinkBetrieb';
import type { EingangsSession } from '@/uplinkBetrieb';

export function UplinkEingang({ session, unavailable }: {
  session?: EingangsSession | null;
  unavailable: boolean;
}) {
  const status = eingangStatus(session, unavailable);
  const source = status.observation;
  const fps = source && source.fps_denominator > 0
    ? Number((source.fps_numerator / source.fps_denominator).toFixed(3)) : null;
  return (
    <section aria-label="Uplink-Eingang" className="panel-card space-y-3 rounded-2xl p-5 md:p-6">
      <h2 className="text-lg font-bold text-white" role="status">{status.label}</h2>
      {source ? <>
        <p className="text-sm text-white">
          {source.codec.toUpperCase()} · {source.width}×{source.height}{fps ? ` · ${fps} fps` : ''}
        </p>
        <p className="text-xs text-text-secondary">
          {source.audio.length} Audiospuren: {source.audio.map((audio) =>
            `Spur ${audio.wire_track}: ${audio.codec.toUpperCase()}, ${audio.sample_rate / 1000} kHz, ${audio.channels === 1 ? 'Mono' : audio.channels === 2 ? 'Stereo' : `${audio.channels} Kanäle`}`
          ).join(' · ')}
        </p>
        <p className="text-xs text-text-secondary">Am empfangenen Stream erkannt; keine Freigabe für eine bestimmte Plattformqualität.</p>
      </> : <p className="text-sm text-text-secondary">{unavailable
        ? 'Der aktuelle Dienststatus konnte nicht geladen werden.'
        : 'Codec, Bildgröße und Audiospuren erscheinen nach dem ersten empfangenen Stream.'}</p>}
      {!unavailable && session?.outputs ? <p className="text-xs text-text-secondary">
        {session.active ? 'Laufende Verarbeitung' : 'Letzte Verarbeitung'}: {session.outputs.video_decoders} Video-Decoder · {session.outputs.encode_groups} {session.outputs.encode_groups === 1 ? 'gemeinsame Encodergruppe' : 'gemeinsame Encodergruppen'}
      </p> : null}
      {!unavailable && session?.error ? <p role="alert" className="text-sm text-warning">{session.error}</p> : null}
      {!unavailable && session && !session.active ? <p className="text-xs text-text-secondary">
        {session.ingest_end_reason === 'ExplicitStop' ? 'OBS hat den Stream ausdrücklich beendet.'
          : 'Der Eingang ist beendet. Der lokale Abschluss bestätigt keine Veröffentlichung auf einer Plattform.'}
      </p> : null}
    </section>
  );
}
