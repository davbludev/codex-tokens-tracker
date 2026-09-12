import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type { BreakdownMetric, DashboardRange, DashboardResponse, EstimatedCost, ObservationTime, UnavailableReason, RangeSelection, TimeWindow } from "./dashboard-types";

export const ranges: [Exclude<DashboardRange, "custom" | "trailing">, string][] = [["last24Hours", "24 hours"], ["last7Days", "7 days"], ["last30Days", "30 days"], ["all", "All"]];
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

const compact = new Intl.NumberFormat(undefined, { notation: "compact", maximumFractionDigits: 1 });
export function compactTokens(value: string | null | undefined): string { return value == null ? "Unavailable" : compact.format(BigInt(value)); }
export function exactTokens(value: string | null | undefined): string { return value == null ? "Unavailable" : BigInt(value).toLocaleString(); }
export function compactCost(value: string | null | undefined): string {
  if (value == null) return "Unpriced";
  const amount = BigInt(value);
  if (amount > 0n && amount < 10_000_000_000n) return "<$0.01";
  const cents = (amount + 5_000_000_000n) / 10_000_000_000n;
  if (cents >= 10_000_000n) return `$${compact.format(cents / 100n)}`;
  return `$${(cents / 100n).toLocaleString()}.${(cents % 100n).toString().padStart(2, "0")}`;
}
export function localTime(time: ObservationTime): string { return new Date(time.seconds * 1000).toLocaleString(undefined, { month: "short", day: "numeric", hour: "numeric", minute: "2-digit" }); }

/** One in-flight read, with at most one trailing refresh. Old ranges never publish. */
export function useDashboard() {
  const [period, setPeriod] = useState<RangeSelection>({ range: "last7Days" });
  const [historyDepth, setHistoryDepth] = useState(0);
  const history = useRef<RangeSelection[]>([]);
  const baseline = useRef<RangeSelection>(period);
  const [breakdownMetric, setBreakdownMetric] = useState<BreakdownMetric>("tokens");
  const [data, setData] = useState<DashboardResponse | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [connectionError, setConnectionError] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);
  const [now, setNow] = useState(Date.now());
  const request = useRef<() => void>(() => {});
  const selection = useRef(period);
  const metricSelection = useRef(breakdownMetric);
  const generation = useRef(0);
  const applyPeriod = (next: RangeSelection) => {
    generation.current++;
    selection.current = next;
    setPeriod(next);
    setData(null);
    request.current();
  };
  const choosePeriod = (next: RangeSelection) => {
    history.current = []; setHistoryDepth(0); baseline.current = next; applyPeriod(next);
  };
  const chooseRange = (range: Exclude<DashboardRange, "custom" | "trailing">) => choosePeriod({ range });
  const selectWindow = (window: TimeWindow) => {
    history.current.push(selection.current); setHistoryDepth(history.current.length);
    applyPeriod({ range: "custom", ...window });
  };
  const goBack = () => { const previous = history.current.pop(); if (previous) { setHistoryDepth(history.current.length); applyPeriod(previous); } };
  const resetZoom = () => { history.current = []; setHistoryDepth(0); applyPeriod(baseline.current); };
  const chooseBreakdownMetric = (next: BreakdownMetric) => {
    generation.current++;
    metricSelection.current = next;
    setBreakdownMetric(next);
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
      const requestedMetric = metricSelection.current;
      const requestedGeneration = generation.current;
      setLoading(true);
      try {
        const result = await invoke<DashboardResponse>("usage_dashboard", { query: { ...requestedRange, breakdownMetric: requestedMetric } });
        if (!disposed && requestedGeneration === generation.current) { setData(result); setError(null); setNow(Date.now()); }
      } catch (error) {
        if (!disposed && requestedGeneration === generation.current) setError(error === "invalidQuery" ? "Choose a valid period ending no later than now." : "Dashboard could not be loaded. Retry to reconnect to local usage.");
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
  return { range: period.range, period, chooseRange, choosePeriod, selectWindow, goBack, resetZoom, historyDepth, breakdownMetric, chooseBreakdownMetric, data, error, connectionError, loading, now, retry: () => request.current() };
}
