import type { ComponentType } from "react";
import {
  Zap,
  BarChart2,
  BarChart3,
  Clapperboard,
  Film,
  Users,
  Activity,
  Shield,
  ShieldCheck,
  Swords,
} from "lucide-react";
import { ScrollReveal } from "./ScrollReveal";

const iconMap: Record<string, ComponentType<{ size?: number; className?: string }>> = {
  Zap,
  BarChart2,
  BarChart3,
  Clapperboard,
  Film,
  Users,
  Activity,
  Shield,
  ShieldCheck,
  Swords,
};

interface FeatureCardProps {
  icon: string;
  title: string;
  description: string;
  delay?: number;
}

export function FeatureCard({
  icon,
  title,
  description,
  delay,
}: FeatureCardProps) {
  const IconComponent = iconMap[icon];

  return (
    <ScrollReveal delay={delay}>
      <div className="bg-[var(--color-card)] rounded-xl border border-[var(--color-border)] p-6 soft-elevate h-full">
        <div className="w-12 h-12 rounded-lg icon-tile flex items-center justify-center mb-4">
          {IconComponent ? (
            <IconComponent size={22} />
          ) : (
            <span className="text-sm font-bold">{icon[0]}</span>
          )}
        </div>
        <h3 className="text-lg font-semibold text-[var(--color-text-primary)] mb-2">
          {title}
        </h3>
        <p className="text-sm text-[var(--color-text-secondary)] leading-relaxed">
          {description}
        </p>
      </div>
    </ScrollReveal>
  );
}
