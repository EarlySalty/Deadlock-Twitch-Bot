import type { ReactNode } from "react";
import { ArrowRight } from "lucide-react";

interface StepCardProps {
  eyebrow: string;
  title: string;
  description: string;
  visualType: "screenshot" | "animation" | "diagram";
  visualSrc?: string;
  visualContent?: ReactNode;
  ctaLabel?: string;
  ctaHref?: string;
  isActive?: boolean;
  onCtaClick?: () => void;
}

export function StepCard({
  eyebrow,
  title,
  description,
  visualType,
  visualSrc,
  visualContent,
  ctaLabel,
  ctaHref,
  isActive = false,
  onCtaClick,
}: StepCardProps) {
  return (
    <article
      className={`panel-card rounded-[1.75rem] overflow-hidden transition-all duration-300 ${
        isActive ? "ring-2 ring-accent/50" : ""
      }`}
    >
      {/* Visual Area */}
      <div className="relative h-48 bg-[rgba(7,21,29,0.46)] border-b border-border overflow-hidden">
        {visualType === "screenshot" && visualSrc && (
          <img
            src={visualSrc}
            alt={title}
            className="w-full h-full object-cover object-top opacity-80 hover:opacity-100 transition-opacity"
          />
        )}
        {visualType === "diagram" && (
          <div className="w-full h-full flex items-center justify-center p-6">
            {visualContent}
          </div>
        )}
        {visualType === "animation" && (
          <div className="w-full h-full flex items-center justify-center">
            {visualContent}
          </div>
        )}
      </div>

      {/* Content */}
      <div className="p-6">
        <p className="text-sm uppercase tracking-[0.18em] text-primary">
          {eyebrow}
        </p>
        <h3 className="mt-3 text-2xl font-bold text-text-primary">
          {title}
        </h3>
        <p className="mt-3 text-sm leading-relaxed text-text-secondary">
          {description}
        </p>

        {ctaLabel && (
          ctaHref ? (
            <a
              href={ctaHref}
              className="mt-5 inline-flex items-center gap-2 text-sm font-semibold text-text-primary no-underline transition-colors duration-200 hover:text-accent"
            >
              {ctaLabel}
              <ArrowRight size={16} />
            </a>
          ) : onCtaClick ? (
            <button
              onClick={onCtaClick}
              className="mt-5 inline-flex items-center gap-2 text-sm font-semibold text-text-primary no-underline transition-colors duration-200 hover:text-accent bg-transparent border-0 cursor-pointer"
            >
              {ctaLabel}
              <ArrowRight size={16} />
            </button>
          ) : null
        )}
      </div>
    </article>
  );
}
