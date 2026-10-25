export const CHUNK_MAX_BYTES = 2 * 1024 * 1024;
export const SESSION_MAX_BYTES = 20 * 1024 * 1024;
export const KEEPALIVE_MAX_BYTES = 60 * 1024;

export type ChunkSender = (body: string, keepalive: boolean) => Promise<{ status: number }>;

export interface ChunkMeta {
  viewport_width: number;
  viewport_height: number;
}

const encoder = new TextEncoder();

function byteLength(text: string): number {
  return encoder.encode(text).length;
}

function stopsRecording(status: number): boolean {
  return status >= 400 && status < 500 && status !== 409;
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
  stopped = false;

  constructor(sessionId: string, send: ChunkSender, meta: ChunkMeta, onStop: () => void) {
    this.sessionId = sessionId;
    this.send = send;
    this.meta = meta;
    this.onStop = onStop;
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
    if (keepalive) return this.deliver(body, byteLength(body) <= KEEPALIVE_MAX_BYTES);
    this.queue = this.queue.then(() => this.deliver(body, false));
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
    try {
      const response = await this.send(body, keepalive);
      if (stopsRecording(response.status)) this.stop();
    } catch {
      return;
    }
  }

  private stop(): void {
    if (this.stopped) return;
    this.discard();
    this.onStop();
  }
}
