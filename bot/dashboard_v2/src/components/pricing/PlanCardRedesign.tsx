import { motion } from 'framer-motion';
import { Check, Sparkles, Star, Zap, Crown } from 'lucide-react';
import type { CatalogPlan } from '../../types/billing';

interface PlanCardRedesignProps {
  plan: CatalogPlan;
  index: number;
}

const tierConfig = {
  free: {
    icon: Zap,
    color: 'text-white/40',
    borderColor: 'border-border',
    gradient: '',
    badge: null,
    ctaStyle: 'bg-white/10 hover:bg-white/15 text-white',
  },
  basic: {
    icon: Star,
    color: 'text-[#ff7a18]',
    borderColor: 'border-[#ff7a18]/40',
    gradient: 'from-[#ff7a18]/5 to-transparent',
    badge: { text: 'Beliebt', icon: Star, className: 'bg-[#ff7a18] text-white' },
    ctaStyle: 'bg-[#ff7a18] hover:bg-[#ff8d39] text-white shadow-lg shadow-[#ff7a18]/20',
  },
  extended: {
    icon: Crown,
    color: 'text-[#10b7ad]',
    borderColor: 'border-[#10b7ad]/40',
    gradient: 'from-[#10b7ad]/5 to-transparent',
    badge: { text: 'Empfohlen', icon: Sparkles, className: 'bg-gradient-to-r from-[#10b7ad] to-[#ff7a18] text-white' },
    ctaStyle: 'bg-gradient-to-r from-[#10b7ad] to-[#ff7a18] hover:opacity-90 text-white shadow-lg shadow-[#10b7ad]/20',
  },
};

const planHighlights: Record<string, string[]> = {
  raid_free: [
    'Basis-Analytics',
    'Viewer-Trend',
    'Stream-Übersicht',
  ],
  raid_boost: [
    'Erweiterte Analytics',
    'Chat-Insights',
    'Growth-Tracking',
    'Priority Support',
  ],
  analysis_dashboard: [
    'Alle Basic-Features',
    'KI-Analyse',
    'Viewer-Profile',
    'Coaching & Monetization',
  ],
};

export default function PlanCardRedesign({ plan, index }: PlanCardRedesignProps) {
  const config = tierConfig[plan.tier];
  const Icon = config.icon;
  const isCurrent = plan.is_current;
  const highlights = planHighlights[plan.id] || plan.features.slice(0, 4);
  const isPopular = plan.tier === 'basic';
  const isRecommended = plan.tier === 'extended';

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
          isPopular ? 'bg-[#ff7a18]/20' : isRecommended ? 'bg-[#10b7ad]/20' : 'bg-white/10'
        }`}
      />

      {/* Card */}
      <div
        className={`relative h-full rounded-2xl border ${config.borderColor} bg-gradient-to-b ${config.gradient} p-6 flex flex-col soft-elevate ${
          isCurrent ? 'ring-2 ring-[#10b7ad]/50' : ''
        }`}
      >
        {/* Popular badge */}
        {config.badge && (
          <div className="absolute -top-3 left-1/2 -translate-x-1/2">
            <div
              className={`flex items-center gap-1.5 px-3 py-1 rounded-full text-xs font-semibold ${config.badge.className}`}
            >
              <config.badge.icon className="w-3 h-3" />
              {config.badge.text}
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
              <span className="text-white/40 text-sm">pro Monat</span>
            )}
          </div>
        </div>

        {/* Key benefits (highlights only) */}
        <ul className="space-y-3 flex-1 mb-6">
          {highlights.map((highlight, i) => (
            <li key={i} className="flex items-start gap-2.5 text-sm">
              <Check
                className={`w-4 h-4 mt-0.5 flex-shrink-0 ${
                  isRecommended
                    ? 'text-[#10b7ad]'
                    : isPopular
                    ? 'text-[#ff7a18]'
                    : 'text-white/30'
                }`}
              />
              <span className="text-white/70">{highlight}</span>
            </li>
          ))}
        </ul>

        {/* CTA */}
        {isCurrent ? (
          <div className="text-center py-3 rounded-xl bg-white/5 text-white/50 text-sm font-medium border border-white/10">
            Aktueller Plan
          </div>
        ) : (
          <a
            href="/twitch/abbo"
            className={`block text-center py-3 rounded-xl text-sm font-semibold transition-all duration-200 ${config.ctaStyle}`}
          >
            {plan.price_monthly === 0 ? 'Kostenlos starten' : '45 Tage kostenlos testen'}
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
