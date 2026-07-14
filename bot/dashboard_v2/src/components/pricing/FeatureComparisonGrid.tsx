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
  if (value === true)  return <Check className="w-4 h-4 mx-auto" style={{ color }} />;
  if (value === false) return <Minus className="w-4 h-4 mx-auto" style={{ color: 'var(--ink-muted)', opacity: 0.4 }} />;
  return <span className="text-xs font-medium" style={{ color }}>{value}</span>;
};

/* Das Datenblatt liegt auf Pergament: dunkle Tinte auf hellem Papier.
   Antik-Gold und Plasma waeren hier unlesbar — dafuer gibt es die ink-Toene. */
const INK_GOLD  = 'var(--ink-gold)';
const INK_BLUE  = 'var(--ink-blue)';
const INK_EMBER = 'var(--ink-ember)';
const INK_MUTED = 'var(--ink-muted)';

export default function FeatureComparisonGrid() {
  return (
    <motion.div
      initial={{ opacity: 0, y: 20 }}
      animate={{ opacity: 1, y: 0 }}
      transition={{ duration: 0.4, delay: 0.4 }}
      className="parchment rounded-2xl p-6 md:p-8 mb-12"
    >
      <div className="flex items-center justify-between mb-6">
        <h2 className="text-xl font-bold" style={{ color: 'var(--parchment-ink)' }}>Feature-Vergleich</h2>
        <span className="text-sm" style={{ color: INK_MUTED }}>Alle Features im Überblick</span>
      </div>

      <div className="overflow-x-auto -mx-4 md:mx-0">
        <table className="w-full min-w-[720px] md:min-w-0 text-sm">
          <thead>
            <tr>
              <th className="text-left py-3 font-normal px-4 md:px-0" style={{ color: INK_MUTED }}>Feature</th>
              <th className="text-center py-3 font-medium w-20" style={{ color: INK_MUTED }}>Free</th>
              <th className="text-center py-3 font-medium w-20" style={{ color: INK_GOLD }}>Werbefrei</th>
              <th className="text-center py-3 font-medium w-20" style={{ color: INK_BLUE }}>Raid Boost</th>
              <th className="text-center py-3 font-medium w-20" style={{ color: INK_BLUE }}>Analyse</th>
              <th className="text-center py-3 font-medium w-20" style={{ color: INK_EMBER }}>Alles drin</th>
            </tr>
          </thead>
          <tbody style={{ color: 'var(--parchment-ink)' }}>
            {featureData.map((cat) => (
              <>
                <tr key={`cat-${cat.category}`}>
                  <td colSpan={6} className="py-3">
                    <span className="text-xs font-semibold uppercase tracking-wider" style={{ color: INK_MUTED }}>
                      {cat.category}
                    </span>
                  </td>
                </tr>
                {cat.features.map((f) => (
                  <tr key={f.name} className="transition-colors">
                    <td className="py-3 px-4 md:px-0">{f.name}</td>
                    <td className="text-center py-3"><Cell value={f.free}      color={INK_MUTED} /></td>
                    <td className="text-center py-3"><Cell value={f.werbefrei} color={INK_GOLD}  /></td>
                    <td className="text-center py-3"><Cell value={f.raid}      color={INK_BLUE}  /></td>
                    <td className="text-center py-3"><Cell value={f.analyse}   color={INK_BLUE}  /></td>
                    <td className="text-center py-3"><Cell value={f.bundle}    color={INK_EMBER} /></td>
                  </tr>
                ))}
              </>
            ))}
          </tbody>
        </table>
      </div>

      <div
        className="flex flex-wrap items-center justify-center gap-6 mt-6 pt-6"
        style={{ borderTop: '1px solid var(--parchment-rule)' }}
      >
        <div className="flex items-center gap-2 text-xs" style={{ color: INK_MUTED }}>
          <Check className="w-3.5 h-3.5" style={{ color: 'var(--parchment-ink)' }} />
          <span>Inklusive</span>
        </div>
        <div className="flex items-center gap-2 text-xs" style={{ color: INK_MUTED }}>
          <Minus className="w-3.5 h-3.5" style={{ color: INK_MUTED, opacity: 0.4 }} />
          <span>Nicht verfügbar</span>
        </div>
        <div className="text-xs" style={{ color: INK_MUTED }}>
          Bot-Werbung deaktivieren ist ausschließlich im Werbefrei-Plan enthalten — nicht im Trial.
        </div>
      </div>
    </motion.div>
  );
}
