import { useCallback, useEffect, useRef, useState } from "react";
import {
  DEMO_EDGES,
  DEMO_NODES,
  type NetworkEdge,
  type NetworkNode,
} from "@/data/raidNetwork";

const API_URL =
  "https://twitch.earlysalty.com/twitch/api/v2/public/network";
const POLL_INTERVAL = 60_000;

interface ApiNode {
  login: string;
  is_live?: boolean;
}

interface NetworkData {
  nodes: NetworkNode[];
  edges: NetworkEdge[];
  isLoading: boolean;
}

export function useNetworkData(): NetworkData {
  const [nodes, setNodes] = useState<NetworkNode[]>(DEMO_NODES);
  const [edges, setEdges] = useState<NetworkEdge[]>(DEMO_EDGES);
  const [isLoading, setIsLoading] = useState(true);
  const timerRef = useRef<ReturnType<typeof setInterval> | null>(null);

  const fetchNetwork = useCallback(async () => {
    try {
      const res = await fetch(API_URL);
      if (!res.ok) throw new Error(`HTTP ${res.status}`);

      const data: { nodes?: ApiNode[]; edges?: NetworkEdge[] } =
        await res.json();

      if (data.nodes && Array.isArray(data.nodes)) {
        const merged = DEMO_NODES.map((demo) => {
          const api = data.nodes!.find(
            (n) => n.login.toLowerCase() === demo.id.toLowerCase(),
          );
          return api ? { ...demo, label: api.login, isLive: api.is_live } : demo;
        });
        setNodes(merged);
      }

      if (data.edges && Array.isArray(data.edges)) {
        setEdges(data.edges);
      }
    } catch {
      // Fallback: keep demo data
      setNodes(DEMO_NODES);
      setEdges(DEMO_EDGES);
    } finally {
      setIsLoading(false);
    }
  }, []);

  useEffect(() => {
    fetchNetwork();
    timerRef.current = setInterval(fetchNetwork, POLL_INTERVAL);
    return () => {
      if (timerRef.current) clearInterval(timerRef.current);
    };
  }, [fetchNetwork]);

  return { nodes, edges, isLoading };
}
