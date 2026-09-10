import { useEffect, useMemo, useRef, useState } from 'react';
import { CalendarDays, ChevronLeft, ChevronRight } from 'lucide-react';

const MONTHS = [
  'Januar', 'Februar', 'März', 'April', 'Mai', 'Juni',
  'Juli', 'August', 'September', 'Oktober', 'November', 'Dezember',
];
const WEEKDAYS = ['Mo', 'Di', 'Mi', 'Do', 'Fr', 'Sa', 'So'];

function toIso(date: Date): string {
  return `${date.getFullYear()}-${String(date.getMonth() + 1).padStart(2, '0')}-${String(date.getDate()).padStart(2, '0')}`;
}

function fromIso(value: string): Date | null {
  const match = /^(\d{4})-(\d{2})-(\d{2})$/.exec(value);
  if (!match) return null;
  const date = new Date(Number(match[1]), Number(match[2]) - 1, Number(match[3]));
  return Number.isNaN(date.getTime()) ? null : date;
}

function displayDate(value: string): string {
  const date = fromIso(value);
  return date ? `${String(date.getDate()).padStart(2, '0')}.${String(date.getMonth() + 1).padStart(2, '0')}.${date.getFullYear()}` : '';
}

export function GermanDatePicker({ value, onChange }: { value: string; onChange: (value: string) => void }) {
  const selected = fromIso(value);
  const [open, setOpen] = useState(false);
  const [month, setMonth] = useState(() => selected ?? new Date());
  const containerRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!open) return;
    const close = (event: MouseEvent) => {
      if (!containerRef.current?.contains(event.target as Node)) setOpen(false);
    };
    document.addEventListener('mousedown', close);
    return () => document.removeEventListener('mousedown', close);
  }, [open]);

  const days = useMemo(() => {
    const first = new Date(month.getFullYear(), month.getMonth(), 1);
    const offset = (first.getDay() + 6) % 7;
    const count = new Date(month.getFullYear(), month.getMonth() + 1, 0).getDate();
    return Array.from({ length: Math.ceil((offset + count) / 7) * 7 }, (_, index) => {
      const day = index - offset + 1;
      return day < 1 || day > count ? null : new Date(month.getFullYear(), month.getMonth(), day);
    });
  }, [month]);

  return (
    <div ref={containerRef} className="relative">
      <div className="relative">
        <input
          type="text"
          inputMode="numeric"
          className="admin-input mt-2 pr-11"
          value={displayDate(value)}
          placeholder="TT.MM.JJJJ"
          readOnly
          aria-label="Ablaufdatum (TT.MM.JJJJ)"
          onClick={() => setOpen(true)}
        />
        <button type="button" className="absolute right-2 top-4 rounded-lg p-1 text-text-secondary hover:text-white" aria-label="Kalender öffnen" onClick={() => setOpen((current) => !current)}>
          <CalendarDays className="h-4 w-4" />
        </button>
      </div>
      {open ? (
        <div className="absolute z-20 mt-2 w-72 rounded-xl border border-white/15 bg-[#211c19] p-3 shadow-2xl">
          <div className="flex items-center justify-between">
            <button type="button" className="rounded-lg p-1 text-text-secondary hover:bg-white/10 hover:text-white" aria-label="Vorheriger Monat" onClick={() => setMonth(new Date(month.getFullYear(), month.getMonth() - 1, 1))}><ChevronLeft className="h-4 w-4" /></button>
            <span className="text-sm font-semibold text-white">{MONTHS[month.getMonth()]} {month.getFullYear()}</span>
            <button type="button" className="rounded-lg p-1 text-text-secondary hover:bg-white/10 hover:text-white" aria-label="Nächster Monat" onClick={() => setMonth(new Date(month.getFullYear(), month.getMonth() + 1, 1))}><ChevronRight className="h-4 w-4" /></button>
          </div>
          <div className="mt-3 grid grid-cols-7 text-center text-[0.68rem] font-semibold text-text-secondary">
            {WEEKDAYS.map((day) => <span key={day}>{day}</span>)}
          </div>
          <div className="mt-2 grid grid-cols-7 gap-1 text-center text-sm">
            {days.map((day, index) => day ? (
              <button
                key={toIso(day)}
                type="button"
                className={`rounded-lg py-1.5 ${value === toIso(day) ? 'bg-primary font-semibold text-black' : 'text-white hover:bg-white/10'}`}
                onClick={() => { onChange(toIso(day)); setOpen(false); }}
              >
                {day.getDate()}
              </button>
            ) : <span key={`empty-${index}`} />)}
          </div>
          <div className="mt-2 flex justify-end border-t border-white/10 pt-2">
            <button type="button" className="text-xs font-semibold text-primary hover:text-white" onClick={() => { onChange(''); setOpen(false); }}>Leeren</button>
          </div>
        </div>
      ) : null}
    </div>
  );
}
