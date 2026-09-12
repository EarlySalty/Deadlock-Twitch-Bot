import test from 'node:test';
import assert from 'node:assert/strict';
import { analysiereObsLog } from '../src/uplinkEncoderAnalyse';

test('NVIDIA ohne Hardware-AV1 empfiehlt NVENC HEVC mit Zielqualitäts-VBR', () => {
  const analyse = analysiereObsLog(`
12:07:10.506: Adapter 0: NVIDIA GeForce RTX 3060 Ti
12:07:11.114: Available Encoders:
12:07:11.114:   Video Encoders:
12:07:11.114:     - ffmpeg_svt_av1 (SVT-AV1)
12:07:11.114:     - ffmpeg_aom_av1 (AOM AV1)
12:07:11.114:     - obs_nvenc_h264_tex (NVIDIA NVENC H.264)
12:07:11.114:     - obs_nvenc_hevc_tex (NVIDIA NVENC HEVC)
12:07:11.114:   Audio Encoders:
12:07:11.114:     - ffmpeg_aac (FFmpeg AAC)
`);

  assert.equal(analyse.gpu, 'NVIDIA GeForce RTX 3060 Ti');
  assert.equal(analyse.hardware.av1, false);
  assert.equal(analyse.hardware.hevc, true);
  assert.equal(analyse.hardware.h264, true);
  assert.equal(analyse.softwareAv1, true);
  assert.equal(analyse.empfehlung.encoder, 'NVIDIA NVENC HEVC');
  assert.equal(analyse.empfehlung.ratensteuerung, 'Variable Bitrate mit Zielqualität');
});

test('NVIDIA mit NVENC AV1 bevorzugt Hardware-AV1', () => {
  const analyse = analysiereObsLog(`
00:00:00.000: Adapter 0: NVIDIA GeForce RTX 4070
00:00:00.001: Available Encoders:
00:00:00.001:   Video Encoders:
00:00:00.001:     - obs_nvenc_av1_tex (NVIDIA NVENC AV1)
00:00:00.001:     - obs_nvenc_hevc_tex (NVIDIA NVENC HEVC)
00:00:00.001:     - obs_nvenc_h264_tex (NVIDIA NVENC H.264)
00:00:00.001:   Audio Encoders:
`);

  assert.equal(analyse.hardware.av1, true);
  assert.equal(analyse.empfehlung.codec, 'AV1');
  assert.equal(analyse.empfehlung.encoder, 'NVIDIA NVENC AV1');
  assert.equal(analyse.empfehlung.ratensteuerung, 'Variable Bitrate mit Zielqualität');
});

test('AMD mit AMF AV1 empfiehlt Hardware-AV1 und HQCBR nur falls angeboten', () => {
  const analyse = analysiereObsLog(`
00:00:00.000: Adapter 0: AMD Radeon RX 7900 XTX
00:00:00.001: Available Encoders:
00:00:00.001:   Video Encoders:
00:00:00.001:     - av1_texture_amf (AMD HW AV1)
00:00:00.001:     - h265_texture_amf (AMD HW H.265)
00:00:00.001:     - h264_texture_amf (AMD HW H.264)
00:00:00.001:   Audio Encoders:
`);

  assert.equal(analyse.hersteller, 'amd');
  assert.equal(analyse.hardware.av1, true);
  assert.equal(analyse.empfehlung.encoder, 'AMD Hardware AV1');
  assert.match(analyse.empfehlung.ratensteuerung, /HQCBR/);
});

test('Software-AV1 ohne Hardwareencoder wird nicht als Live-AV1 empfohlen', () => {
  const analyse = analysiereObsLog(`
00:00:00.000: Adapter 0: Microsoft Basic Render Driver
00:00:00.001: Available Encoders:
00:00:00.001:   Video Encoders:
00:00:00.001:     - ffmpeg_svt_av1 (SVT-AV1)
00:00:00.001:     - ffmpeg_aom_av1 (AOM AV1)
00:00:00.001:     - obs_x264 (x264)
00:00:00.001:   Audio Encoders:
`);

  assert.equal(analyse.softwareAv1, true);
  assert.equal(analyse.hardware.av1, false);
  assert.equal(analyse.empfehlung.codec, 'H.264');
});
