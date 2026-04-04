import { motion } from "framer-motion";
import { Check } from "lucide-react";

interface OnboardingProgressProps {
  steps: string[];
  currentStep: number;
  onStepClick?: (step: number) => void;
}

export function OnboardingProgress({
  steps,
  currentStep,
  onStepClick,
}: OnboardingProgressProps) {
  return (
    <div className="flex items-center justify-center gap-2">
      {steps.map((step, index) => {
        const isCompleted = index < currentStep;
        const isCurrent = index === currentStep;
        const isClickable = onStepClick !== undefined;

        return (
          <div key={step} className="flex items-center gap-2">
            {/* Step indicator */}
            <button
              onClick={() => isClickable && onStepClick?.(index)}
              disabled={!isClickable}
              className={`relative flex items-center justify-center w-10 h-10 rounded-full border-2 transition-all duration-300 ${
                isClickable ? "cursor-pointer" : "cursor-default"
              } ${
                isCompleted
                  ? "bg-accent border-accent"
                  : isCurrent
                    ? "border-accent bg-accent/20"
                    : "border-border bg-[rgba(7,21,29,0.46)]"
              }`}
            >
              {isCompleted ? (
                <Check size={18} className="text-white" />
              ) : (
                <span
                  className={`text-sm font-semibold ${
                    isCurrent ? "text-accent" : "text-text-secondary"
                  }`}
                >
                  {index + 1}
                </span>
              )}

              {/* Active pulse ring */}
              {isCurrent && (
                <motion.span
                  className="absolute inset-0 rounded-full border-2 border-accent"
                  initial={{ scale: 1, opacity: 0.5 }}
                  animate={{ scale: 1.5, opacity: 0 }}
                  transition={{ duration: 1.5, repeat: Infinity }}
                />
              )}
            </button>

            {/* Step label - only show on larger screens */}
            <span
              className={`hidden md:block text-sm font-medium transition-colors ${
                isCurrent
                  ? "text-text-primary"
                  : isCompleted
                    ? "text-accent"
                    : "text-text-secondary"
              }`}
            >
              {step}
            </span>

            {/* Connector line */}
            {index < steps.length - 1 && (
              <div
                className={`hidden md:block w-12 h-0.5 transition-colors ${
                  index < currentStep ? "bg-accent" : "bg-border"
                }`}
              />
            )}
          </div>
        );
      })}
    </div>
  );
}
