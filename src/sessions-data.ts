import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type { AggregateQuery, AggregateResponse } from "./sessions-types";

/** Serial reads retain only the current page; live changes restart mutable paging. */
export function useSessionRead<T>(initial: AggregateQuery) {
  const selected = useRef(initial);
  const generation = useRef(0);
  const refresh = useRef<() => void>(() => {});
  const [data, setData] = useState<AggregateResponse<T> | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [connectionError, setConnectionError] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);
  function choose(query: AggregateQuery) {
    selected.current = query; generation.current++;
    setData(null); setError(null); refresh.current();
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
          : "Sessions could not be loaded. Retry to reconnect to local usage.");
      } finally {
        running = false;
        if (!disposed) {
          if (pending) { pending = false; void read(); }
          else setLoading(false);
        }
      }
    }
    function liveRefresh() {
      if (selected.current.kind === "sessionList") {
        const wasPaged = selected.current.query.offset !== 0;
        selected.current = { kind: "sessionList", query: { ...selected.current.query, offset: 0 } };
        generation.current++;
        if (wasPaged) setData(null);
      }
      void read();
    }
    refresh.current = () => { clearTimeout(timer); void read(); };
    void listen("usage-updated", () => { clearTimeout(timer); timer = setTimeout(liveRefresh, 150); })
      .then(unlisten => { if (disposed) unlisten(); else stop = unlisten; })
      .catch(() => { if (!disposed) setConnectionError("Live notifications unavailable; refreshing every minute."); });
    void read();
    const interval = setInterval(liveRefresh, 60_000);
    return () => { disposed = true; clearTimeout(timer); clearInterval(interval); stop?.(); };
  }, []);
  return { data, error, connectionError, loading, choose, retry: () => refresh.current() };
}
