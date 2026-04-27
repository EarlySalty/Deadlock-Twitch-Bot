import { Lock, Sparkles } from 'lucide-react';
import { usePlan } from '../../context/PlanContext';
import { PREVIEW_PRICING_ROUTE } from '../../preview/routes';
import type { FeatureId } from '../../types/billing';

interface PlanGateCardProps {
  featureId: FeatureId;
  title: string;
  children: React.ReactNode;
}

export function PlanGateCard({ featureId, title, children }: PlanGateCardProps) {
  const { isFeatureLocked, isPreviewMode } = usePlan();
  const locked = isFeatureLocked(featureId);

  if (!locked) return <>{children}</>;

  return (
    <div className="relative">
      <div className="blur-sm pointer-events-none select-none opacity-50">
        {children}
      </div>
      <div className="absolute inset-0 flex items-center justify-center bg-black/20 rounded-xl backdrop-blur-[2px]">
        <div className="text-center p-6">
          <div className="inline-flex items-center justify-center w-12 h-12 rounded-full bg-white/5 border border-white/10 mb-3">
            <Lock className="w-5 h-5 text-white/40" />
          </div>
          <p className="text-sm font-medium text-white/70">{title}</p>
          <p className="text-xs text-white/40 mt-1">Verfügbar ab Erweitert</p>
          {isPreviewMode && (
            <a
              href={PREVIEW_PRICING_ROUTE}
              className="inline-flex items-center gap-1 mt-3 px-3 py-1.5 rounded-lg bg-purple-500/20 text-purple-300 text-xs font-medium hover:bg-purple-500/30 transition-colors"
            >
              <Sparkles className="w-3 h-3" />
              Freischalten
            </a>
          )}
        </div>
      </div>
    </div>
  );
}
