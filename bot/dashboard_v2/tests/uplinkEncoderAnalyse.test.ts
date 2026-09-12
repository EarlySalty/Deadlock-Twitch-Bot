import test from 'node:test';
import assert from 'node:assert/strict';
import { analysiereObsLog } from '../src/uplinkEncoderAnalyse';

function log(gpu: string, encoder: string[], extra = '') {
  return `
00:00:00.000: Adapter 0: ${gpu}
00:00:00.000: Loading up D3D11 on adapter ${gpu} (0)
${extra}
00:00:00.001: Available Encoders:
00:00:00.001:   Video Encoders:
${encoder.map((eintrag) => `00:00:00.001:     - ${eintrag}`).join('\n')}
00:00:00.001:   Audio Encoders:
`;
}

test('RTX 20/30 ohne Hardware-AV1 empfiehlt NVENC HEVC statt Software-AV1', () => {
  for (const gpu of ['NVIDIA GeForce RTX 2080 Ti', 'NVIDIA GeForce RTX 3060 Ti']) {
    const analyse = analysiereObsLog(log(gpu, [
      'ffmpeg_svt_av1 (SVT-AV1)',
      'ffmpeg_aom_av1 (AOM AV1)',
      'obs_nvenc_h264_tex (NVIDIA NVENC H.264)',
      'obs_nvenc_hevc_tex (NVIDIA NVENC HEVC)',
    ]));

    assert.equal(analyse.gpu, gpu);
    assert.equal(analyse.hardware.av1, false);
    assert.equal(analyse.hardware.hevc, true);
    assert.equal(analyse.softwareAv1, true);
    assert.equal(analyse.empfehlung.encoder, 'NVIDIA NVENC HEVC');
    assert.equal(analyse.empfehlung.ratensteuerung, 'Variable Bitrate mit Zielqualität');
  }
});

test('RTX 40/50 mit NVENC AV1 bevorzugt Hardware-AV1', () => {
  for (const gpu of ['NVIDIA GeForce RTX 4070', 'NVIDIA GeForce RTX 5090']) {
    const analyse = analysiereObsLog(log(gpu, [
      'obs_nvenc_av1_tex (NVIDIA NVENC AV1)',
      'obs_nvenc_hevc_tex (NVIDIA NVENC HEVC)',
      'obs_nvenc_h264_tex (NVIDIA NVENC H.264)',
    ]));

    assert.equal(analyse.hardware.av1, true);
    assert.equal(analyse.empfehlung.codec, 'AV1');
    assert.equal(analyse.empfehlung.encoder, 'NVIDIA NVENC AV1');
    assert.equal(analyse.empfehlung.ratensteuerung, 'Variable Bitrate mit Zielqualität');
  }
});

test('ältere AMD-Generationen ohne AV1 fallen anhand der OBS-Encoder auf HEVC', () => {
  for (const gpu of ['AMD Radeon RX 5700 XT', 'AMD Radeon RX 6800 XT']) {
    const analyse = analysiereObsLog(log(gpu, [
      'h265_texture_amf (AMD HW H.265)',
      'h264_texture_amf (AMD HW H.264)',
    ]));

    assert.equal(analyse.hardware.av1, false);
    assert.equal(analyse.empfehlung.encoder, 'AMD Hardware HEVC');
    assert.match(analyse.empfehlung.ratensteuerung, /HQCBR/);
  }
});

test('neuere AMD-Generationen mit AMF AV1 bevorzugen Hardware-AV1', () => {
  for (const gpu of ['AMD Radeon RX 7900 XTX', 'AMD Radeon RX 9070 XT']) {
    const analyse = analysiereObsLog(log(gpu, [
      'av1_texture_amf (AMD HW AV1)',
      'h265_texture_amf (AMD HW H.265)',
      'h264_texture_amf (AMD HW H.264)',
    ]));

    assert.equal(analyse.hersteller, 'amd');
    assert.equal(analyse.hardware.av1, true);
    assert.equal(analyse.empfehlung.encoder, 'AMD Hardware AV1');
    assert.match(analyse.empfehlung.ratensteuerung, /HQCBR/);
  }
});

test('Intel Arc mit QuickSync AV1 bekommt einen eigenen AV1-Pfad', () => {
  const analyse = analysiereObsLog(log('Intel(R) Arc(TM) A770 Graphics', [
    'obs_qsv11_av1 (QuickSync AV1)',
    'obs_qsv11_hevc (QuickSync HEVC)',
    'obs_qsv11_v2 (QuickSync H.264)',
  ]));

  assert.equal(analyse.hersteller, 'intel');
  assert.equal(analyse.hardware.av1, true);
  assert.equal(analyse.empfehlung.encoder, 'Intel QuickSync AV1');
  assert.equal(analyse.empfehlung.ratensteuerung, 'VBR mit Maximalbitrate');
});

test('älteres Intel QuickSync ohne AV1 nutzt den besten tatsächlich angebotenen Codec', () => {
  const analyse = analysiereObsLog(log('Intel(R) UHD Graphics 630', [
    'obs_qsv11_hevc (QuickSync HEVC)',
    'obs_qsv11_v2 (QuickSync H.264)',
  ]));

  assert.equal(analyse.hardware.av1, false);
  assert.equal(analyse.empfehlung.encoder, 'Intel QuickSync HEVC');
  assert.equal(analyse.empfehlung.ratensteuerung, 'VBR mit Maximalbitrate');
});

test('Multi-GPU nimmt den aktiven OBS-Adapter für denselben Codec', () => {
  const analyse = analysiereObsLog(`
00:00:00.000: Adapter 0: Intel(R) Arc(TM) A380 Graphics
00:00:00.000: Adapter 1: NVIDIA GeForce RTX 4070
00:00:00.000: Loading up D3D11 on adapter NVIDIA GeForce RTX 4070 (1)
00:00:00.001: Available Encoders:
00:00:00.001:   Video Encoders:
00:00:00.001:     - obs_qsv11_av1 (QuickSync AV1)
00:00:00.001:     - obs_nvenc_av1_tex (NVIDIA NVENC AV1)
00:00:00.001:     - obs_nvenc_hevc_tex (NVIDIA NVENC HEVC)
00:00:00.001:     - obs_nvenc_h264_tex (NVIDIA NVENC H.264)
00:00:00.001:   Audio Encoders:
`);

  assert.deepEqual(analyse.gpus, ['Intel(R) Arc(TM) A380 Graphics', 'NVIDIA GeForce RTX 4070']);
  assert.equal(analyse.gpu, 'NVIDIA GeForce RTX 4070');
  assert.equal(analyse.empfehlung.encoder, 'NVIDIA NVENC AV1');
});

test('zweite Arc-Karte darf AV1 liefern, wenn der OBS-Renderadapter selbst kein AV1 kann', () => {
  const analyse = analysiereObsLog(`
00:00:00.000: Adapter 0: NVIDIA GeForce RTX 2080 Super
00:00:00.000: Adapter 1: Intel(R) Arc(TM) A380 Graphics
00:00:00.000: Loading up D3D11 on adapter NVIDIA GeForce RTX 2080 Super (0)
00:00:00.001: Available Encoders:
00:00:00.001:   Video Encoders:
00:00:00.001:     - obs_qsv11_av1 (QuickSync AV1)
00:00:00.001:     - obs_nvenc_hevc_tex (NVIDIA NVENC HEVC)
00:00:00.001:     - obs_nvenc_h264_tex (NVIDIA NVENC H.264)
00:00:00.001:   Audio Encoders:
`);

  assert.equal(analyse.empfehlung.encoder, 'Intel QuickSync AV1');
  assert.match(analyse.empfehlung.hinweise.join(' '), /anderen GPU-Hersteller/);
});

test('Software-AV1 ohne Hardwareencoder wird nicht als Live-AV1 empfohlen', () => {
  const analyse = analysiereObsLog(log('Microsoft Basic Render Driver', [
    'ffmpeg_svt_av1 (SVT-AV1)',
    'ffmpeg_aom_av1 (AOM AV1)',
    'obs_x264 (x264)',
  ]));

  assert.equal(analyse.softwareAv1, true);
  assert.equal(analyse.hardware.av1, false);
  assert.equal(analyse.empfehlung.codec, 'H.264');
});
