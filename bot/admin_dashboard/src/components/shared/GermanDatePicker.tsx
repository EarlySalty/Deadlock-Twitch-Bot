import { useEffect, useMemo, useRef, useState, type KeyboardEvent } from 'react';
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
  const year = Number(match[1]);
  const month = Number(match[2]) - 1;
  const day = Number(match[3]);
  const date = new Date(year, month, day);
  return date.getFullYear() === year && date.getMonth() === month && date.getDate() === day ? date : null;
}

function displayDate(value: string): string {
  const date = fromIso(value);
  return date ? `${String(date.getDate()).padStart(2, '0')}.${String(date.getMonth() + 1).padStart(2, '0')}.${date.getFullYear()}` : '';
}

function parseGermanDate(value: string): Date | null {
  const match = /^(\d{1,2})[./](\d{1,2})[./](\d{4})$/.exec(value.trim());
  if (!match) return null;
  const day = Number(match[1]);
  const month = Number(match[2]) - 1;
  const year = Number(match[3]);
  const date = new Date(year, month, day);
  return date.getFullYear() === year && date.getMonth() === month && date.getDate() === day ? date : null;
}

function clampDate(year: number, month: number, day: number): Date {
  const lastDay = new Date(year, month + 1, 0).getDate();
  return new Date(year, month, Math.min(day, lastDay));
}

function segmentForCursor(cursor: number | null): { index: number; start: number; length: number } {
  if ((cursor ?? 0) <= 2) return { index: 0, start: 0, length: 2 };
  if ((cursor ?? 0) <= 5) return { index: 1, start: 3, length: 2 };
  return { index: 2, start: 6, length: 4 };
}

export function GermanDatePicker({ value, onChange }: { value: string; onChange: (value: string) => void }) {
  const selected = fromIso(value);
  const [open, setOpen] = useState(false);
  const [month, setMonth] = useState(() => selected ?? new Date());
  const [inputText, setInputText] = useState(() => displayDate(value));
  const containerRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    setInputText(displayDate(value));
  }, [value]);

  useEffect(() => {
    const date = fromIso(value);
    if (date && !open) setMonth(date);
  }, [value, open]);

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

  const commitDate = (date: Date) => {
    const nextValue = toIso(date);
    setInputText(displayDate(nextValue));
    onChange(nextValue);
  };

  const handleInputKeyDown = (event: KeyboardEvent<HTMLInputElement>) => {
    if (event.key === 'Enter') {
      const parsed = parseGermanDate(inputText);
      if (parsed) commitDate(parsed);
      return;
    }
    if (!['ArrowUp', 'ArrowDown', 'ArrowLeft', 'ArrowRight'].includes(event.key)) return;

    const input = event.currentTarget;
    const current = parseGermanDate(inputText) ?? selected ?? new Date();
    const segment = segmentForCursor(input.selectionStart);

    if (event.key === 'ArrowLeft' || event.key === 'ArrowRight') {
      event.preventDefault();
      const nextIndex = Math.max(0, Math.min(2, segment.index + (event.key === 'ArrowLeft' ? -1 : 1)));
      const starts = [0, 3, 6];
      const lengths = [2, 2, 4];
      const nextStart = starts[nextIndex];
      input.setSelectionRange(nextStart, nextStart + lengths[nextIndex]);
      return;
    }

    event.preventDefault();
    const direction = event.key === 'ArrowUp' ? 1 : -1;
    let nextDate: Date;
    if (segment.index === 0) {
      nextDate = new Date(current.getFullYear(), current.getMonth(), current.getDate() + direction);
    } else if (segment.index === 1) {
      nextDate = clampDate(current.getFullYear(), current.getMonth() + direction, current.getDate());
    } else {
      nextDate = clampDate(current.getFullYear() + direction, current.getMonth(), current.getDate());
    }
    commitDate(nextDate);
    requestAnimationFrame(() => {
      const nextSegment = segmentForCursor(segment.start);
      input.focus();
      input.setSelectionRange(nextSegment.start, nextSegment.start + nextSegment.length);
    });
  };

  const handleInputBlur = () => {
    if (!inputText.trim()) {
      setInputText('');
      onChange('');
      return;
    }
    const parsed = parseGermanDate(inputText);
    if (parsed) {
      commitDate(parsed);
    } else {
      setInputText(displayDate(value));
    }
  };

  return (
    <div ref={containerRef} className="relative">
      <div className="relative">
        <input
          type="text"
          inputMode="numeric"
          className="admin-input mt-2 pr-11"
          value={inputText}
          placeholder="TT.MM.JJJJ"
          aria-label="Ablaufdatum (TT.MM.JJJJ)"
          onChange={(event) => {
            const nextText = event.target.value;
            setInputText(nextText);
          }}
          onKeyDown={handleInputKeyDown}
          onBlur={handleInputBlur}
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
                onClick={() => { commitDate(day); setOpen(false); }}
              >
                {day.getDate()}
              </button>
            ) : <span key={`empty-${index}`} />)}
          </div>
          <div className="mt-2 flex justify-end border-t border-white/10 pt-2">
            <button type="button" className="text-xs font-semibold text-primary hover:text-white" onClick={() => { setInputText(''); onChange(''); setOpen(false); }}>Leeren</button>
          </div>
        </div>
      ) : null}
    </div>
  );
}
