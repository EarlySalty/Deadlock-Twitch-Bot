import { useState } from 'react';
import { Lock } from 'lucide-react';
import { usePlan } from '../../context/PlanContext';
import { UnlockSheet } from '../pricing-v2/UnlockSheet';
import type { FeatureId } from '../../types/billing';

interface PlanGateCardProps {
  featureId: FeatureId;
  title: string;
  children: React.ReactNode;
}

export function PlanGateCard({ featureId, title, children }: PlanGateCardProps) {
  const { isFeatureLocked } = usePlan();
  const [open, setOpen] = useState(false);
  const locked = isFeatureLocked(featureId);

  if (!locked) return <>{children}</>;

  return (
    <>
      <button
        type="button"
        className="relative block w-full text-left"
        onClick={() => setOpen(true)}
      >
        <div className="pointer-events-none select-none opacity-50 blur-sm">{children}</div>
        <div className="absolute inset-0 flex items-center justify-center rounded-xl bg-black/20 backdrop-blur-[2px]">
          <div className="p-6 text-center">
            <div className="mb-3 inline-flex h-12 w-12 items-center justify-center rounded-full border border-white/10 bg-white/5">
              <Lock className="h-5 w-5 text-white/40" />
            </div>
            <p className="text-sm font-medium text-white/70">{title}</p>
            <p className="mt-1 text-xs text-white/40">Premium macht deine Zahlen sichtbar.</p>
          </div>
        </div>
      </button>
      <UnlockSheet open={open} onClose={() => setOpen(false)} />
    </>
  );
}
