import { motion } from 'framer-motion';
import { CheckCircle2, ExternalLink, Receipt } from 'lucide-react';
import type { CurrentSubscription } from '../../types/billing';

interface MySubscriptionCardProps {
  subscription: CurrentSubscription;
  /** Pfad zum Stripe-Kundenportal (Rechnungen, Zahlung, Kündigung). */
  invoiceHref: string;
}

/**
 * Prominenter „Mein Abo"-Block für aktive Abonnenten. Zeigt den laufenden Plan
 * und führt zum Stripe-Kundenportal, in dem Rechnungen, Zahlungsmethode und
 * Kündigung zentral verwaltet werden. Wird nur gerendert, wenn ein bezahltes
 * Abo aktiv ist (siehe isActivePaidSubscription).
 */
export default function MySubscriptionCard({ subscription, invoiceHref }: MySubscriptionCardProps) {
  const planName = subscription.plan_name || 'Aktueller Plan';

  return (
    <motion.div
      initial={{ opacity: 0, y: 16 }}
      animate={{ opacity: 1, y: 0 }}
      transition={{ duration: 0.4 }}
      className="mb-10 rounded-2xl border border-[#00D9FF]/30 bg-gradient-to-r from-[#00D9FF]/10 to-transparent p-6"
    >
      <div className="flex flex-col gap-5 sm:flex-row sm:items-center sm:justify-between">
        <div>
          <div className="mb-2 flex items-center gap-2">
            <span className="inline-flex items-center gap-1.5 rounded-full bg-[#00D9FF]/15 px-2.5 py-0.5 text-xs font-semibold text-[#00D9FF]">
              <CheckCircle2 className="h-3.5 w-3.5" />
              Aktives Abo
            </span>
          </div>
          <h2 className="text-xl font-bold text-white">{planName}</h2>
          <p className="mt-1 text-sm text-white/50">
            Rechnungen, Zahlungsdaten und Kündigung verwaltest du im Kundenportal.
          </p>
        </div>

        <a
          href={invoiceHref}
          className="inline-flex flex-shrink-0 items-center justify-center gap-2 rounded-xl bg-[#00D9FF] px-6 py-3 text-sm font-semibold text-white shadow-lg shadow-[#00D9FF]/20 transition-all duration-200 hover:bg-[#00D9FF]"
        >
          <Receipt className="h-4 w-4" />
          Rechnungen &amp; Abo verwalten
          <ExternalLink className="h-3.5 w-3.5 opacity-70" />
        </a>
      </div>
    </motion.div>
  );
}
