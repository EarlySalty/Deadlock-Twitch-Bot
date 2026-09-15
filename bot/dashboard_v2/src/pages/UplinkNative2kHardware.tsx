import { useEffect, useState } from 'react';
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import {
  deleteUplinkNative2kHardware,
  fetchUplinkNative2kHardware,
  saveUplinkNative2kHardware,
} from '@/api/uplink';
import type { UplinkNative2kClientProfile } from '@/api/uplink';

const BEISPIEL: UplinkNative2kClientProfile = {
  capabilities: {
    cpu: {
      physical_cores: 8,
      logical_cores: 16,
      name: 'CPU-Modell',
      speed: 4500,
    },
    memory: {
      total: 34359738368,
      free: 17179869184,
    },
    system: {
      name: 'Windows',
      version: '11',
      release: '23H2',
      revision: 'Build/Revision',
      bits: 64,
      arm: false,
      build: 22631,
      armEmulation: false,
    },
    gpu: [{
      model: 'GPU-Modell',
      vendor_id: 4098,
      device_id: 0,
      dedicated_video_memory: 17179869184,
      shared_system_memory: 17179869184,
      driver_version: 'Treiber-Version',
    }],
    gaming_features: null,
  },
  hevc_encoder: 'h265_texture_amf',
  h264_encoder: 'h264_texture_amf',
};

function formatiere(profile: UplinkNative2kClientProfile | null) {
  return JSON.stringify(profile ?? BEISPIEL, null, 2);
}

export function UplinkNative2kHardware({ disabled }: { disabled: boolean }) {
  const queryClient = useQueryClient();
  const queryKey = ['uplink-native-2k-hardware'];
  const hardware = useQuery({
    queryKey,
    queryFn: fetchUplinkNative2kHardware,
    staleTime: 30_000,
    retry: false,
  });
  const [text, setText] = useState('');
  const [fehler, setFehler] = useState('');

  useEffect(() => {
    if (hardware.data && !text) setText(formatiere(hardware.data.profile));
  }, [hardware.data, text]);

  const speichern = useMutation({
    mutationFn: async () => {
      let parsed: unknown;
      try {
        parsed = JSON.parse(text);
      } catch {
        throw new Error('Das Hardwareprofil ist kein gültiges JSON.');
      }
      if (!parsed || typeof parsed !== 'object' || Array.isArray(parsed)) {
        throw new Error('Das Hardwareprofil muss ein JSON-Objekt sein.');
      }
      return saveUplinkNative2kHardware(parsed as UplinkNative2kClientProfile);
    },
    onSuccess: async () => {
      setFehler('');
      await queryClient.invalidateQueries({ queryKey });
    },
    onError: (error) => setFehler(error instanceof Error ? error.message : 'Hardwareprofil konnte nicht gespeichert werden.'),
  });

  const loeschen = useMutation({
    mutationFn: deleteUplinkNative2kHardware,
    onSuccess: async () => {
      setFehler('');
      setText(formatiere(null));
      await queryClient.invalidateQueries({ queryKey });
    },
    onError: (error) => setFehler(error instanceof Error ? error.message : 'Hardwareprofil konnte nicht entfernt werden.'),
  });

  return (
    <section className="space-y-3 rounded-xl border border-primary/30 bg-primary/5 p-3" aria-labelledby="native-2k-hardware-title">
      <div className="space-y-1">
        <h4 id="native-2k-hardware-title" className="text-sm font-semibold text-white">Quellrechner für Native 2K</h4>
        <p className="text-xs text-text-secondary">
          Twitch handelt 2K anhand der Hardware aus, die HEVC tatsächlich erzeugt. Deshalb werden CPU-, RAM-, System- und GPU-Daten deines Streaming-PCs an Twitch weitergegeben – nicht die GPU des Uplink-Servers.
        </p>
        <p className="text-xs text-text-secondary">
          Native 2K startet nur mit 2560×1440@60 HEVC aus OBS. AV1/H.264 werden für die 2K-Topspur nicht umgerechnet; Uplink berechnet nur die von Twitch geforderten kleineren H.264-Stufen.
        </p>
      </div>
      <div role="status" className={`text-xs font-semibold ${hardware.data?.configured ? 'text-success' : 'text-warning'}`}>
        {hardware.isLoading ? 'Hardwareprofil wird geladen …'
          : hardware.data?.configured ? 'Hardwareprofil gespeichert.'
            : 'Hardwareprofil fehlt – Native 2K bleibt bis dahin blockiert.'}
      </div>
      <details className="rounded-lg border border-border/70 bg-background/50 p-3">
        <summary className="cursor-pointer text-xs font-semibold text-white">Hardwareprofil bearbeiten</summary>
        <div className="mt-3 space-y-2">
          <p className="text-xs text-text-secondary">
            Trage die echten Werte des Rechners ein, auf dem OBS HEVC encodiert. Dezimalwerte für PCI Vendor-/Device-ID, Bytes für Speicher. Für AMD sind die üblichen OBS-IDs <code>h265_texture_amf</code> und <code>h264_texture_amf</code>; NVIDIA/Intel müssen ihre tatsächlichen OBS-Encoder-IDs verwenden.
          </p>
          <textarea
            value={text || formatiere(hardware.data?.profile ?? null)}
            onChange={(event) => { setText(event.target.value); setFehler(''); }}
            spellCheck={false}
            disabled={disabled || speichern.isPending || loeschen.isPending}
            aria-label="Native-2K-Hardwareprofil als JSON"
            className="min-h-80 w-full rounded-lg border border-border bg-background/80 p-3 font-mono text-xs text-white outline-none focus:border-primary"
          />
          {fehler ? <p className="text-xs text-danger">{fehler}</p> : null}
          <div className="flex flex-wrap gap-2">
            <button type="button" disabled={disabled || speichern.isPending || !text.trim()}
              onClick={() => speichern.mutate()}
              className="min-h-11 rounded-lg bg-primary px-4 py-2 text-xs font-semibold text-[#0D0806] disabled:opacity-50">
              Hardwareprofil speichern
            </button>
            {hardware.data?.configured ? <button type="button" disabled={disabled || loeschen.isPending}
              onClick={() => loeschen.mutate()}
              className="min-h-11 rounded-lg border border-border px-4 py-2 text-xs font-semibold text-text-secondary hover:text-white disabled:opacity-50">
              Hardwareprofil entfernen
            </button> : null}
          </div>
        </div>
      </details>
    </section>
  );
}
