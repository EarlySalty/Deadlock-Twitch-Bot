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

  return {
    gpu,
    gpus,
    hersteller,
    hardware,
    softwareAv1,
    empfehlung: empfehlung(hersteller, hwEncoder, softwareAv1),
  };
}
