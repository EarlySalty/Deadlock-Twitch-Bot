import type { ReactNode, MouseEventHandler } from "react";

interface GlowButtonProps {
  children: ReactNode;
  href: string;
  variant: "primary" | "ghost";
  className?: string;
  onClick?: MouseEventHandler<HTMLAnchorElement>;
}

export function GlowButton({
  children,
  href,
  variant,
  className = "",
  onClick,
}: GlowButtonProps) {
  const baseClasses =
    "inline-flex items-center justify-center rounded-xl px-7 py-3 font-semibold transition-all duration-200 cursor-pointer no-underline";

  const variantClasses =
    variant === "primary"
      ? "gradient-accent text-white shadow-[0_0_0_0_rgba(201, 168, 106, 0)] hover:shadow-[0_0_24px_4px_rgba(201, 168, 106, 0.35)] hover:brightness-110"
      : "bg-transparent border border-[var(--color-border)] text-[var(--color-text-primary)] hover:border-[var(--color-border-hover)] hover:bg-white/5";

  return (
    <a
      href={href}
      onClick={onClick}
      className={`${baseClasses} ${variantClasses} ${className}`}
    >
      {children}
    </a>
  );
}
