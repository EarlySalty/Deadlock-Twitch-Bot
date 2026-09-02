import { useMemo } from "react";

export function NetworkAmbient() {
  const particles = useMemo(
    () =>
      Array.from({ length: 16 }, (_, i) => ({
        left: `${(i * 61 + 7) % 100}%`,
        top: `${(i * 37 + 11) % 100}%`,
        delay: `-${((i * 1.7) % 14).toFixed(1)}s`,
        duration: `${14 + (i % 6) * 3}s`,
        size: 3 + (i % 3),
        teal: i % 3 === 0,
      })),
    [],
  );

  return (
    <div className="v2-ambient-field" aria-hidden="true">
      <div
        className="v2-ambient v2-ambient-gold"
        style={{ top: "-10%", right: "-8%", width: "min(46rem, 74vw)", height: "min(46rem, 74vw)" }}
      />
      <div
        className="v2-ambient v2-ambient-teal"
        style={{
          top: "36%",
          left: "-12%",
          width: "min(40rem, 68vw)",
          height: "min(40rem, 68vw)",
          animationDelay: "-8s",
        }}
      />
      <div
        className="v2-ambient v2-ambient-gold"
        style={{
          bottom: "-8%",
          right: "-4%",
          width: "min(38rem, 62vw)",
          height: "min(38rem, 62vw)",
          animationDelay: "-5s",
          opacity: 0.5,
        }}
      />
      {particles.map((p, i) => (
        <span
          key={i}
          className={`v2-particle${p.teal ? " v2-particle-teal" : ""}`}
          style={{
            left: p.left,
            top: p.top,
            width: p.size,
            height: p.size,
            animationDelay: p.delay,
            animationDuration: p.duration,
          }}
        />
      ))}
    </div>
  );
}
