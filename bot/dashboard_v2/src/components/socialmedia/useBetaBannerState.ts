import { useState } from 'react';

const STORAGE_KEY = 'social-media-early-beta-banner-dismissed';

function wasDismissed(): boolean {
  try {
    return window.localStorage.getItem(STORAGE_KEY) === '1';
  } catch {
    return false;
  }
}

function rememberDismissed() {
  try {
    window.localStorage.setItem(STORAGE_KEY, '1');
  } catch {
    return;
  }
}

export function useBetaBannerState() {
  const [open, setOpen] = useState(() => !wasDismissed());
  return {
    open,
    dismiss() {
      rememberDismissed();
      setOpen(false);
    },
  };
}
