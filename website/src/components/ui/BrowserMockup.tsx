import type { ReactNode } from "react";

interface BrowserMockupProps {
  children: ReactNode;
  className?: string;
  url?: string;
}

export function BrowserMockup({
  children,
  className = "",
  url = "deutsche-deadlock-community.de/demo/twitch/demo",
}: BrowserMockupProps) {
  return (
    <div
      className={`rounded-xl border border-[var(--color-border)] bg-[var(--color-card)] overflow-hidden shadow-2xl ${className}`}
    >
      {/* Title bar */}
      <div className="flex items-center px-4 py-3 bg-[#0a1e2c] border-b border-[var(--color-border)]">
        {/* Traffic lights */}
        <div className="flex items-center gap-2 shrink-0">
          <span className="w-3 h-3 rounded-full bg-[#ff6b5e] block" />
          <span className="w-3 h-3 rounded-full bg-[var(--color-warning)] block" />
          <span className="w-3 h-3 rounded-full bg-[var(--color-success)] block" />
        </div>

        {/* URL bar */}
        <div className="rounded-lg bg-[var(--color-bg)] px-4 py-1.5 text-xs text-[var(--color-text-secondary)] flex-1 mx-4 truncate select-none">
          {url}
        </div>
      </div>

      {/* Content */}
      <div>{children}</div>
    </div>
  );
}
