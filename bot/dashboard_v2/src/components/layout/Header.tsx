import { useEffect, useState } from 'react';
import { AnimatePresence, motion } from 'framer-motion';
import { Activity, ChevronDown, Search, SlidersHorizontal, Sparkles } from 'lucide-react';
import { usePlan } from '@/context/PlanContext';
import { useT } from '@/context/LanguageContext';
import type { TimeRange } from '@/types/analytics';

// Der Marker unter dem aktiven Segment gleitet, statt hart umzuspringen: die
// Auswahl behaelt ihren Ort im Raum. Kritisch gedaempft (bounce 0) — ein
// Ueberschwingen gehoert nur dorthin, wo vorher eine Wischbewegung war.
const SEGMENT_SPRING = { type: 'spring', bounce: 0, duration: 0.32 } as const;

// Menue-Eintritt: 200ms ease-out, aus dem Ausloeser heraus statt aus der Mitte,
// und nie von scale(0) — nichts in der echten Welt entsteht aus dem Nichts.
const MENU_MOTION = {
  initial: { opacity: 0, scale: 0.96, y: -4 },
  animate: { opacity: 1, scale: 1, y: 0 },
  exit: { opacity: 0, scale: 0.97, y: -2 },
} as const;

interface HeaderProps {
  streamer: string | null;
  streamers: { login: string; isPartner: boolean }[];
  days: TimeRange;
  onStreamerChange: (streamer: string | null) => void;
  onDaysChange: (days: TimeRange) => void;
  isLoading?: boolean;
  canViewAllStreamers?: boolean;
  isDemoMode?: boolean;
}

export function Header({
  streamer,
  streamers,
  days,
  onStreamerChange,
  onDaysChange,
  isLoading,
  canViewAllStreamers = false,
  isDemoMode = false,
}: HeaderProps) {
  const { view, setView, hasFullAccess, hasEntitlement } = usePlan();
  const t = useT();
  const [dropdownOpen, setDropdownOpen] = useState(false);
  const [search, setSearch] = useState('');

  const viewOptions: { value: 'basic' | 'extended'; label: string }[] = [
    { value: 'basic', label: t('Basis') },
    { value: 'extended', label: t('Preview') },
  ];

  const timeRanges: { value: TimeRange; label: string }[] = [
    { value: 7, label: '7d' },
    { value: 30, label: '30d' },
    { value: 90, label: '90d' },
  ];

  // Escape schliesst das Menue. Ein Menue, das nur per Klick daneben weggeht,
  // sperrt den Nutzer gefuehlt ein — es muss immer einen Weg heraus geben.
  useEffect(() => {
    if (!dropdownOpen) return;
    const onKey = (event: KeyboardEvent) => {
      if (event.key === 'Escape') {
        setDropdownOpen(false);
        setSearch('');
      }
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [dropdownOpen]);

  const q = search.trim().toLowerCase();
  const partners = streamers.filter(s => s.isPartner && (!q || s.login.includes(q)));
  const others = streamers.filter(s => !s.isPartner && (!q || s.login.includes(q)));
  const allLabel = isDemoMode
    ? t('Demo-Profil')
    : canViewAllStreamers
    ? t('Alle Streamer')
    : t('Alle Partner');
  const canPreviewExtended = !hasFullAccess && !hasEntitlement('analytics');

  // In Beta: Partner koennen vorerst alle Streamer sehen.
  const visiblePartners = partners;
  const visibleOthers = canViewAllStreamers ? others : [];

  return (
    <header className="panel-card rounded-2xl p-4 md:p-6 mb-8">
      <div className="flex flex-col xl:flex-row xl:items-center justify-between gap-5">
        {/* Logo & Title */}
        <div className="flex items-start gap-4">
          <div className="p-3 rounded-2xl bg-gradient-to-br from-primary/30 to-accent/25 border border-primary/25 shadow-lg shadow-primary/10">
            <Activity className="w-6 h-6 text-primary" />
          </div>
          <div>
            <div className="inline-flex items-center gap-2 rounded-full border border-border bg-black/20 px-3 py-1 text-[11px] uppercase tracking-[0.16em] text-text-secondary mb-2">
              <Sparkles className="w-3 h-3 text-accent" />
              Twitch Analytics
            </div>
            <h1 className="display-font text-2xl md:text-3xl font-bold text-white flex items-center gap-2">
              Channel Intelligence
              {isLoading && <span className="w-2 h-2 rounded-full bg-primary animate-pulse" />}
            </h1>
            <p className="text-text-secondary text-sm md:text-base mt-1">
              {t('Fokus: {focus}', { focus: streamer || allLabel })}{' '}
              <span className="mx-1 text-border">•</span> {t('Zeitraum: letzte {days} Tage', { days })}
            </p>
          </div>
        </div>

        {/* Controls */}
        <div className="flex flex-col sm:flex-row sm:items-center gap-3">
          {canPreviewExtended && (
            <div className="flex items-center bg-background/70 rounded-xl border border-border p-1.5">
              {viewOptions.map(option => (
                <button
                  key={option.value}
                  type="button"
                  onClick={() => setView(option.value)}
                  className={`relative px-3 py-1.5 rounded-lg text-sm font-semibold transition-colors ${
                    view === option.value ? 'text-[#0D0806]' : 'text-text-secondary hover:text-white'
                  }`}
                >
                  {view === option.value && (
                    <motion.span
                      layoutId="headerViewIndicator"
                      className="absolute inset-0 rounded-lg bg-gradient-to-r from-primary to-accent shadow-lg shadow-primary/20"
                      initial={false}
                      transition={SEGMENT_SPRING}
                    />
                  )}
                  <span className="relative z-10">{option.label}</span>
                </button>
              ))}
            </div>
          )}

          {/* Streamer Dropdown */}
          <div className="relative">
            <button
              onClick={() => setDropdownOpen(!dropdownOpen)}
              className="w-full sm:w-auto min-w-[220px] flex items-center justify-between gap-2 px-4 py-2.5 rounded-xl border border-border bg-background/70 hover:border-border-hover soft-elevate"
            >
              <span className="text-white font-medium truncate">{streamer || allLabel}</span>
              <ChevronDown className="w-4 h-4 text-text-secondary" />
            </button>

            {/* Die Klickflaeche liegt ausserhalb der AnimatePresence: ein
                Fragment als direktes Kind laesst sich nicht animieren, das
                Menue waere beim Schliessen hart verschwunden. */}
            {dropdownOpen && (
              <div className="fixed inset-0 z-40" onClick={() => { setDropdownOpen(false); setSearch(''); }} />
            )}
            <AnimatePresence>
            {dropdownOpen && (
                <motion.div
                  key="streamer-dropdown"
                  {...MENU_MOTION}
                  transition={{ duration: 0.2, ease: [0.23, 1, 0.32, 1] }}
                  style={{ transformOrigin: 'top right' }}
                  className="absolute top-full right-0 mt-2 w-full sm:w-72 panel-card rounded-xl z-50 flex flex-col"
                >
                  {/* Search */}
                  <div className="p-2 border-b border-border">
                    <div className="flex items-center gap-2 px-2 py-1.5 rounded-lg bg-background/60 border border-border">
                      <Search className="w-3.5 h-3.5 text-text-secondary shrink-0" />
                      <input
                        autoFocus
                        type="text"
                        placeholder={t('Suchen…')}
                        value={search}
                        onChange={e => setSearch(e.target.value)}
                        className="flex-1 bg-transparent text-sm text-white placeholder:text-text-secondary outline-none"
                      />
                    </div>
                  </div>
                  <div className="max-h-80 overflow-y-auto">
                  {/* All Partners Option */}
                  {!isDemoMode && (
                    <button
                      onClick={() => {
                        onStreamerChange(null);
                        setDropdownOpen(false);
                        setSearch('');
                      }}
                      className={`w-full px-4 py-2.5 text-left hover:bg-white/5 transition-colors ${
                        !streamer ? 'bg-accent/15 text-accent' : 'text-white'
                      }`}
                    >
                      {allLabel}
                    </button>
                  )}

                  {/* Partners */}
                  {visiblePartners.length > 0 && (
                    <>
                      <div className="px-4 py-1.5 text-[11px] text-text-secondary uppercase tracking-[0.14em] bg-black/25">
                        {t('Partner')}
                      </div>
                      {visiblePartners.map(s => (
                        <button
                          key={s.login}
                          onClick={() => {
                            onStreamerChange(s.login);
                            setDropdownOpen(false);
                            setSearch('');
                          }}
                          className={`w-full px-4 py-2.5 text-left hover:bg-white/5 transition-colors ${
                            streamer === s.login ? 'bg-accent/15 text-accent' : 'text-white'
                          }`}
                        >
                          {s.login}
                        </button>
                      ))}
                    </>
                  )}

                  {/* Others (Admin only) */}
                  {visibleOthers.length > 0 && (
                    <>
                      <div className="px-4 py-1.5 text-[11px] text-text-secondary uppercase tracking-[0.14em] bg-black/25">
                        {t('Weitere Streamer')}
                      </div>
                      {visibleOthers.map(s => (
                        <button
                          key={s.login}
                          onClick={() => {
                            onStreamerChange(s.login);
                            setDropdownOpen(false);
                            setSearch('');
                          }}
                          className={`w-full px-4 py-2.5 text-left hover:bg-white/5 transition-colors ${
                            streamer === s.login ? 'bg-accent/15 text-accent' : 'text-white'
                          }`}
                        >
                          {s.login}
                          <span className="ml-2 text-text-secondary text-xs">{t('(extern)')}</span>
                        </button>
                      ))}
                    </>
                  )}
                  </div>{/* end scrollable */}
                </motion.div>
            )}
            </AnimatePresence>
          </div>

          {/* Time Range Selector */}
          <div className="flex items-center bg-background/70 rounded-xl border border-border p-1.5">
            <div className="px-2 text-text-secondary">
              <SlidersHorizontal className="w-4 h-4" />
            </div>
            {timeRanges.map(range => (
              <button
                key={range.value}
                onClick={() => onDaysChange(range.value)}
                className={`relative px-4 py-1.5 rounded-lg text-sm font-semibold transition-colors ${
                  days === range.value ? 'text-[#0D0806]' : 'text-text-secondary hover:text-white'
                }`}
              >
                {days === range.value && (
                  <motion.span
                    layoutId="headerRangeIndicator"
                    className="absolute inset-0 rounded-lg bg-gradient-to-r from-primary to-accent shadow-lg shadow-primary/20"
                    initial={false}
                    transition={SEGMENT_SPRING}
                  />
                )}
                <span className="relative z-10">{range.label}</span>
              </button>
            ))}
          </div>
        </div>
      </div>
    </header>
  );
}
