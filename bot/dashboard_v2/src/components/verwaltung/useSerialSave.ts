import { useCallback, useEffect, useRef } from 'react';

export type SaveOutcome<R> = { ok: true; result: R } | { ok: false; error: unknown };

export function useSerialSave<T, R>(
  send: (value: T) => Promise<R>,
  settle: (sent: T, outcome: SaveOutcome<R>) => void,
): (value: T) => void {
  const sendRef = useRef(send);
  const settleRef = useRef(settle);
  const running = useRef(false);
  const queued = useRef<{ value: T } | null>(null);

  useEffect(() => {
    sendRef.current = send;
    settleRef.current = settle;
  });

  const drain = useCallback(async () => {
    running.current = true;
    for (let next = queued.current; next; next = queued.current) {
      queued.current = null;
      let outcome: SaveOutcome<R>;
      try {
        outcome = { ok: true, result: await sendRef.current(next.value) };
      } catch (error) {
        outcome = { ok: false, error };
      }
      settleRef.current(next.value, outcome);
    }
    running.current = false;
  }, []);

  return useCallback((value: T) => {
    queued.current = { value };
    if (!running.current) void drain();
  }, [drain]);
}
