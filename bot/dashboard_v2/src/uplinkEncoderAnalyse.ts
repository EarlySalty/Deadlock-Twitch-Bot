import type { UplinkNative2kClientProfile } from './api/uplink';

export type UplinkGpuHersteller = 'nvidia' | 'amd' | 'intel' | 'unbekannt';
export type UplinkVideoCodec = 'AV1' | 'HEVC / H.265' | 'H.264';

type HardwareCodec = 'av1' | 'hevc' | 'h264';

interface HardwareEncoder {
  hersteller: Exclude<UplinkGpuHersteller, 'unbekannt'>;
  codec: HardwareCodec;
  roh: string;
}

export interface UplinkEncoderAnalyse {
  /** Der Adapter, auf dem OBS laut D3D11-Log tatsächlich rendert; sonst Adapter 0. */
  gpu: string | null;
  /** Alle im OBS-Startlog gefundenen Grafikadapter. */
  gpus: string[];
  hersteller: UplinkGpuHersteller;
  hardware: {
    av1: boolean;
    hevc: boolean;
    h264: boolean;
  };
  softwareAv1: boolean;
  empfehlung: UplinkEncoderEmpfehlung;
  /** Vollständige, aus dem OBS-Host gelesene GoLive-Hardwaredaten. */
  native2kProfile: UplinkNative2kClientProfile | null;
  /** Fehlende OBS-Logfelder, falls das Profil noch nicht sicher weitergegeben werden kann. */
  native2kFehlendeFelder: string[];
}

export interface UplinkEncoderEmpfehlung {
  codec: UplinkVideoCodec;
  encoder: string;
  ratensteuerung: string;
  hinweise: string[];
  status: 'optimal' | 'fallback';
}

const encoderZeile = /-\s+(.+)$/;

function herstellerFuer(gpu: string | null): UplinkGpuHersteller {
  const wert = gpu?.toLowerCase() ?? '';
  if (wert.includes('nvidia')) return 'nvidia';
  if (wert.includes('amd') || wert.includes('radeon')) return 'amd';
  if (wert.includes('intel')) return 'intel';
  return 'unbekannt';
}

function encoderHersteller(encoder: string): HardwareEncoder['hersteller'] | null {
  const wert = encoder.toLowerCase();
  if (wert.includes('nvenc') || wert.includes('nvidia')) return 'nvidia';
  if (wert.includes('amf') || wert.includes('amd hw') || wert.includes('amd hardware')) return 'amd';
  if (wert.includes('qsv') || wert.includes('quicksync') || wert.includes('quick sync')) return 'intel';
  return null;
}

function encoderCodec(encoder: string): HardwareCodec | null {
  const wert = encoder.toLowerCase();
  if (wert.includes('av1')) return 'av1';
  if (wert.includes('hevc') || wert.includes('h265') || wert.includes('h.265')) return 'hevc';
  if (wert.includes('h264') || wert.includes('h.264') || /\bavc\b/.test(wert)) return 'h264';
  return null;
}

function hardwareEncoder(encoders: string[]): HardwareEncoder[] {
  return encoders.flatMap((roh) => {
    const hersteller = encoderHersteller(roh);
    const codec = encoderCodec(roh);
    return hersteller && codec ? [{ hersteller, codec, roh }] : [];
  });
}

function encoderName(hersteller: HardwareEncoder['hersteller'], codec: HardwareCodec) {
  const codecText = codec === 'av1' ? 'AV1' : codec === 'hevc' ? 'HEVC' : 'H.264';
  if (hersteller === 'nvidia') return `NVIDIA NVENC ${codecText}`;
  if (hersteller === 'amd') return `AMD Hardware ${codecText}`;
  return `Intel QuickSync ${codecText}`;
}

function ratensteuerung(hersteller: HardwareEncoder['hersteller']) {
  if (hersteller === 'nvidia') return 'Variable Bitrate mit Zielqualität';
  if (hersteller === 'amd') return 'HQCBR (falls in OBS angeboten), sonst CBR';
  return 'VBR mit Maximalbitrate';
}

function waehleEncoder(
  encoder: HardwareEncoder[],
  codec: HardwareCodec,
  aktiverHersteller: UplinkGpuHersteller,
): HardwareEncoder | null {
  const passend = encoder.filter((eintrag) => eintrag.codec === codec);
  if (!passend.length) return null;

  // Wenn OBS auf derselben GPU rendert, ist dieser Encoder der sauberste Weg.
  // Gibt es dort den gewünschten Codec nicht, darf ein zweiter Hardwareencoder
  // (z. B. Arc AV1 neben einer älteren NVIDIA-Karte) trotzdem genutzt werden.
  const aktiv = aktiverHersteller === 'unbekannt'
    ? null
    : passend.find((eintrag) => eintrag.hersteller === aktiverHersteller);
  return aktiv ?? passend[0];
}

function empfehlungFuerHardware(
  eintrag: HardwareEncoder,
  softwareAv1: boolean,
  aktiverHersteller: UplinkGpuHersteller,
): UplinkEncoderEmpfehlung {
  const codec: UplinkVideoCodec = eintrag.codec === 'av1'
    ? 'AV1'
    : eintrag.codec === 'hevc'
      ? 'HEVC / H.265'
      : 'H.264';
  const hinweise: string[] = [];

  if (eintrag.codec === 'av1') {
    hinweise.push('Hardware-AV1 verwenden; AOM AV1 und SVT-AV1 sind für den Live-Uplink kein automatischer Ersatz.');
    hinweise.push('Für den Uplink-Enhanced-Test: 1920×1080@60 mit 5 Mbit/s starten. Uplink erzeugt daraus die Twitch-H.264-Qualitätsstufen; natives Twitch-2K gehört nicht zu diesem Modus.');
  } else if (softwareAv1) {
    hinweise.push('AV1 ist nur als Softwareencoder sichtbar und wird deshalb nicht empfohlen.');
  }

  if (eintrag.hersteller === 'nvidia') {
    hinweise.push('Zielqualität plus maximale Bitrate verwenden. Falls deine OBS-Version diese Auswahl nicht anbietet, auf VBR mit Maximalbitrate oder CBR zurückfallen.');
  } else if (eintrag.hersteller === 'amd') {
    hinweise.push('HQCBR nur wählen, wenn OBS es für genau diesen AMF-Encoder anbietet; unbegrenztes VBR wird nicht automatisch empfohlen.');
  } else {
    hinweise.push('Bei QuickSync ist für ein begrenztes Uploadbudget VBR mit gesetzter Maximalbitrate der sichere variable Weg; ICQ allein setzt in OBS kein Maximalbitratelimit.');
  }

  if (aktiverHersteller !== 'unbekannt' && eintrag.hersteller !== aktiverHersteller) {
    hinweise.push('Der empfohlene Encoder sitzt auf einem anderen GPU-Hersteller als der OBS-Renderadapter. Das ist erlaubt, sollte aber im Preflight auf Kopierlast und Stabilität geprüft werden.');
  }

  return {
    codec,
    encoder: encoderName(eintrag.hersteller, eintrag.codec),
    ratensteuerung: ratensteuerung(eintrag.hersteller),
    status: eintrag.codec === 'h264' ? 'fallback' : 'optimal',
    hinweise,
  };
}

function encoderId(roh: string) {
  return roh.split(/\s+\(/, 1)[0]?.trim() ?? '';
}

function speicherBytes(text: string): number | null {
  const match = text.match(/([0-9][0-9.,]*)\s*(B|KB|KiB|MB|MiB|GB|GiB)?/i);
  if (!match?.[1]) return null;
  const zahl = Number(match[1].replace(/,/g, '.'));
  if (!Number.isFinite(zahl) || zahl < 0) return null;
  const einheit = (match[2] ?? 'B').toLowerCase();
  const faktor = einheit === 'kb' || einheit === 'kib' ? 1024
    : einheit === 'mb' || einheit === 'mib' ? 1024 ** 2
      : einheit === 'gb' || einheit === 'gib' ? 1024 ** 3 : 1;
  return Math.round(zahl * faktor);
}

interface ObsGpuProfil {
  model: string;
  vendor_id: number | null;
  device_id: number | null;
  dedicated_video_memory: number | null;
  shared_system_memory: number | null;
  driver_version: string | null;
}

function native2kProfilAusObsLog(
  text: string,
  aktiveGpu: string | null,
  encoder: HardwareEncoder[],
): { profile: UplinkNative2kClientProfile | null; fehlend: string[] } {
  const fehlend: string[] = [];
  const zeilen = text.split(/\r?\n/);
  let cpuName: string | null = null;
  let cpuSpeed: number | null = null;
  let physicalCores: number | null = null;
  let logicalCores: number | null = null;
  let memoryTotal: number | null = null;
  let memoryFree: number | null = null;
  let systemName: string | null = null;
  let systemVersion: string | null = null;
  let systemRelease: string | null = null;
  let systemRevision: string | null = null;
  let systemBits: number | null = null;
  let systemBuild: number | null = null;
  let systemArm = false;
  const gpuProfile: ObsGpuProfil[] = [];
  let aktuelleGpu: ObsGpuProfil | null = null;

  for (const zeile of zeilen) {
    const cpu = zeile.match(/CPU Name:\s*(.+)$/i);
    if (cpu?.[1]) cpuName = cpu[1].trim();
    const speed = zeile.match(/CPU Speed:\s*([0-9]+)\s*MHz/i);
    if (speed?.[1]) cpuSpeed = Number(speed[1]);
    const cores = zeile.match(/Physical Cores:\s*([0-9]+)\s*,\s*Logical Cores:\s*([0-9]+)/i);
    if (cores?.[1] && cores?.[2]) {
      physicalCores = Number(cores[1]);
      logicalCores = Number(cores[2]);
    }
    const memory = zeile.match(/Physical Memory:\s*([^,]+)\s+Total\s*,\s*([^,]+)\s+Free/i);
    if (memory?.[1] && memory?.[2]) {
      memoryTotal = speicherBytes(memory[1]);
      memoryFree = speicherBytes(memory[2]);
    }

    const windows = zeile.match(/Windows Version:\s*([^\s]+)(?:\s+Build\s+([0-9]+))?(.*)$/i);
    if (windows?.[1]) {
      systemName = 'Windows';
      systemVersion = windows[1].trim();
      systemBuild = windows[2] ? Number(windows[2]) : 0;
      const rest = windows[3] ?? '';
      systemRelease = rest.match(/release:\s*([^;)]+)/i)?.[1]?.trim() ?? systemVersion;
      systemRevision = rest.match(/revision:\s*([^;)]+)/i)?.[1]?.trim() ?? String(systemBuild ?? 0);
      systemBits = /64-bit|x64|amd64/i.test(rest) ? 64 : /32-bit|x86/i.test(rest) ? 32 : 64;
      systemArm = /arm64|aarch64/i.test(rest);
    }

    const adapter = zeile.match(/Adapter\s+\d+:\s*(.+)$/i);
    if (adapter?.[1]) {
      aktuelleGpu = {
        model: adapter[1].trim(),
        vendor_id: null,
        device_id: null,
        dedicated_video_memory: null,
        shared_system_memory: null,
        driver_version: null,
      };
      gpuProfile.push(aktuelleGpu);
      continue;
    }
    if (!aktuelleGpu) continue;
    const pci = zeile.match(/PCI ID:\s*([0-9a-f]{4}):([0-9a-f]{4})/i);
    if (pci?.[1] && pci?.[2]) {
      aktuelleGpu.vendor_id = Number.parseInt(pci[1], 16);
      aktuelleGpu.device_id = Number.parseInt(pci[2], 16);
    }
    const dedicated = zeile.match(/Dedicated (?:Video Memory|VRAM):\s*(.+)$/i);
    if (dedicated?.[1]) aktuelleGpu.dedicated_video_memory = speicherBytes(dedicated[1]);
    const shared = zeile.match(/Shared (?:System Memory|VRAM):\s*(.+)$/i);
    if (shared?.[1]) aktuelleGpu.shared_system_memory = speicherBytes(shared[1]);
    const driver = zeile.match(/Driver Version:\s*(.+)$/i);
    if (driver?.[1]) aktuelleGpu.driver_version = driver[1].trim();
  }

  const aktiverHersteller = herstellerFuer(aktiveGpu);
  const hevc = waehleEncoder(encoder, 'hevc', aktiverHersteller);
  const h264 = waehleEncoder(encoder, 'h264', aktiverHersteller);
  if (!physicalCores) fehlend.push('physische CPU-Kerne');
  if (!logicalCores) fehlend.push('logische CPU-Kerne');
  if (!memoryTotal) fehlend.push('Gesamtspeicher');
  if (memoryFree === null) fehlend.push('freier Speicher');
  if (!systemName || !systemVersion || !systemRelease || !systemRevision || !systemBits) fehlend.push('Windows-/Systemversion');
  if (!hevc) fehlend.push('Hardware-HEVC-Encoder');
  if (!h264) fehlend.push('Hardware-H.264-Encoder');
  const vollstaendigeGpus = gpuProfile.filter((gpu) => gpu.vendor_id && gpu.device_id
    && gpu.dedicated_video_memory && gpu.shared_system_memory !== null && gpu.driver_version);
  if (!vollstaendigeGpus.length) fehlend.push('GPU PCI-ID/VRAM/Treiber');
  if (fehlend.length) return { profile: null, fehlend };

  return {
    profile: {
      capabilities: {
        cpu: {
          physical_cores: physicalCores!,
          logical_cores: logicalCores!,
          name: cpuName,
          speed: cpuSpeed,
        },
        memory: { total: memoryTotal!, free: memoryFree! },
        system: {
          name: systemName!,
          version: systemVersion!,
          release: systemRelease!,
          revision: systemRevision!,
          bits: systemBits!,
          arm: systemArm,
          build: systemBuild ?? 0,
          armEmulation: false,
        },
        gpu: vollstaendigeGpus.map((gpu) => ({
          model: gpu.model,
          vendor_id: gpu.vendor_id!,
          device_id: gpu.device_id!,
          dedicated_video_memory: gpu.dedicated_video_memory!,
          shared_system_memory: gpu.shared_system_memory!,
          driver_version: gpu.driver_version!,
        })),
        gaming_features: null,
      },
      hevc_encoder: encoderId(hevc!.roh),
      h264_encoder: encoderId(h264!.roh),
    },
    fehlend: [],
  };
}

function empfehlung(
  aktiverHersteller: UplinkGpuHersteller,
  encoder: HardwareEncoder[],
  softwareAv1: boolean,
): UplinkEncoderEmpfehlung {
  // Der Codec wird anhand dessen gewählt, was OBS auf diesem Rechner wirklich
  // registriert hat, nicht anhand einer fest verdrahteten GPU-Generationsliste.
  // Das deckt neue Generationen automatisch ab, sobald OBS/Treiber sie anbieten.
  for (const codec of ['av1', 'hevc', 'h264'] as const) {
    const kandidat = waehleEncoder(encoder, codec, aktiverHersteller);
    if (kandidat) return empfehlungFuerHardware(kandidat, softwareAv1, aktiverHersteller);
  }

  return {
    codec: 'H.264',
    encoder: 'Hardwareencoder in OBS auswählen',
    ratensteuerung: 'CBR',
    status: 'fallback',
    hinweise: [
      'Im Log wurde kein unterstützter Hardwareencoder eindeutig erkannt.',
      'Uplink wählt Software-AV1 nicht automatisch für einen Live-Stream.',
    ],
  };
}

/**
 * Liest ausschließlich den lokalen Text einer OBS-Logdatei. Die Datei wird
 * vom Browser nicht hochgeladen. OBS schreibt GPU und die beim Start
 * registrierten Encoder bereits vor einem Streamstart in das Log.
 */
export function analysiereObsLog(text: string): UplinkEncoderAnalyse {
  const zeilen = text.split(/\r?\n/);
  const gpus: string[] = [];
  let aktiverAdapter: string | null = null;
  let inEncoderListe = false;
  let inVideoEncoder = false;
  const encoders: string[] = [];

  for (const zeile of zeilen) {
    const adapter = zeile.match(/Adapter\s+\d+:\s*(.+)$/i);
    if (adapter?.[1]) {
      const name = adapter[1].trim();
      if (!gpus.includes(name)) gpus.push(name);
    }

    const d3dAdapter = zeile.match(/Loading up D3D11 on adapter\s+(.+?)(?:\s+\(\d+\))?$/i);
    if (d3dAdapter?.[1]) aktiverAdapter = d3dAdapter[1].trim();

    if (/Available Encoders:/i.test(zeile)) {
      inEncoderListe = true;
      continue;
    }
    if (!inEncoderListe) continue;
    if (/Video Encoders:/i.test(zeile)) {
      inVideoEncoder = true;
      continue;
    }
    if (/Audio Encoders:/i.test(zeile)) break;
    if (!inVideoEncoder) continue;
    const treffer = zeile.match(encoderZeile);
    if (treffer?.[1]) encoders.push(treffer[1].trim());
  }

  const gpu = aktiverAdapter ?? gpus[0] ?? null;
  const hersteller = herstellerFuer(gpu);
  const hwEncoder = hardwareEncoder(encoders);
  const hardware = {
    av1: hwEncoder.some((eintrag) => eintrag.codec === 'av1'),
    hevc: hwEncoder.some((eintrag) => eintrag.codec === 'hevc'),
    h264: hwEncoder.some((eintrag) => eintrag.codec === 'h264'),
  };
  const softwareAv1 = encoders.some((e) => /aom av1|svt-av1|ffmpeg_aom_av1|ffmpeg_svt_av1/i.test(e));
  const native2k = native2kProfilAusObsLog(text, gpu, hwEncoder);

  return {
    gpu,
    gpus,
    hersteller,
    hardware,
    softwareAv1,
    empfehlung: empfehlung(hersteller, hwEncoder, softwareAv1),
    native2kProfile: native2k.profile,
    native2kFehlendeFelder: native2k.fehlend,
  };
}
