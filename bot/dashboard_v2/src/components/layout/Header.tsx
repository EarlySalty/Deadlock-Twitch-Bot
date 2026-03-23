import { useState } from 'react';
import { Activity, ChevronDown, Search, SlidersHorizontal, Sparkles } from 'lucide-react';
import { usePlan } from '@/context/PlanContext';
import type { TimeRange } from '@/types/analytics';

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
  const [dropdownOpen, setDropdownOpen] = useState(false);
  const [search, setSearch] = useState('');

  const timeRanges: { value: TimeRange; label: string }[] = [
    { value: 7, label: '7d' },
    { value: 30, label: '30d' },
    { value: 90, label: '90d' },
  ];

  const q = search.trim().toLowerCase();
  const partners = streamers.filter(s => s.isPartner && (!q || s.login.includes(q)));
  const others = streamers.filter(s => !s.isPartner && (!q || s.login.includes(q)));
  const allLabel = isDemoMode ? 'Demo-Profil' : canViewAllStreamers ? 'Alle Streamer' : 'Alle Partner';
  const canPreviewExtended = !hasFullAccess && !hasEntitlement('analytics.extended');

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
              Fokus: {streamer || allLabel} <span className="mx-1 text-border">•</span> Zeitraum: letzte {days} Tage
            </p>
          </div>
        </div>

        {/* Controls */}
        <div className="flex flex-col sm:flex-row sm:items-center gap-3">
          {canPreviewExtended && (
            <div className="flex items-center bg-background/70 rounded-xl border border-border p-1.5">
              <button
                type="button"
                onClick={() => setView('basic')}
                className={`px-3 py-1.5 rounded-lg text-sm font-semibold transition-all ${
                  view === 'basic'
                    ? 'bg-gradient-to-r from-primary to-accent text-white shadow-lg shadow-primary/20'
                    : 'text-text-secondary hover:text-white'
                }`}
              >
                Basis
              </button>
              <button
                type="button"
                onClick={() => setView('extended')}
                className={`px-3 py-1.5 rounded-lg text-sm font-semibold transition-all ${
                  view === 'extended'
                    ? 'bg-gradient-to-r from-primary to-accent text-white shadow-lg shadow-primary/20'
                    : 'text-text-secondary hover:text-white'
                }`}
              >
                Preview
              </button>
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

            {dropdownOpen && (
              <>
                <div className="fixed inset-0 z-40" onClick={() => { setDropdownOpen(false); setSearch(''); }} />
                <div className="absolute top-full right-0 mt-2 w-full sm:w-72 panel-card rounded-xl z-50 flex flex-col">
                  {/* Search */}
                  <div className="p-2 border-b border-border">
                    <div className="flex items-center gap-2 px-2 py-1.5 rounded-lg bg-background/60 border border-border">
                      <Search className="w-3.5 h-3.5 text-text-secondary shrink-0" />
                      <input
                        autoFocus
                        type="text"
                        placeholder="Suchen…"
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
                        Partner
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
                        Weitere Streamer
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
                          <span className="ml-2 text-text-secondary text-xs">(extern)</span>
                        </button>
                      ))}
                    </>
                  )}
                  </div>{/* end scrollable */}
                </div>
              </>
            )}
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
                className={`px-4 py-1.5 rounded-lg text-sm font-semibold transition-all ${
                  days === range.value
                    ? 'bg-gradient-to-r from-primary to-accent text-white shadow-lg shadow-primary/20'
                    : 'text-text-secondary hover:text-white'
                }`}
              >
                {range.label}
              </button>
            ))}
          </div>
        </div>
      </div>
    </header>
  );
}
