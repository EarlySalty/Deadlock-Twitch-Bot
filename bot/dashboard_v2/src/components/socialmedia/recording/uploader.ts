export const CHUNK_MAX_BYTES = 2 * 1024 * 1024;
export const SESSION_MAX_BYTES = 20 * 1024 * 1024;
export const KEEPALIVE_MAX_BYTES = 60 * 1024;

export const RETRY_DELAYS_MS = [1_000, 4_000, 15_000];

export type ChunkSender = (body: string, keepalive: boolean) => Promise<{ status: number }>;
export type Sleep = (ms: number) => Promise<void>;

export interface ChunkMeta {
  viewport_width: number;
  viewport_height: number;
}

const encoder = new TextEncoder();

function byteLength(text: string): number {
  return encoder.encode(text).length;
}

const defaultSleep: Sleep = (ms) => new Promise((resolve) => { setTimeout(resolve, ms); });

function stopsRecording(status: number): boolean {
  return status >= 400 && status < 500 && status !== 409;
}

function isRetryable(status: number | null): boolean {
  return status === null || status >= 500;
}

export class RecordingUploader {
  private buffer: string[] = [];
  private bufferBytes = 0;
  private seq = 0;
  private sentBytes = 0;
  private queue: Promise<void> = Promise.resolve();
  private readonly sessionId: string;
  private readonly send: ChunkSender;
  private readonly meta: ChunkMeta;
  private readonly onStop: () => void;
  private readonly sleep: Sleep;
  stopped = false;

  constructor(sessionId: string, send: ChunkSender, meta: ChunkMeta, onStop: () => void, sleep: Sleep = defaultSleep) {
    this.sessionId = sessionId;
    this.send = send;
    this.meta = meta;
    this.onStop = onStop;
    this.sleep = sleep;
  }

  push(event: unknown): void {
    if (this.stopped) return;
    const json = JSON.stringify(event);
    const size = byteLength(json) + 1;
    if (size > CHUNK_MAX_BYTES - 1024) {
      this.stop();
      return;
    }
    if (this.bufferBytes + size > CHUNK_MAX_BYTES - 1024) void this.flush(false);
    this.buffer.push(json);
    this.bufferBytes += size;
  }

  flush(keepalive: boolean): Promise<void> {
    if (this.stopped || this.buffer.length === 0) return this.queue;
    const body = this.nextBody();
    const useKeepalive = keepalive && byteLength(body) <= KEEPALIVE_MAX_BYTES;
    this.queue = this.queue.then(() => this.deliver(body, useKeepalive));
    return this.queue;
  }

  discard(): void {
    this.buffer = [];
    this.bufferBytes = 0;
    this.stopped = true;
  }

  private nextBody(): string {
    const events = this.buffer;
    this.buffer = [];
    this.bufferBytes = 0;
    const seq = this.seq;
    this.seq += 1;
    return `{"session_id":${JSON.stringify(this.sessionId)},"seq":${seq},"meta":${JSON.stringify(this.meta)},"events":[${events.join(',')}]}`;
  }

  private async deliver(body: string, keepalive: boolean): Promise<void> {
    if (this.stopped) return;
    this.sentBytes += byteLength(body);
    if (this.sentBytes > SESSION_MAX_BYTES) {
      this.stop();
      return;
    }
    for (let attempt = 0; ; attempt += 1) {
      if (this.stopped) return;
      const status = await this.attempt(body, keepalive && attempt === 0);
      if (!isRetryable(status)) {
        if (status !== null && stopsRecording(status)) this.stop();
        return;
      }
      if (attempt >= RETRY_DELAYS_MS.length) {
        this.stop();
        return;
      }
      await this.sleep(RETRY_DELAYS_MS[attempt]);
    }
  }

  private async attempt(body: string, keepalive: boolean): Promise<number | null> {
    try {
      return (await this.send(body, keepalive)).status;
    } catch {
      return null;
    }
  }

  private stop(): void {
    if (this.stopped) return;
    this.discard();
    this.onStop();
  }
}
