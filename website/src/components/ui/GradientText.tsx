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
        backgroundImage: "var(--theme-headline-gradient, linear-gradient(120deg, #efd49d 10%, #c8a86b 55%, #55978f))",
      }}
    >
      {children}
    </span>
  );
}
