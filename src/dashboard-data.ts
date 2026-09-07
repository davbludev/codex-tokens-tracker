import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type { DashboardRange, DashboardResponse, EstimatedCost, ObservationTime, UnavailableReason } from "./dashboard-types";

export const ranges: [DashboardRange, string][] = [["currentCycle", "Current cycle"], ["last24Hours", "24 hours"], ["last7Days", "7 days"], ["last30Days", "30 days"], ["all", "All"]];
export const unavailable: Record<UnavailableReason, string> = {
  insufficientObservations: "Insufficient comparable observations", ambiguousObservation: "Ambiguous observation",
  belowOnePercentagePoint: "Less than 1 percentage point observed", unpricedUsage: "Unpriced usage — estimate unavailable",
};
export function exactTime(time: ObservationTime): string {
  return new Date(time.seconds * 1000).toISOString().replace(/\.\d{3}Z$/, `.${String(time.nanos).padStart(9, "0")}Z`);
}
export function usd(value: string | null | undefined): string { return value == null ? "Unavailable" : `$${value}`; }
export function money(value: string | null | undefined): string {
  if (value == null) return "Unavailable";
  const padded = value.padStart(13, "0");
  const fraction = padded.slice(-12).replace(/0+$/, "");
  return `$${padded.slice(0, -12)}${fraction ? `.${fraction}` : ""}`;
}
export function costText(cost: Pick<EstimatedCost, "knownSubtotal" | "complete"> | null): string {
  return `${money(cost?.knownSubtotal)}${cost && !cost.complete ? " (incomplete known subtotal)" : ""}`;
}

/** One in-flight read, with at most one trailing refresh. Old ranges never publish. */
export function useDashboard() {
  const [range, setRange] = useState<DashboardRange>("currentCycle");
  const [data, setData] = useState<DashboardResponse | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [connectionError, setConnectionError] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);
  const [now, setNow] = useState(Date.now());
  const request = useRef<() => void>(() => {});
  const selection = useRef(range);
  const generation = useRef(0);
  const chooseRange = (next: DashboardRange) => {
    generation.current++;
    selection.current = next;
    setRange(next);
    setData(null);
    request.current();
  };
  useEffect(() => {
    let disposed = false, running = false, pending = false;
    let timer: ReturnType<typeof setTimeout> | undefined;
    let stop: (() => void) | undefined;
    async function refresh() {
      if (disposed) return;
      if (running) { pending = true; return; }
      running = true;
      const requestedRange = selection.current;
      const requestedGeneration = generation.current;
      setLoading(true);
      try {
        const result = await invoke<DashboardResponse>("usage_dashboard", { query: { range: requestedRange } });
        if (!disposed && requestedGeneration === generation.current) { setData(result); setError(null); setNow(Date.now()); }
      } catch {
        if (!disposed && requestedGeneration === generation.current) setError("Dashboard could not be loaded. Retry to reconnect to local usage.");
      } finally {
        running = false;
        if (!disposed) {
          if (pending) { pending = false; void refresh(); }
          else setLoading(false);
        }
      }
    }
    request.current = () => { clearTimeout(timer); void refresh(); };
    void listen("usage-updated", () => {
      clearTimeout(timer);
      timer = setTimeout(() => void refresh(), 150);
    }).then(unlisten => { if (disposed) unlisten(); else stop = unlisten; })
      .catch(() => { if (!disposed) setConnectionError("Live notifications unavailable; refreshing every minute."); });
    void refresh();
    // Refresh the native rolling interval as well as the displayed observation age.
    const interval = setInterval(() => { setNow(Date.now()); void refresh(); }, 60_000);
    return () => { disposed = true; clearTimeout(timer); clearInterval(interval); stop?.(); };
  }, []);
  return { range, chooseRange, data, error, connectionError, loading, now, retry: () => request.current() };
}
