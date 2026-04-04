import type { ComponentType } from "react";
import {
  Zap,
  BarChart2,
  Users,
  Shield,
  Sparkles,
  Rocket,
} from "lucide-react";

const iconMap: Record<string, ComponentType<{ size?: number; className?: string }>> = {
  Zap,
  BarChart2,
  Users,
  Shield,
  Sparkles,
  Rocket,
};

interface FeatureHighlightProps {
  icon: string;
  title: string;
  description: string;
}

export function FeatureHighlight({
  icon,
  title,
  description,
}: FeatureHighlightProps) {
  const IconComponent = iconMap[icon] || Rocket;

  return (
    <article className="panel-card rounded-xl p-5 border border-border bg-[rgba(7,21,29,0.46)]">
      <div className="flex items-start gap-4">
        <div className="w-10 h-10 rounded-lg gradient-accent flex items-center justify-center shrink-0">
          <IconComponent size={18} className="text-white" />
        </div>
        <div>
          <h4 className="text-base font-semibold text-text-primary">
            {title}
          </h4>
          <p className="mt-1 text-sm text-text-secondary leading-relaxed">
            {description}
          </p>
        </div>
      </div>
    </article>
  );
}
