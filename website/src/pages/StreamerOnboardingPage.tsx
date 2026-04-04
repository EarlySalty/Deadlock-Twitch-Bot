import { useState } from "react";
import { ArrowRight, CheckCircle2, LifeBuoy } from "lucide-react";
import { motion } from "framer-motion";
import { GlowOrb } from "@/components/effects/GlowOrb";
import { PublicInfoFooter } from "@/components/layout/PublicInfoFooter";
import { PublicInfoHeader } from "@/components/layout/PublicInfoHeader";
import { OnboardingProgress } from "@/components/onboarding/OnboardingProgress";
import { StepCard } from "@/components/onboarding/StepCard";
import {
  DISCORD_INVITE_URL,
  TWITCH_FAQ_URL,
  buildTwitchBotAuthUrl,
} from "@/data/externalLinks";
import { ONBOARDING_VISUAL_STEPS } from "@/data/twitchKnowledgeBase";

const NAV_LINKS = [
  { label: "Features", href: "#features" },
  { label: "FAQ", href: "#faq-hinweis" },
];

// Network visualization SVG for the first step
function NetworkVisualization() {
  return (
    <svg viewBox="0 0 200 120" className="w-full h-full">
      {/* Central node */}
      <motion.circle
        cx="100"
        cy="60"
        r="16"
        fill="url(#gradient)"
        initial={{ scale: 0.8, opacity: 0 }}
        animate={{ scale: 1, opacity: 1 }}
        transition={{ duration: 0.5 }}
      />
      <motion.text
        x="100"
        y="65"
        textAnchor="middle"
        fill="white"
        fontSize="10"
        fontWeight="bold"
        initial={{ opacity: 0 }}
        animate={{ opacity: 1 }}
        transition={{ delay: 0.3 }}
      >
        DU
      </motion.text>

      {/* Surrounding nodes */}
      {[0, 60, 120, 180, 240, 300].map((angle, i) => {
        const rad = (angle * Math.PI) / 180;
        const x = 100 + Math.cos(rad) * 55;
        const y = 60 + Math.sin(rad) * 40;
        return (
          <motion.circle
            key={angle}
            cx={x}
            cy={y}
            r="10"
            fill="rgba(0,212,170,0.3)"
            stroke="rgba(0,212,170,0.8)"
            strokeWidth="1.5"
            initial={{ scale: 0, opacity: 0 }}
            animate={{ scale: 1, opacity: 1 }}
            transition={{ delay: 0.2 + i * 0.1, duration: 0.4 }}
          />
        );
      })}

      {/* Connection lines */}
      {[0, 60, 120, 180, 240, 300].map((angle, i) => {
        const rad = (angle * Math.PI) / 180;
        const x = 100 + Math.cos(rad) * 55;
        const y = 60 + Math.sin(rad) * 40;
        return (
          <motion.line
            key={`line-${angle}`}
            x1="100"
            y1="60"
            x2={x}
            y2={y}
            stroke="rgba(0,212,170,0.2)"
            strokeWidth="1"
            strokeDasharray="4 2"
            initial={{ pathLength: 0, opacity: 0 }}
            animate={{ pathLength: 1, opacity: 1 }}
            transition={{ delay: 0.4 + i * 0.1, duration: 0.3 }}
          />
        );
      })}

      <defs>
        <linearGradient id="gradient" x1="0%" y1="0%" x2="100%" y2="100%">
          <stop offset="0%" stopColor="#00d4aa" />
          <stop offset="100%" stopColor="#00a8ff" />
        </linearGradient>
      </defs>
    </svg>
  );
}

// Auto-Raid diagram SVG for the second step
function RaidDiagram() {
  return (
    <svg viewBox="0 0 220 100" className="w-full h-full">
      {/* Streamer going offline */}
      <motion.g
        initial={{ opacity: 0, x: -20 }}
        animate={{ opacity: 1, x: 0 }}
        transition={{ duration: 0.5 }}
      >
        <rect x="10" y="30" width="50" height="40" rx="6" fill="rgba(255,100,100,0.2)" stroke="rgba(255,100,100,0.6)" />
        <text x="35" y="55" textAnchor="middle" fill="rgba(255,150,150,0.9)" fontSize="9">OFFLINE</text>
      </motion.g>

      {/* Arrow */}
      <motion.path
        d="M 65 50 L 90 50"
        stroke="rgba(0,212,170,0.8)"
        strokeWidth="2"
        fill="none"
        markerEnd="url(#arrowhead)"
        initial={{ pathLength: 0 }}
        animate={{ pathLength: 1 }}
        transition={{ delay: 0.5, duration: 0.3 }}
      />

      {/* Bot */}
      <motion.g
        initial={{ opacity: 0, scale: 0.8 }}
        animate={{ opacity: 1, scale: 1 }}
        transition={{ delay: 0.3, duration: 0.4 }}
      >
        <rect x="95" y="30" width="30" height="40" rx="6" fill="rgba(0,212,170,0.2)" stroke="rgba(0,212,170,0.8)" />
        <text x="110" y="55" textAnchor="middle" fill="rgba(0,212,170,0.9)" fontSize="8">BOT</text>
      </motion.g>

      {/* Arrow */}
      <motion.path
        d="M 130 50 L 155 50"
        stroke="rgba(0,212,170,0.8)"
        strokeWidth="2"
        fill="none"
        markerEnd="url(#arrowhead)"
        initial={{ pathLength: 0 }}
        animate={{ pathLength: 1 }}
        transition={{ delay: 0.7, duration: 0.3 }}
      />

      {/* Partner going live */}
      <motion.g
        initial={{ opacity: 0, x: 20 }}
        animate={{ opacity: 1, x: 0 }}
        transition={{ delay: 0.6, duration: 0.5 }}
      >
        <rect x="160" y="30" width="50" height="40" rx="6" fill="rgba(0,212,170,0.2)" stroke="rgba(0,212,170,0.8)" />
        <text x="185" y="55" textAnchor="middle" fill="rgba(0,212,170,0.9)" fontSize="9">LIVE</text>
      </motion.g>

      <defs>
        <marker id="arrowhead" markerWidth="6" markerHeight="6" refX="5" refY="3" orient="auto">
          <path d="M0,0 L0,6 L6,3 z" fill="rgba(0,212,170,0.8)" />
        </marker>
      </defs>
    </svg>
  );
}

// Checklist diagram for the fourth step
function ChecklistVisual() {
  const items = [
    "Kanal verbunden",
    "Auto-Raid aktiv",
    "Dashboard offen",
  ];

  return (
    <div className="flex flex-col gap-3 w-full px-4">
      {items.map((item, i) => (
        <motion.div
          key={item}
          className="flex items-center gap-3"
          initial={{ opacity: 0, x: -10 }}
          animate={{ opacity: 1, x: 0 }}
          transition={{ delay: i * 0.15 }}
        >
          <div className="w-5 h-5 rounded-full bg-accent/30 border border-accent flex items-center justify-center">
            <CheckCircle2 size={14} className="text-accent" />
          </div>
          <span className="text-sm text-text-primary">{item}</span>
        </motion.div>
      ))}
    </div>
  );
}

export function StreamerOnboardingPage() {
  const [currentStep, setCurrentStep] = useState(0);
  const onboardingAuthUrl = buildTwitchBotAuthUrl();

  const stepLabels = ONBOARDING_VISUAL_STEPS.map((s) => s.eyebrow.split(". ")[1]);

  // Visual content for each step
  const visualContent = [
    <NetworkVisualization key="network" />,
    <RaidDiagram key="raid" />,
    null, // Uses screenshot
    <ChecklistVisual key="checklist" />,
  ];

  return (
    <>
      <GlowOrb />
      <PublicInfoHeader
        navLinks={NAV_LINKS}
        primaryAction={{
          label: "Bot für deinen Kanal aktivieren",
          href: onboardingAuthUrl,
        }}
        secondaryAction={{ label: "Zur FAQ", href: TWITCH_FAQ_URL, variant: "ghost" }}
      />

      <main className="relative z-10">
        {/* Hero Section */}
        <section className="px-6 pb-12 pt-32">
          <div className="mx-auto max-w-3xl text-center">
            <motion.div
              initial={{ opacity: 0, y: -12 }}
              animate={{ opacity: 1, y: 0 }}
              transition={{ duration: 0.5 }}
              className="inline-flex rounded-full border border-border bg-[rgba(16,38,53,0.76)] px-4 py-1.5 text-sm text-accent"
            >
              Streamer Onboarding auf twitch.earlysalty.com
            </motion.div>

            <motion.h1
              initial={{ opacity: 0, y: 20 }}
              animate={{ opacity: 1, y: 0 }}
              transition={{ duration: 0.6, delay: 0.1 }}
              className="mt-6 text-4xl font-bold leading-tight text-text-primary md:text-5xl lg:text-6xl"
            >
              Werde Teil des
              <br />
              <span className="bg-gradient-to-r from-primary to-accent bg-clip-text text-transparent">
                Deadlock-Partnernetzwerks
              </span>
            </motion.h1>

            <motion.p
              initial={{ opacity: 0, y: 20 }}
              animate={{ opacity: 1, y: 0 }}
              transition={{ duration: 0.6, delay: 0.2 }}
              className="mt-6 text-lg leading-relaxed text-text-secondary md:text-xl"
            >
              Aktiviere den Bot und vernetze dich mit 30+ Deadlock-Streamern.
              Auto-Raids, Dashboard und mehr.
            </motion.p>

            <motion.div
              initial={{ opacity: 0, y: 20 }}
              animate={{ opacity: 1, y: 0 }}
              transition={{ duration: 0.6, delay: 0.3 }}
              className="mt-10"
            >
              <a
                href={onboardingAuthUrl}
                className="gradient-accent inline-flex items-center gap-2 rounded-xl px-8 py-4 font-semibold text-white no-underline transition-all duration-200 hover:brightness-110"
              >
                Jetzt Kanal verbinden
                <ArrowRight size={18} />
              </a>
            </motion.div>
          </div>
        </section>

        {/* Progress Indicator */}
        <section className="px-6 py-8">
          <div className="mx-auto max-w-4xl">
            <OnboardingProgress
              steps={stepLabels}
              currentStep={currentStep}
              onStepClick={setCurrentStep}
            />
          </div>
        </section>

        {/* Step Cards */}
        <section id="features" className="px-6 py-12">
          <div className="mx-auto grid max-w-7xl gap-6 lg:grid-cols-2 xl:grid-cols-4">
            {ONBOARDING_VISUAL_STEPS.map((step, index) => (
              <motion.div
                key={step.eyebrow}
                initial={{ opacity: 0, y: 20 }}
                animate={{ opacity: 1, y: 0 }}
                transition={{ duration: 0.4, delay: index * 0.1 }}
              >
                <StepCard
                  eyebrow={step.eyebrow}
                  title={step.title}
                  description={step.description}
                  visualType={step.visualType}
                  visualSrc={step.visualSrc}
                  visualContent={visualContent[index]}
                  ctaLabel={step.ctaLabel}
                  ctaHref={step.ctaHref}
                  isActive={currentStep === index}
                />
              </motion.div>
            ))}
          </div>
        </section>

        {/* FAQ Section */}
        <section id="faq-hinweis" className="px-6 py-20">
          <div className="mx-auto max-w-7xl">
            <div className="panel-card overflow-hidden rounded-[2rem] p-8 md:p-10">
              <div className="grid gap-8 lg:grid-cols-[1.1fr_0.9fr] lg:items-center">
                <div>
                  <p className="text-sm uppercase tracking-[0.16em] text-primary">
                    FAQ und Support
                  </p>
                  <h2 className="mt-4 text-4xl font-bold text-text-primary md:text-5xl">
                    Alle Details findest du in der FAQ.
                  </h2>
                  <p className="mt-5 max-w-2xl text-base leading-relaxed text-text-secondary">
                    Auto-Raid, Dashboard, Discord und weitere Funktionen -
                    alles erklärt.
                  </p>
                </div>

                <div className="grid gap-4">
                  <a
                    href={TWITCH_FAQ_URL}
                    className="gradient-accent inline-flex items-center justify-between gap-4 rounded-2xl px-6 py-5 font-semibold text-white no-underline transition-all duration-200 hover:brightness-110"
                  >
                    <span>Komplette FAQ</span>
                    <ArrowRight size={18} />
                  </a>
                  <a
                    href={DISCORD_INVITE_URL}
                    className="inline-flex items-center justify-between gap-4 rounded-2xl border border-border px-6 py-5 font-semibold text-text-primary no-underline transition-colors duration-200 hover:border-border-hover hover:bg-white/5"
                  >
                    <span>Discord und Support</span>
                    <LifeBuoy size={18} />
                  </a>
                </div>
              </div>
            </div>
          </div>
        </section>
      </main>

      <PublicInfoFooter />
    </>
  );
}
