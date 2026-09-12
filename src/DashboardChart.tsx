import { useEffect, useRef, useState } from "react";
import uPlot from "uplot";
import "uplot/dist/uPlot.min.css";
import type { ChartPoint, DashboardChart as ChartData, TimeWindow } from "./dashboard-types";
import { attachTimeNavigation } from "./time-navigation";
import { costText, exactTime, unavailable, usd } from "./dashboard-data";

const colors = ["#80b7ff", "#65d8ad", "#edbe74"];
export function DashboardChart({ chart, selectWindow }: { chart: ChartData; selectWindow?: (window: TimeWindow) => void }) {
  const selection = useRef(selectWindow); selection.current = selectWindow;
  const host = useRef<HTMLDivElement>(null);
  const [selected, setSelected] = useState(0);
  const [hovered, setHovered] = useState<number | null>(null);
  useEffect(() => {
    setSelected(0); setHovered(null);
    if (!host.current || !chart.points.length) return;
    const points = chart.points;
    const data: uPlot.AlignedData = [
      points.map(p => p.time.seconds + p.time.nanos / 1e9),
      points.map(p => p.weeklyUsedPercent === null ? null : Number(p.weeklyUsedPercent)),
      points.map(p => p.cumulativeEstimatedCost?.complete && p.cumulativeEstimatedCost.knownSubtotal !== null ? Number(p.cumulativeEstimatedCost.knownSubtotal) / 1e12 : null),
      points.map(p => p.effectiveUsdPerPercent === null ? null : Number(p.effectiveUsdPerPercent)),
    ];
    // Native connectivity is authoritative, including subpixel-time boundaries.
    const paths: uPlot.Series.PathBuilder = (plot, series, first, last) => {
      const stroke = new Path2D();
      let connected = false;
      for (let i = first; i <= last; i++) {
        const value = data[series][i];
        if (value == null) { connected = false; continue; }
        const x = plot.valToPos(data[0][i], "x", true);
        const y = plot.valToPos(value, plot.series[series].scale!, true);
        if (connected && points[i].connectFromPrevious && points[i].segmentId === points[i - 1].segmentId) stroke.lineTo(x, y);
        else stroke.moveTo(x, y);
        connected = true;
      }
      return { stroke };
    };
    const plot = new uPlot({
      width: Math.max(280, host.current.clientWidth), height: 290,
      legend: { show: false }, cursor: { drag: { x: false, y: false } },
      scales: { x: { time: true, range: [chart.start.seconds + chart.start.nanos / 1e9, Math.max(chart.end.seconds + chart.end.nanos / 1e9, chart.start.seconds + chart.start.nanos / 1e9 + 0.001)] }, weekly: { range: [0, 100] }, cost: { auto: true }, ratio: { auto: true } },
      axes: [
        { stroke: "#aebccc", grid: { stroke: "#293442" }, size: 54 },
        { scale: "weekly", label: "Weekly used %", stroke: colors[0], size: 44, labelSize: 18, grid: { stroke: "#293442" } },
        { scale: "cost", label: "Cumulative est. USD", side: 1, stroke: colors[1], size: 48, labelSize: 18, grid: { show: false } },
        { scale: "ratio", label: "Est. USD / 1%", side: 1, stroke: colors[2], size: 48, labelSize: 18, grid: { show: false } },
      ],
      series: [{}, ...["weekly", "cost", "ratio"].map((scale, i) => ({ scale, stroke: colors[i], width: 1.5, paths, points: { show: true, size: 3 }, spanGaps: false }))],
      hooks: {
        setCursor: [plot => { const index = plot.cursor.idx; setHovered(index == null ? null : index); if (index != null) setSelected(index); }],
        draw: [plot => {
          const ctx = plot.ctx; ctx.save(); ctx.strokeStyle = "#ba9362"; ctx.setLineDash([3, 4]);
          for (const boundary of chart.boundaries) {
            const x = plot.valToPos(boundary.firstTime.seconds + boundary.firstTime.nanos / 1e9, "x", true);
            ctx.beginPath(); ctx.moveTo(x, plot.bbox.top); ctx.lineTo(x, plot.bbox.top + plot.bbox.height); ctx.stroke();
          }
          ctx.restore();
        }],
      },
    }, data, host.current);
    const detach = attachTimeNavigation(plot, window => selection.current?.(window));
    const resize = new ResizeObserver(() => { if (host.current) plot.setSize({ width: Math.max(280, host.current.clientWidth), height: 290 }); });
    resize.observe(host.current);
    return () => { detach(); resize.disconnect(); plot.destroy(); };
  }, [chart]);
  const point = chart.points[Math.min(selected, chart.points.length - 1)];
  return <div className="dashboard-chart">
    <div className="dashboard-chart-key"><span>Weekly used %</span><span>Cumulative estimated USD</span><span>Effective USD / 1%</span></div>
    {chart.points.length === 0 ? <p role="status">No weekly observations in this range.</p> : <>
      <div className="dashboard-plot" ref={host} aria-hidden="true" />
      {hovered !== null && chart.points[hovered] && <div role="tooltip" className="dashboard-tooltip"><PointReadout point={chart.points[hovered]} /></div>}
      <details className="quota-inspector"><summary>Inspect quota observations</summary><div className="dashboard-inspection">
        <label htmlFor="dashboard-point">Inspect observation ({Math.min(selected + 1, chart.points.length)} of {chart.points.length})</label>
        <input id="dashboard-point" type="range" min="0" max={chart.points.length - 1} value={Math.min(selected, chart.points.length - 1)}
          onChange={event => setSelected(Number(event.target.value))} aria-describedby="dashboard-selected" />
        <span className="dashboard-muted">Arrow keys move; Home / End jump to first / last.</span>
      </div>
      <div id="dashboard-selected" role="status" aria-live="polite" aria-atomic="true"><PointReadout point={point} /></div></details>
    </>}
    <p className="dashboard-muted">{chart.coverageNote} Each line restarts where the observations stop being comparable; dashed markers show where that happened.</p>
    {chart.boundaries.length > 0 && <details><summary>{count(chart.boundaries.reduce((sum, b) => sum + b.count, 0), "observation boundary", "observation boundaries")}</summary>
      <ul className="dashboard-boundaries">{chart.boundaries.map(b => <li key={b.binIndex}>{exactTime(b.firstTime)}{exactTime(b.lastTime) !== exactTime(b.firstTime) ? ` – ${exactTime(b.lastTime)}` : ""}: {b.kinds.map(kind => kind.replace(/([A-Z])/g, " $1").toLowerCase()).join(", ")} ({b.count}){b.overloaded ? " — multiple boundaries grouped" : ""}</li>)}</ul>
    </details>}
  </div>;
}
function PointReadout({ point }: { point: ChartPoint }) {
  return <dl className="dashboard-point-values">
    <dt>Observation</dt><dd><time>{exactTime(point.time)}</time></dd>
    <dt>Weekly used</dt><dd>{point.weeklyUsedPercent === null ? "Unavailable" : `${point.weeklyUsedPercent}%`}</dd>
    <dt>Cumulative estimated USD</dt><dd>{costText(point.cumulativeEstimatedCost)}</dd>
    <dt>Effective USD / 1%</dt><dd>{usd(point.effectiveUsdPerPercent)}{point.unavailableReason ? ` — ${unavailable[point.unavailableReason]}` : ""}</dd>
    <dt>Segment</dt><dd>{point.segmentId ?? "Unavailable"}{!point.connectFromPrevious ? " — observation boundary; no connection from previous point" : ""}</dd>
  </dl>;
}

/** Singular and plural share one call site, so a count of one never reads wrong. */
function count(value: number, one: string, many: string): string {
  return value.toLocaleString() + " " + (value === 1 ? one : many);
}
