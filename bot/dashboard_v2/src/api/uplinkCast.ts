import { fetchJson, withCookieCredentials } from './core';

export type UplinkCastSourceKind = 'pov' | 'camera';
export type UplinkCastRole = 'single' | 'standby' | 'preview' | 'program';

export interface UplinkCastSession {
  id: number;
  active: boolean;
  source_id?: number | null;
  cast_role?: UplinkCastRole;
  state?: string;
  input_codec?: 'av1' | 'hevc' | 'h264' | null;
  input_bitrate_kbps?: number | null;
  received_bytes?: number;
  error?: string | null;
}

export interface UplinkCastSource {
  source_id: number;
  label: string;
  kind: UplinkCastSourceKind;
  enabled: boolean;
  ingest_key: string;
  ingest_url: string;
  session: UplinkCastSession | null;
}

export interface UplinkCastScene {
  scene_id: number;
  name: string;
  source_id: number;
  sort_order: number;
}

export interface UplinkCastStudio {
  sources: UplinkCastSource[];
  scenes: UplinkCastScene[];
  program_scene_id: number | null;
  preview_scene_id: number | null;
  switch_generation: number;
  limits: { sources: number; scenes: number; online_sources: number };
  preview: {
    available: boolean;
    strategy: string;
    codecs: string[];
    audio: boolean;
    server_encode: boolean;
    standby_output: 'drop';
    reason?: string;
  };
  program_switch: {
    available: boolean;
    transport: string;
    persistent_connection: boolean;
    switch_mode: 'next_keyframe' | string;
    requires_same_track_codecs: boolean;
    seamless_media_mux: boolean;
  };
}

export interface UplinkCastSelectionAck {
  switch_generation: number;
  program_scene_id: number | null;
  preview_scene_id: number | null;
}

export function fetchUplinkCastStudio(): Promise<UplinkCastStudio> {
  return fetchJson<UplinkCastStudio>(
    '/twitch/api/v2/uplink/cast',
    withCookieCredentials(),
  );
}

export function createUplinkCastSource(input: {
  label: string;
  kind: UplinkCastSourceKind;
}): Promise<{
  source_id: number;
  scene_id: number;
  label: string;
  kind: UplinkCastSourceKind;
  ingest_key: string;
  ingest_url: string;
}> {
  return fetchJson(
    '/twitch/api/v2/uplink/cast/sources',
    withCookieCredentials({
      method: 'POST',
      headers: { Accept: 'application/json', 'Content-Type': 'application/json' },
      body: JSON.stringify(input),
    }),
  );
}

export function rotateUplinkCastSource(sourceId: number): Promise<{
  source_id: number;
  ingest_key: string;
}> {
  return fetchJson(
    `/twitch/api/v2/uplink/cast/sources/${sourceId}/rotate`,
    withCookieCredentials({
      method: 'POST',
      headers: { Accept: 'application/json', 'Content-Type': 'application/json' },
      body: '{}',
    }),
  );
}

export function deleteUplinkCastSource(sourceId: number): Promise<{ ok: boolean }> {
  return fetchJson(
    `/twitch/api/v2/uplink/cast/sources/${sourceId}`,
    withCookieCredentials({ method: 'DELETE', headers: { Accept: 'application/json' } }),
  );
}

export function createUplinkCastScene(input: {
  name: string;
  source_id: number;
}): Promise<{ scene_id: number; name: string; source_id: number }> {
  return fetchJson(
    '/twitch/api/v2/uplink/cast/scenes',
    withCookieCredentials({
      method: 'POST',
      headers: { Accept: 'application/json', 'Content-Type': 'application/json' },
      body: JSON.stringify(input),
    }),
  );
}

export function deleteUplinkCastScene(sceneId: number): Promise<{ ok: boolean }> {
  return fetchJson(
    `/twitch/api/v2/uplink/cast/scenes/${sceneId}`,
    withCookieCredentials({ method: 'DELETE', headers: { Accept: 'application/json' } }),
  );
}

export function selectUplinkCastPreview(
  sceneId: number | null,
  expectedGeneration: number,
): Promise<UplinkCastSelectionAck> {
  return fetchJson(
    '/twitch/api/v2/uplink/cast/preview',
    withCookieCredentials({
      method: 'PUT',
      headers: { Accept: 'application/json', 'Content-Type': 'application/json' },
      body: JSON.stringify({ scene_id: sceneId, expected_generation: expectedGeneration }),
    }),
  );
}

export function selectUplinkCastProgram(
  sceneId: number,
  expectedGeneration: number,
): Promise<UplinkCastSelectionAck> {
  return fetchJson(
    '/twitch/api/v2/uplink/cast/program',
    withCookieCredentials({
      method: 'PUT',
      headers: { Accept: 'application/json', 'Content-Type': 'application/json' },
      body: JSON.stringify({
        scene_id: sceneId,
        expected_generation: expectedGeneration,
        swap_preview: true,
      }),
    }),
  );
}
