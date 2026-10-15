import { useCallback, useEffect, useRef, useState } from 'react';

export interface SettingToggleState {
  enabled: boolean | null;
  loading: boolean;
  pending: boolean;
  error: string | null;
  savedAt: number | null;
  toggle: () => Promise<void>;
}

export function useSettingToggle(load: () => Promise<boolean>, save: (next: boolean) => Promise<boolean>): SettingToggleState {
  const [enabled, setEnabled] = useState<boolean | null>(null);
  const [loading, setLoading] = useState(true);
  const [pending, setPending] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [savedAt, setSavedAt] = useState<number | null>(null);
  const loadRef = useRef(load);
  const saveRef = useRef(save);

  useEffect(() => {
    let active = true;
    loadRef.current()
      .then(value => { if (active) setEnabled(value); })
      .catch(() => { if (active) setError('Status konnte nicht geladen werden. Bitte Seite neu laden.'); })
      .finally(() => { if (active) setLoading(false); });
    return () => { active = false; };
  }, []);

  const toggle = useCallback(async () => {
    if (enabled === null || pending) return;
    setPending(true);
    setError(null);
    setSavedAt(null);
    try {
      setEnabled(await saveRef.current(!enabled));
      setSavedAt(Date.now());
    } catch {
      setError('Speichern fehlgeschlagen. Bitte nochmal versuchen.');
    } finally {
      setPending(false);
    }
  }, [enabled, pending]);

  return { enabled, loading, pending, error, savedAt, toggle };
}
