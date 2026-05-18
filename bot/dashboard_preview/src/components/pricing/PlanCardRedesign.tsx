import { motion } from 'framer-motion';
import { BellOff, Check, Sparkles, Star, Zap, Crown } from 'lucide-react';
import { PREVIEW_BILLING_ROUTE } from '../../preview/routes';
import type { CatalogPlan } from '../../types/billing';

interface PlanCardRedesignProps {
  plan: CatalogPlan;
  index: number;
}

const tierConfig = {
  free: {
    icon: Zap,
    color: 'text-[#c1b3a7]',
    borderColor: 'border-border',
    gradient: '',
    badge: null,
    ctaStyle: 'bg-white/6 hover:bg-white/10 text-white border border-white/10',
  },
  basic: {
    icon: Star,
    color: 'text-[#ff7a18]',
    borderColor: 'border-[#ff7a18]/40',
    gradient: 'from-[#ff7a18]/8 to-transparent',
    badge: { text: 'Beliebt', icon: Star, className: 'bg-[#ff7a18] text-white' },
    ctaStyle: 'bg-[#ff7a18] hover:bg-[#ff8d39] text-white shadow-lg shadow-[#ff7a18]/15',
  },
  extended: {
    icon: Crown,
    color: 'text-[#10b7ad]',
    borderColor: 'border-[#10b7ad]/40',
    gradient: 'from-[#10b7ad]/8 to-transparent',
    badge: { text: 'Empfohlen', icon: Sparkles, className: 'bg-gradient-to-r from-[#2b6f6b] to-[#ff7a18] text-white' },
    ctaStyle: 'bg-gradient-to-r from-[#2b6f6b] to-[#ff7a18] hover:opacity-90 text-white shadow-lg shadow-[#10b7ad]/14',
  },
};

const planHighlights: Record<string, string[]> = {
  raid_free: [
    'Auto-Raid bei Stream-Ende',
    'Basis-Dashboard',
    'Discord Go-Live-Posts',
  ],
  chat_quiet: [
    'Chat-Werbung dauerhaft aus',
    'Greift auch bei Admin-Events',
    'Sonst alles wie Free',
    'Monatlich kündbar',
  ],
  raid_boost: [
    'Bevorzugte Raid-Platzierung',
    'Lurker-Tax-Erinnerungen',
    'KI-Mini Insights',
    'Basis-Analytics',
  ],
  bundle_chat_quiet_raid_boost: [
    'Werbung aus + Raid Boost',
    'Spart 2 € gegenüber Einzelkauf',
    'Lurker-Tax-Erinnerungen',
    'Basis-Analytics',
  ],
  analysis_dashboard: [
    'Volles KI-Coaching',
    'Viewer-Profile & Retention',
    'Alle Tabs freigeschaltet',
    'Coaching & Monetization',
  ],
  bundle_analysis_raid_boost: [
    'Alle Analytics-Features',
    'Bevorzugte Raid-Platzierung',
    'Chat-Werbung dauerhaft aus',
    'Spart gegenüber Einzelkauf',
  ],
};

export default function PlanCardRedesign({ plan, index }: PlanCardRedesignProps) {
  const isBundle = plan.id.startsWith('bundle_');
  const isChatQuiet = plan.id === 'chat_quiet';
  const config = tierConfig[plan.tier];
  const Icon = isChatQuiet ? BellOff : config.icon;
  const isCurrent = plan.is_current;
  const highlights = planHighlights[plan.id] || plan.features.slice(0, 4);
  const isPopular = plan.id === 'raid_boost';
  const isRecommended = plan.tier === 'extended' && !isBundle;
  const badge = isBundle
    ? { text: 'Bundle', icon: Sparkles, className: 'bg-gradient-to-r from-[#2b6f6b] to-[#d08a38] text-white' }
    : isPopular || isRecommended
    ? config.badge
    : null;

  return (
    <motion.div
      initial={{ opacity: 0, y: 20 }}
      animate={{ opacity: 1, y: 0 }}
      transition={{ duration: 0.4, delay: 0.1 + index * 0.1 }}
      className={`relative group`}
    >
      {/* Hover glow effect */}
      <div
        className={`absolute -inset-0.5 rounded-2xl opacity-0 group-hover:opacity-100 transition-opacity duration-300 blur-sm ${
          isPopular ? 'bg-[#ff7a18]/14' : isRecommended ? 'bg-[#10b7ad]/14' : 'bg-white/8'
        }`}
      />

      {/* Card */}
      <div
        className={`relative h-full rounded-2xl border ${config.borderColor} bg-gradient-to-b ${config.gradient} bg-[#221a17] p-6 flex flex-col soft-elevate ${
          isCurrent ? 'ring-2 ring-[#10b7ad]/35' : ''
        }`}
      >
        {/* Popular/recommended/bundle badge */}
        {badge && (
          <div className="absolute -top-3 left-1/2 -translate-x-1/2">
            <div
              className={`flex items-center gap-1.5 px-3 py-1 rounded-full text-xs font-semibold ${badge.className}`}
            >
              <badge.icon className="w-3 h-3" />
              {badge.text}
            </div>
          </div>
        )}

        {/* Plan header */}
        <div className="mb-5">
          <div className="flex items-center gap-2 mb-2">
            <Icon className={`w-5 h-5 ${config.color}`} />
            <h3 className="text-lg font-bold text-white">{plan.name}</h3>
          </div>

          {/* Price */}
          <div className="flex items-baseline gap-1.5">
            <span className="text-3xl font-bold text-white">
              {plan.price_monthly === 0
                ? 'Kostenlos'
                : `${plan.price_monthly.toFixed(2).replace('.', ',')}€`}
            </span>
            {plan.price_monthly > 0 && (
              <span className="text-white/45 text-sm leading-tight">/ Mo.<br/>inkl. MwSt.</span>
            )}
          </div>
        </div>

        {/* Key benefits (highlights only) */}
        <ul className="space-y-3 flex-1 mb-6">
          {highlights.map((highlight, i) => (
            <li key={i} className="flex items-start gap-2.5 text-sm">
              <Check
                className={`w-4 h-4 mt-0.5 flex-shrink-0 ${
                  isRecommended || isBundle
                    ? 'text-[#10b7ad]'
                    : isPopular
                    ? 'text-[#ff7a18]'
                    : 'text-white/30'
                }`}
              />
              <span className="text-white/72">{highlight}</span>
            </li>
          ))}
        </ul>

        {/* CTA */}
        {isCurrent ? (
          <div className="text-center py-3 rounded-xl bg-white/4 text-white/50 text-sm font-medium border border-white/8">
            Aktueller Plan
          </div>
        ) : (
          <a
            href={PREVIEW_BILLING_ROUTE}
            className={`block text-center py-3 rounded-xl text-sm font-semibold transition-all duration-200 ${config.ctaStyle}`}
          >
            {plan.price_monthly === 0
              ? 'Kostenlos starten'
              : plan.id === 'analysis_dashboard'
              ? '45 Tage kostenlos testen'
              : 'Jetzt abonnieren'}
          </a>
        )}

        {/* Current plan indicator */}
        {isCurrent && (
          <div className="absolute top-4 right-4">
            <div className="w-2 h-2 rounded-full bg-[#10b7ad]" />
          </div>
        )}
      </div>
    </motion.div>
  );
}
