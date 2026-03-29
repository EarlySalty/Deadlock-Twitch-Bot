import { useEffect, useMemo, useState } from "react";
import { motion, AnimatePresence } from "framer-motion";
import {
  DEMO_RAID_PATHS,
  type NetworkEdge,
  type NetworkNode,
} from "@/data/raidNetwork";

interface RaidNetworkProps {
  nodes: NetworkNode[];
  edges: NetworkEdge[];
  className?: string;
}

function toSvgX(x: number): number {
  return x * 10;
}

function toSvgY(y: number): number {
  return y * 5.5;
}

function edgeKey(edge: NetworkEdge): string {
  return `${edge.from}-${edge.to}`;
}

function buildBezier(
  x1: number,
  y1: number,
  x2: number,
  y2: number,
): string {
  const midX = (x1 + x2) / 2;
  const midY = (y1 + y2) / 2 - 30;
  return `M ${x1} ${y1} Q ${midX} ${midY} ${x2} ${y2}`;
}

export function RaidNetwork({ nodes, edges, className }: RaidNetworkProps) {
  const [activePathIndex, setActivePathIndex] = useState(0);

  useEffect(() => {
    const id = setInterval(() => {
      setActivePathIndex((prev) => (prev + 1) % DEMO_RAID_PATHS.length);
    }, 5000);
    return () => clearInterval(id);
  }, []);

  const activePath = DEMO_RAID_PATHS[activePathIndex];

  const activeNodeIds = useMemo(
    () => new Set(activePath),
    [activePath],
  );

  const activeEdgeKeys = useMemo(() => {
    const keys = new Set<string>();
    for (let i = 0; i < activePath.length - 1; i++) {
      keys.add(`${activePath[i]}-${activePath[i + 1]}`);
      keys.add(`${activePath[i + 1]}-${activePath[i]}`);
    }
    return keys;
  }, [activePath]);

  const nodeMap = useMemo(() => {
    const map = new Map<string, NetworkNode>();
    for (const node of nodes) {
      map.set(node.id, node);
    }
    return map;
  }, [nodes]);

  // Build particle path coordinates for the active raid path
  const particleCoords = useMemo(() => {
    return activePath
      .map((id) => nodeMap.get(id))
      .filter((n): n is NetworkNode => n !== undefined)
      .map((n) => ({ x: toSvgX(n.x), y: toSvgY(n.y) }));
  }, [activePath, nodeMap]);

  return (
    <svg
      viewBox="0 0 1000 550"
      className={`w-full h-auto ${className ?? ""}`}
      role="img"
      aria-label="Raid-Netzwerk-Visualisierung"
    >
      <defs>
        <filter id="glow" x="-50%" y="-50%" width="200%" height="200%">
          <feGaussianBlur in="SourceGraphic" stdDeviation="4" result="blur" />
          <feMerge>
            <feMergeNode in="blur" />
            <feMergeNode in="SourceGraphic" />
          </feMerge>
        </filter>
        <linearGradient id="activeEdge" x1="0%" y1="0%" x2="100%" y2="0%">
          <stop offset="0%" stopColor="#ff7a18" />
          <stop offset="100%" stopColor="#10b7ad" />
        </linearGradient>
      </defs>

      {/* Edges */}
      {edges.map((edge) => {
        const fromNode = nodeMap.get(edge.from);
        const toNode = nodeMap.get(edge.to);
        if (!fromNode || !toNode) return null;

        const x1 = toSvgX(fromNode.x);
        const y1 = toSvgY(fromNode.y);
        const x2 = toSvgX(toNode.x);
        const y2 = toSvgY(toNode.y);
        const d = buildBezier(x1, y1, x2, y2);
        const isActive = activeEdgeKeys.has(edgeKey(edge));

        return (
          <motion.path
            key={edgeKey(edge)}
            d={d}
            fill="none"
            stroke={isActive ? "#ff7a18" : "var(--color-border)"}
            strokeOpacity={isActive ? 1 : 0.25}
            strokeWidth={isActive ? 2.5 : 1.5}
            strokeDasharray={isActive ? "8 4" : "none"}
            animate={
              isActive
                ? { strokeDashoffset: [0, -24] }
                : { strokeDashoffset: 0 }
            }
            transition={
              isActive
                ? { duration: 1.2, repeat: Infinity, ease: "linear" }
                : { duration: 0.3 }
            }
          />
        );
      })}

      {/* Nodes */}
      {nodes.map((node) => {
        const cx = toSvgX(node.x);
        const cy = toSvgY(node.y);
        const isActive = activeNodeIds.has(node.id);
        const isCenter = node.id === "earlysalty";

        return (
          <motion.g
            key={node.id}
            animate={{
              scale: isActive ? 1.1 : 1,
            }}
            transition={{ duration: 0.6 }}
            style={{ originX: `${cx}px`, originY: `${cy}px` }}
          >
            <title>{node.label}</title>
            <motion.circle
              cx={cx}
              cy={cy}
              r={isCenter ? 26 : 22}
              fill="var(--color-card)"
              stroke={
                isActive
                  ? "#ff7a18"
                  : isCenter
                    ? "#10b7ad"
                    : "#1a3a4f"
              }
              strokeWidth={2}
              filter={isActive ? "url(#glow)" : undefined}
              animate={{
                stroke: isActive
                  ? "#ff7a18"
                  : isCenter
                    ? "#10b7ad"
                    : "#1a3a4f",
              }}
              transition={{ duration: 0.6 }}
            />
            <motion.text
              x={cx}
              y={cy}
              fill="var(--color-text-primary)"
              fontSize={isCenter ? 16 : 14}
              fontWeight={600}
              textAnchor="middle"
              dominantBaseline="central"
              style={{ pointerEvents: "none", userSelect: "none" }}
            >
              {node.label.charAt(0)}
            </motion.text>
          </motion.g>
        );
      })}

      {/* Wandernder Partikel */}
      <AnimatePresence mode="wait">
        {particleCoords.length >= 2 && (
          <motion.circle
            key={`particle-${activePathIndex}`}
            r={5}
            fill="#ff7a18"
            filter="url(#glow)"
            initial={{
              cx: particleCoords[0].x,
              cy: particleCoords[0].y,
              opacity: 0,
            }}
            animate={{
              cx: particleCoords.map((c) => c.x),
              cy: particleCoords.map((c) => c.y),
              opacity: [0, 1, 1, 0],
            }}
            exit={{ opacity: 0 }}
            transition={{
              duration: 2 * (particleCoords.length - 1),
              ease: "easeInOut",
              times: particleCoords.length === 3
                ? [0, 0.05, 0.95, 1]
                : [0, 0.1, 0.9, 1],
            }}
          />
        )}
      </AnimatePresence>
    </svg>
  );
}
