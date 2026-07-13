import { motion } from 'framer-motion';
import { Zap, TrendingUp, Users } from 'lucide-react';
import { PREVIEW_BILLING_ROUTE } from '../../preview/routes';

export default function PricingHero() {
  return (
    <section className="relative text-center mb-12 overflow-hidden">
      {/* Background gradient */}
      <div className="absolute inset-0 -z-10">
        <div className="absolute inset-0 bg-gradient-to-b from-[#c8a86b]/8 via-transparent to-transparent" />
        <div
          className="absolute inset-0 opacity-26"
          style={{
            background:
              'radial-gradient(ellipse 80% 50% at 50% 0%, rgba(201, 168, 106, 0.12), transparent 70%)',
          }}
        />
      </div>

      <motion.div
        initial={{ opacity: 0, y: 20 }}
        animate={{ opacity: 1, y: 0 }}
        transition={{ duration: 0.5 }}
      >
        {/* Badge */}
        <div className="inline-flex items-center gap-2 px-4 py-1.5 rounded-full bg-[#c8a86b]/10 border border-[#c8a86b]/20 mb-6">
          <Zap className="w-4 h-4 text-[#c8a86b]" />
          <span className="text-sm font-medium text-[#c8a86b]">Dein Growth Coach für Twitch</span>
        </div>

        {/* Headline */}
        <h1 className="text-4xl md:text-5xl font-bold text-white mb-4 tracking-tight">
          Mehr Wachstum, mehr{' '}
          <span className="text-transparent bg-clip-text bg-gradient-to-r from-[#c8a86b] via-[#55978f] to-[#55978f]">
            Insights
          </span>
        </h1>

        {/* Subheadline */}
        <p className="text-lg md:text-xl text-white/68 max-w-2xl mx-auto mb-8">
          Verstehe deine Zuschauer, optimiere deinen Content und wachse schneller –
          mit KI-gestützten Analysen, die dir zeigen, was wirklich funktioniert.
        </p>

        {/* Value props */}
        <div className="flex flex-wrap justify-center gap-6 mb-8">
          <div className="flex items-center gap-2 text-white/58">
            <TrendingUp className="w-5 h-5 text-[#55978f]" />
            <span>Tracke deinen Fortschritt</span>
          </div>
          <div className="flex items-center gap-2 text-white/58">
            <Users className="w-5 h-5 text-[#55978f]" />
            <span>Verstehe deine Community</span>
          </div>
        </div>

        {/* CTA Button */}
        <a
          href={PREVIEW_BILLING_ROUTE}
          className="inline-flex items-center gap-2 px-8 py-4 rounded-xl bg-gradient-to-r from-[#c8a86b] via-[#55978f] to-[#55978f] text-white font-semibold text-lg shadow-lg shadow-[#c8a86b]/18 hover:shadow-[#c8a86b]/28 hover:scale-105 transition-all duration-200"
        >
          45 Tage kostenlos starten
        </a>
        <p className="mt-3 text-sm text-white/40">Keine Kreditkarte erforderlich</p>
      </motion.div>
    </section>
  );
}
