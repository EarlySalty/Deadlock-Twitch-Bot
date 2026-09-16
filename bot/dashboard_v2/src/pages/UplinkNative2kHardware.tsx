import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import {
  deleteUplinkNative2kHardware,
  fetchUplinkNative2kHardware,
} from '@/api/uplink';

export function UplinkNative2kHardware({ disabled }: { disabled: boolean }) {
  const queryClient = useQueryClient();
  const queryKey = ['uplink-native-2k-hardware'];
  const hardware = useQuery({
    queryKey,
    queryFn: fetchUplinkNative2kHardware,
    staleTime: 30_000,
    retry: false,
  });
  const loeschen = useMutation({
    mutationFn: deleteUplinkNative2kHardware,
    onSuccess: async () => {
      await queryClient.invalidateQueries({ queryKey });
    },
  });

  const gpu = hardware.data?.profile?.capabilities.gpu[0];
  const cpu = hardware.data?.profile?.capabilities.cpu;
  return (
    <section className="space-y-3 rounded-xl border border-primary/30 bg-primary/5 p-3" aria-labelledby="native-2k-hardware-title">
      <div className="space-y-1">
        <h4 id="native-2k-hardware-title" className="text-sm font-semibold text-white">Quellrechner für Native 2K</h4>
        <p className="text-xs text-text-secondary">
          Die Hardwaredaten kommen aus deiner lokalen OBS-Loganalyse. Uplink speichert nur die daraus gelesenen CPU-/GPU-/Treiberwerte und reicht sie bei der Twitch-GoLive-Aushandlung weiter; die Logdatei selbst wird nicht hochgeladen.
        </p>
      </div>
      <div role="status" className={`text-xs font-semibold ${hardware.data?.configured ? 'text-success' : 'text-warning'}`}>
        {hardware.isLoading ? 'Hardwareprofil wird geladen …'
          : hardware.data?.configured ? 'Hardwareprofil gespeichert und für die nächste Twitch-2K-Aushandlung bereit.'
            : 'Hardwareprofil fehlt – oben in der OBS-Encoder-Analyse eine aktuelle Logdatei auswählen und „Für Native 2K übernehmen“ drücken.'}
      </div>
      {hardware.data?.configured && hardware.data.profile ? (
        <dl className="grid gap-2 rounded-lg border border-border/70 bg-background/50 p-3 text-xs sm:grid-cols-2">
          <div><dt className="text-text-secondary">GPU</dt><dd className="font-semibold text-white">{gpu?.model ?? 'Gespeichert'}</dd></div>
          <div><dt className="text-text-secondary">Treiber</dt><dd className="font-semibold text-white">{gpu?.driver_version ?? 'Gespeichert'}</dd></div>
          <div><dt className="text-text-secondary">CPU</dt><dd className="font-semibold text-white">{cpu?.name ?? `${cpu?.physical_cores ?? 0} Kerne`}</dd></div>
          <div><dt className="text-text-secondary">OBS-Encoder</dt><dd className="font-semibold text-white">{hardware.data.profile.hevc_encoder} / {hardware.data.profile.h264_encoder}</dd></div>
        </dl>
      ) : null}
      {hardware.data?.configured ? (
        <button type="button" disabled={disabled || loeschen.isPending}
          onClick={() => loeschen.mutate()}
          className="min-h-10 rounded-lg border border-border px-3 py-2 text-xs font-semibold text-text-secondary hover:text-white disabled:opacity-50">
          Hardwareprofil entfernen
        </button>
      ) : null}
      {loeschen.isError ? <p className="text-xs text-warning">Hardwareprofil konnte nicht entfernt werden.</p> : null}
    </section>
  );
}
