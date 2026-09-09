import type { OnboardingUpdate } from '../../api/onboarding';

/** Reihenfolge der Schreibvorgänge erhalten; Pause entwertet ältere Navigationen. */
export class ProgressCoordinator {
  private queue: Promise<boolean> = Promise.resolve(true);
  private generation = 0;
  private lifecycle = 0;
  private disposed = false;
  private write: (update: OnboardingUpdate) => Promise<boolean>;
  constructor(write: (update: OnboardingUpdate) => Promise<boolean>) { this.write = write; }
  invalidate() { this.generation += 1; }
  activate() { this.disposed = false; }
  dispose() { this.invalidate(); this.lifecycle += 1; this.disposed = true; }
  save(update: OnboardingUpdate): Promise<boolean> {
    const lifecycle = this.lifecycle;
    const request = this.queue.then(() => this.disposed || lifecycle !== this.lifecycle ? false : this.write(update)).catch(() => false);
    this.queue = request;
    return request;
  }
  async resume(update: OnboardingUpdate, onCurrent: () => void) {
    const generation = ++this.generation;
    const saved = await this.save(update);
    if (saved && generation === this.generation) onCurrent();
  }
  pause() { this.invalidate(); return this.save({ paused: true }); }
}
