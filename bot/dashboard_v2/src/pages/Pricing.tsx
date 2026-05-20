import { useState } from 'react';
import { motion, AnimatePresence } from 'framer-motion';
import { ChevronDown, HelpCircle } from 'lucide-react';
import { useBillingCatalog } from '../hooks/useAnalytics';
import PricingHero from '../components/pricing/PricingHero';
import FeaturePicker from '../components/pricing/FeaturePicker';
import FeatureComparisonGrid from '../components/pricing/FeatureComparisonGrid';
import { PREVIEW_HOME_ROUTE } from '../preview/routes';

const faqData = [
  {
    question: 'Wie funktioniert die 30-tägige kostenlose Testphase?',
    answer:
      'Du meldest dich an, wählst einen Plan aus und kannst ihn 30 Tage lang kostenlos nutzen. Deine Kreditkarte wird erst nach Ablauf der Testphase belastet – vorausgesetzt, du kündigst nicht vorher.',
  },
  {
    question: 'Kann ich meinen Plan jederzeit kündigen?',
    answer:
      'Ja, du kannst dein Abonnement jederzeit kündigen. Es gibt keine Mindestlaufzeit. Bis zum Ende des Abrechnungszeitraums behältst du vollen Zugang zu deinen Features.',
  },
  {
    question: 'Was passiert mit meinen Daten nach der Kündigung?',
    answer:
      'Deine Analytics-Daten bleiben für 30 Tage nach Kündigung gespeichert. Du kannst sie jederzeit exportieren oder dein Konto reaktivieren, um wieder Zugang zu erhalten.',
  },
  {
    question: 'Welcher Plan ist der richtige für mich?',
    answer:
      'Der Basic-Plan ist ideal für Streamer, die ihre Analytics verbessern möchten. Der Extended-Plan enthält zusätzlich KI-gestützte Analysen und Coaching-Features für alle, die professionell wachsen wollen.',
  },
];

function FAQItem({ question, answer }: { question: string; answer: string }) {
  const [isOpen, setIsOpen] = useState(false);

  return (
    <div className="border-b border-white/5 last:border-0">
      <button
        onClick={() => setIsOpen(!isOpen)}
        className="w-full flex items-center justify-between py-4 text-left hover:text-white/80 transition-colors"
      >
        <span className="font-medium text-white/80">{question}</span>
        <ChevronDown
          className={`w-5 h-5 text-white/40 transition-transform duration-200 ${
            isOpen ? 'rotate-180' : ''
          }`}
        />
      </button>
      <AnimatePresence>
        {isOpen && (
          <motion.div
            initial={{ height: 0, opacity: 0 }}
            animate={{ height: 'auto', opacity: 1 }}
            exit={{ height: 0, opacity: 0 }}
            transition={{ duration: 0.2 }}
            className="overflow-hidden"
          >
            <p className="pb-4 text-white/50 leading-relaxed">{answer}</p>
          </motion.div>
        )}
      </AnimatePresence>
    </div>
  );
}

export default function Pricing() {
  const [cycle, setCycle] = useState<1 | 12>(1);
  const { data } = useBillingCatalog(cycle);
  const plans = data?.plans ?? [];

  return (
    <div className="max-w-5xl mx-auto px-4 py-8">
      {/* Hero Section */}
      <PricingHero />

      {/* Billing cycle toggle */}
      <div id="plans" className="flex justify-center mb-8">
        <div className="inline-flex items-center gap-1 p-1 rounded-xl bg-white/5 border border-white/10">
          <button
            onClick={() => setCycle(1)}
            className={`px-5 py-2 rounded-lg text-sm font-medium transition-all duration-200 ${
              cycle === 1
                ? 'bg-white/10 text-white shadow'
                : 'text-white/50 hover:text-white/70'
            }`}
          >
            Monatlich
          </button>
          <button
            onClick={() => setCycle(12)}
            className={`relative px-5 py-2 rounded-lg text-sm font-medium transition-all duration-200 ${
              cycle === 12
                ? 'bg-white/10 text-white shadow'
                : 'text-white/50 hover:text-white/70'
            }`}
          >
            Jährlich
            <span className="ml-2 px-1.5 py-0.5 rounded-md text-xs font-semibold bg-[#10b7ad]/20 text-[#10b7ad]">
              2 Mo. gratis
            </span>
          </button>
        </div>
      </div>

      {/* Feature Picker */}
      <motion.div
        initial={{ opacity: 0, y: 16 }}
        animate={{ opacity: 1, y: 0 }}
        transition={{ duration: 0.4, delay: 0.15 }}
        className="mb-12"
      >
        <FeaturePicker plans={plans} cycle={cycle} />
      </motion.div>

      {/* Feature Comparison */}
      <FeatureComparisonGrid />

      {/* FAQ Section */}
      <motion.div
        initial={{ opacity: 0, y: 20 }}
        animate={{ opacity: 1, y: 0 }}
        transition={{ duration: 0.4, delay: 0.5 }}
        className="bg-card rounded-2xl border border-border p-6 md:p-8"
      >
        <div className="flex items-center gap-3 mb-6">
          <HelpCircle className="w-5 h-5 text-[#ff7a18]" />
          <h2 className="text-xl font-bold text-white">Häufige Fragen</h2>
        </div>
        <div>
          {faqData.map((faq, index) => (
            <FAQItem key={index} question={faq.question} answer={faq.answer} />
          ))}
        </div>
      </motion.div>

      {/* Bottom CTA */}
      <motion.div
        initial={{ opacity: 0 }}
        animate={{ opacity: 1 }}
        transition={{ duration: 0.4, delay: 0.6 }}
        className="text-center mt-12 mb-8"
      >
        <p className="text-white/40 mb-4">Noch nicht überzeugt?</p>
        <a
          href={PREVIEW_HOME_ROUTE}
          className="inline-flex items-center gap-2 text-[#10b7ad] hover:text-[#1dd4ca] font-medium transition-colors"
        >
          Zurück zum Dashboard
        </a>
      </motion.div>
    </div>
  );
}
