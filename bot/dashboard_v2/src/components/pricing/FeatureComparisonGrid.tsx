import { motion } from 'framer-motion';
import { Check, Minus } from 'lucide-react';

interface FeatureRow {
  category: string;
  features: {
    name: string;
    free: boolean | string;
    basic: boolean | string;
    extended: boolean | string;
  }[];
}

const featureData: FeatureRow[] = [
  {
    category: 'Analytics',
    features: [
      { name: 'Viewer-Trend', free: true, basic: true, extended: true },
      { name: 'Stream-Übersicht', free: true, basic: true, extended: true },
      { name: 'Schedule Heatmap', free: true, basic: true, extended: true },
      { name: 'Chat-Analytics', free: false, basic: true, extended: true },
      { name: 'Growth-Tracking', free: false, basic: true, extended: true },
      { name: 'Audience-Insights', free: false, basic: true, extended: true },
    ],
  },
  {
    category: 'Erweiterte Features',
    features: [
      { name: 'Kategorie-Vergleich', free: false, basic: true, extended: true },
      { name: 'AI-Analyse', free: false, basic: false, extended: true },
      { name: 'Viewer-Profile', free: false, basic: false, extended: true },
      { name: 'Coaching', free: false, basic: false, extended: true },
      { name: 'Monetization', free: false, basic: false, extended: true },
    ],
  },
  {
    category: 'Community',
    features: [
      { name: 'Lurker-Analyse', free: false, basic: true, extended: true },
      { name: 'Chat-Social-Graph', free: false, basic: false, extended: true },
      { name: 'Raid-Retention', free: false, basic: false, extended: true },
    ],
  },
];

const FreeIcon = ({ value }: { value: boolean | string }) => {
  if (value === true) return <Check className="w-4 h-4 text-white/30 mx-auto" />;
  if (value === false) return <Minus className="w-4 h-4 text-white/10 mx-auto" />;
  return <span className="text-white/30 text-xs">{value}</span>;
};

const BasicIcon = ({ value }: { value: boolean | string }) => {
  if (value === true) return <Check className="w-4 h-4 text-[#ff7a18] mx-auto" />;
  if (value === false) return <Minus className="w-4 h-4 text-white/10 mx-auto" />;
  return <span className="text-[#ff7a18] text-xs font-medium">{value}</span>;
};

const ExtendedIcon = ({ value }: { value: boolean | string }) => {
  if (value === true) return <Check className="w-4 h-4 text-[#10b7ad] mx-auto" />;
  if (value === false) return <Minus className="w-4 h-4 text-white/10 mx-auto" />;
  return <span className="text-[#10b7ad] text-xs font-medium">{value}</span>;
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
        <table className="w-full min-w-[500px] md:min-w-0 text-sm">
          <thead>
            <tr className="border-b border-white/10">
              <th className="text-left py-3 text-white/40 font-normal px-4 md:px-0">Feature</th>
              <th className="text-center py-3 text-white/60 font-medium w-24">Free</th>
              <th className="text-center py-3 text-[#ff7a18] font-medium w-24">Basic</th>
              <th className="text-center py-3 text-[#10b7ad] font-medium w-24">Erweitert</th>
            </tr>
          </thead>
          <tbody className="text-white/60">
            {featureData.map((category) => (
              <>
                <tr key={`cat-${category.category}`} className="border-b border-white/5">
                  <td colSpan={4} className="py-3">
                    <span className="text-xs font-semibold text-white/40 uppercase tracking-wider">
                      {category.category}
                    </span>
                  </td>
                </tr>
                {category.features.map((feature) => (
                  <tr key={feature.name} className="border-b border-white/5 hover:bg-white/[0.02] transition-colors">
                    <td className="py-3 px-4 md:px-0">{feature.name}</td>
                    <td className="text-center py-3">
                      <FreeIcon value={feature.free} />
                    </td>
                    <td className="text-center py-3">
                      <BasicIcon value={feature.basic} />
                    </td>
                    <td className="text-center py-3">
                      <ExtendedIcon value={feature.extended} />
                    </td>
                  </tr>
                ))}
              </>
            ))}
          </tbody>
        </table>
      </div>

      {/* Legend */}
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
