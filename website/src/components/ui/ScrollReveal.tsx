import { motion, useInView, useReducedMotion } from "framer-motion";
import { useEffect, useRef, useState, type ReactNode } from "react";

interface ScrollRevealProps {
  children: ReactNode;
  className?: string;
  delay?: number;
  direction?: "up" | "down" | "left" | "right";
}

export function ScrollReveal({
  children,
  className,
  delay = 0,
  direction = "up",
}: ScrollRevealProps) {
  const isHorizontal = direction === "left" || direction === "right";
  const isNegative = direction === "down" || direction === "right";
  const offset = isNegative ? -30 : 30;

  const ref = useRef<HTMLDivElement>(null);
  const reduce = useReducedMotion();
  const inView = useInView(ref, { once: true, margin: "-80px" });
  const [hidden, setHidden] = useState(false);

  useEffect(() => {
    if (reduce) return;
    const el = ref.current;
    if (!el) return;
    if (el.getBoundingClientRect().top > window.innerHeight) {
      setHidden(true);
    }
  }, [reduce]);

  const versteckt = hidden && !inView;

  const target = isHorizontal
    ? { opacity: versteckt ? 0 : 1, x: versteckt ? offset : 0 }
    : { opacity: versteckt ? 0 : 1, y: versteckt ? offset : 0 };

  return (
    <motion.div
      ref={ref}
      className={className}
      initial={false}
      animate={target}
      transition={
        versteckt
          ? { duration: 0 }
          : { duration: 0.6, delay, ease: "easeOut" }
      }
    >
      {children}
    </motion.div>
  );
}
