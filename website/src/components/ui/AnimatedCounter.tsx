import { landingLocale } from "@/lib/landingLocale";
import type { RefObject } from "react";
import { useCountUp } from "@/hooks/useCountUp";

interface AnimatedCounterProps {
  end: number;
  suffix?: string;
  label: string;
}

export function AnimatedCounter({ end, suffix, label }: AnimatedCounterProps) {
  const { count, ref } = useCountUp(end);

  return (
    <div
      ref={ref as RefObject<HTMLDivElement>}
      className="flex flex-col items-center"
    >
      <div className="text-4xl font-bold font-display text-[var(--color-text-primary)]">
        {count.toLocaleString(landingLocale())}
        {suffix && (
          <span className="text-[var(--color-primary)]">{suffix}</span>
        )}
      </div>
      <div className="text-sm text-[var(--color-text-secondary)] mt-1">
        {label}
      </div>
    </div>
  );
}
