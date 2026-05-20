import { motion } from 'framer-motion';
import { Sparkles, CreditCard, ArrowRight } from 'lucide-react';
import { PREVIEW_BILLING_ROUTE, isPreviewModeEnabled } from '../../preview/routes';

export default function TrialCallout() {
  return (
    <motion.div
      initial={{ opacity: 0, scale: 0.95 }}
      animate={{ opacity: 1, scale: 1 }}
      transition={{ duration: 0.4, delay: 0.1 }}
      className="relative mb-12"
    >
      {/* Animated border glow */}
      <div className="absolute -inset-0.5 rounded-2xl bg-gradient-to-r from-[#ff7a18] via-[#10b7ad] to-[#ff7a18] opacity-60 blur-sm animate-pulse" />
      <div className="absolute -inset-[1px] rounded-2xl bg-gradient-to-r from-[#ff7a18] via-[#10b7ad] to-[#ff7a18] opacity-80" />

      {/* Main card */}
      <div className="relative rounded-2xl bg-[#0e2a3a] p-6 md:p-8">
        <div className="flex flex-col md:flex-row items-center justify-between gap-6">
          {/* Left: Content */}
          <div className="flex items-center gap-4">
            <div className="flex-shrink-0 w-14 h-14 rounded-xl bg-gradient-to-br from-[#ff7a18]/20 to-[#10b7ad]/20 flex items-center justify-center border border-[#ff7a18]/30">
              <Sparkles className="w-7 h-7 text-[#ff7a18]" />
            </div>
            <div>
              <h3 className="text-xl font-bold text-white mb-1">
                30 Tage kostenlos testen
              </h3>
              <p className="text-white/60 flex items-center gap-2">
                <CreditCard className="w-4 h-4" />
                Keine Kreditkarte erforderlich – risikofrei starten
              </p>
            </div>
          </div>

          {/* Right: CTA */}
          <a
            href={isPreviewModeEnabled() ? PREVIEW_BILLING_ROUTE : '/twitch/abbo'}
            className="flex items-center gap-2 px-6 py-3 rounded-xl bg-white/10 hover:bg-white/15 border border-white/20 text-white font-medium transition-all duration-200 hover:gap-3"
          >
            Mehr erfahren
            <ArrowRight className="w-4 h-4" />
          </a>
        </div>

        {/* Decorative elements */}
        <div className="absolute top-0 right-0 w-32 h-32 bg-[#ff7a18]/5 rounded-full blur-3xl" />
        <div className="absolute bottom-0 left-0 w-40 h-40 bg-[#10b7ad]/5 rounded-full blur-3xl" />
      </div>
    </motion.div>
  );
}
