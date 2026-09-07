import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type { EstimatedCost, GlobalSummary, WeeklyEstimate, WeeklySummary } from "./dashboard-types";

export type WeeklyModels = {
  cycleKey: string;
  estimate: WeeklyEstimate;
  items: { id: string; model: string | null; tokens: GlobalSummary["tokens"]; estimatedCost: EstimatedCost }[];
  nextCursor: string | null;
};
type Read = { kind: "history"; before: string | null } | { kind: "models"; cycleKey: string; after: string | null };
const first: Read = { kind: "history", before: null };
const limit = 25;

/** Serialize both commands; selection and live invalidations suppress obsolete results. */
export function useWeeklyHistory() {
  const selected = useRef<Read>(first), generation = useRef(0);
  const refresh = useRef<() => void>(() => {});
  const [history, setHistory] = useState<WeeklySummary | null>(null);
  const [models, setModels] = useState<WeeklyModels | null>(null);
  const [cycleKey, setCycleKey] = useState<string | null>(null);
  const [missing, setMissing] = useState(false);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [connectionError, setConnectionError] = useState<string | null>(null);
  function choose(query: Read) {
    selected.current = query; generation.current++;
    setModels(null); setMissing(false); setError(null); setLoading(true);
    setCycleKey(query.kind === "models" ? query.cycleKey : null);
    if (query.kind === "history") setHistory(null);
    refresh.current();
  }
  useEffect(() => {
    let disposed = false, running = false, pending = false;
    let timer: ReturnType<typeof setTimeout> | undefined, stop: (() => void) | undefined;
    async function read() {
      if (disposed) return;
      if (running) { pending = true; return; }
      running = true; setLoading(true);
      const version = generation.current, query = selected.current;
      try {
        if (query.kind === "history") {
          const result = await invoke<WeeklySummary>("usage_weekly", { query: { before: query.before, limit } });
          if (!disposed && version === generation.current) {
            setHistory(result); setError(null);
            const cycle = result.history[0];
            if (cycle) { selected.current = { kind: "models", cycleKey: cycle.key, after: null }; setCycleKey(cycle.key); pending = true; }
          }
        } else {
          const result = await invoke<WeeklyModels | null>("usage_weekly_models", { query: { cycleKey: query.cycleKey, page: { after: query.after, limit } } });
          if (!disposed && version === generation.current) { setModels(result); setMissing(result === null); setError(null); }
        }
      } catch {
        if (!disposed && version === generation.current) setError(`${query.kind === "history" ? "Weekly history" : "Cycle models"} could not be loaded. Retry to reconnect to local usage.`);
      } finally {
        running = false;
        if (!disposed) {
          if (pending) { pending = false; void read(); }
          else setLoading(false);
        }
      }
    }
    function invalidate() {
      selected.current = first; generation.current++;
      setHistory(null); setModels(null); setCycleKey(null); setMissing(false); setError(null); setLoading(true);
    }
    refresh.current = () => { clearTimeout(timer); void read(); };
    void listen("usage-updated", () => { invalidate(); clearTimeout(timer); timer = setTimeout(() => void read(), 150); })
      .then(unlisten => { if (disposed) unlisten(); else stop = unlisten; })
      .catch(() => { if (!disposed) setConnectionError("Live notifications unavailable; refreshing every minute."); });
    void read();
    const interval = setInterval(() => { invalidate(); void read(); }, 60_000);
    return () => { disposed = true; clearTimeout(timer); clearInterval(interval); stop?.(); };
  }, []);
  return { history, models, cycleKey, missing, loading, error, connectionError,
    retry: () => refresh.current(), loadHistory: (before: string | null) => choose({ kind: "history", before }),
    loadModels: (key: string, after: string | null) => choose({ kind: "models", cycleKey: key, after }) };
}
