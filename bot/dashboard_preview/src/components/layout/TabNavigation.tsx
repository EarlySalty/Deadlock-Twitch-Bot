import { motion } from 'framer-motion';
import {
  LayoutDashboard,
  TrendingUp,
  MessageSquare,
  BarChart3,
  Users,
  Calendar,
  Target,
  GraduationCap,
  DollarSign,
  Globe,
  UserSearch,
  FlaskConical,
  Brain,
  Lock,
  Sparkles,
} from 'lucide-react';
import { usePlan } from '../../context/PlanContext';
import type { TabId as BillingTabId } from '../../types/billing';

export type TabId =
  | 'overview'
  | 'streams'
  | 'chat'
  | 'growth'
  | 'audience'
  | 'viewers'
  | 'compare'
  | 'schedule'
  | 'coaching'
  | 'monetization'
  | 'category'
  | 'experimental'
  | 'ai'
  | 'title';

interface Tab {
  id: TabId;
  label: string;
  icon: React.ComponentType<{ className?: string }>;
  beta?: boolean;
}

const tabs: Tab[] = [
  { id: 'overview', label: 'Übersicht', icon: LayoutDashboard },
  { id: 'streams', label: 'Streams', icon: TrendingUp },
  { id: 'chat', label: 'Chat', icon: MessageSquare },
  { id: 'growth', label: 'Wachstum', icon: BarChart3 },
  { id: 'audience', label: 'Audience', icon: Target },
  { id: 'viewers', label: 'Viewer', icon: UserSearch },
  { id: 'compare', label: 'Vergleich', icon: Users },
  { id: 'schedule', label: 'Zeitplan', icon: Calendar },
  { id: 'coaching', label: 'Coaching', icon: GraduationCap },
  { id: 'monetization', label: 'Monetization', icon: DollarSign },
  { id: 'category', label: 'Kategorie', icon: Globe },
  { id: 'experimental', label: 'Labor', icon: FlaskConical, beta: true },
  { id: 'ai', label: 'KI Analyse', icon: Brain, beta: true },
  { id: 'title', label: 'Titel', icon: Sparkles },
];

interface TabNavigationProps {
  activeTab: TabId;
  onTabChange: (tab: TabId) => void;
}

export function TabNavigation({ activeTab, onTabChange }: TabNavigationProps) {
  const { canAccessTab, isTabLocked, isPreviewMode } = usePlan();

  // Filter tabs: show if accessible, or if in preview mode show locked tabs with opacity
  const visibleTabs = tabs.filter(tab => {
    const tabId = tab.id as BillingTabId;
    if (canAccessTab(tabId)) return true;
    // In preview mode, show locked tabs (with lock icon + reduced opacity)
    if (isPreviewMode) return true;
    return false;
  });

  return (
    <nav className="mb-8 overflow-x-auto">
      <div className="panel-card rounded-2xl p-2.5 min-w-max flex items-center gap-1.5">
        {visibleTabs.map(tab => {
          const Icon = tab.icon;
          const isActive = activeTab === tab.id;
          const accessible = canAccessTab(tab.id as BillingTabId);
          const locked = isTabLocked(tab.id as BillingTabId);

          return (
            <button
              key={tab.id}
              type="button"
              onClick={() => {
                if (accessible) {
                  onTabChange(tab.id);
                }
              }}
              disabled={!accessible}
              aria-disabled={!accessible}
              className={`relative flex items-center gap-2 px-4 py-2.5 rounded-xl text-sm font-semibold transition-colors whitespace-nowrap ${
                isActive ? 'text-white' : 'text-text-secondary hover:text-white'
              } ${locked ? 'opacity-50' : ''} ${!accessible ? 'cursor-not-allowed hover:text-text-secondary' : ''}`}
            >
              {isActive && (
                <motion.div
                  layoutId="activeTab"
                  className="absolute inset-0 rounded-xl border border-primary/30 bg-gradient-to-r from-primary/80 via-primary/75 to-accent/80 shadow-lg shadow-primary/15"
                  initial={false}
                  transition={{ type: 'spring', stiffness: 500, damping: 35 }}
                />
              )}
              <span className="relative z-10 flex items-center gap-2">
                <Icon className="w-4 h-4" />
                <span className="hidden sm:inline">{tab.label}</span>
                {tab.beta && (
                  <span className="hidden sm:inline-block text-[10px] font-bold px-1.5 py-0.5 rounded-full bg-accent/20 text-accent border border-accent/30 leading-none">
                    Beta
                  </span>
                )}
                {locked && (
                  <Lock className="w-3 h-3 text-white/40" />
                )}
              </span>
            </button>
          );
        })}
      </div>
    </nav>
  );
}
