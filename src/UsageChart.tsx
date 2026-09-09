import { useEffect, useId, useRef, useState } from "react";
import uPlot from "uplot";
import "uplot/dist/uPlot.min.css";
import { compactCost, compactTokens, costText, exactTime, exactTokens, localTime } from "./dashboard-data";
import type { LocalUsage, UsageBin } from "./dashboard-types";

/** Only canvas coordinates are floating point; inspection uses the native exact values. */
export function UsageChart({ usage, kind, onOpenPricing }: { usage: LocalUsage; kind: "tokens" | "cost"; onOpenPricing?: () => void }) {
  const host = useRef<HTMLDivElement>(null);
  const id = useId();
  const [selected, setSelected] = useState(0);
  const [hovered, setHovered] = useState<number | null>(null);
  const isCost = kind === "cost";
  const noPrices = isCost && usage.points.length > 0 && usage.points.every(point => point.estimatedCost.knownSubtotal === null);
  const incomplete = usage.points.length > 0 && (isCost ? !usage.summary.estimatedCost.complete : !usage.summary.tokens.totalTokens.complete);
  const point = usage.points[Math.min(selected, usage.points.length - 1)];
  useEffect(() => {
    setSelected(0); setHovered(null);
    if (!host.current || !usage.points.length || noPrices) return;
    const start = usage.start.seconds + usage.start.nanos / 1e9;
    const end = Math.max(usage.end.seconds + usage.end.nanos / 1e9, start + .001);
    const lookup = new Map(usage.points.map((point, index) => [point.index, index]));
    const values: (number | null)[] = Array(usage.binCount).fill(null);
    const partial: (number | null)[] = Array(usage.binCount).fill(null);
    for (const bin of usage.points) {
      const exact = isCost ? bin.estimatedCost.knownSubtotal : bin.tokens.totalTokens.knownTokens;
      if (exact === null) continue;
      const value = Number(exact) / (isCost ? 1e12 : 1);
      if (isCost && !bin.estimatedCost.complete) partial[bin.index] = value;
      else values[bin.index] = value;
    }
    const color = isCost ? "#71d6a4" : "#59d6ed";
    const bars = uPlot.paths.bars!({ size: [.85, 18, 1], radius: .15 });
    const plot = new uPlot({
      width: Math.max(220, host.current.clientWidth), height: 220,
      padding: [12, 8, 0, 0], legend: { show: false }, cursor: { drag: { x: false, y: false } },
      scales: { x: { time: true, range: [start, end] }, y: { range: (_plot, _min, max) => [0, max > 0 ? max * 1.15 : 1] } },
      axes: [
        { stroke: "#91a1b5", grid: { show: false }, ticks: { show: false }, size: 42, gap: 10, space: 75, font: "11px Segoe UI" },
        { stroke: "#91a1b5", grid: { stroke: "#26303c", width: 1 }, ticks: { show: false }, size: 54, gap: 9, space: 45, font: "11px Segoe UI", values: (_plot, ticks) => ticks.map(value => isCost ? new Intl.NumberFormat(undefined, { style: "currency", currency: "USD", notation: "compact", maximumFractionDigits: 2 }).format(value) : new Intl.NumberFormat(undefined, { notation: "compact", maximumFractionDigits: 1 }).format(value)) },
      ],
      series: [{}, { label: isCost ? "Estimated cost" : "Total tokens", fill: color, stroke: color, width: 0, paths: bars, points: { show: false } }, { label: "Incomplete known subtotal", fill: "#e5b96f", stroke: "#e5b96f", width: 0, paths: bars, points: { show: false } }],
      hooks: { setCursor: [plot => { const index = plot.cursor.idx == null ? undefined : lookup.get(plot.cursor.idx); setHovered(index ?? null); if (index !== undefined) setSelected(index); }] },
    }, [Array.from({ length: usage.binCount }, (_, index) => start + (index + .5) * (end - start) / usage.binCount), values, partial], host.current);
    const resize = new ResizeObserver(() => { if (host.current) plot.setSize({ width: Math.max(220, host.current.clientWidth), height: 220 }); });
    resize.observe(host.current);
    return () => { resize.disconnect(); plot.destroy(); };
  }, [usage, isCost, noPrices]);
  return <section className={"dashboard-panel usage-chart usage-chart-" + kind} aria-labelledby={id + "-heading"}>
    <div className="chart-panel-heading"><div><h2 id={id + "-heading"}>{isCost ? "Estimated cost over time" : "Token activity"}</h2><p>{isCost ? "USD at your configured prices" : "Total tokens per time bin"}</p></div><span className={"chart-color-dot " + kind} aria-hidden="true" /></div>
    {usage.points.length === 0 ? <div className="chart-empty" role="status"><span aria-hidden="true">⌁</span><strong>No recorded activity</strong><p>Usage will appear here as sessions are imported.</p></div> : noPrices ? <div className="chart-empty" role="status"><span aria-hidden="true">$</span><strong>Add prices to see estimated cost</strong><p>Your tokens are tracked. Their cost is currently unknown.</p>{onOpenPricing && <button type="button" onClick={onOpenPricing}>Configure prices</button>}</div> : <>
      <div className="usage-plot" ref={host} aria-hidden="true" />
      {hovered !== null && usage.points[hovered] && <div className="usage-tooltip" role="tooltip"><BinReadout point={usage.points[hovered]} kind={kind} /></div>}
    </>}
    <div className="chart-footnote"><span>{incomplete ? (isCost ? "Incomplete known subtotal" : "Some token counts are incomplete") : "Locally observed usage"}</span><span>{isCost ? compactCost(usage.summary.estimatedCost.knownSubtotal) : compactTokens(usage.summary.tokens.totalTokens.knownTokens)}{isCost ? " in range" : " tokens"}</span></div>
    {point && <details className="chart-inspector"><summary>Inspect exact values</summary><label htmlFor={id + "-point"}>Inspect {isCost ? "cost" : "token"} interval ({Math.min(selected + 1, usage.points.length)} of {usage.points.length})</label><input id={id + "-point"} type="range" min="0" max={usage.points.length - 1} value={Math.min(selected, usage.points.length - 1)} onChange={event => setSelected(Number(event.target.value))} aria-describedby={id + "-selected"} /><div id={id + "-selected"} role="status" aria-live="polite" aria-atomic="true"><BinReadout point={point} kind={kind} /></div></details>}
  </section>;
}

function BinReadout({ point, kind }: { point: UsageBin; kind: "tokens" | "cost" }) {
  return <dl className="chart-readout"><dt>Interval</dt><dd>{localTime(point.start)} – {localTime(point.end)}</dd><dt>Exact bounds (start, end]</dt><dd>{exactTime(point.start)} – {exactTime(point.end)}</dd>{kind === "cost" ? <><dt>Estimated cost</dt><dd>{costText(point.estimatedCost)}</dd></> : <><dt>Total tokens</dt><dd>{exactTokens(point.tokens.totalTokens.knownTokens)}{!point.tokens.totalTokens.complete && " · incomplete"}</dd><dt>Input / output</dt><dd>{exactTokens(point.tokens.inputTokens.knownTokens)} / {exactTokens(point.tokens.outputTokens.knownTokens)}</dd></>}<dt>Observed sessions</dt><dd>{point.observedSessions.toLocaleString()}</dd></dl>;
}
