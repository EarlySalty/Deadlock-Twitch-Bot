import { useEffect, useRef, useState } from 'react';

const ISO_LOCAL = /^(\d{4})-(\d{2})-(\d{2})T(\d{2}):(\d{2})/;
const DE_INPUT = /^(\d{1,2})\.(\d{1,2})\.(\d{4})[ ,T]+(\d{1,2}):(\d{2})$/;

export function isoLocalToDe(value: string): string {
  const m = ISO_LOCAL.exec(value);
  if (!m) return '';
  const [, y, mo, d, h, mi] = m;
  return `${d}.${mo}.${y} ${h}:${mi}`;
}

export function deToIsoLocal(text: string): string | null {
  const t = text.trim();
  if (!t) return '';
  const m = DE_INPUT.exec(t);
  if (!m) return null;
  const [, d, mo, y, h, mi] = m;
  const dd = d.padStart(2, '0');
  const mm = mo.padStart(2, '0');
  const hh = h.padStart(2, '0');
  const dNum = Number(dd);
  const moNum = Number(mm);
  const hNum = Number(hh);
  const miNum = Number(mi);
  if (moNum < 1 || moNum > 12) return null;
  if (dNum < 1 || dNum > 31) return null;
  if (hNum > 23 || miNum > 59) return null;
  const probe = new Date(Number(y), moNum - 1, dNum);
  if (probe.getFullYear() !== Number(y) || probe.getMonth() !== moNum - 1 || probe.getDate() !== dNum) {
    return null;
  }
  return `${y}-${mm}-${dd}T${hh}:${mi}`;
}

interface DeDateTimeInputProps {
  id?: string;
  value: string;
  onChange: (next: string) => void;
}

export function DeDateTimeInput({ id, value, onChange }: DeDateTimeInputProps) {
  const [text, setText] = useState(() => isoLocalToDe(value));
  const [invalid, setInvalid] = useState(false);
  const lastValue = useRef(value);

  useEffect(() => {
    if (value !== lastValue.current) {
      lastValue.current = value;
      setText(isoLocalToDe(value));
      setInvalid(false);
    }
  }, [value]);

  return (
    <div className="space-y-1">
      <input
        id={id}
        type="text"
        inputMode="numeric"
        className="admin-input"
        placeholder="TT.MM.JJJJ HH:MM"
        value={text}
        aria-invalid={invalid}
        onChange={(event) => {
          const next = event.target.value;
          setText(next);
          const parsed = deToIsoLocal(next);
          if (parsed !== null) {
            setInvalid(false);
            lastValue.current = parsed;
            onChange(parsed);
          }
        }}
        onBlur={() => {
          const parsed = deToIsoLocal(text);
          if (parsed === null) {
            setInvalid(true);
          } else {
            setInvalid(false);
            lastValue.current = parsed;
            setText(isoLocalToDe(parsed));
            onChange(parsed);
          }
        }}
      />
      {invalid ? (
        <span className="text-xs text-red-400">Bitte im Format TT.MM.JJJJ HH:MM eingeben.</span>
      ) : null}
    </div>
  );
}
