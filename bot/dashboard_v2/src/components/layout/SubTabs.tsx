import { type ReactNode } from 'react';
import { Lock } from 'lucide-react';
import { usePlan } from '../../context/PlanContext';
import type { EntitlementId } from '../../types/billing';

export interface SubTabDef {
  id: string;
  label: string;
  entitlement?: EntitlementId;
  render: () => ReactNode;
}

interface SubTabsProps {
  tabs: SubTabDef[];
  active: string;
  onChange: (id: string) => void;
}

export function SubTabs({ tabs, active, onChange }: SubTabsProps) {
  const { hasEntitlement, isPreviewMode } = usePlan();
  const current = tabs.find((t) => t.id === active) ?? tabs[0];
  const lockedNow = current.entitlement ? !hasEntitlement(current.entitlement) : false;

  return (
    <div className="space-y-5">
      <nav className="flex flex-wrap gap-1.5">
        {tabs.map((tab) => {
          const tabLocked = tab.entitlement ? !hasEntitlement(tab.entitlement) : false;
          const isActive = tab.id === current.id;
          return (
            <button
              key={tab.id}
              type="button"
              onClick={() => onChange(tab.id)}
              className={`flex items-center gap-1.5 rounded-lg px-3.5 py-2 text-sm font-semibold transition-colors ${
                isActive ? 'bg-primary/85 text-white' : 'text-text-secondary hover:text-white'
              }`}
            >
              {tab.label}
              {tabLocked && <Lock className="h-3 w-3 text-white/40" />}
            </button>
          );
        })}
      </nav>
      {lockedNow && !isPreviewMode ? (
        <div className="panel-card flex flex-col items-center gap-3 rounded-2xl p-10 text-center">
          <div className="inline-flex h-12 w-12 items-center justify-center rounded-full border border-white/10 bg-white/5">
            <Lock className="h-5 w-5 text-white/40" />
          </div>
          <p className="text-sm font-medium text-white/70">
            {current.label} ist in deinem Plan nicht enthalten
          </p>
          <p className="text-xs text-white/40">Verfügbar nach einem Upgrade</p>
        </div>
      ) : (
        current.render()
      )}
    </div>
  );
}
