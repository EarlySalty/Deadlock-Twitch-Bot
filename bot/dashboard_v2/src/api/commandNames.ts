export interface CommandNameSetting {
  command: string;
  default_name: string;
  default_aliases: string[];
  custom_name: string | null;
  effective_name: string;
  effective_aliases: string[];
  group: string;
  group_label: string;
  summary: string;
}

export interface CommandNameSaveResult {
  ok: boolean;
  command: string;
  custom_name: string | null;
  effective_name: string;
  effective_aliases: string[];
}

export class CommandNameApiError extends Error {
  status: number;
  code: string;

  constructor(status: number, code: string, message: string) {
    super(message);
    this.name = 'CommandNameApiError';
    this.status = status;
    this.code = code;
  }
}

const BASE = '/twitch/api/v2/streamer/command-names';

async function fetchJson<T>(init: RequestInit = {}): Promise<T> {
  const response = await fetch(BASE, { credentials: 'same-origin', ...init });
  if (!response.ok) {
    let payload: { error?: string; message?: string } = {};
    try {
      payload = await response.json() as { error?: string; message?: string };
    } catch {
      // HTTP-Status bleibt als Fallback sichtbar.
    }
    throw new CommandNameApiError(
      response.status,
      payload.error ?? 'http_error',
      payload.message ?? ('HTTP ' + response.status),
    );
  }
  return await response.json() as T;
}

export function fetchCommandNames(): Promise<{ commands: CommandNameSetting[] }> {
  return fetchJson();
}

export function saveCommandName(command: string, name: string | null): Promise<CommandNameSaveResult> {
  return fetchJson({
    method: 'POST',
    keepalive: true,
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ command, name }),
  });
}

export function validCommandNameInput(value: string): boolean {
  return /^[A-Za-z0-9]*$/.test(value);
}

export interface CommandNameSaveQueue {
  savedName: string;
  lastAttempt: string | null;
  queued: number;
  tail: Promise<void>;
}

export function enqueueCommandNameSave(
  queue: CommandNameSaveQueue,
  requested: string | null,
  save: (name: string) => Promise<void>,
): Promise<void> | null {
  if (requested === null || !validCommandNameInput(requested)
    || (requested === queue.savedName && queue.queued === 0)
    || requested === queue.lastAttempt) return null;
  queue.lastAttempt = requested;
  queue.queued += 1;
  queue.tail = queue.tail.catch(() => {}).then(async () => {
    await save(requested);
    queue.savedName = requested;
  }).finally(() => { queue.queued -= 1; });
  return queue.tail;
}
