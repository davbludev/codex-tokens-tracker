import { useEffect, useMemo, useRef, useState } from "react";
import uPlot from "uplot";
import { compactCost, compactTokens, costText, exactTokens, localTime, money } from "./dashboard-data";
import { percentShares } from "./quota-categories";
import type { TurnActivity as Activity, TurnSeries } from "./dashboard-types";
import "./turn-activity.css";

const surface = "#151b23";
/** Ordered hues for named combinations; the remainder and unattributed turns
 * stay grey, and differ from each other so the chart distinguishes them. */
const palette = ["#5fb3f5", "#f2994a", "#a77bf3", "#3fbf8f", "#ef7c8e", "#4fd0da", "#d9cf5a"];
const REMAINDER = "#93a1b5";
const UNATTRIBUTED = "#5c6b7f";
const metrics = [["turns", "Turns"], ["tokens", "Tokens"], ["cost", "Estimated cost"]] as const;
/** Why some rows report no session count at all. */
const shared = "One session can run several combinations, so their distinct session counts cannot be added";
type Metric = (typeof metrics)[number][0];

function color(series: TurnSeries, index: number): string {
  if (series.kind === "combination") return palette[index % palette.length];
  return series.kind === "other" ? REMAINDER : UNATTRIBUTED;
}
/** Nulls stay out of the drawing rather than becoming a measured zero. */
function value(series: TurnSeries, index: number, metric: Metric): number | null {
  const point = series.points.find(entry => entry.index === index);
  if (!point) return null;
  if (metric === "turns") return point.turns;
  const exact = metric === "tokens" ? point.tokens.knownTokens : point.estimatedCost.knownSubtotal;
  return exact === null ? null : Number(exact) / (metric === "cost" ? 1e12 : 1);
}
/** Per-turn averages answer "is this combination expensive per turn, or just busy?" */
function perTurn(exact: string | null, turns: number, metric: "tokens" | "cost"): string {
  if (exact === null || turns <= 0) return "—";
  const each = BigInt(exact) / BigInt(turns);
  return metric === "tokens" ? compactTokens(String(each)) : compactCost(String(each));
}
function reasoning(series: TurnSeries): string {
  return series.kind === "other" ? "Mixed" : series.effort ?? "Unavailable";
}

/**
 * How many turns ran, split by the combination of model and reasoning effort.
 * Turns, tokens and cost share one binning rule, so each stack of bars adds up
 * to the row beside it whichever metric is selected.
 */
export function TurnActivity({ activity, onOpenPricing }: { activity: Activity; onOpenPricing?: () => void }) {
  const [metric, setMetric] = useState<Metric>("turns");
  const [inspected, setInspected] = useState<number | null>(null);
  const host = useRef<HTMLDivElement>(null);
  const { series, binCount } = activity;
  const start = activity.start.seconds + activity.start.nanos / 1e9;
  const end = Math.max(activity.end.seconds + activity.end.nanos / 1e9, start + 0.001);
  const bound = (index: number) => ({ seconds: Math.round(start + (index * (end - start)) / binCount), nanos: 0 });
  const unpriced = metric === "cost" && series.some(row => !row.estimatedCost.complete);
  const nothingPriced = metric === "cost" && series.every(row => row.estimatedCost.knownSubtotal === null);
  // Largest remainder, so the shares of turns add up to exactly 100.
  const shares = useMemo(() => {
    const counts = series.map(row => BigInt(row.turns));
    return percentShares(counts, counts.reduce((sum, value) => sum + value, 0n));
  }, [series]);

  useEffect(() => {
    setInspected(null);
    if (!host.current || !series.length || nothingPriced) return;
    // Each series holds the running total up to itself and is drawn before the
    // next lower one, so later bars overpaint into a stack.
    const stacked = series.map(() => Array<number | null>(binCount).fill(null));
    for (let index = 0; index < binCount; index++) {
      let running = 0;
      let seen = false;
      series.forEach((row, position) => {
        const amount = value(row, index, metric);
        if (amount === null) {
          if (seen) stacked[position][index] = running;
          return;
        }
        seen = true;
        running += amount;
        stacked[position][index] = running;
      });
    }
    const bars = uPlot.paths.bars!({ size: [0.85, 26, 1], radius: 0.1 });
    const slot = Math.max(220, host.current.clientWidth) / Math.max(1, binCount);
    const separator = slot >= 16 ? 2 : slot >= 8 ? 1 : 0;
    const plot = new uPlot({
      width: Math.max(220, host.current.clientWidth), height: 240,
      padding: [12, 8, 0, 0], legend: { show: false }, cursor: { drag: { x: false, y: false } },
      scales: { x: { time: true, range: [start, end] }, y: { range: (_plot, _min, max) => [0, max > 0 ? max * 1.12 : 1] } },
      axes: [
        { stroke: "#91a1b5", grid: { show: false }, ticks: { show: false }, size: 42, gap: 10, space: 75, font: "11px Segoe UI" },
        { stroke: "#91a1b5", grid: { stroke: "#26303c", width: 1 }, ticks: { show: false }, size: 56, gap: 9, space: 45, font: "11px Segoe UI",
          values: (_plot, ticks) => ticks.map(tick => metric === "cost"
            ? new Intl.NumberFormat(undefined, { style: "currency", currency: "USD", notation: "compact", maximumFractionDigits: 2 }).format(tick)
            : new Intl.NumberFormat(undefined, { notation: "compact", maximumFractionDigits: metric === "turns" ? 0 : 1 }).format(tick)) },
      ],
      series: [{}, ...[...series].reverse().map((row, position) => ({
        label: row.label, fill: color(row, series.length - 1 - position), stroke: surface,
        width: separator, paths: bars, points: { show: false },
      }))],
      hooks: { setCursor: [instance => setInspected(instance.cursor.idx ?? null)] },
    }, [Array.from({ length: binCount }, (_, index) => start + (index + 0.5) * ((end - start) / binCount)), ...[...stacked].reverse()], host.current);
    const resize = new ResizeObserver(() => { if (host.current) plot.setSize({ width: Math.max(220, host.current.clientWidth), height: 240 }); });
    resize.observe(host.current);
    return () => { resize.disconnect(); plot.destroy(); };
  }, [series, binCount, metric, start, end, nothingPriced]);

  const bin = inspected === null ? null : Math.min(inspected, binCount - 1);
  return <section className="turn-activity" aria-labelledby="turn-activity-heading">
    <div className="dashboard-section-heading">
      <div><span className="eyebrow">WORK PATTERN</span><h2 id="turn-activity-heading">Turns by model and reasoning</h2></div>
      <div className="dashboard-ranges" role="group" aria-label="Turn chart metric">{metrics.map(([key, label]) => <button key={key} type="button" aria-pressed={metric === key} onClick={() => setMetric(key)}>{label}</button>)}</div>
    </div>
    <p className="turn-activity-lead">A turn is one exchange with the model. Every turn is counted once, under the model it ran on and the reasoning effort it was configured with, where it first appears in this range — and its tokens and cost are counted there too.</p>
    <div className="turn-activity-summary">
      <span><strong>{activity.totalTurns.toLocaleString()}</strong> turn{activity.totalTurns === 1 ? "" : "s"}</span>
      <span><strong>{activity.combinations.toLocaleString()}</strong> model and reasoning combination{activity.combinations === 1 ? "" : "s"}</span>
      {activity.turnsWithoutIdentity > 0 && <span className="turn-activity-caveat">{activity.turnsWithoutIdentity.toLocaleString()} usage record{activity.turnsWithoutIdentity === 1 ? "" : "s"} had no turn marker; each counts as one turn</span>}
    </div>
    {series.length === 0
      ? <div className="chart-empty" role="status"><span aria-hidden="true">⌁</span><strong>No turns in this range</strong><p>Turns appear here as sessions are imported.</p></div>
      : nothingPriced
        ? <div className="chart-empty" role="status"><span aria-hidden="true">$</span><strong>Add prices to compare cost</strong><p>Turns and tokens are tracked. Their cost is currently unknown.</p>{onOpenPricing && <button type="button" onClick={onOpenPricing}>Configure prices</button>}</div>
        : <div className="turn-activity-plot-wrap">
            <div className="turn-activity-plot" ref={host} role="group" aria-label="Stacked turns by model and reasoning; use arrow keys for interval details" tabIndex={0}
              onKeyDown={event => { if (["ArrowLeft", "ArrowRight", "Home", "End"].includes(event.key)) { event.preventDefault(); setInspected(current => event.key === "Home" ? 0 : event.key === "End" ? binCount - 1 : Math.max(0, Math.min(binCount - 1, (current ?? -1) + (event.key === "ArrowRight" ? 1 : -1)))); } }} />
            {bin !== null && <div className="turn-activity-tooltip" role="status" aria-live="polite">
              <p>{localTime(bound(bin))} – {localTime(bound(bin + 1))}</p>
              <ul>{series.map((row, index) => {
                const amount = value(row, bin, metric);
                return <li key={row.key}><span className="hypothesis-dot" style={{ background: color(row, index) }} aria-hidden="true" />{row.label}<strong>{amount === null ? "—" : metric === "cost" ? compactCost(String(Math.round(amount * 1e12))) : amount.toLocaleString(undefined, { maximumFractionDigits: 0 })}</strong></li>;
              })}</ul>
            </div>}
          <ul className="turn-activity-key" aria-label="Combination colors">{series.map((row, index) => <li key={row.key}><span className="hypothesis-dot" style={{ background: color(row, index) }} aria-hidden="true" />{row.label}</li>)}</ul>
          </div>}
    {unpriced && <p className="turn-activity-note">Turns whose usage has no configured price add nothing to the cost bars. Their turn and token counts are unaffected.</p>}
    <div className="turn-activity-table-wrap" role="region" aria-label="Turns per model and reasoning, scroll horizontally" tabIndex={0}><table>
      <caption>Every turn observed in this range, by the model it ran on and its reasoning effort.</caption>
      <thead><tr><th scope="col">Model</th><th scope="col">Reasoning</th><th scope="col">Turns</th><th scope="col">Share of turns</th><th scope="col">Tokens</th><th scope="col">Tokens / turn</th><th scope="col">Estimated cost</th><th scope="col">Cost / turn</th><th scope="col">Sessions</th></tr></thead>
      <tbody>{series.map((row, index) => <tr key={row.key} className={row.kind === "combination" ? undefined : "turn-activity-secondary"}>
        <th scope="row"><span className="hypothesis-dot" style={{ background: color(row, index) }} aria-hidden="true" />{row.model ?? (row.kind === "other" ? row.label : "Unknown model")}</th>
        <td>{reasoning(row)}</td>
        <td className="turn-activity-count">{row.turns.toLocaleString()}</td>
        <td>{shares[index] === null ? "—" : shares[index].toLocaleString(undefined, { minimumFractionDigits: 1, maximumFractionDigits: 1 }) + "%"}</td>
        <td title={exactTokens(row.tokens.knownTokens)}>{compactTokens(row.tokens.knownTokens)}{!row.tokens.complete && <span className="incomplete-mark" title="Incomplete count">*</span>}</td>
        <td>{perTurn(row.tokens.knownTokens, row.turns, "tokens")}</td>
        <td title={costText(row.estimatedCost)}>{compactCost(row.estimatedCost.knownSubtotal)}{!row.estimatedCost.complete && row.estimatedCost.knownSubtotal !== null && <span className="incomplete-mark" title="Incomplete known subtotal">*</span>}</td>
        <td>{perTurn(row.estimatedCost.knownSubtotal, row.turns, "cost")}</td>
        <td title={row.observedSessions === null ? shared : undefined}>{row.observedSessions === null ? "—" : row.observedSessions.toLocaleString()}</td>
      </tr>)}</tbody>
      <tfoot><tr><th scope="row">All combinations</th><td>—</td><td className="turn-activity-count">{activity.totalTurns.toLocaleString()}</td><td>100.0%</td><td>{compactTokens(sum(series, row => row.tokens.knownTokens))}</td><td>{perTurn(sum(series, row => row.tokens.knownTokens), activity.totalTurns, "tokens")}</td><td title={money(sum(series, row => row.estimatedCost.knownSubtotal))}>{compactCost(sum(series, row => row.estimatedCost.knownSubtotal))}</td><td>{perTurn(sum(series, row => row.estimatedCost.knownSubtotal), activity.totalTurns, "cost")}</td><td title={shared}>—</td></tr></tfoot>
    </table></div>
    <p className="dashboard-muted">{activity.coverageNote} An asterisk marks a value that does not cover every accepted observation.</p>
  </section>;
}

/** Exact sum of the series' known values; null when none of them is known. */
function sum(series: TurnSeries[], pick: (row: TurnSeries) => string | null): string | null {
  const known = series.map(pick).filter((value): value is string => value !== null);
  return known.length === 0 ? null : String(known.reduce((total, value) => total + BigInt(value), 0n));
}
