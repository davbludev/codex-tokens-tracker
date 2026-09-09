import { useEffect, useMemo, useRef, useState } from "react";
import uPlot from "uplot";
import type { QuotaAnalysis as Analysis } from "./dashboard-types";
import { exactTime } from "./dashboard-data";
import { combinations, combinationStats, formatted, perPercent, sample, type WriteInterpretation } from "./quota-combinations";
import "./quota-analysis.css";

export function QuotaAnalysis({ analysis }: { analysis: Analysis }) {
  const [writes, setWrites] = useState<WriteInterpretation>("additional");
  const [selected, setSelected] = useState(combinations.map(c => c.mask));
  const [metric, setMetric] = useState<"usd" | "tokens">("usd");
  const [inspected, setInspected] = useState<number | null>(null);
  const host = useRef<HTMLDivElement>(null);
  const intervals = analysis.intervals;
  const active = useMemo(() => combinations.filter(c => selected.includes(c.mask)), [selected]);
  const rows = useMemo(() => combinations.map(c => ({ ...c, ...combinationStats(intervals, c.mask, writes) })), [intervals, writes]);
  useEffect(() => {
    if (!host.current || !intervals.length) return;
    const data: uPlot.AlignedData = [intervals.map(i => i.end.seconds + i.end.nanos / 1e9), ...active.map(c => intervals.map(i => {
      const value = sample(i, c.mask, writes);
      const units = metric === "usd" ? value?.estimatedUsd : value?.tokens;
      return units == null ? null : Number(perPercent(units, i.consumedPercentagePoints, metric === "usd" ? 12 : 0, 12));
    }))];
    const plot = new uPlot({ width: Math.max(200, host.current.clientWidth), height: 260,
      legend: { show: false }, cursor: { drag: { x: false, y: false } },
      axes: [{ stroke: "#aebccc", grid: { stroke: "#293442" } }, { label: metric === "usd" ? "API-equivalent USD / 1%" : "Tokens / 1%", stroke: "#aebccc", grid: { stroke: "#293442" }, size: 88,
        values: (_plot, ticks) => ticks.map(value => value !== 0 && (Math.abs(value) < 0.001 || Math.abs(value) >= 1e9) ? value.toExponential(2) : value.toLocaleString(undefined, { maximumSignificantDigits: 5 })),
      }],
      // Independent intervals never imply a continuous consumption rate.
      series: [{}, ...active.map(c => ({ label: c.name, stroke: c.color, paths: () => null, points: { show: true, size: 7 } }))],
      hooks: { setCursor: [plot => setInspected(plot.cursor.idx ?? null)] },
    }, data, host.current);
    const resize = new ResizeObserver(() => { if (host.current) plot.setSize({ width: Math.max(200, host.current.clientWidth), height: 260 }); });
    resize.observe(host.current);
    return () => { resize.disconnect(); plot.destroy(); };
  }, [intervals, active, writes, metric]);
  const interval = inspected === null ? null : intervals[Math.min(inspected, intervals.length - 1)];
  return <section className="quota-analysis dashboard-panel" aria-labelledby="quota-analysis-heading">
    <div className="dashboard-section-heading"><h2 id="quota-analysis-heading">Weekly quota hypotheses</h2><div className="dashboard-ranges" role="group" aria-label="Hypothesis chart metric">{(["usd", "tokens"] as const).map(value => <button key={value} type="button" aria-pressed={metric === value} onClick={() => setMetric(value)}>{value === "usd" ? "USD / 1%" : "Tokens / 1%"}</button>)}</div></div>
    <p>Compare how different token categories could correspond to one observed percentage point of your weekly limit.</p>
    <p className="dashboard-muted">API-equivalent · configured model prices · Cache writes {writes === "included" ? "included in input" : "additional to input"}</p>
    {intervals.length ? <>
      <div className="quota-analysis-plot" ref={host} role="group" aria-label={`${metric === "usd" ? "USD" : "Tokens"} per observed 1% comparison chart; use arrow keys for interval details`} tabIndex={0}
        onKeyDown={e => { if (["ArrowLeft", "ArrowRight", "Home", "End"].includes(e.key)) { e.preventDefault(); setInspected(current => e.key === "Home" ? 0 : e.key === "End" ? intervals.length - 1 : Math.max(0, Math.min(intervals.length - 1, (current ?? -1) + (e.key === "ArrowRight" ? 1 : -1)))); } }} />
      <p className="dashboard-muted">Each dot is one comparable interval. Hover or focus the chart and use ← / → for details. Toggle series on the cards.</p>
      {interval && <div className="hypothesis-tooltip" role="status" aria-live="polite"><p>{exactTime(interval.start)} – {exactTime(interval.end)} · +{interval.consumedPercentagePoints} percentage points</p><div>{active.map(c => {
        const value = sample(interval, c.mask, writes);
        return <p key={c.mask}><span style={{ color: c.color }}>{c.name}</span><span>{formatted(value?.tokens == null ? null : perPercent(value.tokens, interval.consumedPercentagePoints, 0, 0))} tokens / 1% · {formatted(value?.estimatedUsd == null ? null : perPercent(value.estimatedUsd, interval.consumedPercentagePoints, 12, 6), true)} USD / 1%</span></p>;
      })}</div></div>}
      {!active.length && <p role="status">Enable a series using its card below.</p>}
      {metric === "usd" && active.length > 0 && !intervals.some(i => active.some(c => sample(i, c.mask, writes)?.estimatedUsd != null)) && <p role="status">USD unavailable: no comparable priced intervals. Tokens can still be compared when counters are complete.</p>}
    </> : <p className="chart-empty">Waiting for two trustworthy observations with an increase of at least 1%. Resets and ambiguous observations are excluded.</p>}
    <div className="hypothesis-grid">{rows.map(row => <article className="hypothesis-card" key={row.mask} style={{ borderTopColor: row.color }} aria-label={row.name}>
      <label className="hypothesis-toggle"><input type="checkbox" checked={selected.includes(row.mask)} onChange={e => setSelected(current => e.target.checked ? [...current, row.mask] : current.filter(mask => mask !== row.mask))} /><span className="hypothesis-dot" style={{ background: row.color }} /><strong>{row.name}</strong><span className="sr-only"> series visibility</span></label>
      <div className="hypothesis-badges">{row.components.map((name, index) => <span key={name} className={index < 2 ? "permanent" : "optional"}>{name}</span>)}</div>
      <dl><div><dt>Tokens / 1%</dt><dd>{formatted(row.tokens)}</dd></div><div><dt>Estimated USD / 1%</dt><dd>{formatted(row.usd, true)}</dd></div><div><dt>Estimated USD / 100%</dt><dd>{formatted(row.fullUsd, true)}</dd></div></dl>
      <p className="dashboard-muted">{row.count} / {intervals.length} usable intervals · weighted average{row.variation !== null && ` · Spread ${row.variation.toLocaleString(undefined, { maximumFractionDigits: 1 })}% CV`}</p>
      {row.tokens === null ? <p className="hypothesis-unavailable">Unavailable — {row.tokenReason}</p> : row.usd === null && <p className="hypothesis-unavailable">USD unavailable — {row.priceReason} ({row.pricedCount} / {row.count} priced)</p>}
    </article>)}</div>
    <p className="dashboard-muted">These are comparisons of local observations, not OpenAI billing rules. Other devices, delayed quota reporting, model mix and percentage rounding can affect the result.</p>
    <details className="hypothesis-method"><summary>Method and data quality</summary>
      <label>Cache-write interpretation<select value={writes} onChange={e => setWrites(e.target.value as WriteInterpretation)}><option value="additional">Additional to input</option><option value="included">Included in input</option></select></label>
      <p>The uncached input badge is input − cached input{writes === "included" ? " − cache writes" : ""}. The visible output badge is output − reasoning. Optional components are added once. Both cache-write interpretations are hypotheses.</p>
      <p>For each usage observation, multiply each selected component by that model’s preserved price version, then sum within the interval. Reasoning uses its separate price when configured, otherwise the output price. Rates are USD per million tokens.</p>
      <p>Weighted tokens / 1% = sum of usable tokens ÷ sum of observed percentage points. USD / 1% = sum of matching estimated USD ÷ those same percentage points; USD / 100% multiplies this ratio by 100. It is an extrapolation, not a subscription price. Any unpriced usage in the usable intervals makes the monetary average unavailable.</p>
      <p>{intervals.length} of {analysis.totalIntervals} intervals retained (latest 512 maximum). Only trustworthy intervals fully within the selected range with at least one percentage point are used. Resets, ambiguous observations and partial intervals are excluded; newer unmatched usage is excluded. Missing counters and invalid subset subtraction remain unavailable, including when aggregate totals would hide the invalid observation. Spread is the coefficient of variation of interval token ratios with at least three samples.</p>
    </details>
  </section>;
}
