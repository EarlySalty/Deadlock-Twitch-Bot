import { useEffect, useLayoutEffect, useRef, useState } from 'react';
import { createPortal } from 'react-dom';
import { AnimatePresence, motion } from 'framer-motion';
import { ArrowLeft, ArrowRight, X } from 'lucide-react';

const STORAGE_KEY = 'analytics-tour-dismissed';
const PENDING_KEY = 'analytics-tour-pending';
const TOUR_DELAY_MS = 600;
const EXIT_DURATION_MS = 240;
const SCROLL_SETTLE_MS = 280;
const SPOTLIGHT_PADDING = 12;
const SPOTLIGHT_GAP = 18;
const VIEWPORT_MARGIN = 16;
const POPOVER_WIDTH = 320;
const POPOVER_FALLBACK_HEIGHT = 216;

interface TourStep {
  anchor: string;
  tag: string;
  title: string;
  description: string;
}

interface AnalyticsTourProps {
  onComplete?: () => void;
}

interface SpotlightRect {
  top: number;
  left: number;
  width: number;
  height: number;
}

interface PopoverSize {
  width: number;
  height: number;
}

interface PopoverPosition {
  top: number;
  left: number;
  placement: 'top' | 'bottom';
}

const TOUR_STEPS: TourStep[] = [
  {
    anchor: 'tour-analytics-kpis',
    tag: 'KPIs',
    title: 'Deine Stream-Daten auf einen Blick',
    description:
      'Durchschnittliche Viewer, Follower-Gewinne, Retention und Hours Watched — nach jedem Stream automatisch erfasst und hier zusammengefasst.',
  },
  {
    anchor: 'tour-analytics-insights',
    tag: 'Insights',
    title: 'Was der Bot für dich analysiert',
    description:
      'Hier erscheinen konkrete KI-Hinweise aus deinen letzten Sessions: was gut läuft, was du verbessern kannst und wann du am effektivsten streamst.',
  },
  {
    anchor: 'tour-analytics-growth',
    tag: 'Wachstum',
    title: 'Dein Langzeittrend',
    description:
      'Follower- und Viewer-Entwicklung über Wochen und Monate. Erkenne Muster, vergleiche Zeiträume und plane deinen Content gezielter.',
  },
  {
    anchor: 'tour-analytics-chat',
    tag: 'Chat',
    title: 'Wer ist wirklich aktiv?',
    description:
      'Top-Chatter, Aktivitätsmuster und Chat-Engagement pro Session. Verstehe welche Inhalte deine Community in Bewegung bringen.',
  },
  {
    anchor: 'tour-analytics-coaching',
    tag: 'Coaching',
    title: 'Persönliches KI-Coaching',
    description:
      'Individuelle Handlungsempfehlungen auf Basis deiner echten Stream-Daten — nicht generisch, sondern auf deinen Kanal zugeschnitten.',
  },
];

function clamp(value: number, min: number, max: number) {
  if (max < min) return min;
  return Math.min(Math.max(value, min), max);
}

function getAnchorElement(anchor: string) {
  const node = document.querySelector(`[data-tour-id="${anchor}"]`);
  return node instanceof HTMLElement ? node : null;
}

function getSpotlightRect(element: HTMLElement): SpotlightRect {
  const rect = element.getBoundingClientRect();
  const left = clamp(rect.left - SPOTLIGHT_PADDING, VIEWPORT_MARGIN, window.innerWidth - VIEWPORT_MARGIN);
  const top = clamp(rect.top - SPOTLIGHT_PADDING, VIEWPORT_MARGIN, window.innerHeight - VIEWPORT_MARGIN);
  const right = clamp(rect.right + SPOTLIGHT_PADDING, VIEWPORT_MARGIN, window.innerWidth - VIEWPORT_MARGIN);
  const bottom = clamp(rect.bottom + SPOTLIGHT_PADDING, VIEWPORT_MARGIN, window.innerHeight - VIEWPORT_MARGIN);
  return {
    top,
    left,
    width: Math.max(right - left, 0),
    height: Math.max(bottom - top, 0),
  };
}

function getPopoverPosition(targetRect: SpotlightRect, popoverSize: PopoverSize): PopoverPosition {
  const width = popoverSize.width || POPOVER_WIDTH;
  const height = popoverSize.height || POPOVER_FALLBACK_HEIGHT;
  const placement = targetRect.top > window.innerHeight / 2 ? 'top' : 'bottom';
  const rawTop =
    placement === 'top'
      ? targetRect.top - height - SPOTLIGHT_GAP
      : targetRect.top + targetRect.height + SPOTLIGHT_GAP;
  const alignLeft = targetRect.left + targetRect.width / 2 < window.innerWidth / 2;
  const rawLeft = alignLeft ? targetRect.left : targetRect.left + targetRect.width - width;
  return {
    placement,
    top: clamp(rawTop, VIEWPORT_MARGIN, window.innerHeight - height - VIEWPORT_MARGIN),
    left: clamp(rawLeft, VIEWPORT_MARGIN, window.innerWidth - width - VIEWPORT_MARGIN),
  };
}

export function AnalyticsTour({ onComplete }: AnalyticsTourProps) {
  const [isVisible, setIsVisible] = useState(false);
  const [currentStep, setCurrentStep] = useState(0);
  const [isExiting, setIsExiting] = useState(false);
  const [targetRect, setTargetRect] = useState<SpotlightRect | null>(null);
  const [popoverSize, setPopoverSize] = useState<PopoverSize>({
    width: POPOVER_WIDTH,
    height: POPOVER_FALLBACK_HEIGHT,
  });

  const popoverRef = useRef<HTMLDivElement | null>(null);
  const measureFrameRef = useRef<number | null>(null);
  const scrollTimeoutRef = useRef<number | null>(null);
  const dismissTimeoutRef = useRef<number | null>(null);

  const findAvailableStep = (startIndex: number, direction: 1 | -1) => {
    for (let index = startIndex; index >= 0 && index < TOUR_STEPS.length; index += direction) {
      if (getAnchorElement(TOUR_STEPS[index].anchor)) return index;
    }
    return null;
  };

  const dismissTour = (persist = true) => {
    if (isExiting) return;
    setIsExiting(true);
    if (dismissTimeoutRef.current !== null) window.clearTimeout(dismissTimeoutRef.current);
    dismissTimeoutRef.current = window.setTimeout(() => {
      if (persist) localStorage.setItem(STORAGE_KEY, 'true');
      setTargetRect(null);
      setIsVisible(false);
      setIsExiting(false);
      onComplete?.();
      dismissTimeoutRef.current = null;
    }, EXIT_DURATION_MS);
  };

  useEffect(() => {
    const dismissed = localStorage.getItem(STORAGE_KEY);
    const pending = localStorage.getItem(PENDING_KEY);
    if (dismissed || !pending) return undefined;

    localStorage.removeItem(PENDING_KEY);
    const timer = window.setTimeout(() => {
      setIsVisible(true);
    }, TOUR_DELAY_MS);
    return () => window.clearTimeout(timer);
  }, []);

  useEffect(() => {
    return () => {
      if (measureFrameRef.current !== null) window.cancelAnimationFrame(measureFrameRef.current);
      if (scrollTimeoutRef.current !== null) window.clearTimeout(scrollTimeoutRef.current);
      if (dismissTimeoutRef.current !== null) window.clearTimeout(dismissTimeoutRef.current);
    };
  }, []);

  useEffect(() => {
    if (!isVisible || isExiting) return undefined;
    const handleKeyDown = (event: KeyboardEvent) => {
      if (event.key === 'Escape') { event.preventDefault(); dismissTour(); }
    };
    window.addEventListener('keydown', handleKeyDown);
    return () => window.removeEventListener('keydown', handleKeyDown);
  }, [isExiting, isVisible]);

  useLayoutEffect(() => {
    if (!isVisible || isExiting) return;
    const nextStep = findAvailableStep(currentStep, 1);
    if (nextStep === null) { dismissTour(false); return; }
    if (nextStep !== currentStep) setCurrentStep(nextStep);
  }, [currentStep, isExiting, isVisible]);

  useLayoutEffect(() => {
    if (!isVisible || isExiting) return undefined;
    const step = TOUR_STEPS[currentStep];
    const target = getAnchorElement(step.anchor);
    if (!target) {
      const nextStep = findAvailableStep(currentStep + 1, 1);
      if (nextStep === null) dismissTour(false);
      else setCurrentStep(nextStep);
      return undefined;
    }

    const measure = () => {
      const element = getAnchorElement(step.anchor);
      if (!element) return;
      setTargetRect(getSpotlightRect(element));
    };

    const scheduleMeasure = () => {
      if (measureFrameRef.current !== null) window.cancelAnimationFrame(measureFrameRef.current);
      measureFrameRef.current = window.requestAnimationFrame(() => {
        measureFrameRef.current = null;
        measure();
      });
    };

    target.scrollIntoView({ block: 'center', inline: 'nearest', behavior: 'smooth' });
    scheduleMeasure();
    scrollTimeoutRef.current = window.setTimeout(() => {
      scheduleMeasure();
      scrollTimeoutRef.current = null;
    }, SCROLL_SETTLE_MS);

    const resizeObserver = new ResizeObserver(() => scheduleMeasure());
    resizeObserver.observe(target);
    window.addEventListener('resize', scheduleMeasure);
    window.addEventListener('scroll', scheduleMeasure, { passive: true });

    return () => {
      resizeObserver.disconnect();
      window.removeEventListener('resize', scheduleMeasure);
      window.removeEventListener('scroll', scheduleMeasure);
      if (measureFrameRef.current !== null) { window.cancelAnimationFrame(measureFrameRef.current); measureFrameRef.current = null; }
      if (scrollTimeoutRef.current !== null) { window.clearTimeout(scrollTimeoutRef.current); scrollTimeoutRef.current = null; }
    };
  }, [currentStep, isExiting, isVisible]);

  useLayoutEffect(() => {
    if (!isVisible || !popoverRef.current) return undefined;
    const updateSize = () => {
      if (!popoverRef.current) return;
      setPopoverSize({ width: popoverRef.current.offsetWidth, height: popoverRef.current.offsetHeight });
    };
    updateSize();
    const resizeObserver = new ResizeObserver(() => updateSize());
    resizeObserver.observe(popoverRef.current);
    return () => resizeObserver.disconnect();
  }, [currentStep, isVisible, targetRect]);

  const handleBack = () => {
    const previousStep = findAvailableStep(currentStep - 1, -1);
    if (previousStep !== null) setCurrentStep(previousStep);
  };

  const handleNext = () => {
    const nextStep = findAvailableStep(currentStep + 1, 1);
    if (nextStep === null) { dismissTour(); return; }
    setCurrentStep(nextStep);
  };

  if (!isVisible) return null;

  const portalTarget = document.body;
  const step = TOUR_STEPS[currentStep];
  const previousStep = findAvailableStep(currentStep - 1, -1);
  const nextStep = findAvailableStep(currentStep + 1, 1);
  const availableSteps = TOUR_STEPS.filter(({ anchor }) => getAnchorElement(anchor)).length || TOUR_STEPS.length;
  const stepNumber =
    TOUR_STEPS.slice(0, currentStep + 1).filter(({ anchor }) => getAnchorElement(anchor)).length || currentStep + 1;
  const popoverPosition = targetRect ? getPopoverPosition(targetRect, popoverSize) : null;

  return createPortal(
    <AnimatePresence>
      {!isExiting && targetRect && popoverPosition && (
        <motion.div
          initial={{ opacity: 0 }}
          animate={{ opacity: 1 }}
          exit={{ opacity: 0 }}
          transition={{ duration: 0.24, ease: 'easeOut' }}
          className="fixed inset-0 z-[100]"
        >
          <div className="absolute inset-0" aria-hidden="true" />

          <motion.div
            aria-hidden="true"
            className="pointer-events-none fixed z-[101]"
            animate={{
              left: targetRect.left,
              top: targetRect.top,
              width: targetRect.width,
              height: targetRect.height,
            }}
            transition={{ type: 'spring', stiffness: 360, damping: 34, mass: 0.85 }}
          >
            <div
              className="absolute inset-0 rounded-[12px]"
              style={{ boxShadow: '0 0 0 9999px rgba(7, 21, 29, 0.72)' }}
            />
            <motion.div
              className="absolute inset-0 rounded-[12px] border-2 border-[color:var(--color-primary)]"
              style={{ boxShadow: '0 0 30px rgba(255, 122, 24, 0.5)' }}
              animate={{ scale: [1, 1.02, 1], opacity: [1, 0.7, 1] }}
              transition={{ duration: 2, ease: 'easeInOut', repeat: Number.POSITIVE_INFINITY }}
            />
          </motion.div>

          <motion.div
            ref={popoverRef}
            role="dialog"
            aria-live="polite"
            className="fixed z-[102] pointer-events-auto"
            style={{ width: 'min(320px, calc(100vw - 32px))' }}
            animate={{ left: popoverPosition.left, top: popoverPosition.top }}
            transition={{ type: 'spring', stiffness: 380, damping: 36, mass: 0.9 }}
          >
            <div className="panel-card rounded-[20px] border border-[color:rgba(255,122,24,0.3)] bg-[linear-gradient(160deg,#143048,#0d2232)] p-4 shadow-[0_24px_60px_-24px_rgba(0,0,0,0.78)]">
              <button
                type="button"
                onClick={() => dismissTour()}
                className="absolute right-3 top-3 inline-flex h-8 w-8 items-center justify-center rounded-xl border border-[color:var(--color-border)] bg-white/5 text-[color:var(--color-text-secondary)] transition-colors hover:text-white"
                aria-label="Tour überspringen"
              >
                <X className="h-4 w-4" />
              </button>

              <AnimatePresence mode="wait" initial={false}>
                <motion.div
                  key={step.anchor}
                  initial={{ opacity: 0, y: popoverPosition.placement === 'top' ? 10 : -10 }}
                  animate={{ opacity: 1, y: 0 }}
                  exit={{ opacity: 0, y: popoverPosition.placement === 'top' ? -10 : 10 }}
                  transition={{ duration: 0.2, ease: 'easeOut' }}
                >
                  <div className="mb-2 pr-10 text-[0.72rem] font-bold uppercase tracking-[0.22em] text-[color:var(--color-primary)]">
                    {`Schritt ${stepNumber} / ${availableSteps} · ${step.tag}`}
                  </div>
                  <h3
                    className="mb-2 text-[1.15rem] font-bold text-white"
                    style={{ fontFamily: 'var(--font-display)' }}
                  >
                    {step.title}
                  </h3>
                  <p className="mb-5 text-sm leading-relaxed text-[color:var(--color-text-secondary)]">
                    {step.description}
                  </p>

                  <div className="flex items-center justify-between gap-3">
                    <button
                      type="button"
                      onClick={() => dismissTour()}
                      className="text-sm font-semibold text-[color:var(--color-text-secondary)] transition-colors hover:text-white"
                    >
                      Überspringen
                    </button>

                    <div className="flex items-center gap-2">
                      <button
                        type="button"
                        onClick={handleBack}
                        disabled={previousStep === null}
                        className="inline-flex items-center gap-2 rounded-xl border border-[color:var(--color-border)] px-3 py-2 text-sm font-semibold text-[color:var(--color-text-secondary)] transition-colors hover:text-white disabled:cursor-not-allowed disabled:opacity-40"
                      >
                        <ArrowLeft className="h-4 w-4" />
                        Zurück
                      </button>

                      <button
                        type="button"
                        onClick={handleNext}
                        className="inline-flex items-center gap-2 rounded-xl bg-[linear-gradient(135deg,var(--color-primary),var(--color-accent))] px-4 py-2 text-sm font-bold text-white transition-opacity hover:opacity-90"
                      >
                        {nextStep === null ? 'Fertig' : 'Weiter'}
                        {nextStep !== null && <ArrowRight className="h-4 w-4" />}
                      </button>
                    </div>
                  </div>
                </motion.div>
              </AnimatePresence>
            </div>
          </motion.div>
        </motion.div>
      )}
    </AnimatePresence>,
    portalTarget,
  );
}

export function resetAnalyticsTour() {
  localStorage.removeItem(STORAGE_KEY);
  localStorage.removeItem(PENDING_KEY);
}
