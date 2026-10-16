import { fetchJson, withCookieCredentials } from "@/api/core";
import type { LoadSourcesPage, PipelinePage } from "../../pipelineApi";
import type { ImportPoolApi, PoolPreferences } from "./types";

const PREFIX = "/twitch/social-media/api/autopilot";
const PIPELINE_SOURCES = "/twitch/social-media/api/pipeline/sources";

function endpoint(
  owner: string,
  path: string,
  pagination?: { limit: number; offset: number },
): string {
  if (!/^\d+$/.test(owner))
    throw new Error("Bitte einen Twitch-Kanal auswählen.");
  const query = new URLSearchParams({ twitch_user_id: owner });
  if (pagination) {
    query.set("limit", String(pagination.limit));
    query.set("offset", String(pagination.offset));
  }
  return `${PREFIX}${path}?${query}`;
}

function request<T>(url: string, init: RequestInit = {}): Promise<T> {
  return fetchJson<T>(url, withCookieCredentials(init));
}

function jsonBody(value: unknown): RequestInit {
  return {
    method: "PUT",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(value),
  };
}

export const fetchPipelineSources: LoadSourcesPage = (owner, limit, offset, signal) => {
  if (!/^\d+$/.test(owner)) return Promise.reject(new Error("Bitte einen Twitch-Kanal auswählen."));
  const query = new URLSearchParams({ twitch_user_id: owner, limit: String(limit), offset: String(offset) });
  return request<PipelinePage>(`${PIPELINE_SOURCES}?${query}`, { signal });
};

export const importPoolApi: ImportPoolApi = {
  getPreferences: (owner, signal) =>
    request(endpoint(owner, "/preferences"), { signal }),
  savePreferences: (owner, preferences) => {
    const body: PoolPreferences = {
      clip_date_filter: preferences.clip_date_filter,
      clip_sort: preferences.clip_sort,
      timezone: preferences.timezone,
    };
    return request(endpoint(owner, "/preferences"), jsonBody(body));
  },
  getPool: (owner, limit, offset, signal) =>
    request(endpoint(owner, "/pool", { limit, offset }), { signal }),
  getLatestImport: (owner, signal) =>
    request(endpoint(owner, "/imports"), { signal }),
  getImport: (owner, id, signal) =>
    request(endpoint(owner, `/imports/${id}`), { signal }),
  startImport: (owner) =>
    request(endpoint(owner, "/imports"), { method: "POST" }),
  resumeImport: (owner, id) =>
    request(endpoint(owner, `/imports/${id}/resume`), { method: "POST" }),
  setSelection: (owner, clipId, selected) =>
    request(
      endpoint(owner, `/pool/${encodeURIComponent(clipId)}/selection`),
      jsonBody({ selected }),
    ),
  getSources: fetchPipelineSources,
};
