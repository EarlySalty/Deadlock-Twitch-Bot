import type { ReactNode } from "react";

interface GradientTextProps {
  children: ReactNode;
  className?: string;
}

export function GradientText({ children, className = "" }: GradientTextProps) {
  return (
    <span
      className={`bg-clip-text text-transparent ${className}`}
      style={{
        backgroundImage: "var(--theme-headline-gradient, linear-gradient(135deg, #06B6D4, #A855F7))",
      }}
    >
      {children}
    </span>
  );
}
