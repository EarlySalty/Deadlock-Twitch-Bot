import { useCallback, useEffect, useRef, useState } from 'react';
import { createPortal } from 'react-dom';
import { AnimatePresence, motion } from 'framer-motion';
import { Heart, Lock, Share2, Star, X } from 'lucide-react';
import { useT } from '@/context/LanguageContext';
import {
  erkenneBrowser,
  kartenPosition,
  lesezeichenAnleitung,
  type LesezeichenAnleitung,
} from '@/utils/browserErkennung';

const STORAGE_KEY = 'lesezeichen-hinweis-erledigt';
const SHOW_DELAY_MS = 450;
const BRAVE_TIMEOUT_MS = 300;
const COPY_RESET_MS = 2000;

const FALLBACK_ANLEITUNG: LesezeichenAnleitung = {
  position: 'unbekannt',
  symbol: null,
  tastenkombi: ['Strg', 'D'],
  hinweis: '',
};

interface BraveNavigator {
  brave?: { isBrave?: () => Promise<boolean> };
  userAgentData?: { platform?: string; mobile?: boolean };
}

interface LesezeichenHinweisProps {
  onErledigt?: () => void;
}

function useAnleitung(): LesezeichenAnleitung | null {
  const [anleitung, setAnleitung] = useState<LesezeichenAnleitung | null>(null);

  useEffect(() => {
    let aktiv = true;
    const nav = navigator as Navigator & BraveNavigator;
    const userAgent = nav.userAgent || '';
    const platform = nav.userAgentData?.platform || nav.platform || '';
    const mobile = Boolean(nav.userAgentData?.mobile);

    const abschliessen = (brave: boolean) => {
      if (!aktiv) {
        return;
      }
      setAnleitung(lesezeichenAnleitung(erkenneBrowser({ userAgent, brave, platform, mobile })));
    };

    const braveApi = nav.brave;
    if (braveApi && typeof braveApi.isBrave === 'function') {
      const timer = window.setTimeout(() => abschliessen(false), BRAVE_TIMEOUT_MS);
      braveApi
        .isBrave()
        .then((ergebnis) => {
          window.clearTimeout(timer);
          abschliessen(Boolean(ergebnis));
        })
        .catch(() => {
          window.clearTimeout(timer);
          abschliessen(false);
        });
    } else {
      abschliessen(false);
    }

    return () => {
      aktiv = false;
    };
  }, []);

  return anleitung;
}

export function LesezeichenHinweis({ onErledigt }: LesezeichenHinweisProps) {
  const t = useT();
  const anleitung = useAnleitung();
  const [sichtbar, setSichtbar] = useState(false);
  const [kopiert, setKopiert] = useState(false);
  const onErledigtRef = useRef(onErledigt);

  useEffect(() => {
    onErledigtRef.current = onErledigt;
  }, [onErledigt]);

  useEffect(() => {
    let bereits: boolean;
    try {
      bereits = localStorage.getItem(STORAGE_KEY) !== null;
    } catch {
      bereits = false;
    }
    if (bereits) {
      onErledigtRef.current?.();
      return undefined;
    }
    const timer = window.setTimeout(() => setSichtbar(true), SHOW_DELAY_MS);
    return () => window.clearTimeout(timer);
  }, []);

  const abschliessen = useCallback(() => {
    try {
      localStorage.setItem(STORAGE_KEY, new Date().toISOString());
    } catch {
      void 0;
    }
    setSichtbar(false);
    onErledigtRef.current?.();
  }, []);

  useEffect(() => {
    if (!sichtbar) {
      return undefined;
    }
    const handler = (event: KeyboardEvent) => {
      if (event.key === 'Escape') {
        event.preventDefault();
        abschliessen();
      }
    };
    window.addEventListener('keydown', handler);
    return () => window.removeEventListener('keydown', handler);
  }, [sichtbar, abschliessen]);

  const link =
    typeof window !== 'undefined'
      ? `${window.location.origin}/twitch/dashboard`
      : '/twitch/dashboard';

  const kopieren = useCallback(async () => {
    try {
      await navigator.clipboard.writeText(link);
      setKopiert(true);
      window.setTimeout(() => setKopiert(false), COPY_RESET_MS);
    } catch {
      setKopiert(false);
    }
  }, [link]);

  const effektiv = anleitung ?? FALLBACK_ANLEITUNG;
  const pos = kartenPosition(effektiv);
  const istMobil = pos.seite === 'menue';
  const zeigeAdressleiste = effektiv.position === 'links' || effektiv.position === 'rechts';
  const symbolLinks = effektiv.position === 'links';
  const SymbolIcon = effektiv.symbol === 'herz' ? Heart : effektiv.symbol === 'teilen' ? Share2 : Star;

  const karteStyle = istMobil
    ? { width: 'min(360px, calc(100vw - 32px))' }
    : {
        width: 'min(360px, calc(100vw - 32px))',
        top: pos.top,
        left: pos.left,
        right: pos.right,
      };

  const pulsSymbol = (
    <motion.span
      aria-hidden="true"
      className="inline-flex h-6 w-6 shrink-0 items-center justify-center rounded-md text-[color:var(--color-primary)]"
      animate={{ scale: [1, 1.18, 1], opacity: [1, 0.65, 1] }}
      transition={{ duration: 1.6, ease: 'easeInOut', repeat: Number.POSITIVE_INFINITY }}
    >
      <SymbolIcon className="h-4 w-4" />
    </motion.span>
  );

  return createPortal(
    <AnimatePresence>
      {sichtbar && (
        <>
          <motion.div
            key="lesezeichen-backdrop"
            aria-hidden="true"
            onClick={abschliessen}
            initial={{ opacity: 0 }}
            animate={{ opacity: 1 }}
            exit={{ opacity: 0 }}
            transition={{ duration: 0.24, ease: 'easeOut' }}
            className="fixed inset-0 z-[102]"
            style={{ background: 'rgba(0,0,0,0.6)' }}
          />
          <motion.div
            key="lesezeichen-karte"
            role="dialog"
            aria-live="polite"
            initial={{ opacity: 0, y: -12 }}
            animate={{ opacity: 1, y: 0 }}
            exit={{ opacity: 0, y: -12 }}
            transition={{ duration: 0.24, ease: 'easeOut' }}
            className={`fixed z-[103] ${istMobil ? 'bottom-4 left-1/2 -translate-x-1/2' : ''}`}
            style={karteStyle}
          >
          <div
            className="panel-card relative rounded-[20px] p-4"
            style={{
              border: '1px solid #C5A059',
              background: 'linear-gradient(160deg,#2a211b,#1a1310)',
              boxShadow:
                '0 0 0 1px rgba(197,160,89,0.35), 0 24px 70px -20px rgba(197,160,89,0.45)',
            }}
          >
            {zeigeAdressleiste && (
              <div
                aria-hidden="true"
                className={`absolute -top-1.5 h-3 w-3 rotate-45 ${symbolLinks ? 'left-6' : 'right-6'}`}
                style={{
                  borderLeft: '1px solid #C5A059',
                  borderTop: '1px solid #C5A059',
                  background: '#2a211b',
                }}
              />
            )}

            <button
              type="button"
              onClick={abschliessen}
              className="absolute right-3 top-3 inline-flex h-8 w-8 items-center justify-center rounded-xl border border-[color:var(--color-border)] bg-white/5 text-[color:var(--color-text-secondary)] transition-colors hover:text-white"
              aria-label={t('Schließen')}
            >
              <X className="h-4 w-4" />
            </button>

            <h3
              className="mb-1 pr-10 text-[1.1rem] font-bold text-white"
              style={{ fontFamily: 'var(--font-display)' }}
            >
              {t('Speichere dir dein Partner Dashboard')}
            </h3>
            <p className="mb-3 text-sm leading-relaxed text-[color:var(--color-text-secondary)]">
              {t('Damit findest du dein Dashboard jederzeit mit einem Klick wieder.')}
            </p>

            {zeigeAdressleiste && (
              <div
                className="mb-3 flex items-center gap-2 rounded-xl border bg-black/40 px-3 py-2"
                style={{ borderColor: 'rgba(197,160,89,0.35)' }}
              >
                {symbolLinks && pulsSymbol}
                <Lock className="h-3.5 w-3.5 shrink-0 text-[color:var(--color-text-secondary)]" />
                <span className="flex-1 truncate text-xs text-[color:var(--color-text-secondary)]">
                  deutsche-deadlock-community.de/twitch/dashboard
                </span>
                {!symbolLinks && pulsSymbol}
              </div>
            )}

            {effektiv.hinweis ? (
              <p className="mb-3 text-sm leading-relaxed text-white/90">{t(effektiv.hinweis)}</p>
            ) : null}

            {!istMobil && (
              <div className="mb-4 flex items-center gap-2 text-sm text-[color:var(--color-text-secondary)]">
                <span>{t('Tastenkombination')}</span>
                <kbd className="rounded-md border border-[color:var(--color-border)] bg-white/5 px-2 py-1 text-xs font-semibold text-white">
                  {effektiv.tastenkombi[0]}
                </kbd>
                <span>+</span>
                <kbd className="rounded-md border border-[color:var(--color-border)] bg-white/5 px-2 py-1 text-xs font-semibold text-white">
                  {effektiv.tastenkombi[1]}
                </kbd>
              </div>
            )}

            <div className="flex items-center justify-between gap-3">
              {!istMobil ? (
                <button
                  type="button"
                  onClick={() => void kopieren()}
                  className="rounded-xl border border-[color:var(--color-border)] px-3 py-2 text-sm font-semibold text-[color:var(--color-text-secondary)] transition-colors hover:text-white"
                >
                  {kopiert ? t('Kopiert') : t('Link kopieren')}
                </button>
              ) : (
                <span />
              )}

              <button
                type="button"
                onClick={abschliessen}
                className="inline-flex items-center gap-2 rounded-xl bg-[linear-gradient(135deg,var(--color-primary),var(--color-accent))] px-4 py-2 text-sm font-bold text-white transition-opacity hover:opacity-90"
              >
                {t('Erledigt')}
              </button>
            </div>
          </div>
          </motion.div>
        </>
      )}
    </AnimatePresence>,
    document.body,
  );
}
