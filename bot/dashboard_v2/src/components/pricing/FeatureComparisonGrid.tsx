import { motion } from 'framer-motion';
import { Check, Minus } from 'lucide-react';

interface FeatureRow {
  name: string;
  free: boolean | string;
  werbefrei: boolean | string;
  raid: boolean | string;
  analyse: boolean | string;
  bundle: boolean | string;
}

interface FeatureCategory {
  category: string;
  features: FeatureRow[];
}

// Spalten = echte Pläne: Free | Werbefrei (chat_quiet) | Raid Boost (raid_boost) | Analyse (analysis_dashboard) | Alles drin (bundle_komplett)
const featureData: FeatureCategory[] = [
  {
    category: 'Analytics',
    features: [
      { name: 'Viewer-Verlauf & Trends', free: true,  werbefrei: true,  raid: true,  analyse: true,  bundle: true  },
      { name: 'Stream-Übersicht',        free: true,  werbefrei: true,  raid: true,  analyse: true,  bundle: true  },
      { name: 'Schedule Heatmap',        free: true,  werbefrei: true,  raid: true,  analyse: true,  bundle: true  },
      { name: 'Chat-Analytics',          free: false, werbefrei: false, raid: true,  analyse: true,  bundle: true  },
      { name: 'Growth-Tracking',         free: false, werbefrei: false, raid: true,  analyse: true,  bundle: true  },
      { name: 'Zeitraumvergleiche',      free: false, werbefrei: false, raid: true,  analyse: true,  bundle: true  },
      { name: 'Audience-Insights',       free: false, werbefrei: false, raid: true,  analyse: true,  bundle: true  },
      { name: 'Follower-Übersichten',    free: false, werbefrei: false, raid: true,  analyse: true,  bundle: true  },
      { name: 'Kategorie-Vergleich',     free: false, werbefrei: false, raid: false, analyse: true,  bundle: true  },
      { name: 'Viewer-Profile',          free: false, werbefrei: false, raid: false, analyse: true,  bundle: true  },
    ],
  },
  {
    category: 'KI-Analyse',
    features: [
      { name: 'KI-Zusammenfassung',     free: false, werbefrei: false, raid: 'Basis', analyse: 'Vollständig', bundle: 'Vollständig' },
      { name: 'Stream-Coaching',        free: false, werbefrei: false, raid: false,   analyse: true,           bundle: true          },
      { name: 'Monetarisierungs-Tipps', free: false, werbefrei: false, raid: false,   analyse: true,           bundle: true          },
    ],
  },
  {
    category: 'Community & Chat',
    features: [
      { name: 'Lurker-Steuer Erinnerungen', free: false, werbefrei: false, raid: true,  analyse: true,  bundle: true },
      { name: 'Chat-Social-Graph',          free: false, werbefrei: false, raid: false, analyse: true,  bundle: true },
      { name: 'Bot-Werbung deaktivieren',   free: false, werbefrei: true,  raid: false, analyse: false, bundle: true },
    ],
  },
  {
    category: 'Raid-Netzwerk',
    features: [
      { name: 'Auto-Raid Grundfunktion',      free: true,  werbefrei: true,  raid: true,  analyse: true,  bundle: true },
      { name: 'Bevorzugte Raid-Platzierung',  free: false, werbefrei: false, raid: true,  analyse: false, bundle: true },
      { name: 'Sichtbarkeit bei Inaktivität', free: false, werbefrei: false, raid: true,  analyse: false, bundle: true },
      { name: 'Raid-Retention-Analyse',       free: false, werbefrei: false, raid: false, analyse: false, bundle: true },
    ],
  },
  {
    category: 'Sonstiges',
    features: [
      { name: '30 Tage Analyse-Testphase',  free: false, werbefrei: false, raid: false, analyse: true, bundle: false },
      { name: 'Priority Support',           free: false, werbefrei: true,  raid: true,  analyse: true, bundle: true  },
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
        <table className="w-full min-w-[720px] md:min-w-0 text-sm">
          <thead>
            <tr className="border-b border-white/10">
              <th className="text-left py-3 text-white/40 font-normal px-4 md:px-0">Feature</th>
              <th className="text-center py-3 text-white/50 font-medium w-20">Free</th>
              <th className="text-center py-3 font-medium w-20" style={{ color: '#ff7a18' }}>Werbefrei</th>
              <th className="text-center py-3 font-medium w-20" style={{ color: '#10b7ad' }}>Raid Boost</th>
              <th className="text-center py-3 font-medium w-20" style={{ color: '#a78bfa' }}>Analyse</th>
              <th className="text-center py-3 font-medium w-20" style={{ color: '#f59e0b' }}>Alles drin</th>
            </tr>
          </thead>
          <tbody className="text-white/60">
            {featureData.map((cat) => (
              <>
                <tr key={`cat-${cat.category}`} className="border-b border-white/5">
                  <td colSpan={6} className="py-3">
                    <span className="text-xs font-semibold text-white/40 uppercase tracking-wider">
                      {cat.category}
                    </span>
                  </td>
                </tr>
                {cat.features.map((f) => (
                  <tr key={f.name} className="border-b border-white/5 hover:bg-white/[0.02] transition-colors">
                    <td className="py-3 px-4 md:px-0">{f.name}</td>
                    <td className="text-center py-3"><Cell value={f.free}      color="text-white/30"    /></td>
                    <td className="text-center py-3"><Cell value={f.werbefrei} color="text-[#ff7a18]"   /></td>
                    <td className="text-center py-3"><Cell value={f.raid}      color="text-[#10b7ad]"   /></td>
                    <td className="text-center py-3"><Cell value={f.analyse}   color="text-[#a78bfa]"   /></td>
                    <td className="text-center py-3"><Cell value={f.bundle}    color="text-[#f59e0b]"   /></td>
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
        <div className="text-white/25 text-xs">
          Bot-Werbung deaktivieren ist ausschließlich im Werbefrei-Plan enthalten — nicht im Trial.
        </div>
      </div>
    </motion.div>
  );
}
