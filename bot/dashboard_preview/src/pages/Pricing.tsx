import { useState } from 'react';
import { motion, AnimatePresence } from 'framer-motion';
import { ChevronDown, HelpCircle } from 'lucide-react';
import { useBillingCatalog } from '../hooks/useAnalytics';
import { PREVIEW_HOME_ROUTE, PREVIEW_ANALYTICS_ROUTE } from '../preview/routes';
import { PricingTour } from '../components/onboarding/PricingTour';
import PricingHero from '../components/pricing/PricingHero';
import TrialCallout from '../components/pricing/TrialCallout';
import PlanCardRedesign from '../components/pricing/PlanCardRedesign';
import FeatureComparisonGrid from '../components/pricing/FeatureComparisonGrid';

const faqData = [
  {
    question: 'Wie funktioniert die 45-tägige kostenlose Testphase?',
    answer:
      'Du meldest dich an, wählst einen Plan aus und kannst ihn 45 Tage lang kostenlos nutzen. Deine Kreditkarte wird erst nach Ablauf der Testphase belastet – vorausgesetzt, du kündigst nicht vorher.',
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
  const { data } = useBillingCatalog();
  const plans = data?.plans ?? [];

  return (
    <div className="max-w-5xl mx-auto px-4 py-8">
      <PricingTour onComplete={() => {
        localStorage.setItem('analytics-tour-pending', '1');
        window.location.href = PREVIEW_ANALYTICS_ROUTE;
      }} />

      {/* Hero Section */}
      <PricingHero />

      {/* Trial Callout Banner */}
      <TrialCallout />

      {/* Plan Cards Grid */}
      <div className="grid sm:grid-cols-2 xl:grid-cols-4 gap-6 mb-12">
        {plans.map((plan, index) => (
          <PlanCardRedesign key={plan.id} plan={plan} index={index} />
        ))}
      </div>

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
