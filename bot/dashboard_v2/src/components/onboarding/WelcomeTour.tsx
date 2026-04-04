import { useState, useEffect } from 'react';
import { motion, AnimatePresence } from 'framer-motion';
import { X, ArrowRight, ArrowLeft, BarChart3, Zap, TrendingUp, Crown } from 'lucide-react';

const STORAGE_KEY = 'welcome-tour-dismissed';

interface TourStep {
  icon: typeof BarChart3;
  title: string;
  description: string;
}

const TOUR_STEPS: TourStep[] = [
  {
    icon: BarChart3,
    title: 'Das ist dein Channel-Gesundheitscheck',
    description: 'Behalte deine Kanal-Kennzahlen im Blick. Wachstum, Retention, Engagement und Community - alles an einem Ort.',
  },
  {
    icon: Zap,
    title: 'Deine wichtigsten Tools auf einen Blick',
    description: 'Schnellzugriff auf alle wichtigen Funktionen: Stream-Analyse, Chat-Statistiken, Follower-Tracking und mehr.',
  },
  {
    icon: TrendingUp,
    title: 'Finde Insights um zu wachsen',
    description: 'Detaillierte Analytics zeigen dir, wann deine Zuschauer aktiv sind und welche Inhalte am besten funktionieren.',
  },
  {
    icon: Crown,
    title: '45 Tage kostenlos alle Features testen',
    description: 'Teste jetzt alle Premium-Funktionen risikofrei. Keine Kreditkarte erforderlich.',
  },
];

interface WelcomeTourProps {
  onComplete?: () => void;
}

export function WelcomeTour({ onComplete }: WelcomeTourProps) {
  const [isVisible, setIsVisible] = useState(false);
  const [currentStep, setCurrentStep] = useState(0);
  const [isExiting, setIsExiting] = useState(false);

  useEffect(() => {
    // Check if tour was already dismissed
    const dismissed = localStorage.getItem(STORAGE_KEY);
    if (!dismissed) {
      // Small delay to let the page render first
      const timer = setTimeout(() => setIsVisible(true), 500);
      return () => clearTimeout(timer);
    }
  }, []);

  const handleDismiss = () => {
    setIsExiting(true);
    setTimeout(() => {
      localStorage.setItem(STORAGE_KEY, 'true');
      setIsVisible(false);
      onComplete?.();
    }, 300);
  };

  const handleNext = () => {
    if (currentStep < TOUR_STEPS.length - 1) {
      setCurrentStep((prev) => prev + 1);
    } else {
      handleDismiss();
    }
  };

  const handleBack = () => {
    if (currentStep > 0) {
      setCurrentStep((prev) => prev - 1);
    }
  };

  const handleSkip = () => {
    handleDismiss();
  };

  if (!isVisible) return null;

  const step = TOUR_STEPS[currentStep];
  const Icon = step.icon;
  const isLastStep = currentStep === TOUR_STEPS.length - 1;

  return (
    <AnimatePresence>
      {!isExiting && (
        <motion.div
          initial={{ opacity: 0 }}
          animate={{ opacity: 1 }}
          exit={{ opacity: 0 }}
          transition={{ duration: 0.3 }}
          className="fixed inset-0 z-[100] flex items-center justify-center p-4"
          style={{ backgroundColor: 'rgba(0, 0, 0, 0.75)' }}
        >
          <motion.div
            initial={{ opacity: 0, scale: 0.95, y: 20 }}
            animate={{ opacity: 1, scale: 1, y: 0 }}
            exit={{ opacity: 0, scale: 0.95, y: 20 }}
            transition={{ duration: 0.3, delay: 0.1 }}
            className="panel-card rounded-2xl p-6 md:p-8 max-w-md w-full shadow-2xl"
          >
            {/* Close button */}
            <button
              onClick={handleSkip}
              className="absolute top-4 right-4 w-8 h-8 rounded-lg bg-white/5 border border-border flex items-center justify-center text-text-secondary hover:text-white hover:bg-white/10 transition-colors"
              aria-label="Tour beenden"
            >
              <X className="w-4 h-4" />
            </button>

            {/* Icon */}
            <div className="w-14 h-14 rounded-2xl gradient-accent flex items-center justify-center mb-6">
              <Icon className="w-7 h-7 text-white" />
            </div>

            {/* Content */}
            <h2 className="text-xl font-bold text-white mb-2">{step.title}</h2>
            <p className="text-sm text-text-secondary leading-relaxed mb-6">
              {step.description}
            </p>

            {/* Progress dots */}
            <div className="flex items-center justify-center gap-2 mb-6">
              {TOUR_STEPS.map((_, index) => (
                <div
                  key={index}
                  className={`w-2 h-2 rounded-full transition-all duration-300 ${
                    index === currentStep
                      ? 'w-6 bg-primary'
                      : index < currentStep
                        ? 'bg-primary/60'
                        : 'bg-white/20'
                  }`}
                />
              ))}
            </div>

            {/* Navigation */}
            <div className="flex items-center justify-between gap-3">
              <button
                onClick={handleBack}
                disabled={currentStep === 0}
                className="inline-flex items-center gap-2 px-4 py-2 text-sm font-semibold text-text-secondary hover:text-white disabled:opacity-40 disabled:cursor-not-allowed transition-colors"
              >
                <ArrowLeft className="w-4 h-4" />
                Zurück
              </button>

              <div className="flex items-center gap-3">
                <button
                  onClick={handleSkip}
                  className="px-4 py-2 text-sm font-semibold text-text-secondary hover:text-white transition-colors"
                >
                  Überspringen
                </button>

                <button
                  onClick={handleNext}
                  className="inline-flex items-center gap-2 px-5 py-2 rounded-xl gradient-accent text-white text-sm font-bold hover:opacity-90 transition-opacity"
                >
                  {isLastStep ? 'Tour beenden' : 'Weiter'}
                  {!isLastStep && <ArrowRight className="w-4 h-4" />}
                </button>
              </div>
            </div>
          </motion.div>
        </motion.div>
      )}
    </AnimatePresence>
  );
}

// Helper function to reset the tour (for testing)
export function resetWelcomeTour() {
  localStorage.removeItem(STORAGE_KEY);
}
