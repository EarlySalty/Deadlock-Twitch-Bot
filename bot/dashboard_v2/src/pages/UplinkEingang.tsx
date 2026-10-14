import { eingangStatus } from '@/uplinkBetrieb';
import type { EingangsSession } from '@/uplinkBetrieb';
import { useT } from '@/context/LanguageContext';

function verarbeitungText(t: ReturnType<typeof useT>, aktiv: boolean, decoder: number, gruppen: number): string {
  const werte = { decoder, gruppen };
  if (aktiv) {
    return gruppen === 1
      ? t('Laufende Verarbeitung: {decoder} Video-Decoder · {gruppen} gemeinsame Encodergruppe', werte)
      : t('Laufende Verarbeitung: {decoder} Video-Decoder · {gruppen} gemeinsame Encodergruppen', werte);
  }
  return gruppen === 1
    ? t('Letzte Verarbeitung: {decoder} Video-Decoder · {gruppen} gemeinsame Encodergruppe', werte)
    : t('Letzte Verarbeitung: {decoder} Video-Decoder · {gruppen} gemeinsame Encodergruppen', werte);
}

export function UplinkEingang({ session, unavailable }: {
  session?: EingangsSession | null;
  unavailable: boolean;
}) {
  const t = useT();
  const status = eingangStatus(session, unavailable);
  const source = status.observation;
  const fps = source && source.fps_denominator > 0
    ? Number((source.fps_numerator / source.fps_denominator).toFixed(3)) : null;
  const bild = source
    ? [source.codec.toUpperCase(), `${source.width}×${source.height}`, ...(fps ? [`${fps} fps`] : [])].join(' · ')
    : '';
  return (
    <section aria-label={t('Uplink-Eingang')} className="panel-card space-y-3 rounded-2xl p-5 md:p-6">
      <h2 className="text-lg font-bold text-white" role="status">{t(status.label)}</h2>
      {source ? <>
        <p className="text-sm text-white">
          {bild}
        </p>
        <p className="text-xs text-text-secondary">
          {t('{anzahl} Audiospuren: {spuren}', {
            anzahl: source.audio.length,
            spuren: source.audio.map((audio) => t('Spur {spur}: {codec}, {khz} kHz, {kanaele}', {
              spur: audio.wire_track,
              codec: audio.codec.toUpperCase(),
              khz: audio.sample_rate / 1000,
              kanaele: audio.channels === 1 ? 'Mono' : audio.channels === 2 ? 'Stereo' : t('{anzahl} Kanäle', { anzahl: audio.channels }),
            })).join(' · '),
          })}
        </p>
        <p className="text-xs text-text-secondary">{t('Am empfangenen Stream erkannt; keine Freigabe für eine bestimmte Plattformqualität.')}</p>
      </> : <p className="text-sm text-text-secondary">{unavailable
        ? t('Der aktuelle Dienststatus konnte nicht geladen werden.')
        : t('Codec, Bildgröße und Audiospuren erscheinen nach dem ersten empfangenen Stream.')}</p>}
      {!unavailable && session?.outputs ? <p className="text-xs text-text-secondary">
        {verarbeitungText(t, session.active, session.outputs.video_decoders, session.outputs.encode_groups)}
      </p> : null}
      {!unavailable && session?.error ? <p role="alert" className="text-sm text-warning">{session.error}</p> : null}
      {!unavailable && session && !session.active ? <p className="text-xs text-text-secondary">
        {session.ingest_end_reason === 'ExplicitStop' ? t('OBS hat den Stream ausdrücklich beendet.')
          : t('Der Eingang ist beendet. Der lokale Abschluss bestätigt keine Veröffentlichung auf einer Plattform.')}
      </p> : null}
    </section>
  );
}
