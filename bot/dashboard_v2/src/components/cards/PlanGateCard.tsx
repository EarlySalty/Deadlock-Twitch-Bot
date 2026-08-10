import { useState } from 'react';
import { Lock } from 'lucide-react';
import { usePlan } from '../../context/PlanContext';
import { PremiumSheet } from '../pricing-v2/PremiumSheet';
import type { FeatureId } from '../../types/billing';

interface PlanGateCardProps {
  featureId: FeatureId;
  title: string;
  children: React.ReactNode;
}

/**
 * Gesperrte Karte: bleibt sichtbar und anklickbar. Ein Tipp oeffnet das Sheet
 * mit dem Preis, statt auf die Preisseite zu springen. Vorher stand hier ein
 * Link, der nur im Vorschaumodus ueberhaupt gerendert wurde — im echten
 * Dashboard war die Sperre eine Sackgasse.
 */
export function PlanGateCard({ featureId, title, children }: PlanGateCardProps) {
  const { isFeatureLocked } = usePlan();
  const [sheetOffen, setSheetOffen] = useState(false);
  const locked = isFeatureLocked(featureId);

  if (!locked) return <>{children}</>;

  return (
    <div className="relative">
      <div className="blur-sm pointer-events-none select-none opacity-50">
        {children}
      </div>
      <button
        type="button"
        data-press="soft"
        onClick={() => setSheetOffen(true)}
        className="absolute inset-0 flex items-center justify-center rounded-xl bg-black/20 backdrop-blur-[2px]"
      >
        <span className="p-6 text-center">
          <span className="mb-3 inline-flex h-12 w-12 items-center justify-center rounded-full border border-white/10 bg-white/5">
            <Lock className="h-5 w-5 text-white/40" />
          </span>
          <span className="block text-sm font-medium text-white/70">{title}</span>
          <span className="mt-1 block text-xs text-white/40">Mit Premium freischalten</span>
        </span>
      </button>
      <PremiumSheet offen={sheetOffen} onSchliessen={() => setSheetOffen(false)} titel={title} />
    </div>
  );
}
