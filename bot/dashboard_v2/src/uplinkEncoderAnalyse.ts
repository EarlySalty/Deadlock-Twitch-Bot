export type UplinkGpuHersteller = 'nvidia' | 'amd' | 'intel' | 'unbekannt';

export interface UplinkEncoderAnalyse {
  gpu: string | null;
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
  codec: 'AV1' | 'HEVC / H.265' | 'H.264';
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

function enthaeltEncoder(encoders: string[], begriffe: string[]) {
  return encoders.some((encoder) => {
    const wert = encoder.toLowerCase();
    return begriffe.every((begriff) => wert.includes(begriff));
  });
}

function empfehlung(
  hersteller: UplinkGpuHersteller,
  hardware: UplinkEncoderAnalyse['hardware'],
  softwareAv1: boolean,
): UplinkEncoderEmpfehlung {
  if (hersteller === 'amd' && hardware.av1) {
    return {
      codec: 'AV1',
      encoder: 'AMD Hardware AV1',
      ratensteuerung: 'HQCBR (falls in OBS angeboten)',
      status: 'optimal',
      hinweise: [
        'Hardware-AV1 verwenden; AOM AV1 und SVT-AV1 sind für den Live-Uplink kein automatischer Ersatz.',
        'HQCBR nur wählen, wenn OBS es für diesen AMF-Encoder tatsächlich anbietet.',
      ],
    };
  }

  if (hersteller === 'nvidia' && hardware.av1) {
    return {
      codec: 'AV1',
      encoder: 'NVIDIA NVENC AV1',
      ratensteuerung: 'Variable Bitrate mit Zielqualität',
      status: 'optimal',
      hinweise: [
        'Zielqualität plus maximale Bitrate verwenden; so darf die Bitrate bei einfachen Szenen sinken.',
        'AOM AV1 und SVT-AV1 nicht auswählen, solange NVENC AV1 verfügbar ist.',
      ],
    };
  }

  if (hersteller === 'nvidia' && hardware.hevc) {
    return {
      codec: 'HEVC / H.265',
      encoder: 'NVIDIA NVENC HEVC',
      ratensteuerung: 'Variable Bitrate mit Zielqualität',
      status: 'optimal',
      hinweise: [
        'HEVC läuft über NVENC und vermeidet AV1-Softwareencoding auf der CPU.',
        ...(softwareAv1 ? ['AV1 ist nur als Softwareencoder sichtbar und wird deshalb nicht empfohlen.'] : []),
      ],
    };
  }

  if (hersteller === 'amd' && hardware.hevc) {
    return {
      codec: 'HEVC / H.265',
      encoder: 'AMD Hardware HEVC',
      ratensteuerung: 'CBR/HQCBR nach tatsächlich angebotenen OBS-Optionen',
      status: 'optimal',
      hinweise: [
        'HEVC ist der Hardware-Fallback, wenn auf dieser AMD-GPU kein Hardware-AV1 angeboten wird.',
        'Unbegrenztes VBR wird nicht automatisch empfohlen.',
      ],
    };
  }

  if (hardware.h264) {
    const encoder = hersteller === 'nvidia'
      ? 'NVIDIA NVENC H.264'
      : hersteller === 'amd'
        ? 'AMD Hardware H.264'
        : hersteller === 'intel'
          ? 'Intel QSV H.264'
          : 'Hardware H.264';
    return {
      codec: 'H.264',
      encoder,
      ratensteuerung: 'CBR',
      status: 'fallback',
      hinweise: [
        'Kein besser geeigneter Hardware-AV1/HEVC-Weg wurde im Log nachgewiesen.',
        ...(softwareAv1 ? ['Software-AV1 wurde erkannt, wird für Echtzeit aber nicht automatisch gewählt.'] : []),
      ],
    };
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
  let gpu: string | null = null;
  let inEncoderListe = false;
  let inVideoEncoder = false;
  const encoders: string[] = [];

  for (const zeile of zeilen) {
    const adapter = zeile.match(/Adapter\s+\d+:\s*(.+)$/i);
    if (!gpu && adapter?.[1]) gpu = adapter[1].trim();

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

  const hersteller = herstellerFuer(gpu);
  const nvidia = (codec: string) => enthaeltEncoder(encoders, ['nvenc', codec]);
  const amd = (codec: string) => encoders.some((e) => {
    const wert = e.toLowerCase();
    return (wert.includes('amf') || wert.includes('amd')) && wert.includes(codec);
  });
  const intel = (codec: string) => encoders.some((e) => {
    const wert = e.toLowerCase();
    return (wert.includes('qsv') || wert.includes('quick sync')) && wert.includes(codec);
  });
  const hw = (codec: string) => nvidia(codec) || amd(codec) || intel(codec);

  const hardware = {
    av1: hw('av1'),
    hevc: hw('hevc') || hw('h265') || hw('h.265'),
    h264: hw('h264') || hw('h.264'),
  };
  const softwareAv1 = encoders.some((e) => /aom av1|svt-av1|ffmpeg_aom_av1|ffmpeg_svt_av1/i.test(e));

  return {
    gpu,
    hersteller,
    hardware,
    softwareAv1,
    empfehlung: empfehlung(hersteller, hardware, softwareAv1),
  };
}
