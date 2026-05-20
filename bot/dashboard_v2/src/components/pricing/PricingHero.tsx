import { motion } from 'framer-motion';
import { Zap, TrendingUp, Users } from 'lucide-react';
import { PREVIEW_BILLING_ROUTE, isPreviewModeEnabled } from '../../preview/routes';

export default function PricingHero() {
  return (
    <section className="relative text-center mb-12 overflow-hidden">
      {/* Background gradient */}
      <div className="absolute inset-0 -z-10">
        <div className="absolute inset-0 bg-gradient-to-b from-[#ff7a18]/10 via-transparent to-transparent" />
        <div
          className="absolute inset-0 opacity-30"
          style={{
            background:
              'radial-gradient(ellipse 80% 50% at 50% 0%, rgba(255, 122, 24, 0.15), transparent 70%)',
          }}
        />
      </div>

      <motion.div
        initial={{ opacity: 0, y: 20 }}
        animate={{ opacity: 1, y: 0 }}
        transition={{ duration: 0.5 }}
      >
        {/* Badge */}
        <div className="inline-flex items-center gap-2 px-4 py-1.5 rounded-full bg-[#ff7a18]/10 border border-[#ff7a18]/20 mb-6">
          <Zap className="w-4 h-4 text-[#ff7a18]" />
          <span className="text-sm font-medium text-[#ff7a18]">Dein Growth Coach für Twitch</span>
        </div>

        {/* Headline */}
        <h1 className="text-4xl md:text-5xl font-bold text-white mb-4 tracking-tight">
          Mehr Wachstum, mehr{' '}
          <span className="text-transparent bg-clip-text bg-gradient-to-r from-[#ff7a18] to-[#10b7ad]">
            Insights
          </span>
        </h1>

        {/* Subheadline */}
        <p className="text-lg md:text-xl text-white/60 max-w-2xl mx-auto mb-8">
          Verstehe deine Zuschauer, optimiere deinen Content und wachse schneller –
          mit KI-gestützten Analysen, die dir zeigen, was wirklich funktioniert.
        </p>

        {/* Value props */}
        <div className="flex flex-wrap justify-center gap-6 mb-8">
          <div className="flex items-center gap-2 text-white/50">
            <TrendingUp className="w-5 h-5 text-[#10b7ad]" />
            <span>Tracke deinen Fortschritt</span>
          </div>
          <div className="flex items-center gap-2 text-white/50">
            <Users className="w-5 h-5 text-[#10b7ad]" />
            <span>Verstehe deine Community</span>
          </div>
        </div>

        {/* CTA Button */}
        <a
          href={isPreviewModeEnabled() ? PREVIEW_BILLING_ROUTE : '/twitch/pricing#plans'}
          className="inline-flex items-center gap-2 px-8 py-4 rounded-xl bg-gradient-to-r from-[#ff7a18] to-[#ff8d39] text-white font-semibold text-lg shadow-lg shadow-[#ff7a18]/25 hover:shadow-[#ff7a18]/40 hover:scale-105 transition-all duration-200"
        >
          45 Tage kostenlos starten
        </a>
        <p className="mt-3 text-sm text-white/40">Keine Kreditkarte erforderlich</p>
      </motion.div>
    </section>
  );
}
