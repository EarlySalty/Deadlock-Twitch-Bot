import { useState } from 'react';
import { motion, AnimatePresence } from 'framer-motion';
import { BellOff, Zap, BarChart2, Check, Info } from 'lucide-react';
import { getPlanCheckoutHref } from '../../preview/routes';
import type { CatalogPlan } from '../../types/billing';

type FeatureId = 'werbefrei' | 'raid' | 'analyse';

const FEATURES: {
  id: FeatureId;
  planId: string;
  label: string;
  icon: React.ElementType;
  accentColor: string;
  color: string;
  borderActive: string;
  bgActive: string;
  desc: string;
  highlights: string[];
}[] = [
  {
    id: 'werbefrei',
    planId: 'chat_quiet',
    label: 'Werbefrei',
    icon: BellOff,
    accentColor: '#ff7a18',
    color: 'text-[#ff7a18]',
    borderActive: 'border-[#ff7a18]/60',
    bgActive: 'bg-[#ff7a18]/8',
    desc: 'Keine Chat-Werbung mehr',
    highlights: ['Chat-Werbung dauerhaft aus', 'Greift auch bei Admin-Events'],
  },
  {
    id: 'raid',
    planId: 'raid_boost',
    label: 'Raid Boost',
    icon: Zap,
    accentColor: '#10b7ad',
    color: 'text-[#10b7ad]',
    borderActive: 'border-[#10b7ad]/60',
    bgActive: 'bg-[#10b7ad]/8',
    desc: 'Bessere Raid-Platzierung',
    highlights: ['Bevorzugte Raid-Platzierung', 'Lurker-Tax-Erinnerungen'],
  },
  {
    id: 'analyse',
    planId: 'analysis_dashboard',
    label: 'Analyse',
    icon: BarChart2,
    accentColor: '#a78bfa',
    color: 'text-[#a78bfa]',
    borderActive: 'border-[#a78bfa]/60',
    bgActive: 'bg-[#a78bfa]/8',
    desc: 'KI-Coaching & Dashboard',
    highlights: ['Volles KI-Coaching', 'Viewer-Profile & Retention'],
  },
];

const COMBO_TO_PLAN: Record<string, string> = {
  '':                        'raid_free',
  'werbefrei':               'chat_quiet',
  'raid':                    'raid_boost',
  'analyse':                 'analysis_dashboard',
  'raid,werbefrei':          'bundle_chat_quiet_raid_boost',
  'analyse,werbefrei':       'bundle_werbefrei_analyse',
  'analyse,raid':            'bundle_analysis_raid_boost',
  'analyse,raid,werbefrei':  'bundle_komplett',
};

function comboKey(selected: Set<FeatureId>): string {
  return Array.from(selected).sort().join(',');
}

interface FeaturePickerProps {
  plans: CatalogPlan[];
  cycle: 1 | 12;
}

export default function FeaturePicker({ plans, cycle }: FeaturePickerProps) {
  const [selected, setSelected] = useState<Set<FeatureId>>(new Set());
  const [tooltip, setTooltip] = useState<FeatureId | null>(null);
  const [hoveredId, setHoveredId] = useState<FeatureId | null>(null);

  const planById = Object.fromEntries(plans.map(p => [p.id, p]));

  const planId = COMBO_TO_PLAN[comboKey(selected)] ?? 'raid_free';
  const plan = planById[planId];

  const toggle = (id: FeatureId) => {
    setSelected(prev => {
      const next = new Set(prev);
      next.has(id) ? next.delete(id) : next.add(id);
      return next;
    });
  };

  // Calculate savings: sum of individual feature prices vs bundle price
  const individualSum = Array.from(selected).reduce((sum, fid) => {
    const solo = planById[FEATURES.find(f => f.id === fid)!.planId];
    return sum + (solo?.price_monthly ?? 0);
  }, 0);
  const bundlePrice = plan?.price_monthly ?? 0;
  const savings = selected.size > 1 && bundlePrice > 0 ? individualSum - bundlePrice : 0;

  const isFree = bundlePrice === 0;
  const isBundle = planId.startsWith('bundle_');

  return (
    <div>
      {/* Section header */}
      <div className="mb-5">
        <p className="text-base font-semibold text-white mb-1">Welche Features brauchst du?</p>
        <p className="text-sm text-white/40">Klicke auf eine oder mehrere Kacheln — der passende Plan wird automatisch ermittelt.</p>
      </div>

      {/* Feature toggle cards */}
      <div className="grid grid-cols-3 gap-4 mb-8">
        {FEATURES.map(f => {
          const isActive = selected.has(f.id);
          const Icon = f.icon;
          const soloPlan = planById[f.planId];
          const price = soloPlan?.price_monthly ?? 0;
          return (
            <div
              key={f.id}
              className="relative"
              data-tour-id={
                f.id === 'werbefrei' ? 'tour-pricing-werbefrei' :
                f.id === 'raid' ? 'tour-pricing-raid' :
                'tour-pricing-analyse'
              }
            >
              <button
                onClick={() => toggle(f.id)}
                onMouseEnter={() => setHoveredId(f.id)}
                onMouseLeave={() => setHoveredId(null)}
                className={`w-full h-full rounded-2xl border p-5 text-left transition-all duration-200 cursor-pointer ${
                  isActive ? `${f.borderActive} ${f.bgActive}` : ''
                }`}
                style={!isActive ? {
                  borderColor: f.accentColor + (hoveredId === f.id ? '65' : '35'),
                  backgroundColor: f.accentColor + (hoveredId === f.id ? '18' : '0B'),
                } : undefined}
              >
                {/* Selected checkmark */}
                <AnimatePresence>
                  {isActive && (
                    <motion.div
                      initial={{ scale: 0 }}
                      animate={{ scale: 1 }}
                      exit={{ scale: 0 }}
                      className="absolute top-3 right-3 w-5 h-5 rounded-full bg-white/20 flex items-center justify-center"
                    >
                      <Check className="w-3 h-3 text-white" />
                    </motion.div>
                  )}
                </AnimatePresence>

                <Icon
                  className="w-6 h-6 mb-3"
                  style={{ color: f.accentColor, opacity: isActive ? 1 : 0.55 }}
                />
                <p className={`font-semibold mb-1 transition-colors ${isActive ? 'text-white' : 'text-white/75'}`}>
                  {f.label}
                </p>
                <p className="text-white/40 text-xs leading-snug mb-3">{f.desc}</p>
                <p className="text-sm font-medium transition-colors" style={{ color: f.accentColor, opacity: isActive ? 1 : 0.65 }}>
                  {price > 0 ? `${price.toFixed(2).replace('.', ',')} €/Mo.` : 'Kostenlos'}
                </p>
                {!isActive && (
                  <p className="text-xs mt-2 font-semibold" style={{ color: f.accentColor, opacity: 0.55 }}>+ Auswählen</p>
                )}
              </button>

              {/* Info tooltip toggle */}
              <button
                onClick={e => { e.stopPropagation(); setTooltip(tooltip === f.id ? null : f.id); }}
                className="absolute bottom-3 right-3 text-white/20 hover:text-white/50 transition-colors"
              >
                <Info className="w-4 h-4" />
              </button>

              {/* Inline info panel */}
              <AnimatePresence>
                {tooltip === f.id && (
                  <motion.div
                    initial={{ opacity: 0, y: -4 }}
                    animate={{ opacity: 1, y: 0 }}
                    exit={{ opacity: 0, y: -4 }}
                    transition={{ duration: 0.15 }}
                    className="absolute z-10 left-0 right-0 top-full mt-2 rounded-xl border border-white/10 bg-[#1a1a2e] p-3 shadow-xl"
                  >
                    <ul className="space-y-1.5">
                      {f.highlights.map((h, i) => (
                        <li key={i} className="flex items-start gap-2 text-xs text-white/60">
                          <Check className={`w-3 h-3 mt-0.5 flex-shrink-0 ${f.color}`} />
                          {h}
                        </li>
                      ))}
                    </ul>
                  </motion.div>
                )}
              </AnimatePresence>
            </div>
          );
        })}
      </div>

      {/* Result panel — nur wenn etwas ausgewählt */}
      <AnimatePresence mode="wait">
        {selected.size > 0 && <motion.div
          key={planId + cycle}
          initial={{ opacity: 0, y: 8 }}
          animate={{ opacity: 1, y: 0 }}
          exit={{ opacity: 0, y: -8 }}
          transition={{ duration: 0.2 }}
          className={`rounded-2xl border p-6 flex flex-col sm:flex-row items-start sm:items-center justify-between gap-4 ${
            isBundle
              ? 'border-[#10b7ad]/30 bg-gradient-to-r from-[#10b7ad]/8 to-transparent'
              : isFree
              ? 'border-white/10 bg-white/3'
              : 'border-white/15 bg-white/5'
          }`}
        >
          <div>
            {selected.size === 0 ? (
              <>
                <p className="text-white/40 text-sm mb-1">Nichts ausgewählt</p>
                <p className="text-white font-semibold text-lg">Auto-Raid & Basis-Dashboard</p>
                <p className="text-white/40 text-sm mt-1">Kostenlos — keine Kreditkarte nötig</p>
              </>
            ) : (
              <>
                {isBundle && (
                  <span className="inline-block text-xs font-semibold px-2 py-0.5 rounded-full bg-gradient-to-r from-[#10b7ad] to-[#ff7a18] text-white mb-2">
                    Bundle
                  </span>
                )}
                <p className="text-white font-semibold text-lg">{plan?.name ?? planId}</p>
                <div className="flex items-baseline gap-1.5 mt-1">
                  <span className="text-2xl font-bold text-white">
                    {bundlePrice > 0 ? `${bundlePrice.toFixed(2).replace('.', ',')} €` : 'Kostenlos'}
                  </span>
                  {bundlePrice > 0 && (
                    <span className="text-white/40 text-sm">/ Mo. inkl. MwSt.</span>
                  )}
                </div>
                {cycle === 12 && bundlePrice > 0 && (
                  <p className="text-white/30 text-xs mt-0.5">jährlich abgerechnet</p>
                )}
                {savings > 0.005 && (
                  <p className="text-[#10b7ad] text-sm mt-1.5">
                    Du sparst {savings.toFixed(2).replace('.', ',')} € gegenüber Einzelkauf
                  </p>
                )}
              </>
            )}
          </div>

          <a
            href={getPlanCheckoutHref(isFree ? null : planId, isFree, cycle)}
            className={`flex-shrink-0 px-6 py-3 rounded-xl text-sm font-semibold transition-all duration-200 whitespace-nowrap ${
              isFree
                ? 'bg-white/10 hover:bg-white/15 text-white'
                : isBundle
                ? 'bg-gradient-to-r from-[#10b7ad] to-[#ff7a18] hover:opacity-90 text-white shadow-lg shadow-[#10b7ad]/20'
                : 'bg-white/15 hover:bg-white/20 text-white'
            }`}
          >
            {isFree ? 'Kostenlos starten' : selected.size > 0 ? 'Jetzt abonnieren' : 'Auswahl treffen'}
          </a>
        </motion.div>}
      </AnimatePresence>
    </div>
  );
}
