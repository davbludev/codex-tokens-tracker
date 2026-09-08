import { useEffect, useMemo, useRef, useState } from "react";
import uPlot from "uplot";
import type { QuotaAnalysis as Analysis } from "./dashboard-types";
import { exactTime, localTime } from "./dashboard-data";
import { formatted, perPercent } from "./quota-combinations";
import { categories, categoryPerPercent, categoryStats } from "./quota-categories";
import "./quota-analysis.css";

const surface = "#151b23";

/** Stacked USD / 1% per comparable interval, one segment per token category. */
export function QuotaCategoryCosts({ analysis }: { analysis: Analysis }) {
  const [inspected, setInspected] = useState<number | null>(null);
  const host = useRef<HTMLDivElement>(null);
  const intervals = analysis.intervals;
  const stats = useMemo(() => categoryStats(intervals), [intervals]);
  const priced = intervals.some(interval => interval.categories.reason === null);
  useEffect(() => {
    setInspected(null);
    if (!host.current || !priced) return;
    // Each series holds the running total up to its category and is drawn
    // before the next lower one, so later bars overpaint into a stack.
    const stacked = categories.map(() => intervals.map<number | null>(() => null));
    intervals.forEach((interval, index) => {
      if (interval.categories.reason !== null) return;
      let running = 0n;
      categories.forEach((category, position) => {
        running += BigInt(interval.categories[category.key]!);
        stacked[position][index] = Number(perPercent(String(running), interval.consumedPercentagePoints, 12, 12));
      });
    });
    const bars = uPlot.paths.bars!({ size: [.7, 28, 1], radius: .12, align: 0 });
    const plot = new uPlot({ width: Math.max(200, host.current.clientWidth), height: 260,
      legend: { show: false }, cursor: { drag: { x: false, y: false } },
      scales: { x: { time: false, range: [.5, intervals.length + .5] }, y: { range: (_plot, _min, max) => [0, max > 0 ? max * 1.1 : 1] } },
      axes: [{ stroke: "#aebccc", grid: { show: false }, incrs: [1, 2, 5, 10, 20, 50, 100, 200], values: (_plot, ticks) => ticks.map(tick => intervals[tick - 1] ? localTime(intervals[tick - 1].end) : "") },
        { label: "USD / 1%", stroke: "#aebccc", grid: { stroke: "#293442" }, size: 88,
          values: (_plot, ticks) => ticks.map(value => value !== 0 && (Math.abs(value) < 0.001 || Math.abs(value) >= 1e9) ? value.toExponential(2) : value.toLocaleString(undefined, { maximumSignificantDigits: 5 })) }],
      // A 2px surface stroke separates stacked segments and adjacent bars.
      series: [{}, ...[...categories].reverse().map(category => ({ label: category.label, fill: category.color, stroke: surface, width: 2, paths: bars, points: { show: false } }))],
      hooks: { setCursor: [plot => setInspected(plot.cursor.idx ?? null)] },
    }, [intervals.map((_, index) => index + 1), ...[...stacked].reverse()], host.current);
    const resize = new ResizeObserver(() => { if (host.current) plot.setSize({ width: Math.max(200, host.current.clientWidth), height: 260 }); });
    resize.observe(host.current);
    return () => { resize.disconnect(); plot.destroy(); };
  }, [intervals, priced]);
  const interval = inspected === null ? null : intervals[Math.min(inspected, intervals.length - 1)];
  return <section className="quota-categories dashboard-panel" aria-labelledby="quota-categories-heading">
    <div className="dashboard-section-heading"><h2 id="quota-categories-heading">Estimated cost by token category</h2><span className="dashboard-muted">{stats.count} / {intervals.length} priced intervals</span></div>
    <p>How much of each observed weekly percentage point is input, cached input, cache-write or output cost, at each observation's own model price.</p>
    <p className="dashboard-muted">API-equivalent · configured model prices · reasoning inside output · cache writes additional to input</p>
    {intervals.length ? priced ? <>
      <div className="quota-categories-plot" ref={host} role="group" aria-label="Stacked USD per observed 1% by token category; use arrow keys for interval details" tabIndex={0}
        onKeyDown={e => { if (["ArrowLeft", "ArrowRight", "Home", "End"].includes(e.key)) { e.preventDefault(); setInspected(current => e.key === "Home" ? 0 : e.key === "End" ? intervals.length - 1 : Math.max(0, Math.min(intervals.length - 1, (current ?? -1) + (e.key === "ArrowRight" ? 1 : -1)))); } }} />
      <ul className="category-key" aria-label="Category colors">{categories.map(category => <li key={category.key}><span className="hypothesis-dot" style={{ background: category.color }} aria-hidden="true" />{category.label}</li>)}</ul>
      <p className="dashboard-muted">Each bar is one comparable interval, stacked bottom-up in the key's order. Hover or focus the chart and use ← / → for details.</p>
      {interval && <div className="category-tooltip" role="status" aria-live="polite"><p>{exactTime(interval.start)} – {exactTime(interval.end)} · +{interval.consumedPercentagePoints} percentage points{interval.categories.reason && ` · ${interval.categories.reason}`}</p><div>{categories.map(category => <p key={category.key}><span style={{ color: category.color }}>{category.label}</span><span>{formatted(categoryPerPercent(interval, category.key), true)} USD / 1%</span></p>)}</div></div>}
    </> : <p role="status">USD unavailable: no comparable priced intervals. {stats.reason}</p> : <p className="chart-empty">Waiting for two trustworthy observations with an increase of at least 1%. Resets and ambiguous observations are excluded.</p>}
    <div className="category-grid">{stats.rows.map(row => <article className="category-card" key={row.key} style={{ borderTopColor: row.color }} aria-label={row.label}>
      <h3><span className="hypothesis-dot" style={{ background: row.color }} aria-hidden="true" />{row.label}</h3>
      <dl><div><dt>Estimated USD / 1%</dt><dd>{formatted(row.usd, true)}</dd></div><div><dt>Share of cost</dt><dd>{row.share === null ? "Unavailable" : row.share.toLocaleString(undefined, { maximumFractionDigits: 1 }) + "%"}</dd></div><div><dt>Estimated USD / 100%</dt><dd>{formatted(row.fullUsd, true)}</dd></div></dl>
      <div className="category-share" aria-hidden="true"><div style={{ width: (row.share ?? 0) + "%", background: row.color }} /></div>
    </article>)}</div>
    <p className="category-total"><span>Total across categories</span><span><strong>{formatted(stats.totalUsd, true)}</strong> USD / 1% · {formatted(stats.totalFullUsd, true)} USD / 100%</span></p>
    {stats.totalUsd === null && <p className="hypothesis-unavailable">USD unavailable — {stats.reason} ({stats.count} / {intervals.length} priced)</p>}
    <p className="dashboard-muted">Weighted by observed percentage points across all retained intervals. Categories add up to the interval's estimated token cost; any unpriced usage makes the whole comparison unavailable rather than showing a priced subset.</p>
  </section>;
}
