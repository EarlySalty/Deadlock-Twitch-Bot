import {
  Percent,
  UserPlus,
  CreditCard,
  Infinity,
  LayoutDashboard,
  LogIn,
} from "lucide-react";
import type { ComponentType } from "react";
import { affiliateFeatures } from "@/data/affiliateFeatures";
import { SectionHeading } from "@/components/ui/SectionHeading";
import { ScrollReveal } from "@/components/ui/ScrollReveal";

interface AffiliateSectionProps {
  standalone?: boolean;
}

const iconMap: Record<string, ComponentType<{ size?: number; className?: string }>> = {
  Percent,
  UserPlus,
  CreditCard,
  Infinity,
  LayoutDashboard,
  LogIn,
};

export function AffiliateSection({ standalone = false }: AffiliateSectionProps) {
  return (
    <section id={standalone ? undefined : "affiliate"} className={standalone ? "pt-32 pb-24" : "py-24"}>
      <div className="max-w-7xl mx-auto px-6">
        <SectionHeading
          badge="Affiliate-Programm"
          title="Werden Sie Vertriebler"
          subtitle="Verdienen Sie 30% Provision auf jede Zahlung — dauerhaft und ohne Limit."
        />

        <div className="mt-16 grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 gap-6">
          {affiliateFeatures.map((feature, index) => {
            const IconComponent = iconMap[feature.icon];
            return (
              <ScrollReveal key={feature.title} delay={index * 0.1}>
                <div className="bg-[var(--color-card)] rounded-xl border border-[var(--color-border)] p-6 soft-elevate h-full">
                  <div className="w-12 h-12 rounded-lg gradient-accent flex items-center justify-center mb-4">
                    {IconComponent ? (
                      <IconComponent size={22} className="text-white" />
                    ) : (
                      <span className="text-white text-sm font-bold">
                        {feature.icon[0]}
                      </span>
                    )}
                  </div>
                  <h3 className="text-lg font-semibold text-[var(--color-text-primary)] mb-2">
                    {feature.title}
                  </h3>
                  <p className="text-sm text-[var(--color-text-secondary)] leading-relaxed">
                    {feature.description}
                  </p>
                </div>
              </ScrollReveal>
            );
          })}
        </div>

        <div className="mt-12 text-center">
          <ScrollReveal delay={0.6}>
            <a
              href="/twitch/affiliate/signup"
              className="gradient-accent rounded-xl px-7 py-3.5 font-semibold text-white inline-flex items-center gap-2 transition-all duration-200 hover:brightness-110 hover:shadow-[0_0_24px_4px_rgba(201, 168, 106, 0.3)]"
            >
              Jetzt Vertriebler werden
            </a>
          </ScrollReveal>
        </div>
      </div>
    </section>
  );
}
