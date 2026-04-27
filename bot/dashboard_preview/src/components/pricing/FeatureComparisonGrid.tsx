import { motion } from 'framer-motion';
import { Check, Minus } from 'lucide-react';

interface FeatureRow {
  name: string;
  free: boolean | string;
  basic: boolean | string;
  extended: boolean | string;
  bundle: boolean | string;
}

interface FeatureCategory {
  category: string;
  features: FeatureRow[];
}

const featureData: FeatureCategory[] = [
  {
    category: 'Analytics',
    features: [
      { name: 'Viewer-Verlauf & Trends', free: true,  basic: true,  extended: true,  bundle: true  },
      { name: 'Stream-Übersicht',        free: true,  basic: true,  extended: true,  bundle: true  },
      { name: 'Schedule Heatmap',        free: true,  basic: true,  extended: true,  bundle: true  },
      { name: 'Chat-Analytics',          free: false, basic: true,  extended: true,  bundle: true  },
      { name: 'Growth-Tracking',         free: false, basic: true,  extended: true,  bundle: true  },
      { name: 'Zeitraumvergleiche',       free: false, basic: true,  extended: true,  bundle: true  },
      { name: 'Audience-Insights',       free: false, basic: true,  extended: true,  bundle: true  },
      { name: 'Follower-Übersichten',    free: false, basic: true,  extended: true,  bundle: true  },
      { name: 'Kategorie-Vergleich',     free: false, basic: false, extended: true,  bundle: true  },
      { name: 'Viewer-Profile',          free: false, basic: false, extended: true,  bundle: true  },
    ],
  },
  {
    category: 'KI-Analyse',
    features: [
      { name: 'KI-Zusammenfassung',      free: false, basic: 'Basis', extended: 'Vollständig', bundle: 'Vollständig' },
      { name: 'Stream-Coaching',         free: false, basic: false, extended: true,  bundle: true  },
      { name: 'Monetarisierungs-Tipps',  free: false, basic: false, extended: true,  bundle: true  },
    ],
  },
  {
    category: 'Community & Chat',
    features: [
      { name: 'Lurker-Steuer Erinnerungen', free: false, basic: true,  extended: true,  bundle: true  },
      { name: 'Chat-Social-Graph',          free: false, basic: false, extended: true,  bundle: true  },
      { name: 'Bot-Werbung deaktivieren',   free: false, basic: false, extended: false, bundle: true  },
    ],
  },
  {
    category: 'Raid-Netzwerk',
    features: [
      { name: 'Auto-Raid Grundfunktion',         free: true,  basic: true,  extended: true,  bundle: true  },
      { name: 'Bevorzugte Raid-Platzierung',      free: false, basic: true,  extended: false, bundle: true  },
      { name: 'Raid-Retention-Analyse',           free: false, basic: false, extended: false, bundle: true  },
      { name: 'Sichtbarkeit bei Inaktivität',     free: false, basic: true,  extended: false, bundle: true  },
    ],
  },
  {
    category: 'Sonstiges',
    features: [
      { name: '45 Tage kostenlose Testphase', free: false, basic: false, extended: true,  bundle: false },
      { name: 'Priority Support',             free: false, basic: true,  extended: true,  bundle: true  },
    ],
  },
];

const Cell = ({ value, color }: { value: boolean | string; color: string }) => {
  if (value === true)  return <Check className={`w-4 h-4 ${color} mx-auto`} />;
  if (value === false) return <Minus className="w-4 h-4 text-white/10 mx-auto" />;
  return <span className={`${color} text-xs font-medium`}>{value}</span>;
};

export default function FeatureComparisonGrid() {
  return (
    <motion.div
      initial={{ opacity: 0, y: 20 }}
      animate={{ opacity: 1, y: 0 }}
      transition={{ duration: 0.4, delay: 0.4 }}
      className="bg-card rounded-2xl border border-border p-6 md:p-8 mb-12"
    >
      <div className="flex items-center justify-between mb-6">
        <h2 className="text-xl font-bold text-white">Feature-Vergleich</h2>
        <span className="text-sm text-white/40">Alle Features im Überblick</span>
      </div>

      <div className="overflow-x-auto -mx-4 md:mx-0">
        <table className="w-full min-w-[620px] md:min-w-0 text-sm">
          <thead>
            <tr className="border-b border-white/10">
              <th className="text-left py-3 text-white/40 font-normal px-4 md:px-0">Feature</th>
              <th className="text-center py-3 text-white/50 font-medium w-20">Free</th>
              <th className="text-center py-3 text-[#ff7a18] font-medium w-20">Basic</th>
              <th className="text-center py-3 text-[#10b7ad] font-medium w-20">Erweitert</th>
              <th className="text-center py-3 font-medium w-20" style={{ color: '#0ea5e9' }}>Bundle</th>
            </tr>
          </thead>
          <tbody className="text-white/60">
            {featureData.map((cat) => (
              <>
                <tr key={`cat-${cat.category}`} className="border-b border-white/5">
                  <td colSpan={5} className="py-3">
                    <span className="text-xs font-semibold text-white/40 uppercase tracking-wider">
                      {cat.category}
                    </span>
                  </td>
                </tr>
                {cat.features.map((f) => (
                  <tr key={f.name} className="border-b border-white/5 hover:bg-white/[0.02] transition-colors">
                    <td className="py-3 px-4 md:px-0">{f.name}</td>
                    <td className="text-center py-3"><Cell value={f.free}     color="text-white/30" /></td>
                    <td className="text-center py-3"><Cell value={f.basic}    color="text-[#ff7a18]" /></td>
                    <td className="text-center py-3"><Cell value={f.extended} color="text-[#10b7ad]" /></td>
                    <td className="text-center py-3"><Cell value={f.bundle}   color="text-[#0ea5e9]" /></td>
                  </tr>
                ))}
              </>
            ))}
          </tbody>
        </table>
      </div>

      <div className="flex flex-wrap items-center justify-center gap-6 mt-6 pt-6 border-t border-white/5">
        <div className="flex items-center gap-2 text-white/40 text-xs">
          <Check className="w-3.5 h-3.5 text-white/30" />
          <span>Inklusive</span>
        </div>
        <div className="flex items-center gap-2 text-white/40 text-xs">
          <Minus className="w-3.5 h-3.5 text-white/10" />
          <span>Nicht verfügbar</span>
        </div>
      </div>
    </motion.div>
  );
}
