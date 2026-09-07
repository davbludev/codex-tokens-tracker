import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type { AggregateQuery, AggregateResponse } from "./sessions-types";
import type { GlobalSummary, TokenCategory } from "./dashboard-types";

export const tokenCategories: [keyof GlobalSummary["tokens"], string][] = [
  ["totalTokens", "Total"], ["inputTokens", "Input"], ["cachedInputTokens", "Cached input"],
  ["cacheWriteTokens", "Cache writes"], ["reasoningTokens", "Reasoning"], ["outputTokens", "Output"],
];
export function categoryText(value: TokenCategory): string {
  return value.knownTokens === null ? "Unavailable" : `${BigInt(value.knownTokens).toLocaleString()}${value.complete ? "" : " (incomplete)"}`;
}
/** Axis labels only; readouts retain exact decimal strings. */
export function plotTickText(value: number): string {
  return value.toLocaleString(undefined, { notation: "compact", maximumFractionDigits: 2 });
}
export function coverageText(summary: GlobalSummary): string {
  const c = summary.coverage;
  return `${c.incompleteSessions} incomplete sessions; ${c.unavailableSessions} unavailable sessions. ${[
    c.unresolvedUsage && "Unresolved usage", c.unknownModel && "Unknown model attribution",
    c.unattributedProject && "Project attribution unavailable", c.sourceDiagnostics && "Source diagnostics present",
  ].filter(Boolean).join(" · ")}`;
}

/** Serial reads retain only the current page; live changes restart mutable paging. */
export function useSessionRead<T>(initial: AggregateQuery, subject = "Sessions") {
  const selected = useRef(initial);
  const generation = useRef(0);
  const refresh = useRef<() => void>(() => {});
  const [data, setData] = useState<AggregateResponse<T> | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [connectionError, setConnectionError] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);
  function choose(query: AggregateQuery) {
    selected.current = query; generation.current++;
    setData(null); setError(null); setLoading(true); refresh.current();
  }
  useEffect(() => {
    let disposed = false, running = false, pending = false;
    let timer: ReturnType<typeof setTimeout> | undefined;
    let stop: (() => void) | undefined;
    async function read() {
      if (disposed) return;
      if (running) { pending = true; return; }
      running = true; setLoading(true);
      const version = generation.current;
      try {
        const result = await invoke<AggregateResponse<T>>("usage_aggregates", { query: selected.current });
        if (!disposed && version === generation.current) { setData(result); setError(null); }
      } catch (failure) {
        if (!disposed && version === generation.current) setError(failure === "invalidQuery"
          ? "These filters are invalid. Check the date range and shorten text filters."
          : failure === "hierarchyPending" ? "Hierarchy pending — relationships are being reconciled."
          : `${subject} could not be loaded. Retry to reconnect to local usage.`);
      } finally {
        running = false;
        if (!disposed) {
          if (pending) { pending = false; void read(); }
          else setLoading(false);
        }
      }
    }
    function invalidateLive() {
      generation.current++;
      setLoading(true);
      if (selected.current.kind === "sessionList") {
        const wasPaged = selected.current.query.offset !== 0;
        selected.current = { kind: "sessionList", query: { ...selected.current.query, offset: 0 } };
        if (wasPaged) setData(null);
      } else if ("page" in selected.current) {
        const wasPaged = selected.current.page.after !== null;
        selected.current = { ...selected.current, page: { ...selected.current.page, after: null } };
        if (wasPaged) setData(null);
      }
    }
    function liveRefresh() {
      invalidateLive();
      void read();
    }
    refresh.current = () => { clearTimeout(timer); void read(); };
    void listen("usage-updated", () => { invalidateLive(); clearTimeout(timer); timer = setTimeout(() => void read(), 150); })
      .then(unlisten => { if (disposed) unlisten(); else stop = unlisten; })
      .catch(() => { if (!disposed) setConnectionError("Live notifications unavailable; refreshing every minute."); });
    void read();
    const interval = setInterval(liveRefresh, 60_000);
    return () => { disposed = true; clearTimeout(timer); clearInterval(interval); stop?.(); };
  }, []);
  return { data, error, connectionError, loading, choose, retry: () => refresh.current() };
}
