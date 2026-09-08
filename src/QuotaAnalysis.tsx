import { useEffect, useMemo, useRef, useState } from "react";
import uPlot from "uplot";
import type { ObservationTime, QuotaAnalysis as Analysis } from "./dashboard-types";
import { exactTime } from "./dashboard-data";
import { combinations, combinationStats, combinationTokens, tokensPerPercent, type WriteInterpretation } from "./quota-combinations";
import "./quota-analysis.css";

const colors = ["#80b7ff", "#65d8ad", "#edbe74", "#c1a0ff", "#ef94ac", "#86dce5"];
function savedChoices(): { writes: WriteInterpretation; selected: number[] } {
  try {
    const value = JSON.parse(localStorage.getItem("quota-combinations") ?? "null");
    if (value && ["included", "additional"].includes(value.writes) && Array.isArray(value.selected) && value.selected.length <= 6 && value.selected.every((mask: unknown) => typeof mask === "number" && Number.isInteger(mask) && mask >= 1 && mask <= 31)) {
      return { writes: value.writes, selected: [...new Set<number>(value.selected)] };
    }
  } catch { /* Storage is optional; comparison remains available. */ }
  return { writes: "additional", selected: [31, 11, 3, 24] };
}
const number = (value: number | null) => value === null ? "—" : value.toLocaleString(undefined, { maximumFractionDigits: 2 });
function exactNumber(value: string | null) {
  if (value === null) return "—";
  const [whole, fraction] = value.split(".");
  return BigInt(whole).toLocaleString() + (fraction ? `.${fraction}` : "");
}
export function QuotaAnalysis({ analysis, now }: { analysis: Analysis; now: ObservationTime }) {
  const [saved] = useState(savedChoices);
  const [writes, setWrites] = useState<WriteInterpretation>(saved.writes);
  const [selected, setSelected] = useState(saved.selected);
  const [scope, setScope] = useState("range");
  const [inspected, setInspected] = useState(0);
  const host = useRef<HTMLDivElement>(null);
  useEffect(() => {
    try { localStorage.setItem("quota-combinations", JSON.stringify({ writes, selected })); }
    catch { /* A disabled local store does not prevent analysis. */ }
  }, [writes, selected]);
  const intervals = useMemo(() => analysis.intervals.filter(interval => scope === "range" || interval.start.seconds > now.seconds - 900 || interval.start.seconds === now.seconds - 900 && interval.start.nanos >= now.nanos), [analysis, now, scope]);
  const rows = useMemo(() => combinations.map(combination => ({ ...combination, ...combinationStats(intervals, combination.mask, writes) })), [intervals, writes]);
  const active = useMemo(() => selected.map(mask => combinations[mask - 1]), [selected]);
  useEffect(() => {
    setInspected(0);
    if (!host.current || !intervals.length || !active.length) return;
    const data: uPlot.AlignedData = [intervals.map(i => i.end.seconds + i.end.nanos / 1e9), ...active.map(({ mask }) => intervals.map(i => {
      const tokens = combinationTokens(i, mask, writes);
      return tokens === null ? null : Number(tokensPerPercent(tokens, i.consumedPercentagePoints));
    }))];
    // Independent disjoint intervals are dots, not a continuous inferred rate.
    const plot = new uPlot({ width: Math.max(240, host.current.clientWidth), height: 260,
      legend: { show: false }, cursor: { drag: { x: false, y: false } },
      axes: [{ stroke: "#aebccc", grid: { stroke: "#293442" } }, { label: "Tokens / 1%", stroke: "#aebccc", grid: { stroke: "#293442" }, size: 75 }],
      series: [{}, ...active.map((c, index) => ({ label: c.label, stroke: colors[index], paths: () => null, points: { show: true, size: 6 } }))],
      hooks: { setCursor: [plot => { if (plot.cursor.idx != null) setInspected(plot.cursor.idx); }] },
    }, data, host.current);
    const resize = new ResizeObserver(() => { if (host.current) plot.setSize({ width: Math.max(240, host.current.clientWidth), height: 260 }); });
    resize.observe(host.current);
    return () => { resize.disconnect(); plot.destroy(); };
  }, [intervals, active, writes]);
  const interval = intervals[Math.min(inspected, intervals.length - 1)];
  return <section className="quota-analysis dashboard-panel" aria-labelledby="quota-analysis-heading">
    <div className="dashboard-section-heading"><div><span className="eyebrow">SUBSCRIPTION EXPERIMENTS · NO PRICES REQUIRED</span><h2 id="quota-analysis-heading">Tokens per 1% of weekly quota</h2></div></div>
    <p>Compare all 31 nonempty combinations of five token categories. Each dot uses matching local tokens and an observed weekly increase of at least 1 percentage point.</p>
    <div className="quota-analysis-controls">
      <label>Cache-write interpretation<select value={writes} onChange={e => setWrites(e.target.value as WriteInterpretation)}>
        <option value="additional">Hypothesis: writes are additional to input</option>
        <option value="included">Hypothesis: writes are inside uncached input</option>
      </select></label>
      <label>Compare intervals<select value={scope} onChange={e => setScope(e.target.value)}><option value="range">Selected dashboard range</option><option value="recent">Fully within last 15 minutes</option></select></label>
    </div>
    <p className="dashboard-muted">Uncached input = input − cached input{writes === "included" ? " − cache write" : ""}. Visible output = output − reasoning. Selected categories are added once. Both cache-write interpretations are hypotheses, not subscription billing rules.</p>
    <p role="status">{intervals.length} complete quota intervals available{analysis.totalIntervals > analysis.intervals.length ? ` · latest ${analysis.intervals.length} of ${analysis.totalIntervals} retained for comparison` : ""}. Choose up to six combinations to plot.</p>
    {!intervals.length ? <p className="chart-empty">Waiting for two trustworthy quota observations with an increase of at least 1%. Partial intervals, resets and conflicting observations are not bridged.</p> : <>
      <div className="quota-analysis-key">{active.map((c, i) => <span key={c.mask} style={{ color: colors[i] }}>{c.label}</span>)}</div>
      {active.length ? <div className="quota-analysis-plot" ref={host} aria-hidden="true" /> : <p>Select a combination below to show its chart.</p>}
      <label className="quota-analysis-inspector">Inspect interval ({Math.min(inspected + 1, intervals.length)} / {intervals.length})<input type="range" min="0" max={intervals.length - 1} value={Math.min(inspected, intervals.length - 1)} onChange={e => setInspected(Number(e.target.value))} /></label>
      {interval && <div className="quota-analysis-readout" role="status" aria-live="polite"><p>{exactTime(interval.start)} – {exactTime(interval.end)} · {interval.consumedPercentagePoints} percentage points</p><dl>{active.map(c => {
        const tokens = combinationTokens(interval, c.mask, writes);
        return <div key={c.mask}><dt>{c.label}</dt><dd>{tokens === null ? "Unavailable — missing counters or invalid subtraction" : `${tokens.toLocaleString()} tokens · ${exactNumber(tokensPerPercent(tokens, interval.consumedPercentagePoints))} / 1%`}</dd></div>;
      })}</dl></div>}
    </>}
    <div className="quota-analysis-table" tabIndex={0} role="region" aria-label="All token combinations"><table>
      <thead><tr><th scope="col">Plot / combination</th><th scope="col">Tokens / 1%<small>weighted average</small></th><th scope="col">Min – max / 1%</th><th scope="col">Variation<small>CV · 3+ intervals</small></th><th scope="col">Usable intervals</th></tr></thead>
      <tbody>{rows.map(row => <tr key={row.mask}><th scope="row"><label><input type="checkbox" checked={selected.includes(row.mask)} disabled={!selected.includes(row.mask) && selected.length >= 6} onChange={e => setSelected(current => e.target.checked ? [...current, row.mask] : current.filter(mask => mask !== row.mask))} />{row.label}</label></th><td>{exactNumber(row.ratio)}</td><td>{number(row.min)} – {number(row.max)}</td><td>{row.variation === null ? "—" : `${number(row.variation)}%`}</td><td>{row.count} / {intervals.length}</td></tr>)}</tbody>
    </table></div>
    <p className="dashboard-muted">Weighted average = selected tokens ÷ observed percentage points, using only usable intervals for that combination. Variation measures spread between interval ratios; lower variation does not prove how OpenAI counts tokens. Model mix, quota rounding, delayed reports and usage on other devices can affect results. Missing local counters remain unknown. Prices, including codex-auto-review pricing, do not affect this comparison.</p>
  </section>;
}
